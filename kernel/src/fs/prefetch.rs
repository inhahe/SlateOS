//! File access pattern hinting and prefetch control.
//!
//! Provides the equivalent of `posix_fadvise()` — applications can inform
//! the kernel about their intended file access patterns so the VFS and
//! cache can optimize accordingly.
//!
//! ## Access Advice
//!
//! | Advice       | Effect                                        |
//! |--------------|-----------------------------------------------|
//! | Normal       | Default readahead behavior                    |
//! | Sequential   | Aggressive readahead (double window)          |
//! | Random       | Disable readahead (each read is independent)  |
//! | WillNeed     | Initiate immediate prefetch into cache        |
//! | DontNeed     | Hint that cached data can be evicted          |
//!
//! ## Architecture
//!
//! ```text
//! Application → advise(path, Sequential)
//!   → prefetch module stores advice per-path
//!
//! VFS read_file/read_at
//!   → checks prefetch::get_advice(path)
//!   → adjusts readahead window accordingly
//!
//! Application → prefetch(path, offset, len)
//!   → triggers immediate async read into buffer cache
//! ```
//!
//! ## Design Notes
//!
//! - Advice is per-path (not per-handle) for simplicity in the kernel shell.
//!   A production implementation would use per-file-descriptor advice.
//! - WillNeed triggers an actual VFS read to warm the cache.
//! - DontNeed is purely advisory (hint for future cache eviction policy).
//! - Limited to 256 active advice entries (LRU eviction when full).

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use super::path::{Path, PathBuf};
use crate::error::KernelResult;
use crate::serial_println;
use crate::sync::PreemptSpinMutex;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// File access pattern advisory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessAdvice {
    /// Default behavior — moderate readahead.
    Normal,
    /// Sequential access — aggressive readahead (2x window).
    Sequential,
    /// Random access — disable readahead.
    Random,
    /// Will need this data soon — prefetch immediately.
    WillNeed,
    /// Won't need this data — can evict from cache.
    DontNeed,
}

impl AccessAdvice {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Sequential => "sequential",
            Self::Random => "random",
            Self::WillNeed => "willneed",
            Self::DontNeed => "dontneed",
        }
    }

    /// Parse from string.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "normal" | "default" => Some(Self::Normal),
            "sequential" | "seq" => Some(Self::Sequential),
            "random" | "rand" => Some(Self::Random),
            "willneed" | "need" => Some(Self::WillNeed),
            "dontneed" | "noreuse" => Some(Self::DontNeed),
            _ => None,
        }
    }

    /// Readahead multiplier for this advice.
    /// Normal = 1x, Sequential = 2x, Random = 0x.
    pub fn readahead_multiplier(self) -> u32 {
        match self {
            Self::Normal => 1,
            Self::Sequential => 2,
            Self::Random => 0,
            Self::WillNeed => 1,
            Self::DontNeed => 0,
        }
    }
}

/// An active advice entry.
#[derive(Debug, Clone)]
struct AdviceEntry {
    /// A `PathBuf`, not a `String`: the key must be able to hold any name the
    /// filesystem can, and ours allow every byte but `/` and NUL. A `String`
    /// key silently makes advice unsettable for the very files whose names
    /// are unusual. See `design-decisions.md` §261.
    path: PathBuf,
    advice: AccessAdvice,
    /// Nanosecond timestamp when advice was set (for LRU eviction).
    timestamp_ns: u64,
}

/// Prefetch request result.
#[derive(Debug, Clone)]
pub struct PrefetchResult {
    /// Bytes successfully prefetched.
    pub bytes_prefetched: u64,
    /// Whether the data was already in cache.
    pub was_cached: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Maximum number of active advice entries.
const MAX_ENTRIES: usize = 256;

/// Active advice table.
static ADVICE_TABLE: PreemptSpinMutex<Vec<AdviceEntry>> =
    PreemptSpinMutex::named(Vec::new(), b"ADVICE_TABLE");

/// Statistics.
static ADVISE_COUNT: AtomicU64 = AtomicU64::new(0);
static PREFETCH_COUNT: AtomicU64 = AtomicU64::new(0);
static PREFETCH_BYTES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Set access pattern advice for a file.
///
/// The advice remains active until overridden or the entry is evicted
/// by LRU when the table is full.
pub fn advise(path: impl AsRef<Path>, advice: AccessAdvice) {
    let path = path.as_ref();
    ADVISE_COUNT.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();

    let mut table = ADVICE_TABLE.lock();

    // Update existing entry or find free slot.
    for entry in table.iter_mut() {
        if entry.path.as_path() == path {
            entry.advice = advice;
            entry.timestamp_ns = now;
            return;
        }
    }

    // Insert new entry.
    if table.len() >= MAX_ENTRIES {
        // Evict oldest entry (LRU).
        if let Some(oldest_idx) = table
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.timestamp_ns)
            .map(|(i, _)| i)
        {
            table.swap_remove(oldest_idx);
        }
    }

    table.push(AdviceEntry {
        path: path.to_path_buf(),
        advice,
        timestamp_ns: now,
    });
}

/// Get current advice for a path. Returns Normal if no advice is set.
pub fn get_advice(path: impl AsRef<Path>) -> AccessAdvice {
    let path = path.as_ref();
    let table = ADVICE_TABLE.lock();
    for entry in table.iter() {
        if entry.path.as_path() == path {
            return entry.advice;
        }
    }
    AccessAdvice::Normal
}

/// Clear advice for a specific path.
pub fn clear_advice(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    let mut table = ADVICE_TABLE.lock();
    let len_before = table.len();
    table.retain(|e| e.path.as_path() != path);
    table.len() < len_before
}

/// Clear all advice entries.
pub fn clear_all() {
    ADVICE_TABLE.lock().clear();
}

/// Prefetch file data into the buffer cache.
///
/// Reads the specified range (or entire file if offset=0 and len=0)
/// to warm the cache for subsequent reads.
pub fn prefetch(path: impl AsRef<Path>, offset: u64, len: u64) -> KernelResult<PrefetchResult> {
    use crate::fs::Vfs;

    let path = path.as_ref();
    PREFETCH_COUNT.fetch_add(1, Ordering::Relaxed);

    if len == 0 && offset == 0 {
        // Prefetch entire file.
        let data = Vfs::read_file(path)?;
        let bytes = data.len() as u64;
        PREFETCH_BYTES.fetch_add(bytes, Ordering::Relaxed);
        Ok(PrefetchResult {
            bytes_prefetched: bytes,
            was_cached: false, // We can't easily detect this without cache hooks.
        })
    } else {
        // Prefetch specific range.
        let read_len = if len > 1024 * 1024 {
            1024 * 1024
        } else {
            len as usize
        }; // Cap at 1 MiB.
        let data = Vfs::read_at(path, offset, read_len)?;
        let bytes = data.len() as u64;
        PREFETCH_BYTES.fetch_add(bytes, Ordering::Relaxed);
        Ok(PrefetchResult {
            bytes_prefetched: bytes,
            was_cached: false,
        })
    }
}

/// List all active advice entries.
pub fn list_active() -> Vec<(PathBuf, AccessAdvice)> {
    let table = ADVICE_TABLE.lock();
    table.iter().map(|e| (e.path.clone(), e.advice)).collect()
}

/// Quick summary stats.
pub fn stats() -> (u64, u64, u64, usize) {
    let active = ADVICE_TABLE.lock().len();
    (
        ADVISE_COUNT.load(Ordering::Relaxed),
        PREFETCH_COUNT.load(Ordering::Relaxed),
        PREFETCH_BYTES.load(Ordering::Relaxed),
        active,
    )
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Unlike most of its neighbours this suite never wiped the table — its
/// `test_clear` exercises the per-entry clear rather than emptying the
/// store — and it removes its own fixtures one call at a time.  That is a
/// claim nobody re-checks when a test is added to the list below.  Running
/// against a table moved aside for the duration makes "leaves no trace"
/// structural instead: whatever the suite creates goes away with the
/// substitute.  See `crate::fs::selftest`.
pub fn self_test() -> KernelResult<()> {
    // These counters live outside the table, so `with_pristine` cannot
    // see them; save and restore them here so a run leaves no trace.
    let saved_advise_count = ADVISE_COUNT.load(Ordering::Relaxed);
    let saved_prefetch_count = PREFETCH_COUNT.load(Ordering::Relaxed);
    let saved_prefetch_bytes = PREFETCH_BYTES.load(Ordering::Relaxed);
    let result = crate::fs::selftest::with_pristine(&ADVICE_TABLE, Vec::new(), self_test_inner);
    ADVISE_COUNT.store(saved_advise_count, Ordering::Relaxed);
    PREFETCH_COUNT.store(saved_prefetch_count, Ordering::Relaxed);
    PREFETCH_BYTES.store(saved_prefetch_bytes, Ordering::Relaxed);
    result
}

fn self_test_inner() -> KernelResult<()> {
    serial_println!("[prefetch] Running self-test...");

    test_advice_parse();
    test_advise_get();
    test_clear();
    test_non_utf8_path();
    test_prefetch_file();
    test_lru_eviction();
    test_multiplier();

    serial_println!("[prefetch] Self-test passed (7 tests).");
    Ok(())
}

fn test_advice_parse() {
    assert_eq!(
        AccessAdvice::from_name("normal"),
        Some(AccessAdvice::Normal)
    );
    assert_eq!(
        AccessAdvice::from_name("seq"),
        Some(AccessAdvice::Sequential)
    );
    assert_eq!(AccessAdvice::from_name("rand"), Some(AccessAdvice::Random));
    assert_eq!(
        AccessAdvice::from_name("willneed"),
        Some(AccessAdvice::WillNeed)
    );
    assert_eq!(
        AccessAdvice::from_name("dontneed"),
        Some(AccessAdvice::DontNeed)
    );
    assert_eq!(AccessAdvice::from_name("bogus"), None);
    serial_println!("[prefetch]   advice_parse: ok");
}

fn test_advise_get() {
    // Default is Normal.
    assert_eq!(get_advice("/nonexistent"), AccessAdvice::Normal);

    // Set and retrieve.
    advise("/test/seq", AccessAdvice::Sequential);
    assert_eq!(get_advice("/test/seq"), AccessAdvice::Sequential);

    advise("/test/rand", AccessAdvice::Random);
    assert_eq!(get_advice("/test/rand"), AccessAdvice::Random);

    // Update.
    advise("/test/seq", AccessAdvice::Normal);
    assert_eq!(get_advice("/test/seq"), AccessAdvice::Normal);

    // Cleanup.
    clear_advice("/test/seq");
    clear_advice("/test/rand");
    serial_println!("[prefetch]   advise_get: ok");
}

fn test_clear() {
    advise("/test/clear", AccessAdvice::WillNeed);
    assert_eq!(get_advice("/test/clear"), AccessAdvice::WillNeed);

    assert!(clear_advice("/test/clear"));
    assert_eq!(get_advice("/test/clear"), AccessAdvice::Normal);

    // Clear nonexistent returns false.
    assert!(!clear_advice("/test/nonexistent"));
    serial_println!("[prefetch]   clear: ok");
}

/// A path that is not valid UTF-8 must be storable, findable, and clearable.
///
/// This is the whole point of keying the table on `PathBuf` rather than
/// `String`, and it is not observable from any of the tests above: every one
/// of them uses an ASCII name, which a `String` key handles perfectly well.
/// The two names below differ only in a byte that cannot appear in UTF-8, so
/// a table that lost or folded that byte would report the advice set on the
/// first when asked about the second.
fn test_non_utf8_path() {
    let a = Path::new(&b"/tmp/\xFFa"[..]);
    let b = Path::new(&b"/tmp/\xFEa"[..]);

    advise(a, AccessAdvice::Sequential);
    assert_eq!(get_advice(a), AccessAdvice::Sequential);
    // Distinct invalid bytes must not collide -- a lossy key would map both
    // to U+FFFD and make this read back `Sequential`.
    assert_eq!(get_advice(b), AccessAdvice::Normal);

    advise(b, AccessAdvice::Random);
    assert_eq!(get_advice(a), AccessAdvice::Sequential);
    assert_eq!(get_advice(b), AccessAdvice::Random);

    assert!(clear_advice(a));
    assert!(clear_advice(b));
    assert_eq!(get_advice(a), AccessAdvice::Normal);
    serial_println!("[prefetch]   non_utf8_path: ok");
}

fn test_prefetch_file() {
    use crate::fs::Vfs;

    // Create a test file.
    let path = "/tmp/_prefetch_test";
    let data = alloc::vec![0xABu8; 1024];
    Vfs::write_file(path, &data).unwrap();

    // Prefetch entire file.
    let result = prefetch(path, 0, 0);
    assert!(result.is_ok());
    let r = result.unwrap();
    assert_eq!(r.bytes_prefetched, 1024);

    // Prefetch range.
    let result2 = prefetch(path, 0, 512);
    assert!(result2.is_ok());

    let _ = Vfs::remove(path);
    serial_println!("[prefetch]   prefetch_file: ok");
}

fn test_lru_eviction() {
    use alloc::format;

    // Fill the table to capacity.
    let initial_active = ADVICE_TABLE.lock().len();
    let to_add = MAX_ENTRIES.saturating_sub(initial_active) + 1;

    for i in 0..to_add {
        advise(format!("/lru_test/{i}"), AccessAdvice::Sequential);
    }

    // Table should not exceed MAX_ENTRIES.
    let table_size = ADVICE_TABLE.lock().len();
    assert!(table_size <= MAX_ENTRIES);

    // Clean up test entries.
    let mut table = ADVICE_TABLE.lock();
    table.retain(|e| !e.path.starts_with("/lru_test/"));
    drop(table);

    serial_println!("[prefetch]   lru_eviction: ok");
}

fn test_multiplier() {
    assert_eq!(AccessAdvice::Normal.readahead_multiplier(), 1);
    assert_eq!(AccessAdvice::Sequential.readahead_multiplier(), 2);
    assert_eq!(AccessAdvice::Random.readahead_multiplier(), 0);
    assert_eq!(AccessAdvice::WillNeed.readahead_multiplier(), 1);
    assert_eq!(AccessAdvice::DontNeed.readahead_multiplier(), 0);
    serial_println!("[prefetch]   multiplier: ok");
}

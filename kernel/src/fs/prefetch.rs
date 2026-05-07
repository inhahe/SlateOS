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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelResult;
use crate::serial_println;

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
    path: String,
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
static ADVICE_TABLE: spin::Mutex<Vec<AdviceEntry>> = spin::Mutex::new(Vec::new());

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
pub fn advise(path: &str, advice: AccessAdvice) {
    ADVISE_COUNT.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();

    let mut table = ADVICE_TABLE.lock();

    // Update existing entry or find free slot.
    for entry in table.iter_mut() {
        if entry.path == path {
            entry.advice = advice;
            entry.timestamp_ns = now;
            return;
        }
    }

    // Insert new entry.
    if table.len() >= MAX_ENTRIES {
        // Evict oldest entry (LRU).
        if let Some(oldest_idx) = table.iter().enumerate()
            .min_by_key(|(_, e)| e.timestamp_ns)
            .map(|(i, _)| i)
        {
            table.swap_remove(oldest_idx);
        }
    }

    table.push(AdviceEntry {
        path: String::from(path),
        advice,
        timestamp_ns: now,
    });
}

/// Get current advice for a path. Returns Normal if no advice is set.
pub fn get_advice(path: &str) -> AccessAdvice {
    let table = ADVICE_TABLE.lock();
    for entry in table.iter() {
        if entry.path == path {
            return entry.advice;
        }
    }
    AccessAdvice::Normal
}

/// Clear advice for a specific path.
pub fn clear_advice(path: &str) -> bool {
    let mut table = ADVICE_TABLE.lock();
    let len_before = table.len();
    table.retain(|e| e.path != path);
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
pub fn prefetch(path: &str, offset: u64, len: u64) -> KernelResult<PrefetchResult> {
    use crate::fs::Vfs;

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
        let read_len = if len > 1024 * 1024 { 1024 * 1024 } else { len as usize }; // Cap at 1 MiB.
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
pub fn list_active() -> Vec<(String, AccessAdvice)> {
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

pub fn self_test() -> KernelResult<()> {
    serial_println!("[prefetch] Running self-test...");

    test_advice_parse();
    test_advise_get();
    test_clear();
    test_prefetch_file();
    test_lru_eviction();
    test_multiplier();

    serial_println!("[prefetch] Self-test passed (6 tests).");
    Ok(())
}

fn test_advice_parse() {
    assert_eq!(AccessAdvice::from_name("normal"), Some(AccessAdvice::Normal));
    assert_eq!(AccessAdvice::from_name("seq"), Some(AccessAdvice::Sequential));
    assert_eq!(AccessAdvice::from_name("rand"), Some(AccessAdvice::Random));
    assert_eq!(AccessAdvice::from_name("willneed"), Some(AccessAdvice::WillNeed));
    assert_eq!(AccessAdvice::from_name("dontneed"), Some(AccessAdvice::DontNeed));
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
        advise(&format!("/lru_test/{}", i), AccessAdvice::Sequential);
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

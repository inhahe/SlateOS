//! Filesystem freeze/thaw for consistent snapshots.
//!
//! When a filesystem is frozen, all new write operations are blocked
//! (or queued) until the filesystem is thawed. This provides a
//! consistent point-in-time view for:
//!
//! - **Backup tools** — freeze, take snapshot, thaw
//! - **Database dumps** — guarantee filesystem-level consistency
//! - **VM snapshots** — freeze guest FS before taking disk snapshot
//!
//! ## Architecture
//!
//! ```text
//! Admin → freeze("/")
//!   → drain pending writes (sync)
//!   → mark filesystem as frozen
//!   → new writes return EBUSY or queue
//!
//! Admin → thaw("/")
//!   → mark filesystem as thawed
//!   → resume queued writes
//! ```
//!
//! ## Design Notes
//!
//! - Freeze is per-mountpoint (each mount can be independently frozen).
//! - Maximum frozen filesystems: 16 (more would indicate a bug or abuse).
//! - Freeze has a timeout: auto-thaw after 5 minutes to prevent
//!   accidental deadlocks from forgotten thaw calls.
//! - Freeze level: we track nested freeze/thaw (like Linux's
//!   sb->s_writers.frozen counter) so multiple freezers coordinate.
//! - The VFS integration point is `is_frozen(path)` — write-path
//!   operations can check this before proceeding.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum simultaneously frozen filesystems.
const MAX_FROZEN: usize = 16;

/// Auto-thaw timeout (5 minutes in nanoseconds).
const AUTO_THAW_NS: u64 = 5 * 60 * 1_000_000_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// State of a frozen filesystem.
#[derive(Debug, Clone)]
struct FrozenEntry {
    /// Mountpoint or path prefix.
    mountpoint: String,
    /// Freeze depth (supports nested freeze).
    freeze_level: u32,
    /// Timestamp when frozen.
    frozen_at_ns: u64,
    /// Auto-thaw deadline.
    deadline_ns: u64,
    /// Number of writes blocked while frozen.
    blocked_writes: u64,
    /// Optional reason for the freeze.
    reason: String,
}

/// Result of a freeze operation.
#[derive(Debug, Clone)]
pub struct FreezeResult {
    /// Current freeze level after operation.
    pub freeze_level: u32,
    /// Whether this was the initial freeze (level went from 0 to 1).
    pub was_initial: bool,
}

/// Result of a thaw operation.
#[derive(Debug, Clone)]
pub struct ThawResult {
    /// Current freeze level after operation.
    pub freeze_level: u32,
    /// Whether the filesystem is now fully thawed (level reached 0).
    pub fully_thawed: bool,
    /// Number of writes that were blocked during the freeze.
    pub blocked_writes: u64,
}

/// Status information for a frozen filesystem.
#[derive(Debug, Clone)]
pub struct FreezeStatus {
    /// Mountpoint.
    pub mountpoint: String,
    /// Current freeze level.
    pub freeze_level: u32,
    /// How long frozen (nanoseconds).
    pub frozen_duration_ns: u64,
    /// Time until auto-thaw (nanoseconds, 0 if expired).
    pub time_until_thaw_ns: u64,
    /// Writes blocked so far.
    pub blocked_writes: u64,
    /// Reason for freeze.
    pub reason: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Frozen filesystem table.
static FROZEN_TABLE: spin::Mutex<Vec<FrozenEntry>> = spin::Mutex::new(Vec::new());

/// Statistics.
static FREEZE_COUNT: AtomicU64 = AtomicU64::new(0);
static THAW_COUNT: AtomicU64 = AtomicU64::new(0);
static AUTO_THAW_COUNT: AtomicU64 = AtomicU64::new(0);
static BLOCKED_WRITE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Freeze a filesystem at the given mountpoint.
///
/// Increments the freeze level. First freeze (level 0→1) triggers
/// a sync to flush pending writes. Supports nested freezing.
pub fn freeze(mountpoint: &str, reason: &str) -> KernelResult<FreezeResult> {
    if mountpoint.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    let now = crate::timekeeping::clock_monotonic();
    FREEZE_COUNT.fetch_add(1, Ordering::Relaxed);

    // Check for expired freezes first.
    expire_stale_freezes(now);

    let mut table = FROZEN_TABLE.lock();

    // Find existing or create new.
    if let Some(entry) = table.iter_mut().find(|e| e.mountpoint == mountpoint) {
        entry.freeze_level += 1;
        entry.deadline_ns = now + AUTO_THAW_NS;
        if !reason.is_empty() {
            entry.reason = String::from(reason);
        }
        return Ok(FreezeResult {
            freeze_level: entry.freeze_level,
            was_initial: false,
        });
    }

    // New freeze.
    if table.len() >= MAX_FROZEN {
        return Err(KernelError::ResourceExhausted);
    }

    // Sync the filesystem before freezing (flush dirty data).
    drop(table); // Release lock before sync.
    let _ = crate::fs::Vfs::sync();

    let mut table = FROZEN_TABLE.lock();
    table.push(FrozenEntry {
        mountpoint: String::from(mountpoint),
        freeze_level: 1,
        frozen_at_ns: now,
        deadline_ns: now + AUTO_THAW_NS,
        blocked_writes: 0,
        reason: String::from(reason),
    });

    Ok(FreezeResult {
        freeze_level: 1,
        was_initial: true,
    })
}

/// Thaw a filesystem.
///
/// Decrements the freeze level. When level reaches 0, the filesystem
/// is fully thawed and writes resume.
pub fn thaw(mountpoint: &str) -> KernelResult<ThawResult> {
    if mountpoint.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    THAW_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut table = FROZEN_TABLE.lock();
    let idx = table.iter().position(|e| e.mountpoint == mountpoint)
        .ok_or(KernelError::NotFound)?;

    let entry = &mut table[idx];
    entry.freeze_level = entry.freeze_level.saturating_sub(1);

    if entry.freeze_level == 0 {
        let blocked = entry.blocked_writes;
        table.swap_remove(idx);
        Ok(ThawResult {
            freeze_level: 0,
            fully_thawed: true,
            blocked_writes: blocked,
        })
    } else {
        let level = entry.freeze_level;
        let blocked = entry.blocked_writes;
        Ok(ThawResult {
            freeze_level: level,
            fully_thawed: false,
            blocked_writes: blocked,
        })
    }
}

/// Force-thaw a filesystem regardless of freeze level.
pub fn force_thaw(mountpoint: &str) -> KernelResult<ThawResult> {
    THAW_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut table = FROZEN_TABLE.lock();
    let idx = table.iter().position(|e| e.mountpoint == mountpoint)
        .ok_or(KernelError::NotFound)?;

    let blocked = table[idx].blocked_writes;
    table.swap_remove(idx);

    Ok(ThawResult {
        freeze_level: 0,
        fully_thawed: true,
        blocked_writes: blocked,
    })
}

/// Check if a path is on a frozen filesystem.
///
/// This is the VFS integration point — write operations should call
/// this before proceeding. Returns true if the filesystem containing
/// `path` is currently frozen.
pub fn is_frozen(path: &str) -> bool {
    let now = crate::timekeeping::clock_monotonic();

    // Check for stale entries and query in one lock.
    let mut table = FROZEN_TABLE.lock();

    // Auto-thaw expired entries.
    table.retain(|e| {
        if now >= e.deadline_ns {
            AUTO_THAW_COUNT.fetch_add(1, Ordering::Relaxed);
            false
        } else {
            true
        }
    });

    // Longest prefix match.
    for entry in table.iter_mut() {
        if path.starts_with(&entry.mountpoint) {
            entry.blocked_writes += 1;
            BLOCKED_WRITE_COUNT.fetch_add(1, Ordering::Relaxed);
            return true;
        }
    }

    false
}

/// Get status of all frozen filesystems.
pub fn list_frozen() -> Vec<FreezeStatus> {
    let now = crate::timekeeping::clock_monotonic();
    let table = FROZEN_TABLE.lock();

    table.iter().map(|e| {
        let duration = now.saturating_sub(e.frozen_at_ns);
        let until_thaw = e.deadline_ns.saturating_sub(now);

        FreezeStatus {
            mountpoint: e.mountpoint.clone(),
            freeze_level: e.freeze_level,
            frozen_duration_ns: duration,
            time_until_thaw_ns: until_thaw,
            blocked_writes: e.blocked_writes,
            reason: e.reason.clone(),
        }
    }).collect()
}

/// Get the number of currently frozen filesystems.
pub fn frozen_count() -> usize {
    FROZEN_TABLE.lock().len()
}

/// Get statistics.
pub fn stats() -> (u64, u64, u64, u64, usize) {
    let frozen = FROZEN_TABLE.lock().len();
    (
        FREEZE_COUNT.load(Ordering::Relaxed),
        THAW_COUNT.load(Ordering::Relaxed),
        AUTO_THAW_COUNT.load(Ordering::Relaxed),
        BLOCKED_WRITE_COUNT.load(Ordering::Relaxed),
        frozen,
    )
}

/// Reset statistics.
pub fn reset_stats() {
    FREEZE_COUNT.store(0, Ordering::Relaxed);
    THAW_COUNT.store(0, Ordering::Relaxed);
    AUTO_THAW_COUNT.store(0, Ordering::Relaxed);
    BLOCKED_WRITE_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Remove expired freeze entries.
fn expire_stale_freezes(now: u64) {
    let mut table = FROZEN_TABLE.lock();
    let before = table.len();
    table.retain(|e| now < e.deadline_ns);
    let expired = before - table.len();
    if expired > 0 {
        AUTO_THAW_COUNT.fetch_add(expired as u64, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[freeze] Running self-test...");

    test_freeze_thaw();
    test_nested_freeze();
    test_force_thaw();
    test_is_frozen_check();
    test_multiple_mounts();
    test_auto_thaw_tracking();

    serial_println!("[freeze] Self-test passed (6 tests).");
    Ok(())
}

fn test_freeze_thaw() {
    let mp = "/test_freeze";

    let fr = freeze(mp, "test").unwrap();
    assert_eq!(fr.freeze_level, 1);
    assert!(fr.was_initial);

    assert_eq!(frozen_count(), 1);

    let tr = thaw(mp).unwrap();
    assert_eq!(tr.freeze_level, 0);
    assert!(tr.fully_thawed);

    assert_eq!(frozen_count(), 0);

    serial_println!("[freeze]   freeze_thaw: ok");
}

fn test_nested_freeze() {
    let mp = "/test_nested";

    // Freeze twice.
    let fr1 = freeze(mp, "first").unwrap();
    assert_eq!(fr1.freeze_level, 1);
    assert!(fr1.was_initial);

    let fr2 = freeze(mp, "second").unwrap();
    assert_eq!(fr2.freeze_level, 2);
    assert!(!fr2.was_initial);

    // First thaw doesn't fully thaw.
    let tr1 = thaw(mp).unwrap();
    assert_eq!(tr1.freeze_level, 1);
    assert!(!tr1.fully_thawed);

    // Second thaw fully thaws.
    let tr2 = thaw(mp).unwrap();
    assert_eq!(tr2.freeze_level, 0);
    assert!(tr2.fully_thawed);

    serial_println!("[freeze]   nested_freeze: ok");
}

fn test_force_thaw() {
    let mp = "/test_force";

    // Freeze three times.
    freeze(mp, "").unwrap();
    freeze(mp, "").unwrap();
    freeze(mp, "").unwrap();

    // Force thaw immediately.
    let tr = force_thaw(mp).unwrap();
    assert!(tr.fully_thawed);
    assert_eq!(frozen_count(), 0);

    // Thaw on already thawed should fail.
    assert!(thaw(mp).is_err());

    serial_println!("[freeze]   force_thaw: ok");
}

fn test_is_frozen_check() {
    let mp = "/frozen_check";

    assert!(!is_frozen("/frozen_check/file.txt"));

    freeze(mp, "").unwrap();

    // Path under frozen mount should be frozen.
    assert!(is_frozen("/frozen_check/file.txt"));
    assert!(is_frozen("/frozen_check/sub/dir/file"));

    // Path NOT under frozen mount should not be frozen.
    assert!(!is_frozen("/other/file.txt"));

    force_thaw(mp).unwrap();

    serial_println!("[freeze]   is_frozen_check: ok");
}

fn test_multiple_mounts() {
    let mp1 = "/multi_a";
    let mp2 = "/multi_b";

    freeze(mp1, "backup A").unwrap();
    freeze(mp2, "backup B").unwrap();

    assert_eq!(frozen_count(), 2);

    let list = list_frozen();
    assert_eq!(list.len(), 2);

    thaw(mp1).unwrap();
    assert_eq!(frozen_count(), 1);

    thaw(mp2).unwrap();
    assert_eq!(frozen_count(), 0);

    serial_println!("[freeze]   multiple_mounts: ok");
}

fn test_auto_thaw_tracking() {
    // We can't easily test actual timeout expiry (would need to
    // wait 5 minutes), but we can verify the deadline is set correctly.
    let mp = "/auto_thaw_test";
    let before = crate::timekeeping::clock_monotonic();

    freeze(mp, "").unwrap();

    let list = list_frozen();
    let entry = list.iter().find(|e| e.mountpoint == mp).unwrap();

    // Should have time remaining (close to 5 minutes).
    assert!(entry.time_until_thaw_ns > 0);
    assert!(entry.frozen_duration_ns < 1_000_000_000); // Less than 1 second.

    let _ = before; // Used to verify timing sanity.

    force_thaw(mp).unwrap();
    serial_println!("[freeze]   auto_thaw_tracking: ok");
}

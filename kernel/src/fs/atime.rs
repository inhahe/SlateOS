//! Access time (atime) update policy management.
//!
//! Controls when file access timestamps (atime) are updated during read
//! operations.  Naive atime-on-every-read is a significant I/O overhead
//! for workloads that read files frequently (compilers, indexers, etc.).
//!
//! ## Policies
//!
//! | Policy    | Description                                           |
//! |-----------|-------------------------------------------------------|
//! | Always    | Update atime on every read (Linux `strictatime`)      |
//! | Relative  | Update only if atime < mtime or atime > 24h old       |
//! | NoAtime   | Never update atime on reads                           |
//! | LazyDay   | Update at most once per calendar day                  |
//!
//! The default is `Relative` (relatime) which balances correctness with
//! performance.  Programs that need accurate last-access times (e.g.,
//! backup tools using atime to detect unread files) still work correctly
//! since atime is updated when the file hasn't been accessed since its
//! last modification.
//!
//! ## Architecture
//!
//! ```text
//! VFS read operation
//!   → atime::should_update(path, current_meta)
//!     → checks policy + conditions
//!     → returns true/false
//!   → if true, update atime in filesystem
//! ```
//!
//! Per-mount override is supported: individual filesystems can have
//! stricter or more relaxed atime policies than the global default.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Atime update policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AtimePolicy {
    /// Update atime on every read (traditional Unix behavior).
    /// High I/O overhead but fully accurate access times.
    Always = 0,
    /// Update atime only if older than mtime or more than 24 hours old.
    /// Default — good balance of correctness and performance.
    Relative = 1,
    /// Never update atime on reads.  Lowest overhead but access times
    /// become stale.  Useful for SSDs where write amplification matters.
    NoAtime = 2,
    /// Update atime at most once per calendar day (24-hour window).
    /// Good for backup tools that check "accessed today" patterns.
    LazyDay = 3,
}

impl AtimePolicy {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Relative => "relatime",
            Self::NoAtime => "noatime",
            Self::LazyDay => "lazyday",
        }
    }

    /// Parse from string.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "always" | "strict" | "strictatime" | "0" => Some(Self::Always),
            "relatime" | "relative" | "1" => Some(Self::Relative),
            "noatime" | "none" | "2" => Some(Self::NoAtime),
            "lazyday" | "lazy" | "3" => Some(Self::LazyDay),
            _ => None,
        }
    }

    /// All policies.
    pub const ALL: &'static [AtimePolicy] = &[
        Self::Always, Self::Relative, Self::NoAtime, Self::LazyDay,
    ];
}

/// Per-mount atime override entry.
#[derive(Debug, Clone)]
pub struct MountOverride {
    /// Mount path prefix (e.g., "/tmp", "/home").
    pub mount_path: String,
    /// Policy override for this mount.
    pub policy: AtimePolicy,
}

/// Atime module statistics.
#[derive(Debug, Clone, Default)]
pub struct AtimeStats {
    /// Total atime check calls.
    pub checks: u64,
    /// Times atime was updated (should_update returned true).
    pub updates: u64,
    /// Times atime update was skipped.
    pub skipped: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Global default policy (stored as u8 discriminant).
static GLOBAL_POLICY: AtomicU64 = AtomicU64::new(AtimePolicy::Relative as u64);

/// Counters.
static CHECK_COUNT: AtomicU64 = AtomicU64::new(0);
static UPDATE_COUNT: AtomicU64 = AtomicU64::new(0);
static SKIP_COUNT: AtomicU64 = AtomicU64::new(0);

/// Per-mount overrides (limited capacity, behind a spinlock since
/// modifications are rare and only happen during mount/remount).
static MOUNT_OVERRIDES: spin::Mutex<Vec<MountOverride>> = spin::Mutex::new(Vec::new());

/// 24 hours in nanoseconds.
const DAY_NS: u64 = 24 * 60 * 60 * 1_000_000_000;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Get the global atime policy.
pub fn global_policy() -> AtimePolicy {
    let val = GLOBAL_POLICY.load(Ordering::Relaxed);
    match val {
        0 => AtimePolicy::Always,
        1 => AtimePolicy::Relative,
        2 => AtimePolicy::NoAtime,
        3 => AtimePolicy::LazyDay,
        _ => AtimePolicy::Relative, // Fallback.
    }
}

/// Set the global atime policy.
pub fn set_global_policy(policy: AtimePolicy) {
    GLOBAL_POLICY.store(policy as u64, Ordering::Relaxed);
    serial_println!("[atime] Global policy set to: {}", policy.label());
}

/// Get the effective policy for a given path (checks mount overrides first).
pub fn effective_policy(path: &str) -> AtimePolicy {
    let overrides = MOUNT_OVERRIDES.lock();
    // Longest prefix match.
    let mut best: Option<&MountOverride> = None;
    let mut best_len = 0;
    for entry in overrides.iter() {
        if path.starts_with(&entry.mount_path) && entry.mount_path.len() > best_len {
            best = Some(entry);
            best_len = entry.mount_path.len();
        }
    }
    match best {
        Some(ovr) => ovr.policy,
        None => global_policy(),
    }
}

/// Add a per-mount atime policy override.
pub fn add_override(mount_path: &str, policy: AtimePolicy) {
    let mut overrides = MOUNT_OVERRIDES.lock();
    // Update existing or insert new.
    for entry in overrides.iter_mut() {
        if entry.mount_path == mount_path {
            entry.policy = policy;
            serial_println!("[atime] Updated override: {} → {}", mount_path, policy.label());
            return;
        }
    }
    if overrides.len() < 64 {
        overrides.push(MountOverride {
            mount_path: String::from(mount_path),
            policy,
        });
        serial_println!("[atime] Added override: {} → {}", mount_path, policy.label());
    }
}

/// Remove a per-mount override.
pub fn remove_override(mount_path: &str) -> bool {
    let mut overrides = MOUNT_OVERRIDES.lock();
    let len_before = overrides.len();
    overrides.retain(|e| e.mount_path != mount_path);
    overrides.len() < len_before
}

/// List all mount overrides.
pub fn list_overrides() -> Vec<MountOverride> {
    MOUNT_OVERRIDES.lock().clone()
}

/// Determine whether atime should be updated for a file access.
///
/// Arguments:
/// - `path`: file path (for per-mount policy lookup)
/// - `current_atime_ns`: file's current atime in nanoseconds
/// - `current_mtime_ns`: file's current mtime in nanoseconds
///
/// Returns `true` if atime should be updated to `now`.
pub fn should_update(path: &str, current_atime_ns: u64, current_mtime_ns: u64) -> bool {
    CHECK_COUNT.fetch_add(1, Ordering::Relaxed);

    let policy = effective_policy(path);
    let now = crate::timekeeping::clock_monotonic();

    let result = match policy {
        AtimePolicy::Always => true,
        AtimePolicy::NoAtime => false,
        AtimePolicy::Relative => {
            // Update if atime < mtime (file modified since last access)
            // OR if atime is more than 24 hours old.
            current_atime_ns < current_mtime_ns
                || now.saturating_sub(current_atime_ns) > DAY_NS
        }
        AtimePolicy::LazyDay => {
            // Update only if last update was more than 24 hours ago.
            now.saturating_sub(current_atime_ns) > DAY_NS
        }
    };

    if result {
        UPDATE_COUNT.fetch_add(1, Ordering::Relaxed);
    } else {
        SKIP_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    result
}

/// Get statistics.
pub fn stats() -> AtimeStats {
    AtimeStats {
        checks: CHECK_COUNT.load(Ordering::Relaxed),
        updates: UPDATE_COUNT.load(Ordering::Relaxed),
        skipped: SKIP_COUNT.load(Ordering::Relaxed),
    }
}

/// Reset statistics counters.
pub fn reset_stats() {
    CHECK_COUNT.store(0, Ordering::Relaxed);
    UPDATE_COUNT.store(0, Ordering::Relaxed);
    SKIP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[atime] Running self-test...");

    test_policy_parse();
    test_global_policy();
    test_should_update_always();
    test_should_update_noatime();
    test_should_update_relatime();
    test_overrides();

    serial_println!("[atime] Self-test passed (6 tests).");
    Ok(())
}

fn test_policy_parse() {
    assert_eq!(AtimePolicy::from_name("always"), Some(AtimePolicy::Always));
    assert_eq!(AtimePolicy::from_name("relatime"), Some(AtimePolicy::Relative));
    assert_eq!(AtimePolicy::from_name("noatime"), Some(AtimePolicy::NoAtime));
    assert_eq!(AtimePolicy::from_name("lazyday"), Some(AtimePolicy::LazyDay));
    assert_eq!(AtimePolicy::from_name("bogus"), None);
    serial_println!("[atime]   policy_parse: ok");
}

fn test_global_policy() {
    let original = global_policy();
    set_global_policy(AtimePolicy::NoAtime);
    assert_eq!(global_policy(), AtimePolicy::NoAtime);
    set_global_policy(AtimePolicy::Always);
    assert_eq!(global_policy(), AtimePolicy::Always);
    // Restore.
    set_global_policy(original);
    serial_println!("[atime]   global_policy: ok");
}

fn test_should_update_always() {
    set_global_policy(AtimePolicy::Always);
    let now = crate::timekeeping::clock_monotonic();
    // Always should return true regardless of times.
    assert!(should_update("/test", now, now));
    assert!(should_update("/test", now.saturating_sub(1000), now));
    set_global_policy(AtimePolicy::Relative); // Restore.
    serial_println!("[atime]   should_update_always: ok");
}

fn test_should_update_noatime() {
    set_global_policy(AtimePolicy::NoAtime);
    let now = crate::timekeeping::clock_monotonic();
    // NoAtime should always return false.
    assert!(!should_update("/test", 0, now));
    assert!(!should_update("/test", now, now));
    set_global_policy(AtimePolicy::Relative); // Restore.
    serial_println!("[atime]   should_update_noatime: ok");
}

fn test_should_update_relatime() {
    set_global_policy(AtimePolicy::Relative);
    let now = crate::timekeeping::clock_monotonic();

    // atime < mtime: should update (file modified since last access).
    let atime = now.saturating_sub(1_000_000_000); // 1s ago
    let mtime = now; // just modified
    assert!(should_update("/test", atime, mtime));

    // atime > mtime but atime is recent: should NOT update.
    let atime_recent = now.saturating_sub(100_000_000); // 100ms ago
    let mtime_old = now.saturating_sub(2_000_000_000); // 2s ago
    assert!(!should_update("/test", atime_recent, mtime_old));

    // atime > 24h old: should update regardless of mtime.
    let atime_stale = now.saturating_sub(DAY_NS + 1);
    assert!(should_update("/test", atime_stale, mtime_old));

    serial_println!("[atime]   should_update_relatime: ok");
}

fn test_overrides() {
    set_global_policy(AtimePolicy::Relative);

    // Add override for /tmp.
    add_override("/tmp", AtimePolicy::NoAtime);

    // Path under /tmp should use noatime.
    assert_eq!(effective_policy("/tmp/foo.txt"), AtimePolicy::NoAtime);

    // Path outside /tmp should use global.
    assert_eq!(effective_policy("/home/test"), AtimePolicy::Relative);

    // Remove override.
    assert!(remove_override("/tmp"));
    assert_eq!(effective_policy("/tmp/foo.txt"), AtimePolicy::Relative);

    serial_println!("[atime]   overrides: ok");
}

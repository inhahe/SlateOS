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

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use super::path::{Path, PathBuf};
use crate::serial_println;
use crate::sync::PreemptSpinMutex;

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
    pub const ALL: &'static [AtimePolicy] =
        &[Self::Always, Self::Relative, Self::NoAtime, Self::LazyDay];
}

/// Per-mount atime override entry.
#[derive(Debug, Clone)]
pub struct MountOverride {
    /// Mount path prefix (e.g., "/tmp", "/home").
    ///
    /// A `PathBuf`, not a `String`: this is a filesystem path, and ours
    /// admit every byte but `/` and NUL. A `String` field cannot hold a
    /// legal mountpoint whose name is not UTF-8, so the override for it
    /// would be unsettable — or worse, would fold onto a *different*
    /// mountpoint's key and silently change its policy.  See
    /// `design-decisions.md` §261.
    pub mount_path: PathBuf,
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
static MOUNT_OVERRIDES: PreemptSpinMutex<Vec<MountOverride>> = PreemptSpinMutex::named(Vec::new(), b"MOUNT_OVERRIDES");

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
///
/// When several overrides cover the path, the *deepest* mountpoint wins —
/// an override on `/home/user` beats one on `/home`, which is what makes a
/// nested mount able to relax or tighten its parent's policy.
pub fn effective_policy(path: impl AsRef<Path>) -> AtimePolicy {
    let path = path.as_ref();
    let overrides = MOUNT_OVERRIDES.lock();
    // Longest (deepest) prefix match.
    let mut best: Option<&MountOverride> = None;
    let mut best_depth = 0usize;
    for entry in overrides.iter() {
        // The canonical subtree predicate (see `fs::pathutil`) rather than a
        // hand-rolled prefix test plus a separator probe.  Besides being
        // byte-clean, it is more correct: the old form indexed by byte
        // offset, so a mountpoint recorded with a trailing slash — `/tmp/`
        // — probed the byte *after* its own separator and therefore matched
        // no child at all, silently reverting every file under it to the
        // global policy.
        if !crate::fs::pathutil::path_in_subtree(path, entry.mount_path.as_path()) {
            continue;
        }
        // Depth in components, not bytes: `/tmp` and `/tmp/` name the same
        // mountpoint at the same depth, so a trailing slash must not change
        // which of two overlapping overrides wins.
        let depth = entry.mount_path.as_path().components().count();
        // `best.is_none()` is load-bearing for a root override: `/` has zero
        // components, so a bare `depth > best_depth` would never select it.
        if best.is_none() || depth > best_depth {
            best = Some(entry);
            best_depth = depth;
        }
    }
    match best {
        Some(ovr) => ovr.policy,
        None => global_policy(),
    }
}

/// Add a per-mount atime policy override.
pub fn add_override(mount_path: impl AsRef<Path>, policy: AtimePolicy) {
    let mount_path = mount_path.as_ref();
    let mut overrides = MOUNT_OVERRIDES.lock();
    // Update existing or insert new.
    for entry in overrides.iter_mut() {
        if entry.mount_path.as_path() == mount_path {
            entry.policy = policy;
            serial_println!(
                "[atime] Updated override: {} → {}",
                mount_path.display(),
                policy.label()
            );
            return;
        }
    }
    if overrides.len() < 64 {
        overrides.push(MountOverride {
            mount_path: mount_path.to_path_buf(),
            policy,
        });
        serial_println!(
            "[atime] Added override: {} → {}",
            mount_path.display(),
            policy.label()
        );
    }
}

/// Remove a per-mount override.
///
/// Matches the recorded mountpoint byte-for-byte, so an override added as
/// `/tmp/` is not removed by `/tmp`.  Exact-key removal is deliberate: the
/// caller that adds an override is the mount path that owns it, and it
/// passes back the same bytes.
pub fn remove_override(mount_path: impl AsRef<Path>) -> bool {
    let mount_path = mount_path.as_ref();
    let mut overrides = MOUNT_OVERRIDES.lock();
    let len_before = overrides.len();
    overrides.retain(|e| e.mount_path.as_path() != mount_path);
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
pub fn should_update(
    path: impl AsRef<Path>,
    current_atime_ns: u64,
    current_mtime_ns: u64,
) -> bool {
    CHECK_COUNT.fetch_add(1, Ordering::Relaxed);

    let policy = effective_policy(path);
    let now = crate::timekeeping::clock_monotonic();

    let result = match policy {
        AtimePolicy::Always => true,
        AtimePolicy::NoAtime => false,
        AtimePolicy::Relative => {
            // Update if atime < mtime (file modified since last access)
            // OR if atime is more than 24 hours old.
            current_atime_ns < current_mtime_ns || now.saturating_sub(current_atime_ns) > DAY_NS
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
    test_non_utf8_mountpoint();
    test_trailing_slash_mountpoint();
    test_deepest_override_wins();

    serial_println!("[atime] Self-test passed (9 tests).");
    Ok(())
}

fn test_policy_parse() {
    assert_eq!(AtimePolicy::from_name("always"), Some(AtimePolicy::Always));
    assert_eq!(
        AtimePolicy::from_name("relatime"),
        Some(AtimePolicy::Relative)
    );
    assert_eq!(
        AtimePolicy::from_name("noatime"),
        Some(AtimePolicy::NoAtime)
    );
    assert_eq!(
        AtimePolicy::from_name("lazyday"),
        Some(AtimePolicy::LazyDay)
    );
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

/// Two mountpoints differing only in a byte that cannot appear in UTF-8
/// must key separate overrides.  A lossy `String` key mapped both to the
/// same U+FFFD-bearing name, so setting a policy on one silently changed
/// the other's — and `remove_override` on one removed the other.
fn test_non_utf8_mountpoint() {
    set_global_policy(AtimePolicy::Relative);

    let a = Path::new(&b"/mnt/\xFFdisk"[..]);
    let b = Path::new(&b"/mnt/\xFEdisk"[..]);

    add_override(a, AtimePolicy::NoAtime);
    assert_eq!(effective_policy(a), AtimePolicy::NoAtime);
    assert_eq!(
        effective_policy(Path::new(&b"/mnt/\xFFdisk/file"[..])),
        AtimePolicy::NoAtime
    );
    // The sibling must be untouched: with a lossy key it would read back
    // `NoAtime` here.
    assert_eq!(effective_policy(b), AtimePolicy::Relative);

    add_override(b, AtimePolicy::Always);
    assert_eq!(effective_policy(a), AtimePolicy::NoAtime);
    assert_eq!(effective_policy(b), AtimePolicy::Always);

    assert!(remove_override(a));
    // Removing one must not remove the other.
    assert_eq!(effective_policy(b), AtimePolicy::Always);
    assert!(remove_override(b));
    assert_eq!(effective_policy(a), AtimePolicy::Relative);

    serial_println!("[atime]   non_utf8_mountpoint: ok");
}

/// A mountpoint recorded with a trailing slash must still cover its
/// children, and must still *not* cover a sibling that merely shares its
/// spelling as a byte prefix.  The old hand-rolled predicate got the first
/// wrong (it probed the byte after the trailing `/`, so no child matched)
/// and only got the second right by accident.
fn test_trailing_slash_mountpoint() {
    set_global_policy(AtimePolicy::Relative);

    add_override("/atslash/", AtimePolicy::NoAtime);

    // Children are covered.
    assert_eq!(effective_policy("/atslash/data"), AtimePolicy::NoAtime);
    assert_eq!(effective_policy("/atslash/a/b/c"), AtimePolicy::NoAtime);
    // The mountpoint itself, spelled either way.
    assert_eq!(effective_policy("/atslash"), AtimePolicy::NoAtime);
    assert_eq!(effective_policy("/atslash/"), AtimePolicy::NoAtime);
    // A sibling sharing the byte prefix is NOT covered.
    assert_eq!(effective_policy("/atslashed"), AtimePolicy::Relative);
    assert_eq!(effective_policy("/atslashed/x"), AtimePolicy::Relative);

    assert!(remove_override("/atslash/"));
    assert_eq!(effective_policy("/atslash/data"), AtimePolicy::Relative);

    serial_println!("[atime]   trailing_slash_mountpoint: ok");
}

/// Overlapping overrides resolve to the deepest one, and a trailing slash
/// must not change which wins — depth is counted in components, not bytes.
fn test_deepest_override_wins() {
    set_global_policy(AtimePolicy::Relative);

    add_override("/atdeep", AtimePolicy::NoAtime);
    add_override("/atdeep/inner/", AtimePolicy::Always);

    assert_eq!(effective_policy("/atdeep/file"), AtimePolicy::NoAtime);
    assert_eq!(effective_policy("/atdeep/inner"), AtimePolicy::Always);
    assert_eq!(effective_policy("/atdeep/inner/file"), AtimePolicy::Always);
    // Insertion order must not matter, so check the shallower one again
    // after the deeper one exists.
    assert_eq!(effective_policy("/atdeep/other/x"), AtimePolicy::NoAtime);

    assert!(remove_override("/atdeep/inner/"));
    assert_eq!(effective_policy("/atdeep/inner/file"), AtimePolicy::NoAtime);
    assert!(remove_override("/atdeep"));
    assert_eq!(effective_policy("/atdeep/file"), AtimePolicy::Relative);

    serial_println!("[atime]   deepest_override_wins: ok");
}

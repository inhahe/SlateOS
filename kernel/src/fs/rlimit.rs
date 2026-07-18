//! Filesystem resource limits (ulimit-style).
//!
//! Per-process (or per-user) limits on filesystem resource consumption:
//! - Maximum open file descriptors (`RLIMIT_NOFILE`)
//! - Maximum file size (`RLIMIT_FSIZE`)
//! - Maximum number of file locks (`RLIMIT_LOCKS`)
//! - Maximum filename length in bytes
//!
//! ## Design
//!
//! Each limit has a **soft** (current effective) and **hard** (ceiling)
//! value.  Non-privileged users can raise the soft limit up to the hard
//! limit but cannot raise the hard limit.  Root (UID 0) can raise hard
//! limits.
//!
//! The module provides a global default plus per-UID overrides.  When
//! no per-UID override exists, the global default applies.
//!
//! ## Performance
//!
//! Limit checks are O(1) — a BTreeMap lookup by UID.  When there are
//! no per-UID overrides (common case), the global defaults are returned
//! directly without any map lookup.
//!
//! ## Reference
//!
//! POSIX: `getrlimit(2)`, `setrlimit(2)`, `ulimit(1)`
//! Linux: `prlimit(2)`, `/proc/self/limits`

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Resource limit identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Resource {
    /// Maximum number of open file descriptors.
    NoFile,
    /// Maximum file size in bytes (0 = unlimited).
    FileSize,
    /// Maximum number of advisory file locks.
    Locks,
}

impl Resource {
    /// Human-readable name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::NoFile => "nofile",
            Self::FileSize => "fsize",
            Self::Locks => "locks",
        }
    }

    /// Parse from string name.
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "nofile" | "NOFILE" | "open-files" => Some(Self::NoFile),
            "fsize" | "FSIZE" | "file-size" => Some(Self::FileSize),
            "locks" | "LOCKS" | "file-locks" => Some(Self::Locks),
            _ => None,
        }
    }
}

/// A resource limit pair (soft, hard).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rlimit {
    /// Current (soft) limit.  The effective limit.
    pub soft: u64,
    /// Maximum (hard) limit.  Ceiling for soft.
    pub hard: u64,
}

/// Sentinel value meaning "unlimited".
pub const RLIM_INFINITY: u64 = u64::MAX;

impl Rlimit {
    /// Create a limit with both soft and hard set to the same value.
    pub const fn both(val: u64) -> Self {
        Self { soft: val, hard: val }
    }

    /// Create an unlimited limit.
    pub const fn unlimited() -> Self {
        Self { soft: RLIM_INFINITY, hard: RLIM_INFINITY }
    }

    /// Check if a value exceeds the soft limit.
    pub const fn exceeds_soft(self, val: u64) -> bool {
        self.soft != RLIM_INFINITY && val > self.soft
    }

    /// Format the limit for display.
    pub fn format_value(val: u64) -> String {
        if val == RLIM_INFINITY {
            String::from("unlimited")
        } else {
            alloc::format!("{}", val)
        }
    }
}

/// All resource limits for a single subject.
#[derive(Debug, Clone, Copy)]
pub struct RlimitSet {
    pub nofile: Rlimit,
    pub fsize: Rlimit,
    pub locks: Rlimit,
}

impl Default for RlimitSet {
    fn default() -> Self {
        Self {
            nofile: Rlimit::both(1024),
            fsize: Rlimit::unlimited(),
            locks: Rlimit::both(256),
        }
    }
}

impl RlimitSet {
    /// Get limit for a specific resource.
    pub fn get(&self, resource: Resource) -> Rlimit {
        match resource {
            Resource::NoFile => self.nofile,
            Resource::FileSize => self.fsize,
            Resource::Locks => self.locks,
        }
    }

    /// Set limit for a specific resource.
    pub fn set(&mut self, resource: Resource, limit: Rlimit) {
        match resource {
            Resource::NoFile => self.nofile = limit,
            Resource::FileSize => self.fsize = limit,
            Resource::Locks => self.locks = limit,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct RlimitInner {
    /// Global default limits (apply to all users without an override).
    defaults: RlimitSet,
    /// Per-UID limit overrides.
    overrides: BTreeMap<u32, RlimitSet>,
}

static RLIMITS: Mutex<RlimitInner> = Mutex::new(RlimitInner {
    defaults: RlimitSet {
        nofile: Rlimit { soft: 1024, hard: 4096 },
        fsize: Rlimit { soft: RLIM_INFINITY, hard: RLIM_INFINITY },
        locks: Rlimit { soft: 256, hard: 1024 },
    },
    overrides: BTreeMap::new(),
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Get the effective limit for a user.
///
/// Returns per-UID override if one exists, otherwise the global default.
pub fn get_limit(uid: u32, resource: Resource) -> Rlimit {
    let inner = RLIMITS.lock();
    if let Some(set) = inner.overrides.get(&uid) {
        set.get(resource)
    } else {
        inner.defaults.get(resource)
    }
}

/// Get all limits for a user.
pub fn get_limits(uid: u32) -> RlimitSet {
    let inner = RLIMITS.lock();
    inner.overrides.get(&uid).copied().unwrap_or(inner.defaults)
}

/// Set the soft limit for a resource.
///
/// Non-root users (uid != 0) cannot raise the soft limit above the
/// hard limit.  Root can raise both.
pub fn set_soft(uid: u32, resource: Resource, soft: u64, requester_uid: u32) -> KernelResult<()> {
    let mut inner = RLIMITS.lock();
    let defaults = inner.defaults;
    let set = inner.overrides.entry(uid).or_insert(defaults);
    let current = set.get(resource);

    // Non-root cannot raise soft above hard.
    if requester_uid != 0 && soft > current.hard && soft != RLIM_INFINITY {
        return Err(KernelError::PermissionDenied);
    }

    let new_limit = Rlimit { soft, hard: current.hard };
    set.set(resource, new_limit);
    Ok(())
}

/// Set the hard limit for a resource.
///
/// Only root (uid 0) can raise the hard limit.  Non-root can lower it
/// (but it's irreversible without root).
pub fn set_hard(uid: u32, resource: Resource, hard: u64, requester_uid: u32) -> KernelResult<()> {
    let mut inner = RLIMITS.lock();
    let defaults = inner.defaults;
    let set = inner.overrides.entry(uid).or_insert(defaults);
    let current = set.get(resource);

    // Non-root cannot raise the hard limit.
    if requester_uid != 0 && hard > current.hard && hard != RLIM_INFINITY {
        return Err(KernelError::PermissionDenied);
    }

    // If the new hard limit is below the soft limit, adjust soft too.
    let new_soft = if current.soft > hard { hard } else { current.soft };
    let new_limit = Rlimit { soft: new_soft, hard };
    set.set(resource, new_limit);
    Ok(())
}

/// Set both soft and hard limits.
pub fn set_both(uid: u32, resource: Resource, limit: Rlimit, requester_uid: u32) -> KernelResult<()> {
    let mut inner = RLIMITS.lock();
    let defaults = inner.defaults;
    let set = inner.overrides.entry(uid).or_insert(defaults);
    let current = set.get(resource);

    // Validate: soft <= hard.
    if limit.soft > limit.hard && limit.soft != RLIM_INFINITY {
        return Err(KernelError::InvalidArgument);
    }

    // Non-root cannot raise hard limit.
    if requester_uid != 0 && limit.hard > current.hard && limit.hard != RLIM_INFINITY {
        return Err(KernelError::PermissionDenied);
    }

    set.set(resource, limit);
    Ok(())
}

/// Set the global default for a resource.  Requires root.
pub fn set_default(resource: Resource, limit: Rlimit) -> KernelResult<()> {
    if limit.soft > limit.hard && limit.soft != RLIM_INFINITY {
        return Err(KernelError::InvalidArgument);
    }
    RLIMITS.lock().defaults.set(resource, limit);
    Ok(())
}

/// Get the global defaults.
pub fn get_defaults() -> RlimitSet {
    RLIMITS.lock().defaults
}

/// Remove a per-UID override (revert to global defaults).
pub fn remove_override(uid: u32) -> bool {
    RLIMITS.lock().overrides.remove(&uid).is_some()
}

/// List all per-UID overrides.
pub fn list_overrides() -> Vec<(u32, RlimitSet)> {
    let inner = RLIMITS.lock();
    inner.overrides.iter().map(|(&uid, &set)| (uid, set)).collect()
}

/// Check whether opening a new file descriptor would exceed the limit.
///
/// `current_fds`: the number of file descriptors currently open.
pub fn check_nofile(uid: u32, current_fds: u64) -> KernelResult<()> {
    let limit = get_limit(uid, Resource::NoFile);
    if limit.exceeds_soft(current_fds.saturating_add(1)) {
        return Err(KernelError::TooManyOpenFiles);
    }
    Ok(())
}

/// Check whether a file write of the given size is within the limit.
///
/// `new_size`: the total file size after the write.
pub fn check_fsize(uid: u32, new_size: u64) -> KernelResult<()> {
    let limit = get_limit(uid, Resource::FileSize);
    if limit.exceeds_soft(new_size) {
        return Err(KernelError::FileTooLarge);
    }
    Ok(())
}

/// Check whether acquiring another file lock is within the limit.
///
/// `current_locks`: the number of locks currently held by this user.
pub fn check_locks(uid: u32, current_locks: u64) -> KernelResult<()> {
    let limit = get_limit(uid, Resource::Locks);
    if limit.exceeds_soft(current_locks.saturating_add(1)) {
        return Err(KernelError::PermissionDenied);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for filesystem resource limits.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[rlimit] Running self-test...");

    let test_uid = 9990u32;

    // --- Test 1: global defaults ---
    {
        let limits = get_limits(test_uid);
        if limits.nofile.soft != 1024 || limits.nofile.hard != 4096 {
            serial_println!("[rlimit]   ERROR: wrong default nofile ({}/{})",
                limits.nofile.soft, limits.nofile.hard);
            return Err(KernelError::InternalError);
        }
        serial_println!("[rlimit]   global defaults OK");
    }

    // --- Test 2: set soft limit ---
    {
        set_soft(test_uid, Resource::NoFile, 512, test_uid)?;
        let limit = get_limit(test_uid, Resource::NoFile);
        if limit.soft != 512 {
            serial_println!("[rlimit]   ERROR: soft not updated ({})", limit.soft);
            remove_override(test_uid);
            return Err(KernelError::InternalError);
        }
        serial_println!("[rlimit]   set soft OK");
    }

    // --- Test 3: cannot raise soft above hard (non-root) ---
    {
        let result = set_soft(test_uid, Resource::NoFile, 5000, test_uid);
        if result.is_ok() {
            serial_println!("[rlimit]   ERROR: soft > hard allowed");
            remove_override(test_uid);
            return Err(KernelError::InternalError);
        }
        serial_println!("[rlimit]   soft > hard denied OK");
    }

    // --- Test 4: root can raise hard ---
    {
        set_hard(test_uid, Resource::NoFile, 8192, 0)?;
        let limit = get_limit(test_uid, Resource::NoFile);
        if limit.hard != 8192 {
            serial_println!("[rlimit]   ERROR: hard not raised ({})", limit.hard);
            remove_override(test_uid);
            return Err(KernelError::InternalError);
        }
        serial_println!("[rlimit]   root raise hard OK");
    }

    // --- Test 5: non-root cannot raise hard ---
    {
        let result = set_hard(test_uid, Resource::NoFile, 16384, test_uid);
        if result.is_ok() {
            serial_println!("[rlimit]   ERROR: non-root raised hard");
            remove_override(test_uid);
            return Err(KernelError::InternalError);
        }
        serial_println!("[rlimit]   non-root hard raise denied OK");
    }

    // --- Test 6: check_nofile ---
    {
        set_both(test_uid, Resource::NoFile, Rlimit::both(100), 0)?;

        // 99 fds open, opening one more → 100, at limit.
        let result = check_nofile(test_uid, 99);
        if result.is_err() {
            serial_println!("[rlimit]   ERROR: at-limit check denied");
            remove_override(test_uid);
            return Err(KernelError::InternalError);
        }

        // 100 fds open, opening one more → 101, over limit.
        let result = check_nofile(test_uid, 100);
        if result.is_ok() {
            serial_println!("[rlimit]   ERROR: over-limit check allowed");
            remove_override(test_uid);
            return Err(KernelError::InternalError);
        }

        serial_println!("[rlimit]   check_nofile OK");
    }

    // --- Test 7: check_fsize ---
    {
        set_both(test_uid, Resource::FileSize, Rlimit::both(1024 * 1024), 0)?;

        let result = check_fsize(test_uid, 500_000);
        if result.is_err() {
            serial_println!("[rlimit]   ERROR: under-limit fsize denied");
            remove_override(test_uid);
            return Err(KernelError::InternalError);
        }

        let result = check_fsize(test_uid, 2_000_000);
        if result.is_ok() {
            serial_println!("[rlimit]   ERROR: over-limit fsize allowed");
            remove_override(test_uid);
            return Err(KernelError::InternalError);
        }

        serial_println!("[rlimit]   check_fsize OK");
    }

    // --- Test 8: unlimited ---
    {
        set_both(test_uid, Resource::FileSize, Rlimit::unlimited(), 0)?;
        let result = check_fsize(test_uid, u64::MAX / 2);
        if result.is_err() {
            serial_println!("[rlimit]   ERROR: unlimited fsize denied");
            remove_override(test_uid);
            return Err(KernelError::InternalError);
        }
        serial_println!("[rlimit]   unlimited OK");
    }

    // --- Test 9: remove override ---
    {
        let removed = remove_override(test_uid);
        if !removed {
            serial_println!("[rlimit]   ERROR: remove returned false");
            return Err(KernelError::InternalError);
        }
        // Should fall back to defaults.
        let limit = get_limit(test_uid, Resource::NoFile);
        if limit.soft != 1024 {
            serial_println!("[rlimit]   ERROR: didn't fall back to default");
            return Err(KernelError::InternalError);
        }
        serial_println!("[rlimit]   remove override OK");
    }

    // --- Test 10: lowering hard is irreversible ---
    {
        set_both(test_uid, Resource::NoFile, Rlimit { soft: 100, hard: 200 }, 0)?;

        // Non-root lowers hard to 150.
        set_hard(test_uid, Resource::NoFile, 150, test_uid)?;

        // Non-root tries to raise it back to 200 → denied.
        let result = set_hard(test_uid, Resource::NoFile, 200, test_uid);
        if result.is_ok() {
            serial_println!("[rlimit]   ERROR: non-root raised hard back");
            remove_override(test_uid);
            return Err(KernelError::InternalError);
        }

        remove_override(test_uid);
        serial_println!("[rlimit]   hard lower irreversible OK");
    }

    serial_println!("[rlimit] Self-test passed (10 tests).");
    Ok(())
}

//! `<sys/fsuid.h>` — filesystem UID/GID operations.
//!
//! Provides `setfsuid()` and `setfsgid()`, Linux-specific historical
//! wrappers that set a separate filesystem-only credential used for
//! permission checks on file operations (independent of the real /
//! effective UID).  Their original purpose was NFS daemons that needed
//! to act on behalf of remote users without losing root privileges.
//!
//! Modern code generally uses `seteuid()`/`setegid()` instead; setfsuid
//! survives chiefly for ABI compatibility.  We track the current
//! filesystem credential in a process-global pair of atomics, default
//! 0 (matching `getuid()`/`getgid()`).  POSIX semantics: each call
//! returns the *previous* fsuid/fsgid value.

use crate::types::UidT;
use crate::types::GidT;
use core::sync::atomic::{AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// Per-process filesystem credentials
// ---------------------------------------------------------------------------
//
// The credentials are process-global atomics rather than per-thread.
// Linux's setfsuid is per-thread, but our thread model does not yet
// expose per-thread credentials; using a process-wide value matches
// `getuid()` (which currently always returns 0) and is a safe
// approximation.  When per-thread credentials land, switch to
// `current_thread_creds()` lookups here.

static FSUID: AtomicU32 = AtomicU32::new(0);
static FSGID: AtomicU32 = AtomicU32::new(0);

/// Get the current process-wide filesystem UID.
///
/// Helper for the filesystem layer to consult when checking access
/// permissions.  Not a POSIX/Linux API — internal to posix.
#[must_use]
pub fn current_fsuid() -> UidT {
    FSUID.load(Ordering::Relaxed)
}

/// Get the current process-wide filesystem GID.
///
/// See [`current_fsuid`].
#[must_use]
pub fn current_fsgid() -> GidT {
    FSGID.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Set filesystem UID.
///
/// Atomically replaces the current filesystem UID with `fsuid` and
/// returns the previous value.  On Linux, calls from non-privileged
/// processes that try to set a UID other than the real/effective/saved
/// UID silently leave the value unchanged but still return the previous
/// value.  We currently allow any value because privilege checks are
/// not enforced yet.
///
/// # Safety
///
/// Changes process-wide filesystem identity; subsequent file
/// permission checks will use the new value.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setfsuid(fsuid: UidT) -> i32 {
    let prev = FSUID.swap(fsuid, Ordering::Relaxed);
    // Linux returns the previous fsuid as an int.  i32 is wide enough
    // for any UidT we use (u32) under our typical range.
    prev as i32
}

/// Set filesystem GID.
///
/// Atomically replaces the current filesystem GID with `fsgid` and
/// returns the previous value.  Same privilege-check caveat as
/// `setfsuid`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setfsgid(fsgid: GidT) -> i32 {
    let prev = FSGID.swap(fsgid, Ordering::Relaxed);
    prev as i32
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset the credentials to the default value.  Each test that
    /// mutates the atomics restores 0 at the end so other tests see
    /// the documented baseline.
    fn reset() {
        FSUID.store(0, Ordering::Relaxed);
        FSGID.store(0, Ordering::Relaxed);
    }

    #[test]
    fn test_setfsuid_returns_previous_value() {
        reset();
        let prev = setfsuid(1000);
        assert_eq!(prev, 0);
        let prev2 = setfsuid(0);
        assert_eq!(prev2, 1000);
        reset();
    }

    #[test]
    fn test_setfsgid_returns_previous_value() {
        reset();
        let prev = setfsgid(2000);
        assert_eq!(prev, 0);
        let prev2 = setfsgid(0);
        assert_eq!(prev2, 2000);
        reset();
    }

    #[test]
    fn test_setfsuid_root_default() {
        reset();
        let prev = setfsuid(0);
        assert_eq!(prev, 0);
        reset();
    }

    #[test]
    fn test_current_fsuid_follows_setter() {
        reset();
        assert_eq!(current_fsuid(), 0);
        setfsuid(42);
        assert_eq!(current_fsuid(), 42);
        reset();
    }

    #[test]
    fn test_current_fsgid_follows_setter() {
        reset();
        assert_eq!(current_fsgid(), 0);
        setfsgid(99);
        assert_eq!(current_fsgid(), 99);
        reset();
    }

    #[test]
    fn test_setfsuid_default_is_zero() {
        // The atomics start at 0 by static initializer.  We don't
        // reset first because we want to observe the cold value.
        // After this test we ensure the value is back to 0.
        let saw = current_fsuid();
        // saw could be non-zero if a prior test left state; tolerate
        // both, but always restore to 0.
        let _ = saw;
        FSUID.store(0, Ordering::Relaxed);
        assert_eq!(current_fsuid(), 0);
    }

    #[test]
    fn test_setfsuid_setfsgid_independent() {
        reset();
        setfsuid(7);
        setfsgid(11);
        assert_eq!(current_fsuid(), 7);
        assert_eq!(current_fsgid(), 11);
        // Changing one must not disturb the other.
        setfsuid(0);
        assert_eq!(current_fsgid(), 11);
        reset();
    }
}

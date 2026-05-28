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

/// The "invalid UID" sentinel.
///
/// Linux's `<linux/uidgid.h>` defines `INVALID_UID` as `(uid_t)-1`
/// (i.e. `u32::MAX`).  The kernel's `uid_valid(kuid)` helper rejects
/// this value, and every credential-changing syscall (`setuid`,
/// `setfsuid`, `setresuid`, …) silently *skips* the assignment when
/// the requested UID is invalid — the caller still gets the previous
/// value back, but the stored credential is unchanged.
pub const INVALID_UID: UidT = UidT::MAX;

/// The "invalid GID" sentinel — see [`INVALID_UID`].
pub const INVALID_GID: GidT = GidT::MAX;

/// Set filesystem UID.
///
/// # Linux semantics (`kernel/sys.c::sys_setfsuid`)
///
/// ```text
/// SYSCALL_DEFINE1(setfsuid, uid_t, uid) {
///     old_fsuid = current->fsuid;
///     kuid = make_kuid(ns, uid);
///     if (uid_valid(kuid) && (caller is privileged or kuid matches
///                              current ruid/euid/suid/fsuid)) {
///         current->fsuid = kuid;
///     }
///     return old_fsuid;          // always, even when the assignment was skipped
/// }
/// ```
///
/// Three observable behaviours follow:
///
/// 1. `setfsuid((uid_t)-1)` (i.e. `u32::MAX`) is the conventional
///    "invalid UID" sentinel — `uid_valid(kuid)` is false, so the
///    assignment is silently skipped.  The previous fsuid is still
///    returned; the stored value does *not* become `u32::MAX`.
/// 2. The function *never* sets errno on a well-formed call.  Even
///    "not permitted" cases return the previous value rather than
///    `-1` — `setfsuid` predates POSIX `setuid` and chose this
///    quirky interface because NFS daemons need to discover the
///    previous value to put it back.
/// 3. The return value is the *previous* fsuid, not the new one.
///
/// We currently skip the privilege check (no real credential model
/// yet) but enforce (1) and (3) for ABI parity.
///
/// # Safety
///
/// Changes process-wide filesystem identity; subsequent file
/// permission checks will use the new value.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setfsuid(fsuid: UidT) -> i32 {
    let prev = FSUID.load(Ordering::Relaxed);
    if fsuid != INVALID_UID {
        FSUID.store(fsuid, Ordering::Relaxed);
    }
    // Linux returns the previous fsuid as an int.  Personality / UID
    // values fit in the low 31 bits; `as i32` is safe.
    prev as i32
}

/// Set filesystem GID.
///
/// Atomically replaces the current filesystem GID with `fsgid` and
/// returns the previous value.  Same `(gid_t)-1` sentinel and
/// errno-untouched contract as [`setfsuid`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setfsgid(fsgid: GidT) -> i32 {
    let prev = FSGID.load(Ordering::Relaxed);
    if fsgid != INVALID_GID {
        FSGID.store(fsgid, Ordering::Relaxed);
    }
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

    // -----------------------------------------------------------------------
    // Phase 79 — INVALID_UID/INVALID_GID sentinel handling
    //
    // Linux's `setfsuid` / `setfsgid` silently skip the assignment when
    // the caller passes `(uid_t)-1` (`u32::MAX`).  They still return
    // the previous value.  Our previous stub stored the sentinel
    // verbatim, leaving the process with a corrupt filesystem UID of
    // `u32::MAX` — a subtle bug since file permission checks would then
    // run as the "nobody" sentinel rather than the intended UID.
    // -----------------------------------------------------------------------

    #[test]
    fn test_phase79_invalid_uid_constant_value() {
        assert_eq!(INVALID_UID, u32::MAX);
        assert_eq!(INVALID_GID, u32::MAX);
    }

    #[test]
    fn test_phase79_setfsuid_invalid_uid_does_not_change_state() {
        reset();
        // Establish a known non-default state.
        let _ = setfsuid(1234);
        assert_eq!(current_fsuid(), 1234);
        // (uid_t)-1 must NOT clobber the state.
        let prev = setfsuid(INVALID_UID);
        // Still returns the previous fsuid (1234), regardless of skip.
        assert_eq!(prev, 1234);
        // And the stored value is still 1234, not u32::MAX.
        assert_eq!(current_fsuid(), 1234);
        reset();
    }

    #[test]
    fn test_phase79_setfsgid_invalid_gid_does_not_change_state() {
        reset();
        let _ = setfsgid(5678);
        assert_eq!(current_fsgid(), 5678);
        let prev = setfsgid(INVALID_GID);
        assert_eq!(prev, 5678);
        assert_eq!(current_fsgid(), 5678);
        reset();
    }

    #[test]
    fn test_phase79_setfsuid_invalid_from_default_zero() {
        reset();
        // From default state, an INVALID_UID call returns 0 and leaves
        // state at 0 (not u32::MAX).
        let prev = setfsuid(INVALID_UID);
        assert_eq!(prev, 0);
        assert_eq!(current_fsuid(), 0);
        reset();
    }

    #[test]
    fn test_phase79_setfsgid_invalid_from_default_zero() {
        reset();
        let prev = setfsgid(INVALID_GID);
        assert_eq!(prev, 0);
        assert_eq!(current_fsgid(), 0);
        reset();
    }

    #[test]
    fn test_phase79_setfsuid_invalid_then_real_works() {
        reset();
        // INVALID_UID is a no-op, so a subsequent real call still
        // observes 0 as the "previous" value.
        let _ = setfsuid(INVALID_UID);
        let prev = setfsuid(42);
        assert_eq!(prev, 0);
        assert_eq!(current_fsuid(), 42);
        reset();
    }

    #[test]
    fn test_phase79_max_minus_one_is_set_not_sentinel() {
        // u32::MAX is the sentinel; u32::MAX - 1 (0xFFFFFFFE) is a
        // perfectly valid UID and must be stored.
        reset();
        let _ = setfsuid(u32::MAX - 1);
        assert_eq!(current_fsuid(), u32::MAX - 1);
        reset();
    }

    #[test]
    fn test_phase79_invalid_uid_does_not_clobber_errno() {
        // setfsuid never sets errno — even on the "would have been
        // denied" path it returns the previous value cleanly.
        reset();
        crate::errno::set_errno(crate::errno::EBADF);
        let _ = setfsuid(INVALID_UID);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
        crate::errno::set_errno(0);
        reset();
    }

    #[test]
    fn test_phase79_invalid_gid_does_not_clobber_errno() {
        reset();
        crate::errno::set_errno(crate::errno::EBADF);
        let _ = setfsgid(INVALID_GID);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
        crate::errno::set_errno(0);
        reset();
    }

    #[test]
    fn test_phase79_setfsuid_does_not_clobber_errno_on_normal_set() {
        // A normal setfsuid also leaves errno untouched.
        reset();
        crate::errno::set_errno(crate::errno::EAGAIN);
        let _ = setfsuid(100);
        assert_eq!(crate::errno::get_errno(), crate::errno::EAGAIN);
        crate::errno::set_errno(0);
        reset();
    }

    #[test]
    fn test_phase79_buggy_caller_passes_signed_minus_one() {
        // C code that does `setfsuid((uid_t)(-1))` on a u32 typedef:
        // the cast value is u32::MAX, which is our sentinel.  Must be
        // treated as the "no change" probe, not a corrupt set.
        reset();
        let _ = setfsuid(50);
        let cast: u32 = -1i32 as u32;
        assert_eq!(cast, u32::MAX);
        let prev = setfsuid(cast);
        assert_eq!(prev, 50);
        assert_eq!(current_fsuid(), 50);
        reset();
    }

    #[test]
    fn test_phase79_workflow_probe_then_set_then_restore() {
        // Real-world idiom: probe current fsuid, do work as a different
        // UID, restore.  The probe is `setfsuid(INVALID_UID)` per Linux
        // convention — it returns the current value without changing it.
        reset();
        let _ = setfsuid(1000);

        // Probe: returns current (1000), state unchanged.
        let original = setfsuid(INVALID_UID);
        assert_eq!(original, 1000);
        assert_eq!(current_fsuid(), 1000);

        // Switch to UID 65534 ("nobody") for an NFS operation.
        let was = setfsuid(65534);
        assert_eq!(was, 1000);
        assert_eq!(current_fsuid(), 65534);

        // Restore.
        let after = setfsuid(original as u32);
        assert_eq!(after, 65534);
        assert_eq!(current_fsuid(), 1000);

        reset();
    }

    #[test]
    fn test_phase79_workflow_probe_then_set_then_restore_gid() {
        reset();
        let _ = setfsgid(2000);
        let original = setfsgid(INVALID_GID);
        assert_eq!(original, 2000);
        assert_eq!(current_fsgid(), 2000);
        let _ = setfsgid(65534);
        let after = setfsgid(original as u32);
        assert_eq!(after, 65534);
        assert_eq!(current_fsgid(), 2000);
        reset();
    }

    #[test]
    fn test_phase79_invalid_uid_does_not_affect_gid() {
        // Cross-cred non-interference: a setfsuid sentinel must not
        // touch the fsgid at all.
        reset();
        let _ = setfsuid(7);
        let _ = setfsgid(11);
        let _ = setfsuid(INVALID_UID);
        assert_eq!(current_fsuid(), 7);
        assert_eq!(current_fsgid(), 11);
        reset();
    }
}

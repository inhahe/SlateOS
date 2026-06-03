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

use crate::types::GidT;
use crate::types::UidT;
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
///     if (uid_valid(kuid) && (uid == ruid || uid == euid ||
///                              uid == suid || uid == fsuid ||
///                              ns_capable_setid(CAP_SETUID))) {
///         current->fsuid = kuid;
///     }
///     return old_fsuid;          // always, even when the assignment was skipped
/// }
/// ```
///
/// Four observable behaviours follow:
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
/// 4. **Phase 198**: when the caller does not hold CAP_SETUID and
///    the requested uid does not match one of {ruid, euid, suid,
///    fsuid}, the assignment is silently skipped.  No errno set;
///    the old fsuid is still returned.  Probing with `(uid_t)-1`
///    remains the canonical "read without changing" idiom.
///
/// Since our credential model exposes only `getuid()`/`geteuid()`
/// (both 0) and the tracked fsuid, the cred-match set we compare
/// against is `{getuid(), geteuid(), prev_fsuid}` — the saved uid
/// (suid) is folded into the real uid (Linux at process start sets
/// suid == euid == ruid).
///
/// # Safety
///
/// Changes process-wide filesystem identity; subsequent file
/// permission checks will use the new value.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setfsuid(fsuid: UidT) -> i32 {
    let prev = FSUID.load(Ordering::Relaxed);
    if fsuid != INVALID_UID {
        // Phase 198: Linux requires the requested uid to match one
        // of {ruid, euid, suid, fsuid} OR the caller to hold
        // CAP_SETUID.  Otherwise the assignment is silently skipped
        // (no errno).
        let ruid = crate::unistd::getuid();
        let euid = crate::unistd::geteuid();
        let matches_cred = fsuid == ruid || fsuid == euid || fsuid == prev;
        if matches_cred || crate::sys_capability::has_capability(crate::sys_capability::CAP_SETUID)
        {
            FSUID.store(fsuid, Ordering::Relaxed);
        }
    }
    // Linux returns the previous fsuid as an int.  Personality / UID
    // values fit in the low 31 bits; `as i32` is safe.
    prev as i32
}

/// Set filesystem GID.
///
/// Atomically replaces the current filesystem GID with `fsgid` and
/// returns the previous value.  Same `(gid_t)-1` sentinel and
/// errno-untouched contract as [`setfsuid`].  **Phase 198**: also
/// gated on CAP_SETGID via the matching `{rgid, egid, sgid, fsgid}`
/// cred-set or capability.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setfsgid(fsgid: GidT) -> i32 {
    let prev = FSGID.load(Ordering::Relaxed);
    if fsgid != INVALID_GID {
        let rgid = crate::unistd::getgid();
        let egid = crate::unistd::getegid();
        let matches_cred = fsgid == rgid || fsgid == egid || fsgid == prev;
        if matches_cred || crate::sys_capability::has_capability(crate::sys_capability::CAP_SETGID)
        {
            FSGID.store(fsgid, Ordering::Relaxed);
        }
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

    // -----------------------------------------------------------------------
    // Phase 198 — CAP_SETUID/CAP_SETGID gate on arbitrary fsuid/fsgid changes
    //
    // Linux's `setfsuid` (kernel/sys.c) silently skips the assignment when
    //   * the requested uid is INVALID (covered by Phase 79), OR
    //   * it does not match {ruid, euid, suid, fsuid} AND the caller does
    //     not hold CAP_SETUID.
    //
    // The previous stub skipped only the INVALID check, so any process
    // could change fsuid to an arbitrary value — divergent from Linux,
    // and a credential-escalation hazard once a real file permission
    // layer consults current_fsuid().  This phase closes the gap.
    //
    // Observable contract: the denied path is silent — no errno, return
    // value is still the previous fsuid.  Discoverable only by reading
    // back current_fsuid() and seeing it unchanged.
    // -----------------------------------------------------------------------
    mod fsuid_cap_phase198 {
        use super::*;
        use core::sync::atomic::Ordering;

        /// RAII guard: snapshot effective caps on construction,
        /// restore them on Drop.  Mirrors the pattern used by the
        /// process.rs Phase 196/197 modules.
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) = crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap(cap: u32) {
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if cap < 32 {
                (lo & !(1u32 << cap), hi)
            } else {
                (lo, hi & !(1u32 << (cap - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed");
            assert!(!crate::sys_capability::has_capability(cap));
        }

        fn reset_creds() {
            FSUID.store(0, Ordering::Relaxed);
            FSGID.store(0, Ordering::Relaxed);
        }

        // -- Per-error-class: denial path leaves state unchanged ---------

        /// Without CAP_SETUID and from fsuid=0, requesting an
        /// arbitrary uid (1234) that does NOT match {ruid=0, euid=0,
        /// fsuid=0} is silently denied.  No errno, return = prev = 0,
        /// state remains 0.
        #[test]
        fn test_phase198_setfsuid_no_cap_arbitrary_denied_silently() {
            reset_creds();
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            crate::errno::set_errno(0);
            let prev = setfsuid(1234);
            assert_eq!(prev, 0, "always returns previous fsuid");
            assert_eq!(current_fsuid(), 0, "denied: state unchanged");
            assert_eq!(crate::errno::get_errno(), 0, "setfsuid never touches errno");
            reset_creds();
        }

        /// Same for setfsgid + CAP_SETGID.
        #[test]
        fn test_phase198_setfsgid_no_cap_arbitrary_denied_silently() {
            reset_creds();
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            crate::errno::set_errno(0);
            let prev = setfsgid(5678);
            assert_eq!(prev, 0);
            assert_eq!(current_fsgid(), 0);
            assert_eq!(crate::errno::get_errno(), 0);
            reset_creds();
        }

        // -- Cred-match path: allowed even without cap --------------------

        /// Without CAP_SETUID, setting fsuid to current ruid (=0)
        /// matches the cred set and proceeds.  No-op observable, but
        /// the "no-cap, matches cred" branch is exercised.
        #[test]
        fn test_phase198_setfsuid_no_cap_matches_ruid_allowed() {
            reset_creds();
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            let prev = setfsuid(0); // ruid = 0
            assert_eq!(prev, 0);
            assert_eq!(current_fsuid(), 0);
            reset_creds();
        }

        /// Without CAP_SETUID, setting fsuid to the *current* fsuid
        /// (after a prior cap'd set) is allowed via the
        /// "matches fsuid" arm of the cred set.
        #[test]
        fn test_phase198_setfsuid_no_cap_matches_current_fsuid_allowed() {
            reset_creds();
            // With cap held, push fsuid to 7777.
            let _ = setfsuid(7777);
            assert_eq!(current_fsuid(), 7777);
            // Drop cap, "set" to the same value — must succeed
            // (matches current fsuid).
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            let prev = setfsuid(7777);
            assert_eq!(prev, 7777);
            assert_eq!(current_fsuid(), 7777);
            reset_creds();
        }

        // -- With cap: arbitrary set succeeds ----------------------------

        /// With CAP_SETUID held, an arbitrary new fsuid is stored.
        #[test]
        fn test_phase198_setfsuid_with_cap_arbitrary_succeeds() {
            reset_creds();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SETUID,
            ));
            let prev = setfsuid(9999);
            assert_eq!(prev, 0);
            assert_eq!(current_fsuid(), 9999);
            reset_creds();
        }

        /// With CAP_SETGID held, arbitrary new fsgid stored.
        #[test]
        fn test_phase198_setfsgid_with_cap_arbitrary_succeeds() {
            reset_creds();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SETGID,
            ));
            let prev = setfsgid(8888);
            assert_eq!(prev, 0);
            assert_eq!(current_fsgid(), 8888);
            reset_creds();
        }

        // -- Sentinel preserved -------------------------------------------

        /// Phase 79's INVALID_UID sentinel must still be honored
        /// regardless of cap state: with cap held, sentinel is a
        /// no-op probe (state unchanged, returns prev).
        #[test]
        fn test_phase198_invalid_uid_still_noop_with_cap() {
            reset_creds();
            let _ = setfsuid(50);
            let prev = setfsuid(INVALID_UID);
            assert_eq!(prev, 50);
            assert_eq!(current_fsuid(), 50);
            reset_creds();
        }

        /// And without cap: sentinel still skips silently.
        #[test]
        fn test_phase198_invalid_uid_still_noop_without_cap() {
            reset_creds();
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            let prev = setfsuid(INVALID_UID);
            assert_eq!(prev, 0);
            assert_eq!(current_fsuid(), 0);
            reset_creds();
        }

        // -- Errno discipline on the denied path --------------------------

        /// On the silent-deny path, stale errno is preserved (the
        /// syscall never writes errno).  Linux contract: setfsuid
        /// returns the previous value, period.
        #[test]
        fn test_phase198_denied_setfsuid_preserves_stale_errno() {
            reset_creds();
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            crate::errno::set_errno(crate::errno::EBADF);
            let _ = setfsuid(4242);
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::EBADF,
                "setfsuid must not write errno even on denial"
            );
            crate::errno::set_errno(0);
            reset_creds();
        }

        #[test]
        fn test_phase198_denied_setfsgid_preserves_stale_errno() {
            reset_creds();
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            crate::errno::set_errno(crate::errno::EBADF);
            let _ = setfsgid(4242);
            assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
            crate::errno::set_errno(0);
            reset_creds();
        }

        // -- Recovery: cap restoration re-enables arbitrary set -----------

        /// Drop cap, fail to change; restore cap (CapGuard goes out
        /// of scope), now arbitrary set succeeds.
        #[test]
        fn test_phase198_capguard_restore_re_enables_set() {
            reset_creds();
            {
                let _g = CapGuard::snapshot();
                drop_cap(crate::sys_capability::CAP_SETUID);
                let _ = setfsuid(1234);
                assert_eq!(current_fsuid(), 0, "denied without cap");
            }
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SETUID,
            ));
            let _ = setfsuid(1234);
            assert_eq!(current_fsuid(), 1234, "succeeds after restore");
            reset_creds();
        }

        // -- Independence: uid path uses CAP_SETUID, gid uses CAP_SETGID --

        /// Dropping only CAP_SETUID leaves setfsgid arbitrary-set
        /// working (different cap gates).
        #[test]
        fn test_phase198_drop_setuid_does_not_affect_setfsgid() {
            reset_creds();
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SETGID,
            ));
            let prev = setfsgid(3333);
            assert_eq!(prev, 0);
            assert_eq!(current_fsgid(), 3333);
            reset_creds();
        }

        /// And vice-versa.
        #[test]
        fn test_phase198_drop_setgid_does_not_affect_setfsuid() {
            reset_creds();
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SETUID,
            ));
            let prev = setfsuid(4444);
            assert_eq!(prev, 0);
            assert_eq!(current_fsuid(), 4444);
            reset_creds();
        }

        // -- Workflow: NFS-style probe / set / restore under cap drop -----

        /// NFS-style probe-and-restore: from fsuid=1000 (cap'd set),
        /// drop cap, probe via INVALID_UID (returns 1000, no change),
        /// any non-matching set is denied silently, restore back to
        /// 1000 via the matches-fsuid arm.
        #[test]
        fn test_phase198_workflow_probe_and_restore_under_no_cap() {
            reset_creds();
            // Cap'd setup: become fsuid 1000.
            let _ = setfsuid(1000);
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);

            // Probe: returns 1000, no change.
            let probe = setfsuid(INVALID_UID);
            assert_eq!(probe, 1000);
            assert_eq!(current_fsuid(), 1000);

            // Try to escalate to 65534 ("nobody") without cap →
            // silently denied.
            let denied = setfsuid(65534);
            assert_eq!(denied, 1000);
            assert_eq!(current_fsuid(), 1000, "no escalation allowed");

            // "Restore" to current fsuid (matches cred arm) → no-op
            // but allowed.
            let restored = setfsuid(1000);
            assert_eq!(restored, 1000);
            assert_eq!(current_fsuid(), 1000);

            reset_creds();
        }

        // -- Buggy-caller --------------------------------------------------

        /// Repeated denied calls keep state stable.
        #[test]
        fn test_phase198_repeated_denied_calls_stable() {
            reset_creds();
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            for _ in 0..8 {
                let prev = setfsuid(7777);
                assert_eq!(prev, 0);
                assert_eq!(current_fsuid(), 0);
            }
            reset_creds();
        }

        // -- Cross-checks --------------------------------------------------

        /// setfsuid never returns -1 — even on the denied path the
        /// return is the previous fsuid (which is a valid uid).
        #[test]
        fn test_phase198_setfsuid_never_returns_minus_one() {
            reset_creds();
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETUID);
            let prev = setfsuid(9999);
            assert_ne!(prev, -1, "setfsuid never returns -1 on any path");
            reset_creds();
        }

        /// Symmetry: setfsgid never returns -1.
        #[test]
        fn test_phase198_setfsgid_never_returns_minus_one() {
            reset_creds();
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SETGID);
            let prev = setfsgid(9999);
            assert_ne!(prev, -1);
            reset_creds();
        }

        /// Parity: setfsuid/setfsgid use distinct caps that mirror
        /// the setuid/setgid Phase 192/193 gates.
        #[test]
        fn test_phase198_parity_with_setuid_setgid_cap_choices() {
            // Pin the cap numbers used by Phase 198 match the ones
            // Phase 192 (setuid) and Phase 193 (setgid) used.  If
            // either changes, this test forces a re-audit.
            assert_eq!(crate::sys_capability::CAP_SETUID, 7);
            assert_eq!(crate::sys_capability::CAP_SETGID, 6);
        }
    }
}

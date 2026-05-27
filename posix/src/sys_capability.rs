//! `<sys/capability.h>` — POSIX capabilities (Linux implementation).
//!
//! Defines Linux capability constants and the capability header/data
//! structures used by `capget()` / `capset()`.

use crate::errno;

// ---------------------------------------------------------------------------
// Capability version
// ---------------------------------------------------------------------------

/// Version 3 capability header (Linux 2.6.26+, supports 64-bit sets).
pub const _LINUX_CAPABILITY_VERSION_3: u32 = 0x20080522;

/// Number of u32 words for capability sets in v3.
pub const _LINUX_CAPABILITY_U32S_3: usize = 2;

// ---------------------------------------------------------------------------
// Capability header
// ---------------------------------------------------------------------------

/// Capability header for `capget()`/`capset()`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CapUserHeader {
    /// Capability version (`_LINUX_CAPABILITY_VERSION_3`).
    pub version: u32,
    /// PID (0 = calling process).
    pub pid: i32,
}

/// Capability data for `capget()`/`capset()`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CapUserData {
    /// Effective capability set.
    pub effective: u32,
    /// Permitted capability set.
    pub permitted: u32,
    /// Inheritable capability set.
    pub inheritable: u32,
}

// ---------------------------------------------------------------------------
// Capability constants
// ---------------------------------------------------------------------------

/// Override DAC read/search.
pub const CAP_DAC_READ_SEARCH: u32 = 2;

/// Override DAC write.
pub const CAP_DAC_OVERRIDE: u32 = 1;

/// Bypass file ownership checks.
pub const CAP_FOWNER: u32 = 3;

/// Set file SUID/SGID.
pub const CAP_FSETID: u32 = 4;

/// Kill processes.
pub const CAP_KILL: u32 = 5;

/// Set UID/GID.
pub const CAP_SETUID: u32 = 7;

/// Set GID.
pub const CAP_SETGID: u32 = 6;

/// Set process capabilities.
pub const CAP_SETPCAP: u32 = 8;

/// Bypass file read/write/execute permission checks.
pub const CAP_CHOWN: u32 = 0;

/// Bind to privileged ports (< 1024).
pub const CAP_NET_BIND_SERVICE: u32 = 10;

/// Various network admin operations.
pub const CAP_NET_ADMIN: u32 = 12;

/// Use RAW and PACKET sockets.
pub const CAP_NET_RAW: u32 = 13;

/// Lock memory (mlock, mlockall).
pub const CAP_IPC_LOCK: u32 = 14;

/// Override IPC ownership checks.
pub const CAP_IPC_OWNER: u32 = 15;

/// Load and unload kernel modules.
pub const CAP_SYS_MODULE: u32 = 16;

/// Perform I/O port operations (ioperm, iopl).
pub const CAP_SYS_RAWIO: u32 = 17;

/// Use chroot.
pub const CAP_SYS_CHROOT: u32 = 18;

/// Trace arbitrary processes (ptrace).
pub const CAP_SYS_PTRACE: u32 = 19;

/// Accounting.
pub const CAP_SYS_PACCT: u32 = 20;

/// Various system admin operations.
pub const CAP_SYS_ADMIN: u32 = 21;

/// Use reboot.
pub const CAP_SYS_BOOT: u32 = 22;

/// Raise process nice value, change scheduling.
pub const CAP_SYS_NICE: u32 = 23;

/// Override resource limits.
pub const CAP_SYS_RESOURCE: u32 = 24;

/// Manipulate system clock.
pub const CAP_SYS_TIME: u32 = 25;

/// Configure tty devices.
pub const CAP_SYS_TTY_CONFIG: u32 = 26;

/// Create special files (mknod).
pub const CAP_MKNOD: u32 = 27;

/// Set file capabilities.
pub const CAP_SETFCAP: u32 = 31;

/// Audit control.
pub const CAP_AUDIT_CONTROL: u32 = 30;

/// Write audit log entries.
pub const CAP_AUDIT_WRITE: u32 = 29;

/// Configure MAC (Mandatory Access Control).
pub const CAP_MAC_ADMIN: u32 = 33;

/// Override MAC.
pub const CAP_MAC_OVERRIDE: u32 = 32;

/// Use `syslog()`.
pub const CAP_SYSLOG: u32 = 34;

/// Trigger wake-ups (via `/dev/wakealarm`).
pub const CAP_WAKE_ALARM: u32 = 35;

/// Block suspend.
pub const CAP_BLOCK_SUSPEND: u32 = 36;

/// Read audit log.
pub const CAP_AUDIT_READ: u32 = 37;

/// Perform perfmon operations.
pub const CAP_PERFMON: u32 = 38;

/// Use BPF.
pub const CAP_BPF: u32 = 39;

/// Use checkpoint/restore.
pub const CAP_CHECKPOINT_RESTORE: u32 = 40;

/// Last valid capability number.
pub const CAP_LAST_CAP: u32 = 40;

// ---------------------------------------------------------------------------
// Process capability sets
// ---------------------------------------------------------------------------
//
// Linux capability v3 holds 64 bits per set across two u32 words (the
// `datap[2]` array passed to capget/capset).  We model that as a pair
// of AtomicU32 per set.  The default value is "all caps held" — we
// run as root with no security boundary yet, so dropping a cap means
// the process voluntarily declines a privilege, but querying always
// reports whatever the process previously stored.

use core::sync::atomic::{AtomicU32, Ordering};

/// Initial value with every defined capability bit set (caps 0..=40
/// occupy the low 41 bits of the combined 64-bit set).
const DEFAULT_CAPS_LOW: u32 = u32::MAX;
const DEFAULT_CAPS_HIGH: u32 = (1u32 << 9).wrapping_sub(1); // caps 32..40 → 9 bits

// effective / permitted / inheritable, each (low, high) word.
static CAP_EFF_LO: AtomicU32 = AtomicU32::new(DEFAULT_CAPS_LOW);
static CAP_EFF_HI: AtomicU32 = AtomicU32::new(DEFAULT_CAPS_HIGH);
static CAP_PRM_LO: AtomicU32 = AtomicU32::new(DEFAULT_CAPS_LOW);
static CAP_PRM_HI: AtomicU32 = AtomicU32::new(DEFAULT_CAPS_HIGH);
static CAP_INH_LO: AtomicU32 = AtomicU32::new(0);
static CAP_INH_HI: AtomicU32 = AtomicU32::new(0);

/// Read the currently-held effective capability set as (low, high).
#[must_use]
pub fn current_caps_effective() -> (u32, u32) {
    (
        CAP_EFF_LO.load(Ordering::Relaxed),
        CAP_EFF_HI.load(Ordering::Relaxed),
    )
}

/// Test whether the calling process holds capability `cap`.
///
/// Helper for permission checks elsewhere in the posix layer.  `cap`
/// must be one of the `CAP_*` constants; returns false if `cap >
/// CAP_LAST_CAP`.
#[must_use]
pub fn has_capability(cap: u32) -> bool {
    if cap > CAP_LAST_CAP {
        return false;
    }
    let (lo, hi) = current_caps_effective();
    if cap < 32 {
        lo & (1u32 << cap) != 0
    } else {
        hi & (1u32 << (cap.wrapping_sub(32))) != 0
    }
}

// ---------------------------------------------------------------------------
// capget / capset
// ---------------------------------------------------------------------------

/// Validate the header passed to `capget` / `capset`.
///
/// Linux behaviour: if `version` is unsupported, the kernel writes the
/// preferred version back into `*hdrp` and returns -1 with EINVAL.
/// `pid` must be 0 or the caller's own PID.
fn validate_cap_header(hdrp: *mut CapUserHeader) -> Result<(), i32> {
    if hdrp.is_null() {
        return Err(errno::EFAULT);
    }
    // SAFETY: hdrp is non-null by check above; caller contract for layout.
    let (version, pid) = unsafe { ((*hdrp).version, (*hdrp).pid) };
    if version != _LINUX_CAPABILITY_VERSION_3 {
        // Tell the caller which version we support.
        // SAFETY: hdrp non-null.
        unsafe { (*hdrp).version = _LINUX_CAPABILITY_VERSION_3; }
        return Err(errno::EINVAL);
    }
    // We treat pid 0 as "self".  Non-zero pids would require looking up
    // the target task's credential, which we don't expose; reject with
    // EPERM to match Linux behaviour for "other process" requests
    // without the needed capability.
    if pid != 0 {
        return Err(errno::EPERM);
    }
    Ok(())
}

/// Get process capabilities.
///
/// Writes the calling process's effective, permitted, and inheritable
/// sets into `datap[0..2]` (low and high u32 words).  Returns 0 on
/// success, -1 with errno on validation failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn capget(
    hdrp: *mut CapUserHeader,
    datap: *mut CapUserData,
) -> i32 {
    if let Err(e) = validate_cap_header(hdrp) {
        errno::set_errno(e);
        return -1;
    }
    if datap.is_null() {
        // A null `datap` is permitted: Linux uses it as a "probe" to
        // discover the supported version.  Header was already
        // validated/written by validate_cap_header.
        return 0;
    }
    let eff_lo = CAP_EFF_LO.load(Ordering::Relaxed);
    let eff_hi = CAP_EFF_HI.load(Ordering::Relaxed);
    let prm_lo = CAP_PRM_LO.load(Ordering::Relaxed);
    let prm_hi = CAP_PRM_HI.load(Ordering::Relaxed);
    let inh_lo = CAP_INH_LO.load(Ordering::Relaxed);
    let inh_hi = CAP_INH_HI.load(Ordering::Relaxed);
    // SAFETY: caller guarantees datap points to two writable
    // CapUserData entries (low + high words) per the v3 ABI.
    unsafe {
        *datap = CapUserData { effective: eff_lo, permitted: prm_lo, inheritable: inh_lo };
        *datap.add(1) = CapUserData { effective: eff_hi, permitted: prm_hi, inheritable: inh_hi };
    }
    0
}

/// Set process capabilities.
///
/// Reads `datap[0..2]` and atomically updates the effective, permitted,
/// and inheritable sets.  Linux enforces several invariants
/// (effective ⊆ permitted; inheritable ⊆ permitted ∪ inheritable-old;
/// only `CAP_SETPCAP` allows raising permitted) — we currently apply
/// only the basic effective-⊆-permitted check, since the full rules
/// require a real security model.  Returns 0 on success.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn capset(
    hdrp: *mut CapUserHeader,
    datap: *const CapUserData,
) -> i32 {
    if let Err(e) = validate_cap_header(hdrp) {
        errno::set_errno(e);
        return -1;
    }
    if datap.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: caller contract — datap points to two CapUserData entries.
    let (lo, hi) = unsafe { (*datap, *datap.add(1)) };
    // Effective must be a subset of permitted (POSIX/Linux invariant).
    if (lo.effective & !lo.permitted) != 0 || (hi.effective & !hi.permitted) != 0 {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    CAP_EFF_LO.store(lo.effective, Ordering::Relaxed);
    CAP_EFF_HI.store(hi.effective, Ordering::Relaxed);
    CAP_PRM_LO.store(lo.permitted, Ordering::Relaxed);
    CAP_PRM_HI.store(hi.permitted, Ordering::Relaxed);
    CAP_INH_LO.store(lo.inheritable, Ordering::Relaxed);
    CAP_INH_HI.store(hi.inheritable, Ordering::Relaxed);
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_header_size() {
        assert_eq!(core::mem::size_of::<CapUserHeader>(), 8);
    }

    #[test]
    fn test_cap_data_size() {
        assert_eq!(core::mem::size_of::<CapUserData>(), 12);
    }

    #[test]
    fn test_cap_constants_in_range() {
        let caps = [
            CAP_CHOWN, CAP_DAC_OVERRIDE, CAP_DAC_READ_SEARCH,
            CAP_FOWNER, CAP_FSETID, CAP_KILL, CAP_SETGID,
            CAP_SETUID, CAP_SETPCAP, CAP_NET_BIND_SERVICE,
            CAP_NET_ADMIN, CAP_NET_RAW, CAP_IPC_LOCK,
            CAP_IPC_OWNER, CAP_SYS_MODULE, CAP_SYS_RAWIO,
            CAP_SYS_CHROOT, CAP_SYS_PTRACE, CAP_SYS_PACCT,
            CAP_SYS_ADMIN, CAP_SYS_BOOT, CAP_SYS_NICE,
            CAP_SYS_RESOURCE, CAP_SYS_TIME, CAP_SYS_TTY_CONFIG,
            CAP_MKNOD, CAP_AUDIT_WRITE, CAP_AUDIT_CONTROL,
            CAP_SETFCAP, CAP_MAC_OVERRIDE, CAP_MAC_ADMIN,
            CAP_SYSLOG, CAP_WAKE_ALARM, CAP_BLOCK_SUSPEND,
            CAP_AUDIT_READ, CAP_PERFMON, CAP_BPF,
            CAP_CHECKPOINT_RESTORE,
        ];
        for &c in &caps {
            assert!(c <= CAP_LAST_CAP, "CAP_{c} exceeds CAP_LAST_CAP");
        }
    }

    #[test]
    fn test_cap_constants_distinct() {
        let caps = [
            CAP_CHOWN, CAP_DAC_OVERRIDE, CAP_DAC_READ_SEARCH,
            CAP_FOWNER, CAP_FSETID, CAP_KILL, CAP_SETGID,
            CAP_SETUID, CAP_SETPCAP, CAP_NET_BIND_SERVICE,
            CAP_NET_ADMIN, CAP_NET_RAW, CAP_IPC_LOCK,
            CAP_IPC_OWNER, CAP_SYS_MODULE, CAP_SYS_RAWIO,
            CAP_SYS_CHROOT, CAP_SYS_PTRACE, CAP_SYS_PACCT,
            CAP_SYS_ADMIN, CAP_SYS_BOOT, CAP_SYS_NICE,
            CAP_SYS_RESOURCE, CAP_SYS_TIME, CAP_SYS_TTY_CONFIG,
            CAP_MKNOD, CAP_AUDIT_WRITE, CAP_AUDIT_CONTROL,
            CAP_SETFCAP, CAP_MAC_OVERRIDE, CAP_MAC_ADMIN,
            CAP_SYSLOG, CAP_WAKE_ALARM, CAP_BLOCK_SUSPEND,
            CAP_AUDIT_READ, CAP_PERFMON, CAP_BPF,
            CAP_CHECKPOINT_RESTORE,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(
                    caps[i], caps[j],
                    "CAP constants must be distinct"
                );
            }
        }
    }

    #[test]
    fn test_cap_last_cap() {
        assert_eq!(CAP_LAST_CAP, 40);
    }

    #[test]
    fn test_cap_version_3() {
        assert_eq!(_LINUX_CAPABILITY_VERSION_3, 0x20080522);
    }

    /// Restore the capability sets to their cold-boot defaults so tests
    /// that mutate state don't leak into one another.
    fn reset_caps() {
        CAP_EFF_LO.store(DEFAULT_CAPS_LOW, Ordering::Relaxed);
        CAP_EFF_HI.store(DEFAULT_CAPS_HIGH, Ordering::Relaxed);
        CAP_PRM_LO.store(DEFAULT_CAPS_LOW, Ordering::Relaxed);
        CAP_PRM_HI.store(DEFAULT_CAPS_HIGH, Ordering::Relaxed);
        CAP_INH_LO.store(0, Ordering::Relaxed);
        CAP_INH_HI.store(0, Ordering::Relaxed);
    }

    #[test]
    fn test_capget_null_header_efault() {
        let mut data = [CapUserData { effective: 0, permitted: 0, inheritable: 0 }; 2];
        let ret = capget(core::ptr::null_mut(), data.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_capset_null_header_efault() {
        let data = [CapUserData { effective: 0, permitted: 0, inheritable: 0 }; 2];
        let ret = capset(core::ptr::null_mut(), data.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_capset_null_data_efault() {
        let mut hdr = CapUserHeader { version: _LINUX_CAPABILITY_VERSION_3, pid: 0 };
        let ret = capset(&mut hdr, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_capget_version_mismatch_rewrites_header() {
        let mut hdr = CapUserHeader { version: 0xdeadbeef, pid: 0 };
        let mut data = [CapUserData { effective: 0, permitted: 0, inheritable: 0 }; 2];
        let ret = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // Linux kernel writes the preferred version back into the header.
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
    }

    #[test]
    fn test_capset_version_mismatch_rewrites_header() {
        let mut hdr = CapUserHeader { version: 1, pid: 0 };
        let data = [CapUserData { effective: 0, permitted: 0, inheritable: 0 }; 2];
        let ret = capset(&mut hdr, data.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
    }

    #[test]
    fn test_capget_nonzero_pid_eperm() {
        let mut hdr = CapUserHeader { version: _LINUX_CAPABILITY_VERSION_3, pid: 42 };
        let mut data = [CapUserData { effective: 0, permitted: 0, inheritable: 0 }; 2];
        let ret = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_capset_nonzero_pid_eperm() {
        let mut hdr = CapUserHeader { version: _LINUX_CAPABILITY_VERSION_3, pid: 99 };
        let data = [CapUserData { effective: 0, permitted: 0, inheritable: 0 }; 2];
        let ret = capset(&mut hdr, data.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_capget_null_datap_is_probe() {
        // A null datap is a valid "probe" — Linux uses it to discover
        // the supported version. Returns 0; header is left intact
        // since it already matched our preferred version.
        let mut hdr = CapUserHeader { version: _LINUX_CAPABILITY_VERSION_3, pid: 0 };
        let ret = capget(&mut hdr, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
    }

    #[test]
    fn test_capget_returns_defaults() {
        reset_caps();
        let mut hdr = CapUserHeader { version: _LINUX_CAPABILITY_VERSION_3, pid: 0 };
        let mut data = [CapUserData { effective: 0, permitted: 0, inheritable: 0 }; 2];
        let ret = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(ret, 0);
        assert_eq!(data[0].effective, DEFAULT_CAPS_LOW);
        assert_eq!(data[0].permitted, DEFAULT_CAPS_LOW);
        assert_eq!(data[0].inheritable, 0);
        assert_eq!(data[1].effective, DEFAULT_CAPS_HIGH);
        assert_eq!(data[1].permitted, DEFAULT_CAPS_HIGH);
        assert_eq!(data[1].inheritable, 0);
        reset_caps();
    }

    #[test]
    fn test_capset_then_capget_roundtrip() {
        reset_caps();
        let mut hdr = CapUserHeader { version: _LINUX_CAPABILITY_VERSION_3, pid: 0 };
        // Drop everything except CAP_NET_BIND_SERVICE (bit 10) in
        // effective, keep all in permitted.
        let want_eff_lo: u32 = 1u32 << CAP_NET_BIND_SERVICE;
        let want_inh_lo: u32 = 1u32 << CAP_CHOWN;
        let set_data = [
            CapUserData { effective: want_eff_lo, permitted: DEFAULT_CAPS_LOW, inheritable: want_inh_lo },
            CapUserData { effective: 0,           permitted: DEFAULT_CAPS_HIGH, inheritable: 0 },
        ];
        let ret = capset(&mut hdr, set_data.as_ptr());
        assert_eq!(ret, 0);

        let mut data = [CapUserData { effective: 0, permitted: 0, inheritable: 0 }; 2];
        let ret = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(ret, 0);
        assert_eq!(data[0].effective, want_eff_lo);
        assert_eq!(data[0].permitted, DEFAULT_CAPS_LOW);
        assert_eq!(data[0].inheritable, want_inh_lo);
        assert_eq!(data[1].effective, 0);
        assert_eq!(data[1].permitted, DEFAULT_CAPS_HIGH);
        assert_eq!(data[1].inheritable, 0);
        reset_caps();
    }

    #[test]
    fn test_capset_rejects_effective_not_subset_of_permitted() {
        reset_caps();
        let mut hdr = CapUserHeader { version: _LINUX_CAPABILITY_VERSION_3, pid: 0 };
        // Effective claims CAP_KILL (bit 5) but permitted does not.
        let bad = [
            CapUserData { effective: 1u32 << CAP_KILL, permitted: 0, inheritable: 0 },
            CapUserData { effective: 0, permitted: 0, inheritable: 0 },
        ];
        let ret = capset(&mut hdr, bad.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
        reset_caps();
    }

    #[test]
    fn test_capset_rejects_effective_not_subset_high_word() {
        reset_caps();
        let mut hdr = CapUserHeader { version: _LINUX_CAPABILITY_VERSION_3, pid: 0 };
        // High-word violation: claim CAP_BPF (bit 39, → high bit 7) in
        // effective without permitted.
        let bad = [
            CapUserData { effective: 0, permitted: 0, inheritable: 0 },
            CapUserData { effective: 1u32 << 7, permitted: 0, inheritable: 0 },
        ];
        let ret = capset(&mut hdr, bad.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
        reset_caps();
    }

    #[test]
    fn test_has_capability_default_holds_known_caps() {
        reset_caps();
        assert!(has_capability(CAP_CHOWN));
        assert!(has_capability(CAP_KILL));
        assert!(has_capability(CAP_SYS_ADMIN));
        // High-word cap defined within DEFAULT_CAPS_HIGH range (cap 39
        // = bit 7 of high; DEFAULT_CAPS_HIGH = 0x1FF covers bits 0..8).
        assert!(has_capability(CAP_BPF));
        assert!(has_capability(CAP_CHECKPOINT_RESTORE));
        reset_caps();
    }

    #[test]
    fn test_has_capability_out_of_range() {
        // Anything past CAP_LAST_CAP is rejected outright.
        assert!(!has_capability(CAP_LAST_CAP + 1));
        assert!(!has_capability(63));
        assert!(!has_capability(u32::MAX));
    }

    #[test]
    fn test_has_capability_follows_capset() {
        reset_caps();
        let mut hdr = CapUserHeader { version: _LINUX_CAPABILITY_VERSION_3, pid: 0 };
        // Drop everything in effective.
        let zero = [
            CapUserData { effective: 0, permitted: DEFAULT_CAPS_LOW, inheritable: 0 },
            CapUserData { effective: 0, permitted: DEFAULT_CAPS_HIGH, inheritable: 0 },
        ];
        let ret = capset(&mut hdr, zero.as_ptr());
        assert_eq!(ret, 0);
        assert!(!has_capability(CAP_CHOWN));
        assert!(!has_capability(CAP_SYS_ADMIN));
        assert!(!has_capability(CAP_BPF));
        reset_caps();
        // After reset, defaults should restore visibility.
        assert!(has_capability(CAP_CHOWN));
    }

    #[test]
    fn test_current_caps_effective_default() {
        reset_caps();
        let (lo, hi) = current_caps_effective();
        assert_eq!(lo, DEFAULT_CAPS_LOW);
        assert_eq!(hi, DEFAULT_CAPS_HIGH);
    }

    #[test]
    fn test_cap_known_values() {
        assert_eq!(CAP_CHOWN, 0);
        assert_eq!(CAP_DAC_OVERRIDE, 1);
        assert_eq!(CAP_KILL, 5);
        assert_eq!(CAP_SYS_ADMIN, 21);
        assert_eq!(CAP_NET_BIND_SERVICE, 10);
    }
}

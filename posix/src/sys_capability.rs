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
// capget / capset
// ---------------------------------------------------------------------------

/// Get process capabilities.
///
/// Stub — always returns -1 with `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn capget(
    _hdrp: *mut CapUserHeader,
    _datap: *mut CapUserData,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Set process capabilities.
///
/// Stub — always returns -1 with `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn capset(
    _hdrp: *mut CapUserHeader,
    _datap: *const CapUserData,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
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

    #[test]
    fn test_capget_stub() {
        let ret = capget(core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_capset_stub() {
        let ret = capset(core::ptr::null_mut(), core::ptr::null());
        assert_eq!(ret, -1);
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

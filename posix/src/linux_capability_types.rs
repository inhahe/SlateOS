//! `<linux/capability.h>` — POSIX capabilities constants.
//!
//! Linux capabilities split the traditional root privilege into
//! distinct units. Instead of all-or-nothing root, processes can
//! hold specific capabilities (e.g., CAP_NET_BIND_SERVICE to bind
//! ports below 1024 without being root). Capabilities are stored
//! per-thread in three sets: effective, permitted, and inheritable.

// ---------------------------------------------------------------------------
// Capability indices (CAP_* values)
// ---------------------------------------------------------------------------

/// Override DAC read/search.
pub const CAP_DAC_READ_SEARCH: u32 = 2;
/// Override DAC write.
pub const CAP_DAC_OVERRIDE: u32 = 1;
/// Override file ownership checks.
pub const CAP_FOWNER: u32 = 3;
/// Bypass file setuid/setgid restrictions.
pub const CAP_FSETID: u32 = 4;
/// Allow killing any process.
pub const CAP_KILL: u32 = 5;
/// Set UID (setuid, setreuid, setresuid).
pub const CAP_SETUID: u32 = 7;
/// Set GID (setgid, setregid, setresgid).
pub const CAP_SETGID: u32 = 6;
/// Set file capabilities.
pub const CAP_SETFCAP: u32 = 31;
/// Set PCAP on other processes.
pub const CAP_SETPCAP: u32 = 8;
/// Bypass immutable/append-only flags.
pub const CAP_LINUX_IMMUTABLE: u32 = 9;
/// Bind to ports below 1024.
pub const CAP_NET_BIND_SERVICE: u32 = 10;
/// Broadcast and multicast.
pub const CAP_NET_BROADCAST: u32 = 11;
/// Network administration (firewall, routing).
pub const CAP_NET_ADMIN: u32 = 12;
/// Raw sockets and packet capture.
pub const CAP_NET_RAW: u32 = 13;
/// Lock memory (mlock, mlockall).
pub const CAP_IPC_LOCK: u32 = 14;
/// Override IPC permissions.
pub const CAP_IPC_OWNER: u32 = 15;
/// Load/unload kernel modules.
pub const CAP_SYS_MODULE: u32 = 16;
/// Raw I/O port access.
pub const CAP_SYS_RAWIO: u32 = 17;
/// chroot().
pub const CAP_SYS_CHROOT: u32 = 18;
/// ptrace any process.
pub const CAP_SYS_PTRACE: u32 = 19;
/// Accounting control.
pub const CAP_SYS_PACCT: u32 = 20;
/// System administration (mount, sethostname, etc).
pub const CAP_SYS_ADMIN: u32 = 21;
/// Reboot the system.
pub const CAP_SYS_BOOT: u32 = 22;
/// Set scheduling priority/policy.
pub const CAP_SYS_NICE: u32 = 23;
/// Override resource limits.
pub const CAP_SYS_RESOURCE: u32 = 24;
/// Set system time.
pub const CAP_SYS_TIME: u32 = 25;
/// Use tty config ioctl.
pub const CAP_SYS_TTY_CONFIG: u32 = 26;
/// Create device special files.
pub const CAP_MKNOD: u32 = 27;
/// Filesystem lease.
pub const CAP_LEASE: u32 = 28;
/// Write audit log.
pub const CAP_AUDIT_WRITE: u32 = 29;
/// Audit control.
pub const CAP_AUDIT_CONTROL: u32 = 30;
/// Allow MAC override (Smack, SELinux).
pub const CAP_MAC_OVERRIDE: u32 = 32;
/// Allow MAC admin.
pub const CAP_MAC_ADMIN: u32 = 33;
/// Use syslog().
pub const CAP_SYSLOG: u32 = 34;
/// Allow wake alarm timer.
pub const CAP_WAKE_ALARM: u32 = 35;
/// Block suspend.
pub const CAP_BLOCK_SUSPEND: u32 = 36;
/// Audit read.
pub const CAP_AUDIT_READ: u32 = 37;
/// Perform BPF operations.
pub const CAP_BPF: u32 = 39;
/// Performance monitoring (perf_event_open).
pub const CAP_PERFMON: u32 = 38;
/// Checkpoint/restore.
pub const CAP_CHECKPOINT_RESTORE: u32 = 40;

/// Highest valid capability number.
pub const CAP_LAST_CAP: u32 = 40;

// ---------------------------------------------------------------------------
// Capability version
// ---------------------------------------------------------------------------

/// Linux capability version 3 (64-bit, current).
pub const _LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
/// Number of u32 data elements per set.
pub const _LINUX_CAPABILITY_U32S_3: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities_distinct() {
        let caps = [
            CAP_DAC_OVERRIDE,
            CAP_DAC_READ_SEARCH,
            CAP_FOWNER,
            CAP_FSETID,
            CAP_KILL,
            CAP_SETGID,
            CAP_SETUID,
            CAP_SETPCAP,
            CAP_LINUX_IMMUTABLE,
            CAP_NET_BIND_SERVICE,
            CAP_NET_BROADCAST,
            CAP_NET_ADMIN,
            CAP_NET_RAW,
            CAP_IPC_LOCK,
            CAP_IPC_OWNER,
            CAP_SYS_MODULE,
            CAP_SYS_RAWIO,
            CAP_SYS_CHROOT,
            CAP_SYS_PTRACE,
            CAP_SYS_PACCT,
            CAP_SYS_ADMIN,
            CAP_SYS_BOOT,
            CAP_SYS_NICE,
            CAP_SYS_RESOURCE,
            CAP_SYS_TIME,
            CAP_SYS_TTY_CONFIG,
            CAP_MKNOD,
            CAP_LEASE,
            CAP_AUDIT_WRITE,
            CAP_AUDIT_CONTROL,
            CAP_SETFCAP,
            CAP_MAC_OVERRIDE,
            CAP_MAC_ADMIN,
            CAP_SYSLOG,
            CAP_WAKE_ALARM,
            CAP_BLOCK_SUSPEND,
            CAP_AUDIT_READ,
            CAP_PERFMON,
            CAP_BPF,
            CAP_CHECKPOINT_RESTORE,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_cap_range() {
        let caps = [
            CAP_DAC_OVERRIDE,
            CAP_DAC_READ_SEARCH,
            CAP_FOWNER,
            CAP_FSETID,
            CAP_KILL,
            CAP_SETGID,
            CAP_SETUID,
            CAP_SETPCAP,
            CAP_LINUX_IMMUTABLE,
            CAP_NET_BIND_SERVICE,
            CAP_NET_BROADCAST,
            CAP_NET_ADMIN,
            CAP_NET_RAW,
            CAP_IPC_LOCK,
            CAP_IPC_OWNER,
            CAP_SYS_MODULE,
            CAP_SYS_RAWIO,
            CAP_SYS_CHROOT,
            CAP_SYS_PTRACE,
            CAP_SYS_PACCT,
            CAP_SYS_ADMIN,
            CAP_SYS_BOOT,
            CAP_SYS_NICE,
            CAP_SYS_RESOURCE,
            CAP_SYS_TIME,
            CAP_SYS_TTY_CONFIG,
            CAP_MKNOD,
            CAP_LEASE,
            CAP_AUDIT_WRITE,
            CAP_AUDIT_CONTROL,
            CAP_SETFCAP,
            CAP_MAC_OVERRIDE,
            CAP_MAC_ADMIN,
            CAP_SYSLOG,
            CAP_WAKE_ALARM,
            CAP_BLOCK_SUSPEND,
            CAP_AUDIT_READ,
            CAP_PERFMON,
            CAP_BPF,
            CAP_CHECKPOINT_RESTORE,
        ];
        for &c in &caps {
            assert!(c <= CAP_LAST_CAP);
        }
    }

    #[test]
    fn test_version_nonzero() {
        assert_ne!(_LINUX_CAPABILITY_VERSION_3, 0);
        assert_eq!(_LINUX_CAPABILITY_U32S_3, 2);
    }
}

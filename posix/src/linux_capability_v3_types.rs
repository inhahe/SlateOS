//! `<linux/capability.h>` — Linux capability number constants.
//!
//! Capabilities split the privileges of the superuser into distinct
//! units that can be independently enabled/disabled per thread.
//! These constants define the individual capability numbers.

// ---------------------------------------------------------------------------
// Capability numbers (CAP_*)
// ---------------------------------------------------------------------------

/// Override DAC access restrictions.
pub const CAP_DAC_OVERRIDE: u32 = 1;
/// Override DAC read restrictions.
pub const CAP_DAC_READ_SEARCH: u32 = 2;
/// Override file ownership restrictions.
pub const CAP_FOWNER: u32 = 3;
/// Don't clear set-uid/set-gid on write.
pub const CAP_FSETID: u32 = 4;
/// Kill any process.
pub const CAP_KILL: u32 = 5;
/// Set GID.
pub const CAP_SETGID: u32 = 6;
/// Set UID.
pub const CAP_SETUID: u32 = 7;
/// Set capabilities on files and processes.
pub const CAP_SETPCAP: u32 = 8;
/// Set immutable/append file attributes.
pub const CAP_LINUX_IMMUTABLE: u32 = 9;
/// Bind to privileged ports (< 1024).
pub const CAP_NET_BIND_SERVICE: u32 = 10;
/// Configure network interfaces.
pub const CAP_NET_ADMIN: u32 = 12;
/// Use raw/packet sockets.
pub const CAP_NET_RAW: u32 = 13;
/// Lock memory (mlock, mlockall).
pub const CAP_IPC_LOCK: u32 = 14;
/// Override IPC ownership checks.
pub const CAP_IPC_OWNER: u32 = 15;
/// Load/unload kernel modules.
pub const CAP_SYS_MODULE: u32 = 16;
/// Perform raw I/O (iopl, ioperm).
pub const CAP_SYS_RAWIO: u32 = 17;
/// chroot().
pub const CAP_SYS_CHROOT: u32 = 18;
/// ptrace any process.
pub const CAP_SYS_PTRACE: u32 = 19;
/// Configure accounting.
pub const CAP_SYS_PACCT: u32 = 20;
/// System administration (mount, sethostname, etc.).
pub const CAP_SYS_ADMIN: u32 = 21;
/// Reboot the system.
pub const CAP_SYS_BOOT: u32 = 22;
/// Set scheduling priority/policy.
pub const CAP_SYS_NICE: u32 = 23;
/// Override resource limits.
pub const CAP_SYS_RESOURCE: u32 = 24;
/// Set system clock.
pub const CAP_SYS_TIME: u32 = 25;
/// Configure TTY devices.
pub const CAP_SYS_TTY_CONFIG: u32 = 26;
/// Create special files (mknod).
pub const CAP_MKNOD: u32 = 27;
/// Set file capabilities.
pub const CAP_SETFCAP: u32 = 31;
/// Override MAC (SELinux, Smack, etc.).
pub const CAP_MAC_OVERRIDE: u32 = 32;
/// Set MAC labels.
pub const CAP_MAC_ADMIN: u32 = 33;
/// Configure syslog.
pub const CAP_SYSLOG: u32 = 34;
/// Set oom_adj / oom_score_adj.
pub const CAP_WAKE_ALARM: u32 = 35;
/// Block/allow suspend.
pub const CAP_BLOCK_SUSPEND: u32 = 36;
/// Audit control.
pub const CAP_AUDIT_CONTROL: u32 = 30;
/// Write audit log.
pub const CAP_AUDIT_WRITE: u32 = 29;
/// BPF operations.
pub const CAP_BPF: u32 = 39;
/// Checkpoint/restore operations.
pub const CAP_CHECKPOINT_RESTORE: u32 = 40;
/// Performance monitoring.
pub const CAP_PERFMON: u32 = 38;

/// Last defined capability number.
pub const CAP_LAST_CAP: u32 = 40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_caps() {
        assert_eq!(CAP_DAC_OVERRIDE, 1);
        assert_eq!(CAP_NET_BIND_SERVICE, 10);
        assert_eq!(CAP_SYS_ADMIN, 21);
        assert_eq!(CAP_SETUID, 7);
    }

    #[test]
    fn test_last_cap() {
        assert_eq!(CAP_LAST_CAP, 40);
    }

    #[test]
    fn test_caps_distinct() {
        let caps = [
            CAP_DAC_OVERRIDE, CAP_DAC_READ_SEARCH, CAP_FOWNER,
            CAP_FSETID, CAP_KILL, CAP_SETGID, CAP_SETUID,
            CAP_SETPCAP, CAP_LINUX_IMMUTABLE, CAP_NET_BIND_SERVICE,
            CAP_NET_ADMIN, CAP_NET_RAW, CAP_IPC_LOCK, CAP_IPC_OWNER,
            CAP_SYS_MODULE, CAP_SYS_RAWIO, CAP_SYS_CHROOT,
            CAP_SYS_PTRACE, CAP_SYS_PACCT, CAP_SYS_ADMIN,
            CAP_SYS_BOOT, CAP_SYS_NICE, CAP_SYS_RESOURCE,
            CAP_SYS_TIME, CAP_SYS_TTY_CONFIG, CAP_MKNOD,
            CAP_SETFCAP, CAP_MAC_OVERRIDE, CAP_MAC_ADMIN,
            CAP_SYSLOG, CAP_WAKE_ALARM, CAP_BLOCK_SUSPEND,
            CAP_AUDIT_CONTROL, CAP_AUDIT_WRITE,
            CAP_BPF, CAP_CHECKPOINT_RESTORE, CAP_PERFMON,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }
}

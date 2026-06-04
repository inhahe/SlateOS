//! `<linux/audit.h>` continuation — audit record types (`AUDIT_*` events).
//!
//! The Linux audit subsystem emits records with numeric type codes
//! that classify events (syscall, login, AVC, MAC, etc.). Userspace
//! daemons (`auditd`, `auparse`) match on these codes when filtering.

// ---------------------------------------------------------------------------
// Class boundaries — records are grouped into ranges
// ---------------------------------------------------------------------------

pub const AUDIT_FIRST_USER_MSG: u32 = 1100;
pub const AUDIT_LAST_USER_MSG: u32 = 1199;
pub const AUDIT_FIRST_USER_MSG2: u32 = 2100;
pub const AUDIT_LAST_USER_MSG2: u32 = 2999;
pub const AUDIT_FIRST_KERN_MSG: u32 = 1300;
pub const AUDIT_LAST_KERN_MSG: u32 = 1399;
pub const AUDIT_FIRST_AVC: u32 = 1400;
pub const AUDIT_LAST_AVC: u32 = 1499;
pub const AUDIT_FIRST_INTEGRITY: u32 = 1800;
pub const AUDIT_LAST_INTEGRITY: u32 = 1899;

// ---------------------------------------------------------------------------
// Common event codes (subset users actually filter on)
// ---------------------------------------------------------------------------

pub const AUDIT_SYSCALL: u32 = 1300;
pub const AUDIT_PATH: u32 = 1302;
pub const AUDIT_IPC: u32 = 1303;
pub const AUDIT_SOCKETCALL: u32 = 1304;
pub const AUDIT_CONFIG_CHANGE: u32 = 1305;
pub const AUDIT_SOCKADDR: u32 = 1306;
pub const AUDIT_CWD: u32 = 1307;
pub const AUDIT_EXECVE: u32 = 1309;
pub const AUDIT_FD_PAIR: u32 = 1331;

pub const AUDIT_AVC: u32 = 1400;
pub const AUDIT_SELINUX_ERR: u32 = 1401;
pub const AUDIT_AVC_PATH: u32 = 1402;
pub const AUDIT_MAC_POLICY_LOAD: u32 = 1403;

pub const AUDIT_USER_LOGIN: u32 = 1112;
pub const AUDIT_USER_LOGOUT: u32 = 1113;
pub const AUDIT_USER_AUTH: u32 = 1100;
pub const AUDIT_USER_ACCT: u32 = 1101;
pub const AUDIT_USER_MGMT: u32 = 1116;
pub const AUDIT_USER_AVC: u32 = 1107;

pub const AUDIT_DAEMON_START: u32 = 1200;
pub const AUDIT_DAEMON_END: u32 = 1201;
pub const AUDIT_DAEMON_ABORT: u32 = 1202;
pub const AUDIT_DAEMON_CONFIG: u32 = 1203;
pub const AUDIT_DAEMON_RECONFIG: u32 = 1204;
pub const AUDIT_DAEMON_ROTATE: u32 = 1205;

pub const AUDIT_KERNEL: u32 = 2000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_class_ranges_well_ordered() {
        // Each (first, last) pair has first <= last and ranges don't overlap.
        let ranges = [
            (AUDIT_FIRST_USER_MSG, AUDIT_LAST_USER_MSG),
            (AUDIT_FIRST_KERN_MSG, AUDIT_LAST_KERN_MSG),
            (AUDIT_FIRST_AVC, AUDIT_LAST_AVC),
            (AUDIT_FIRST_INTEGRITY, AUDIT_LAST_INTEGRITY),
            (AUDIT_FIRST_USER_MSG2, AUDIT_LAST_USER_MSG2),
        ];
        for (a, b) in ranges {
            assert!(a <= b);
            assert_eq!(b % 100, 99);
        }
        // User2 starts above the integrity range.
        assert!(AUDIT_FIRST_USER_MSG2 > AUDIT_LAST_INTEGRITY);
    }

    #[test]
    fn test_syscall_events_in_kern_range() {
        for v in [
            AUDIT_SYSCALL,
            AUDIT_PATH,
            AUDIT_IPC,
            AUDIT_SOCKETCALL,
            AUDIT_CONFIG_CHANGE,
            AUDIT_SOCKADDR,
            AUDIT_CWD,
            AUDIT_EXECVE,
            AUDIT_FD_PAIR,
        ] {
            assert!(v >= AUDIT_FIRST_KERN_MSG && v <= AUDIT_LAST_KERN_MSG);
        }
        // SYSCALL is the range opener.
        assert_eq!(AUDIT_SYSCALL, AUDIT_FIRST_KERN_MSG);
    }

    #[test]
    fn test_avc_events_in_avc_range() {
        for v in [
            AUDIT_AVC,
            AUDIT_SELINUX_ERR,
            AUDIT_AVC_PATH,
            AUDIT_MAC_POLICY_LOAD,
        ] {
            assert!(v >= AUDIT_FIRST_AVC && v <= AUDIT_LAST_AVC);
        }
        assert_eq!(AUDIT_AVC, AUDIT_FIRST_AVC);
    }

    #[test]
    fn test_user_events_in_user_range() {
        // All AUDIT_USER_* belong to the user-msg range 1100..1199.
        for v in [
            AUDIT_USER_AUTH,
            AUDIT_USER_ACCT,
            AUDIT_USER_LOGIN,
            AUDIT_USER_LOGOUT,
            AUDIT_USER_MGMT,
            AUDIT_USER_AVC,
        ] {
            assert!(v >= AUDIT_FIRST_USER_MSG);
            assert!(v <= AUDIT_LAST_USER_MSG);
        }
    }

    #[test]
    fn test_daemon_events_dense_1200_to_1205() {
        let d = [
            AUDIT_DAEMON_START,
            AUDIT_DAEMON_END,
            AUDIT_DAEMON_ABORT,
            AUDIT_DAEMON_CONFIG,
            AUDIT_DAEMON_RECONFIG,
            AUDIT_DAEMON_ROTATE,
        ];
        for (i, &v) in d.iter().enumerate() {
            assert_eq!(v as usize, 1200 + i);
        }
    }

    #[test]
    fn test_kernel_event_sits_between_class_ranges() {
        // AUDIT_KERNEL (2000) is a standalone marker emitted by the
        // kernel during boot. It deliberately sits in the 100-wide gap
        // between the integrity range (1800..1899) and the start of
        // USER_MSG2 (2100..2999), so it never collides with either.
        assert_eq!(AUDIT_KERNEL, 2000);
        assert!(AUDIT_KERNEL > AUDIT_LAST_INTEGRITY);
        assert!(AUDIT_KERNEL < AUDIT_FIRST_USER_MSG2);
    }
}

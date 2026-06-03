//! `<linux/audit.h>` — netlink-AUDIT message-type ABI.
//!
//! auditd and `audisp-*` plugins read NETLINK_AUDIT messages out of
//! the kernel; rules are written via AUDIT_ADD_RULE. The message-type
//! numbers below carve audit traffic into get/set/event categories.

// ---------------------------------------------------------------------------
// Netlink family
// ---------------------------------------------------------------------------

/// `NETLINK_AUDIT` family number.
pub const NETLINK_AUDIT: u32 = 9;

// ---------------------------------------------------------------------------
// Message-type ranges (struct nlmsghdr.nlmsg_type)
// ---------------------------------------------------------------------------

/// Lowest audit message type.
pub const AUDIT_FIRST_USER_MSG: u32 = 1100;
/// Lowest unreserved user message.
pub const AUDIT_FIRST_USER_MSG2: u32 = 2100;
/// Lowest kernel audit message.
pub const AUDIT_FIRST_KERN_ANOM_MSG: u32 = 1700;
/// Highest user-message id.
pub const AUDIT_LAST_USER_MSG: u32 = 1199;

// ---------------------------------------------------------------------------
// Control / status
// ---------------------------------------------------------------------------

/// `AUDIT_GET` — query audit status struct.
pub const AUDIT_GET: u32 = 1000;
/// `AUDIT_SET` — set audit configuration.
pub const AUDIT_SET: u32 = 1001;
/// `AUDIT_LIST` — list rules (deprecated; use _RULES).
pub const AUDIT_LIST: u32 = 1002;
/// `AUDIT_ADD` — install a rule (deprecated; use _RULES).
pub const AUDIT_ADD: u32 = 1003;
/// `AUDIT_DEL` — remove a rule (deprecated; use _RULES).
pub const AUDIT_DEL: u32 = 1004;
/// `AUDIT_USER` — userspace-injected message.
pub const AUDIT_USER: u32 = 1005;
/// `AUDIT_LOGIN` — login event.
pub const AUDIT_LOGIN: u32 = 1006;
/// `AUDIT_LIST_RULES` — list current rules.
pub const AUDIT_LIST_RULES: u32 = 1013;
/// `AUDIT_ADD_RULE` — install a new rule (current API).
pub const AUDIT_ADD_RULE: u32 = 1011;
/// `AUDIT_DEL_RULE` — delete a rule.
pub const AUDIT_DEL_RULE: u32 = 1012;
/// `AUDIT_TRIM` — trim trees.
pub const AUDIT_TRIM: u32 = 1014;
/// `AUDIT_MAKE_EQUIV` — equivalence class.
pub const AUDIT_MAKE_EQUIV: u32 = 1015;
/// `AUDIT_TTY_GET` — query TTY-input recording.
pub const AUDIT_TTY_GET: u32 = 1016;
/// `AUDIT_TTY_SET` — set TTY-input recording.
pub const AUDIT_TTY_SET: u32 = 1017;
/// `AUDIT_SET_FEATURE` — toggle audit features.
pub const AUDIT_SET_FEATURE: u32 = 1018;
/// `AUDIT_GET_FEATURE` — query audit features.
pub const AUDIT_GET_FEATURE: u32 = 1019;

// ---------------------------------------------------------------------------
// Event categories (subset that every userspace audit tool handles)
// ---------------------------------------------------------------------------

/// `AUDIT_SYSCALL` — syscall entry/exit record.
pub const AUDIT_SYSCALL: u32 = 1300;
/// `AUDIT_PATH` — accompanying path.
pub const AUDIT_PATH: u32 = 1302;
/// `AUDIT_IPC` — IPC permissions record.
pub const AUDIT_IPC: u32 = 1303;
/// `AUDIT_CONFIG_CHANGE`.
pub const AUDIT_CONFIG_CHANGE: u32 = 1305;
/// `AUDIT_CWD` — current working directory at syscall entry.
pub const AUDIT_CWD: u32 = 1307;
/// `AUDIT_EXECVE` — execve argv/envp.
pub const AUDIT_EXECVE: u32 = 1309;
/// `AUDIT_EOE` — end of event marker.
pub const AUDIT_EOE: u32 = 1320;
/// `AUDIT_PROCTITLE` — process title.
pub const AUDIT_PROCTITLE: u32 = 1327;

// ---------------------------------------------------------------------------
// Status fields (struct audit_status)
// ---------------------------------------------------------------------------

/// `AUDIT_STATUS_ENABLED` — audit is on.
pub const AUDIT_STATUS_ENABLED: u32 = 0x0001;
/// `AUDIT_STATUS_FAILURE` — failure-mode field is set.
pub const AUDIT_STATUS_FAILURE: u32 = 0x0002;
/// `AUDIT_STATUS_PID` — audit's pid field is set.
pub const AUDIT_STATUS_PID: u32 = 0x0004;
/// `AUDIT_STATUS_RATE_LIMIT` — rate-limit is set.
pub const AUDIT_STATUS_RATE_LIMIT: u32 = 0x0008;
/// `AUDIT_STATUS_BACKLOG_LIMIT`.
pub const AUDIT_STATUS_BACKLOG_LIMIT: u32 = 0x0010;
/// `AUDIT_STATUS_BACKLOG_WAIT_TIME`.
pub const AUDIT_STATUS_BACKLOG_WAIT_TIME: u32 = 0x0020;
/// `AUDIT_STATUS_LOST` — lost-record count is set.
pub const AUDIT_STATUS_LOST: u32 = 0x0040;

// ---------------------------------------------------------------------------
// Failure modes (audit_status.failure)
// ---------------------------------------------------------------------------

/// Silent — drop records on overflow.
pub const AUDIT_FAIL_SILENT: u32 = 0;
/// Printk — log overflow via printk.
pub const AUDIT_FAIL_PRINTK: u32 = 1;
/// Panic — panic the kernel on overflow.
pub const AUDIT_FAIL_PANIC: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_netlink_family() {
        assert_eq!(NETLINK_AUDIT, 9);
    }

    #[test]
    fn test_message_ranges_ordered() {
        assert!(AUDIT_FIRST_USER_MSG < AUDIT_LAST_USER_MSG);
        assert!(AUDIT_LAST_USER_MSG < AUDIT_FIRST_KERN_ANOM_MSG);
        assert!(AUDIT_FIRST_KERN_ANOM_MSG < AUDIT_FIRST_USER_MSG2);
        // The kernel reserves blocks of 100 to keep room for new
        // message types.
        assert_eq!(AUDIT_FIRST_USER_MSG, 1100);
        assert_eq!(AUDIT_LAST_USER_MSG, 1199);
    }

    #[test]
    fn test_control_messages_distinct() {
        let m = [
            AUDIT_GET,
            AUDIT_SET,
            AUDIT_LIST,
            AUDIT_ADD,
            AUDIT_DEL,
            AUDIT_USER,
            AUDIT_LOGIN,
            AUDIT_LIST_RULES,
            AUDIT_ADD_RULE,
            AUDIT_DEL_RULE,
            AUDIT_TRIM,
            AUDIT_MAKE_EQUIV,
            AUDIT_TTY_GET,
            AUDIT_TTY_SET,
            AUDIT_SET_FEATURE,
            AUDIT_GET_FEATURE,
        ];
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
            // Control messages live in 1000..1099.
            assert!(m[i] >= 1000 && m[i] < AUDIT_FIRST_USER_MSG);
        }
    }

    #[test]
    fn test_event_messages_distinct_and_in_kernel_range() {
        let e = [
            AUDIT_SYSCALL,
            AUDIT_PATH,
            AUDIT_IPC,
            AUDIT_CONFIG_CHANGE,
            AUDIT_CWD,
            AUDIT_EXECVE,
            AUDIT_EOE,
            AUDIT_PROCTITLE,
        ];
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
            // Kernel events live in 1300..1399.
            assert!(e[i] >= 1300 && e[i] < 1400);
        }
    }

    #[test]
    fn test_status_flags_pow2_distinct() {
        let f = [
            AUDIT_STATUS_ENABLED,
            AUDIT_STATUS_FAILURE,
            AUDIT_STATUS_PID,
            AUDIT_STATUS_RATE_LIMIT,
            AUDIT_STATUS_BACKLOG_LIMIT,
            AUDIT_STATUS_BACKLOG_WAIT_TIME,
            AUDIT_STATUS_LOST,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_failure_modes_dense() {
        assert_eq!(AUDIT_FAIL_SILENT, 0);
        assert_eq!(AUDIT_FAIL_PRINTK, 1);
        assert_eq!(AUDIT_FAIL_PANIC, 2);
    }
}

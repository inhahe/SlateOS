//! `<linux/audit.h>` — Audit subsystem constants (extended).
//!
//! Extended audit constants covering audit message types,
//! filter flags, field types, action masks, and audit
//! architecture constants.

// ---------------------------------------------------------------------------
// Audit message types
// ---------------------------------------------------------------------------

/// Get audit status.
pub const AUDIT_GET: u32 = 1000;
/// Set audit status.
pub const AUDIT_SET: u32 = 1001;
/// List audit rules.
pub const AUDIT_LIST: u32 = 1002;
/// Add an audit rule.
pub const AUDIT_ADD: u32 = 1003;
/// Delete an audit rule.
pub const AUDIT_DEL: u32 = 1004;
/// User space message.
pub const AUDIT_USER: u32 = 1005;
/// Login event.
pub const AUDIT_LOGIN: u32 = 1006;
/// Watch file (deprecated).
pub const AUDIT_WATCH_INS: u32 = 1007;
/// Remove watch (deprecated).
pub const AUDIT_WATCH_REM: u32 = 1008;
/// Watch list (deprecated).
pub const AUDIT_WATCH_LIST: u32 = 1009;
/// Signal info.
pub const AUDIT_SIGNAL_INFO: u32 = 1010;
/// Add a rule (data).
pub const AUDIT_ADD_RULE: u32 = 1011;
/// Delete a rule (data).
pub const AUDIT_DEL_RULE: u32 = 1012;
/// List rules (data).
pub const AUDIT_LIST_RULES: u32 = 1013;
/// Trim directory watches.
pub const AUDIT_TRIM: u32 = 1014;
/// Append to rule path.
pub const AUDIT_MAKE_EQUIV: u32 = 1015;
/// Get feature set.
pub const AUDIT_GET_FEATURE: u32 = 1019;
/// Set feature.
pub const AUDIT_SET_FEATURE: u32 = 1020;

// ---------------------------------------------------------------------------
// Audit event message types (records)
// ---------------------------------------------------------------------------

/// Syscall event.
pub const AUDIT_SYSCALL: u32 = 1300;
/// File system path.
pub const AUDIT_PATH: u32 = 1302;
/// IPC record.
pub const AUDIT_IPC: u32 = 1303;
/// Socket address.
pub const AUDIT_SOCKADDR: u32 = 1306;
/// Current working directory.
pub const AUDIT_CWD: u32 = 1307;
/// Exec arguments.
pub const AUDIT_EXECVE: u32 = 1309;
/// IPC set permissions.
pub const AUDIT_IPC_SET_PERM: u32 = 1311;
/// BPF event.
pub const AUDIT_BPF: u32 = 1334;

// ---------------------------------------------------------------------------
// Audit filter flags (AUDIT_FILTER_*)
// ---------------------------------------------------------------------------

/// Filter on task creation.
pub const AUDIT_FILTER_TASK: u32 = 0x01;
/// Filter on syscall entry.
pub const AUDIT_FILTER_ENTRY: u32 = 0x02;
/// Filter on syscall exit.
pub const AUDIT_FILTER_EXIT: u32 = 0x04;
/// Filter for user messages.
pub const AUDIT_FILTER_USER: u32 = 0x08;
/// Filter for excludes.
pub const AUDIT_FILTER_EXCLUDE: u32 = 0x10;
/// Filter for filesystem.
pub const AUDIT_FILTER_FS: u32 = 0x20;

// ---------------------------------------------------------------------------
// Audit action masks
// ---------------------------------------------------------------------------

/// Never generate record.
pub const AUDIT_NEVER: u32 = 0;
/// Always generate record.
pub const AUDIT_ALWAYS: u32 = 1;
/// Possible (kernel internal use).
pub const AUDIT_POSSIBLE: u32 = 2;

// ---------------------------------------------------------------------------
// Audit field types (AUDIT_*)
// ---------------------------------------------------------------------------

/// Process ID.
pub const AUDIT_PID: u32 = 0;
/// User ID.
pub const AUDIT_UID: u32 = 1;
/// Effective UID.
pub const AUDIT_EUID: u32 = 2;
/// Saved UID.
pub const AUDIT_SUID: u32 = 3;
/// FS UID.
pub const AUDIT_FSUID: u32 = 4;
/// Group ID.
pub const AUDIT_GID: u32 = 5;
/// Effective GID.
pub const AUDIT_EGID: u32 = 6;
/// Saved GID.
pub const AUDIT_SGID: u32 = 7;
/// FS GID.
pub const AUDIT_FSGID: u32 = 8;
/// Login UID.
pub const AUDIT_LOGINUID: u32 = 9;
/// Architecture.
pub const AUDIT_ARCH: u32 = 11;
/// Personality.
pub const AUDIT_PERS: u32 = 10;
/// Exit code.
pub const AUDIT_EXIT: u32 = 103;
/// Success/failure.
pub const AUDIT_SUCCESS: u32 = 104;

// ---------------------------------------------------------------------------
// Audit comparison operators
// ---------------------------------------------------------------------------

/// Bit mask comparison.
pub const AUDIT_BIT_MASK: u32 = 0x08000000;
/// Less than.
pub const AUDIT_LESS_THAN: u32 = 0x10000000;
/// Greater than.
pub const AUDIT_GREATER_THAN: u32 = 0x20000000;
/// Not equal.
pub const AUDIT_NOT_EQUAL: u32 = 0x30000000;
/// Equal.
pub const AUDIT_EQUAL: u32 = 0x40000000;
/// Bit test.
pub const AUDIT_BIT_TEST: u32 = 0x48000000;
/// Less than or equal.
pub const AUDIT_LESS_THAN_OR_EQUAL: u32 = 0x50000000;
/// Greater than or equal.
pub const AUDIT_GREATER_THAN_OR_EQUAL: u32 = 0x60000000;
/// Operator mask.
pub const AUDIT_OPERATORS: u32 = 0x78000000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        let msgs = [
            AUDIT_GET,
            AUDIT_SET,
            AUDIT_LIST,
            AUDIT_ADD,
            AUDIT_DEL,
            AUDIT_USER,
            AUDIT_LOGIN,
            AUDIT_WATCH_INS,
            AUDIT_WATCH_REM,
            AUDIT_WATCH_LIST,
            AUDIT_SIGNAL_INFO,
            AUDIT_ADD_RULE,
            AUDIT_DEL_RULE,
            AUDIT_LIST_RULES,
            AUDIT_TRIM,
            AUDIT_MAKE_EQUIV,
            AUDIT_GET_FEATURE,
            AUDIT_SET_FEATURE,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_get_is_1000() {
        assert_eq!(AUDIT_GET, 1000);
    }

    #[test]
    fn test_event_types_distinct() {
        let evts = [
            AUDIT_SYSCALL,
            AUDIT_PATH,
            AUDIT_IPC,
            AUDIT_SOCKADDR,
            AUDIT_CWD,
            AUDIT_EXECVE,
            AUDIT_IPC_SET_PERM,
            AUDIT_BPF,
        ];
        for i in 0..evts.len() {
            for j in (i + 1)..evts.len() {
                assert_ne!(evts[i], evts[j]);
            }
        }
    }

    #[test]
    fn test_filter_flags_no_overlap() {
        let flags = [
            AUDIT_FILTER_TASK,
            AUDIT_FILTER_ENTRY,
            AUDIT_FILTER_EXIT,
            AUDIT_FILTER_USER,
            AUDIT_FILTER_EXCLUDE,
            AUDIT_FILTER_FS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_filter_flags_are_powers_of_two() {
        let flags = [
            AUDIT_FILTER_TASK,
            AUDIT_FILTER_ENTRY,
            AUDIT_FILTER_EXIT,
            AUDIT_FILTER_USER,
            AUDIT_FILTER_EXCLUDE,
            AUDIT_FILTER_FS,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_actions_distinct() {
        let acts = [AUDIT_NEVER, AUDIT_ALWAYS, AUDIT_POSSIBLE];
        for i in 0..acts.len() {
            for j in (i + 1)..acts.len() {
                assert_ne!(acts[i], acts[j]);
            }
        }
    }

    #[test]
    fn test_field_types_distinct() {
        let fields = [
            AUDIT_PID,
            AUDIT_UID,
            AUDIT_EUID,
            AUDIT_SUID,
            AUDIT_FSUID,
            AUDIT_GID,
            AUDIT_EGID,
            AUDIT_SGID,
            AUDIT_FSGID,
            AUDIT_LOGINUID,
            AUDIT_ARCH,
            AUDIT_PERS,
            AUDIT_EXIT,
            AUDIT_SUCCESS,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }

    #[test]
    fn test_operators_distinct() {
        let ops = [
            AUDIT_BIT_MASK,
            AUDIT_LESS_THAN,
            AUDIT_GREATER_THAN,
            AUDIT_NOT_EQUAL,
            AUDIT_EQUAL,
            AUDIT_BIT_TEST,
            AUDIT_LESS_THAN_OR_EQUAL,
            AUDIT_GREATER_THAN_OR_EQUAL,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_never_is_zero() {
        assert_eq!(AUDIT_NEVER, 0);
    }
}

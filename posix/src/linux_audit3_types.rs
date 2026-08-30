//! `<linux/audit.h>` — Additional Linux audit constants.
//!
//! Supplementary audit constants covering message types,
//! filter lists, action types, and field comparisons.

// ---------------------------------------------------------------------------
// Audit filter lists (AUDIT_FILTER_*)
// ---------------------------------------------------------------------------

/// User filter.
pub const AUDIT_FILTER_USER: u32 = 0x00;
/// Task filter.
pub const AUDIT_FILTER_TASK: u32 = 0x01;
/// Entry filter (deprecated).
pub const AUDIT_FILTER_ENTRY: u32 = 0x02;
/// Watch filter.
pub const AUDIT_FILTER_WATCH: u32 = 0x03;
/// Exit filter.
pub const AUDIT_FILTER_EXIT: u32 = 0x04;
/// Filesystem filter.
pub const AUDIT_FILTER_FS: u32 = 0x06;

// ---------------------------------------------------------------------------
// Audit actions (AUDIT_*)
// ---------------------------------------------------------------------------

/// Never audit.
pub const AUDIT_NEVER: u32 = 0;
/// Possible audit.
pub const AUDIT_POSSIBLE: u32 = 1;
/// Always audit.
pub const AUDIT_ALWAYS: u32 = 2;

// ---------------------------------------------------------------------------
// Audit fields (AUDIT_*)
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
/// Personality.
pub const AUDIT_PERS: u32 = 10;
/// Architecture.
pub const AUDIT_ARCH: u32 = 11;
/// Message type.
pub const AUDIT_MSGTYPE: u32 = 12;
/// Subject user.
pub const AUDIT_SUBJ_USER: u32 = 13;
/// Subject role.
pub const AUDIT_SUBJ_ROLE: u32 = 14;
/// Subject type.
pub const AUDIT_SUBJ_TYPE: u32 = 15;
/// Subject sensitivity.
pub const AUDIT_SUBJ_SEN: u32 = 16;
/// Subject clearance.
pub const AUDIT_SUBJ_CLR: u32 = 17;
/// PPID.
pub const AUDIT_PPID: u32 = 18;
/// Exit code.
pub const AUDIT_EXIT: u32 = 103;
/// Success/failure.
pub const AUDIT_SUCCESS: u32 = 104;

// ---------------------------------------------------------------------------
// Audit field operators
// ---------------------------------------------------------------------------

/// Bitmask AND.
pub const AUDIT_BIT_MASK: u32 = 0x08000000;
/// Less than.
pub const AUDIT_LESS_THAN: u32 = 0x10000000;
/// Greater than.
pub const AUDIT_GREATER_THAN: u32 = 0x20000000;
/// Not equal.
pub const AUDIT_NOT_EQUAL: u32 = 0x30000000;
/// Equal.
pub const AUDIT_EQUAL: u32 = 0x40000000;
/// Bitmask test.
pub const AUDIT_BIT_TEST: u32 = 0x48000000;
/// Less or equal.
pub const AUDIT_LESS_THAN_OR_EQUAL: u32 = 0x50000000;
/// Greater or equal.
pub const AUDIT_GREATER_THAN_OR_EQUAL: u32 = 0x60000000;
/// Operators mask.
pub const AUDIT_OPERATORS: u32 = 0x78000000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_lists_distinct() {
        let lists = [
            AUDIT_FILTER_USER,
            AUDIT_FILTER_TASK,
            AUDIT_FILTER_ENTRY,
            AUDIT_FILTER_WATCH,
            AUDIT_FILTER_EXIT,
            AUDIT_FILTER_FS,
        ];
        for i in 0..lists.len() {
            for j in (i + 1)..lists.len() {
                assert_ne!(lists[i], lists[j]);
            }
        }
    }

    #[test]
    fn test_actions_sequential() {
        assert_eq!(AUDIT_NEVER, 0);
        assert_eq!(AUDIT_POSSIBLE, 1);
        assert_eq!(AUDIT_ALWAYS, 2);
    }

    #[test]
    fn test_fields_distinct() {
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
            AUDIT_PERS,
            AUDIT_ARCH,
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
}

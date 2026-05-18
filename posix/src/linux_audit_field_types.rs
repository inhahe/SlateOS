//! `<linux/audit.h>` — Audit filter field and operator constants.
//!
//! Audit rules match on fields (UID, GID, PID, syscall number, etc.)
//! using comparison operators. The kernel evaluates these rules in
//! the audit filter to decide which events to log. Fields are
//! combined with AND logic within a single rule.

// ---------------------------------------------------------------------------
// Audit filter field IDs
// ---------------------------------------------------------------------------

/// Process ID field.
pub const AUDIT_PID: u32 = 0;
/// User ID field.
pub const AUDIT_UID: u32 = 1;
/// Effective UID field.
pub const AUDIT_EUID: u32 = 2;
/// Saved-set UID field.
pub const AUDIT_SUID: u32 = 3;
/// Filesystem UID field.
pub const AUDIT_FSUID: u32 = 4;
/// Group ID field.
pub const AUDIT_GID: u32 = 5;
/// Effective GID field.
pub const AUDIT_EGID: u32 = 6;
/// Saved-set GID field.
pub const AUDIT_SGID: u32 = 7;
/// Filesystem GID field.
pub const AUDIT_FSGID: u32 = 8;
/// Login UID field (auid).
pub const AUDIT_LOGINUID: u32 = 9;
/// Architecture field.
pub const AUDIT_ARCH: u32 = 11;
/// Syscall number field.
pub const AUDIT_MSGTYPE: u32 = 12;
/// Personality (execution domain) field.
pub const AUDIT_PERS: u32 = 10;
/// Object UID (file owner).
pub const AUDIT_OBJ_UID: u32 = 21;
/// Object GID (file group).
pub const AUDIT_OBJ_GID: u32 = 22;
/// Process PPid.
pub const AUDIT_PPID: u32 = 18;
/// Exit value field.
pub const AUDIT_EXIT: u32 = 103;
/// Executable path field.
pub const AUDIT_EXE: u32 = 112;

// ---------------------------------------------------------------------------
// Audit filter comparison operators
// ---------------------------------------------------------------------------

/// Equal to.
pub const AUDIT_EQUAL: u32 = 0;
/// Not equal to.
pub const AUDIT_NOT_EQUAL: u32 = 1;
/// Less than.
pub const AUDIT_LESS_THAN: u32 = 2;
/// Less than or equal to.
pub const AUDIT_LESS_THAN_OR_EQUAL: u32 = 3;
/// Greater than.
pub const AUDIT_GREATER_THAN: u32 = 4;
/// Greater than or equal to.
pub const AUDIT_GREATER_THAN_OR_EQUAL: u32 = 5;
/// Bitmask test (field & value != 0).
pub const AUDIT_BIT_MASK: u32 = 6;
/// Bitmask test (field & value == value).
pub const AUDIT_BIT_TEST: u32 = 7;

// ---------------------------------------------------------------------------
// Audit filter list IDs
// ---------------------------------------------------------------------------

/// User-space message filter.
pub const AUDIT_FILTER_USER: u32 = 0;
/// Task creation filter (fork/clone).
pub const AUDIT_FILTER_TASK: u32 = 1;
/// Syscall entry filter.
pub const AUDIT_FILTER_ENTRY: u32 = 2;
/// Watch filter (filesystem watches, deprecated).
pub const AUDIT_FILTER_WATCH: u32 = 3;
/// Syscall exit filter (main filter point).
pub const AUDIT_FILTER_EXIT: u32 = 4;
/// Filesystem filter type.
pub const AUDIT_FILTER_FS: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_ids_distinct() {
        let fields = [
            AUDIT_PID, AUDIT_UID, AUDIT_EUID, AUDIT_SUID,
            AUDIT_FSUID, AUDIT_GID, AUDIT_EGID, AUDIT_SGID,
            AUDIT_FSGID, AUDIT_LOGINUID, AUDIT_ARCH, AUDIT_MSGTYPE,
            AUDIT_PERS, AUDIT_OBJ_UID, AUDIT_OBJ_GID, AUDIT_PPID,
            AUDIT_EXIT, AUDIT_EXE,
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
            AUDIT_EQUAL, AUDIT_NOT_EQUAL, AUDIT_LESS_THAN,
            AUDIT_LESS_THAN_OR_EQUAL, AUDIT_GREATER_THAN,
            AUDIT_GREATER_THAN_OR_EQUAL, AUDIT_BIT_MASK,
            AUDIT_BIT_TEST,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_filter_lists_distinct() {
        let filters = [
            AUDIT_FILTER_USER, AUDIT_FILTER_TASK, AUDIT_FILTER_ENTRY,
            AUDIT_FILTER_WATCH, AUDIT_FILTER_EXIT, AUDIT_FILTER_FS,
        ];
        for i in 0..filters.len() {
            for j in (i + 1)..filters.len() {
                assert_ne!(filters[i], filters[j]);
            }
        }
    }
}

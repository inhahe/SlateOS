//! `<linux/audit.h>` — `AUDIT_*` rule field identifiers.
//!
//! Each comparison in an `auditctl` rule names a field
//! (e.g. `pid`, `uid`, `arch`, `path`). The kernel encodes these as
//! small u32 tags shared between the inbound rule message and the
//! outbound record's `key=value` formatting.

// ---------------------------------------------------------------------------
// Process identity
// ---------------------------------------------------------------------------

pub const AUDIT_PID: u32 = 0;
pub const AUDIT_UID: u32 = 1;
pub const AUDIT_EUID: u32 = 2;
pub const AUDIT_SUID: u32 = 3;
pub const AUDIT_FSUID: u32 = 4;
pub const AUDIT_GID: u32 = 5;
pub const AUDIT_EGID: u32 = 6;
pub const AUDIT_SGID: u32 = 7;
pub const AUDIT_FSGID: u32 = 8;
pub const AUDIT_LOGINUID: u32 = 9;

// ---------------------------------------------------------------------------
// Path / filesystem
// ---------------------------------------------------------------------------

pub const AUDIT_PERS: u32 = 10;
pub const AUDIT_ARCH: u32 = 11;
pub const AUDIT_MSGTYPE: u32 = 12;

pub const AUDIT_DEVMAJOR: u32 = 100;
pub const AUDIT_DEVMINOR: u32 = 101;
pub const AUDIT_INODE: u32 = 102;
pub const AUDIT_EXIT: u32 = 103;
pub const AUDIT_SUCCESS: u32 = 104;
pub const AUDIT_WATCH: u32 = 105;
pub const AUDIT_PERM: u32 = 106;
pub const AUDIT_DIR: u32 = 107;
pub const AUDIT_FILETYPE: u32 = 108;
pub const AUDIT_OBJ_UID: u32 = 109;
pub const AUDIT_OBJ_GID: u32 = 110;
pub const AUDIT_FIELD_COMPARE: u32 = 111;
pub const AUDIT_EXE: u32 = 112;
pub const AUDIT_SADDR_FAM: u32 = 113;

// ---------------------------------------------------------------------------
// Syscall arguments
// ---------------------------------------------------------------------------

pub const AUDIT_ARG0: u32 = 200;
pub const AUDIT_ARG1: u32 = 201;
pub const AUDIT_ARG2: u32 = 202;
pub const AUDIT_ARG3: u32 = 203;

// ---------------------------------------------------------------------------
// Permission mask bits (`-F perm=` accepts r,w,x,a)
// ---------------------------------------------------------------------------

pub const AUDIT_PERM_EXEC: u32 = 1;
pub const AUDIT_PERM_WRITE: u32 = 2;
pub const AUDIT_PERM_READ: u32 = 4;
pub const AUDIT_PERM_ATTR: u32 = 8;

// ---------------------------------------------------------------------------
// `field_compare` operand encodings (intra-record cross-references)
// ---------------------------------------------------------------------------

pub const AUDIT_COMPARE_UID_TO_OBJ_UID: u32 = 1;
pub const AUDIT_COMPARE_GID_TO_OBJ_GID: u32 = 2;
pub const AUDIT_COMPARE_EUID_TO_OBJ_UID: u32 = 3;
pub const AUDIT_COMPARE_EGID_TO_OBJ_GID: u32 = 4;
pub const AUDIT_COMPARE_AUID_TO_OBJ_UID: u32 = 5;
pub const AUDIT_COMPARE_SUID_TO_OBJ_UID: u32 = 6;
pub const AUDIT_COMPARE_SGID_TO_OBJ_GID: u32 = 7;
pub const AUDIT_COMPARE_FSUID_TO_OBJ_UID: u32 = 8;
pub const AUDIT_COMPARE_FSGID_TO_OBJ_GID: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_fields_dense_0_to_9() {
        let id = [
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
        ];
        for (i, &v) in id.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_path_fields_in_100_block() {
        let p = [
            AUDIT_DEVMAJOR,
            AUDIT_DEVMINOR,
            AUDIT_INODE,
            AUDIT_EXIT,
            AUDIT_SUCCESS,
            AUDIT_WATCH,
            AUDIT_PERM,
            AUDIT_DIR,
            AUDIT_FILETYPE,
            AUDIT_OBJ_UID,
            AUDIT_OBJ_GID,
            AUDIT_FIELD_COMPARE,
            AUDIT_EXE,
            AUDIT_SADDR_FAM,
        ];
        // All in 100..=199, contiguous from 100 upward.
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, 100 + i);
        }
    }

    #[test]
    fn test_arg_fields_in_200_block_dense() {
        let a = [AUDIT_ARG0, AUDIT_ARG1, AUDIT_ARG2, AUDIT_ARG3];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, 200 + i);
        }
    }

    #[test]
    fn test_perm_bits_each_single_bit() {
        let p = [
            AUDIT_PERM_EXEC,
            AUDIT_PERM_WRITE,
            AUDIT_PERM_READ,
            AUDIT_PERM_ATTR,
        ];
        let mut or = 0;
        for &v in &p {
            assert!(v.is_power_of_two());
            or |= v;
        }
        // EXEC|WRITE|READ|ATTR == low nibble 0xF.
        assert_eq!(or, 0x0F);
    }

    #[test]
    fn test_field_compare_codes_dense_1_to_9() {
        let c = [
            AUDIT_COMPARE_UID_TO_OBJ_UID,
            AUDIT_COMPARE_GID_TO_OBJ_GID,
            AUDIT_COMPARE_EUID_TO_OBJ_UID,
            AUDIT_COMPARE_EGID_TO_OBJ_GID,
            AUDIT_COMPARE_AUID_TO_OBJ_UID,
            AUDIT_COMPARE_SUID_TO_OBJ_UID,
            AUDIT_COMPARE_SGID_TO_OBJ_GID,
            AUDIT_COMPARE_FSUID_TO_OBJ_UID,
            AUDIT_COMPARE_FSGID_TO_OBJ_GID,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, 1 + i);
        }
    }

    #[test]
    fn test_pers_arch_msgtype_dense_after_loginuid() {
        assert_eq!(AUDIT_PERS, AUDIT_LOGINUID + 1);
        assert_eq!(AUDIT_ARCH, AUDIT_PERS + 1);
        assert_eq!(AUDIT_MSGTYPE, AUDIT_ARCH + 1);
        // These three close out the 0..12 identity block; path
        // fields restart at 100.
        assert!(AUDIT_MSGTYPE < AUDIT_DEVMAJOR);
    }
}

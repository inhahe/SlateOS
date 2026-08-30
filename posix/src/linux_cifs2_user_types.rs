//! `<linux/cifs/cifs_fs_sb.h>` — CIFS info levels and search attributes.
//!
//! CIFS clients use TRANS2 (transaction-2) commands to query and set
//! file information. Each subcommand selects an "information level"
//! that determines the layout of the returned buffer. Search attributes
//! filter directory enumeration results.

// ---------------------------------------------------------------------------
// TRANS2 subcommands
// ---------------------------------------------------------------------------

pub const TRANS2_OPEN: u16 = 0x00;
pub const TRANS2_FIND_FIRST2: u16 = 0x01;
pub const TRANS2_FIND_NEXT2: u16 = 0x02;
pub const TRANS2_QUERY_FS_INFORMATION: u16 = 0x03;
pub const TRANS2_SET_FS_INFORMATION: u16 = 0x04;
pub const TRANS2_QUERY_PATH_INFORMATION: u16 = 0x05;
pub const TRANS2_SET_PATH_INFORMATION: u16 = 0x06;
pub const TRANS2_QUERY_FILE_INFORMATION: u16 = 0x07;
pub const TRANS2_SET_FILE_INFORMATION: u16 = 0x08;

// ---------------------------------------------------------------------------
// FIND_FIRST2 information levels
// ---------------------------------------------------------------------------

pub const SMB_INFO_STANDARD: u16 = 0x0001;
pub const SMB_INFO_QUERY_EA_SIZE: u16 = 0x0002;
pub const SMB_INFO_QUERY_EAS_FROM_LIST: u16 = 0x0003;
pub const SMB_FIND_FILE_DIRECTORY_INFO: u16 = 0x0101;
pub const SMB_FIND_FILE_FULL_DIRECTORY_INFO: u16 = 0x0102;
pub const SMB_FIND_FILE_NAMES_INFO: u16 = 0x0103;
pub const SMB_FIND_FILE_BOTH_DIRECTORY_INFO: u16 = 0x0104;
pub const SMB_FIND_FILE_ID_FULL_DIRECTORY_INFO: u16 = 0x0105;
pub const SMB_FIND_FILE_ID_BOTH_DIRECTORY_INFO: u16 = 0x0106;

// ---------------------------------------------------------------------------
// Search attribute flags (DOS attribute byte)
// ---------------------------------------------------------------------------

pub const ATTR_READONLY: u16 = 0x0001;
pub const ATTR_HIDDEN: u16 = 0x0002;
pub const ATTR_SYSTEM: u16 = 0x0004;
pub const ATTR_VOLUME: u16 = 0x0008;
pub const ATTR_DIRECTORY: u16 = 0x0010;
pub const ATTR_ARCHIVE: u16 = 0x0020;

// ---------------------------------------------------------------------------
// FIND_FIRST2 flags
// ---------------------------------------------------------------------------

/// Close search after first response.
pub const FIND_CLOSE_AFTER_REQUEST: u16 = 0x0001;
/// Close search when end-of-search reached.
pub const FIND_CLOSE_AT_EOS: u16 = 0x0002;
/// Return resume keys for each entry.
pub const FIND_RETURN_RESUME_KEYS: u16 = 0x0004;
/// Continue search from a previous resume key.
pub const FIND_CONTINUE_FROM_LAST: u16 = 0x0008;
/// Backup-intent search (bypass ACL checks if privileged).
pub const FIND_WITH_BACKUP_INTENT: u16 = 0x0010;

// ---------------------------------------------------------------------------
// Maximum filename length on the wire (Unicode, UCS-2 bytes)
// ---------------------------------------------------------------------------

/// Max path length on the SMB wire in bytes (UCS-2, so 260 chars * 2).
pub const SMB_MAX_PATH_LEN_BYTES: usize = 520;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trans2_subcommands_dense_0_to_8() {
        let s = [
            TRANS2_OPEN,
            TRANS2_FIND_FIRST2,
            TRANS2_FIND_NEXT2,
            TRANS2_QUERY_FS_INFORMATION,
            TRANS2_SET_FS_INFORMATION,
            TRANS2_QUERY_PATH_INFORMATION,
            TRANS2_SET_PATH_INFORMATION,
            TRANS2_QUERY_FILE_INFORMATION,
            TRANS2_SET_FILE_INFORMATION,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_info_levels_distinct() {
        let l = [
            SMB_INFO_STANDARD,
            SMB_INFO_QUERY_EA_SIZE,
            SMB_INFO_QUERY_EAS_FROM_LIST,
            SMB_FIND_FILE_DIRECTORY_INFO,
            SMB_FIND_FILE_FULL_DIRECTORY_INFO,
            SMB_FIND_FILE_NAMES_INFO,
            SMB_FIND_FILE_BOTH_DIRECTORY_INFO,
            SMB_FIND_FILE_ID_FULL_DIRECTORY_INFO,
            SMB_FIND_FILE_ID_BOTH_DIRECTORY_INFO,
        ];
        for (i, &x) in l.iter().enumerate() {
            for &y in &l[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // SMB_INFO_* in 0x00XX, SMB_FIND_FILE_* in 0x01XX
        assert_eq!(SMB_INFO_STANDARD & 0xFF00, 0x0000);
        assert_eq!(SMB_FIND_FILE_DIRECTORY_INFO & 0xFF00, 0x0100);
    }

    #[test]
    fn test_attr_flags_distinct_single_bit() {
        let a = [
            ATTR_READONLY,
            ATTR_HIDDEN,
            ATTR_SYSTEM,
            ATTR_VOLUME,
            ATTR_DIRECTORY,
            ATTR_ARCHIVE,
        ];
        for (i, &x) in a.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &a[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
        // OR of all six = 0x003F
        let or_all = a.iter().fold(0u16, |acc, &v| acc | v);
        assert_eq!(or_all, 0x003F);
    }

    #[test]
    fn test_find_flags_distinct_single_bit() {
        let f = [
            FIND_CLOSE_AFTER_REQUEST,
            FIND_CLOSE_AT_EOS,
            FIND_RETURN_RESUME_KEYS,
            FIND_CONTINUE_FROM_LAST,
            FIND_WITH_BACKUP_INTENT,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
    }

    #[test]
    fn test_max_path_len_is_260_ucs2() {
        // 260 chars (Windows MAX_PATH) * 2 bytes (UCS-2) = 520 bytes.
        assert_eq!(SMB_MAX_PATH_LEN_BYTES, 520);
        assert_eq!(SMB_MAX_PATH_LEN_BYTES / 2, 260);
    }
}

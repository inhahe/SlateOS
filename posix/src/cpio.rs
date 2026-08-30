//! `<cpio.h>` — cpio archive format constants.
//!
//! Defines the magic number and file-type/permission constants
//! for the POSIX cpio archive format (IEEE Std 1003.1).

// ---------------------------------------------------------------------------
// Magic
// ---------------------------------------------------------------------------

/// cpio magic number (octal string "070707").
pub const MAGIC: &[u8] = b"070707";

// ---------------------------------------------------------------------------
// File type bits (stored in the mode field, upper 4 bits of 16-bit value)
// ---------------------------------------------------------------------------

/// Directory.
pub const C_ISDIR: u32 = 0o040000;

/// FIFO (named pipe).
pub const C_ISFIFO: u32 = 0o010000;

/// Regular file.
pub const C_ISREG: u32 = 0o100000;

/// Block special device.
pub const C_ISBLK: u32 = 0o060000;

/// Character special device.
pub const C_ISCHR: u32 = 0o020000;

/// Network special (not widely used).
pub const C_ISCTG: u32 = 0o110000;

/// Symbolic link.
pub const C_ISLNK: u32 = 0o120000;

/// Socket.
pub const C_ISSOCK: u32 = 0o140000;

// ---------------------------------------------------------------------------
// Permission / special bits
// ---------------------------------------------------------------------------

/// Set UID on execution.
pub const C_ISUID: u32 = 0o004000;

/// Set GID on execution.
pub const C_ISGID: u32 = 0o002000;

/// Sticky bit (save text image, restricted deletion).
pub const C_ISVTX: u32 = 0o001000;

/// Owner read.
pub const C_IRUSR: u32 = 0o000400;

/// Owner write.
pub const C_IWUSR: u32 = 0o000200;

/// Owner execute.
pub const C_IXUSR: u32 = 0o000100;

/// Group read.
pub const C_IRGRP: u32 = 0o000040;

/// Group write.
pub const C_IWGRP: u32 = 0o000020;

/// Group execute.
pub const C_IXGRP: u32 = 0o000010;

/// Other read.
pub const C_IROTH: u32 = 0o000004;

/// Other write.
pub const C_IWOTH: u32 = 0o000002;

/// Other execute.
pub const C_IXOTH: u32 = 0o000001;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Magic
    // -----------------------------------------------------------------------

    #[test]
    fn test_magic() {
        assert_eq!(MAGIC, b"070707");
        assert_eq!(MAGIC.len(), 6);
    }

    #[test]
    fn test_magic_is_ascii_digits() {
        for &b in MAGIC {
            assert!(b.is_ascii_digit(), "magic should be all ASCII digits");
        }
    }

    // -----------------------------------------------------------------------
    // File type constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_file_types() {
        assert_eq!(C_ISDIR, 0o040000);
        assert_eq!(C_ISFIFO, 0o010000);
        assert_eq!(C_ISREG, 0o100000);
        assert_eq!(C_ISBLK, 0o060000);
        assert_eq!(C_ISCHR, 0o020000);
        assert_eq!(C_ISCTG, 0o110000);
        assert_eq!(C_ISLNK, 0o120000);
        assert_eq!(C_ISSOCK, 0o140000);
    }

    #[test]
    fn test_file_types_distinct() {
        let types = [
            C_ISDIR, C_ISFIFO, C_ISREG, C_ISBLK, C_ISCHR, C_ISCTG, C_ISLNK, C_ISSOCK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j], "file types should be distinct");
            }
        }
    }

    #[test]
    fn test_file_types_no_permission_overlap() {
        // File type bits (upper bits) should not overlap with
        // permission bits (lower 12 bits).
        let perm_mask = 0o007777;
        assert_eq!(C_ISDIR & perm_mask, 0);
        assert_eq!(C_ISREG & perm_mask, 0);
        assert_eq!(C_ISBLK & perm_mask, 0);
        assert_eq!(C_ISCHR & perm_mask, 0);
        assert_eq!(C_ISLNK & perm_mask, 0);
        assert_eq!(C_ISSOCK & perm_mask, 0);
        assert_eq!(C_ISFIFO & perm_mask, 0);
        assert_eq!(C_ISCTG & perm_mask, 0);
    }

    // -----------------------------------------------------------------------
    // Permission bits
    // -----------------------------------------------------------------------

    #[test]
    fn test_special_bits() {
        assert_eq!(C_ISUID, 0o004000);
        assert_eq!(C_ISGID, 0o002000);
        assert_eq!(C_ISVTX, 0o001000);
    }

    #[test]
    fn test_user_permission_bits() {
        assert_eq!(C_IRUSR, 0o000400);
        assert_eq!(C_IWUSR, 0o000200);
        assert_eq!(C_IXUSR, 0o000100);
    }

    #[test]
    fn test_group_permission_bits() {
        assert_eq!(C_IRGRP, 0o000040);
        assert_eq!(C_IWGRP, 0o000020);
        assert_eq!(C_IXGRP, 0o000010);
    }

    #[test]
    fn test_other_permission_bits() {
        assert_eq!(C_IROTH, 0o000004);
        assert_eq!(C_IWOTH, 0o000002);
        assert_eq!(C_IXOTH, 0o000001);
    }

    #[test]
    fn test_permission_bits_no_overlap() {
        // No two permission bits should share a bit.
        let perms = [
            C_ISUID, C_ISGID, C_ISVTX, C_IRUSR, C_IWUSR, C_IXUSR, C_IRGRP, C_IWGRP, C_IXGRP,
            C_IROTH, C_IWOTH, C_IXOTH,
        ];
        for i in 0..perms.len() {
            for j in (i + 1)..perms.len() {
                assert_eq!(
                    perms[i] & perms[j],
                    0,
                    "permission bits should not overlap: 0o{:o} & 0o{:o}",
                    perms[i],
                    perms[j]
                );
            }
        }
    }

    #[test]
    fn test_all_permissions_mask() {
        let all = C_ISUID
            | C_ISGID
            | C_ISVTX
            | C_IRUSR
            | C_IWUSR
            | C_IXUSR
            | C_IRGRP
            | C_IWGRP
            | C_IXGRP
            | C_IROTH
            | C_IWOTH
            | C_IXOTH;
        assert_eq!(all, 0o007777);
    }

    #[test]
    fn test_permission_shift_pattern() {
        // Group = user >> 3, other = group >> 3.
        assert_eq!(C_IRGRP, C_IRUSR >> 3);
        assert_eq!(C_IWGRP, C_IWUSR >> 3);
        assert_eq!(C_IXGRP, C_IXUSR >> 3);
        assert_eq!(C_IROTH, C_IRGRP >> 3);
        assert_eq!(C_IWOTH, C_IWGRP >> 3);
        assert_eq!(C_IXOTH, C_IXGRP >> 3);
    }

    // -----------------------------------------------------------------------
    // Consistency with tar module
    // -----------------------------------------------------------------------

    #[test]
    fn test_matches_tar_permission_values() {
        // cpio and tar use the same POSIX permission bits.
        assert_eq!(C_ISUID, crate::tar::TSUID);
        assert_eq!(C_ISGID, crate::tar::TSGID);
        assert_eq!(C_ISVTX, crate::tar::TSVTX);
        assert_eq!(C_IRUSR, crate::tar::TUREAD);
        assert_eq!(C_IWUSR, crate::tar::TUWRITE);
        assert_eq!(C_IXUSR, crate::tar::TUEXEC);
        assert_eq!(C_IRGRP, crate::tar::TGREAD);
        assert_eq!(C_IWGRP, crate::tar::TGWRITE);
        assert_eq!(C_IXGRP, crate::tar::TGEXEC);
        assert_eq!(C_IROTH, crate::tar::TOREAD);
        assert_eq!(C_IWOTH, crate::tar::TOWRITE);
        assert_eq!(C_IXOTH, crate::tar::TOEXEC);
    }

    // -----------------------------------------------------------------------
    // Typical mode combos
    // -----------------------------------------------------------------------

    #[test]
    fn test_regular_file_644() {
        let mode = C_ISREG | C_IRUSR | C_IWUSR | C_IRGRP | C_IROTH;
        assert_eq!(mode, 0o100644);
    }

    #[test]
    fn test_directory_755() {
        let mode = C_ISDIR | C_IRUSR | C_IWUSR | C_IXUSR | C_IRGRP | C_IXGRP | C_IROTH | C_IXOTH;
        assert_eq!(mode, 0o040755);
    }

    #[test]
    fn test_symlink() {
        let mode = C_ISLNK
            | C_IRUSR
            | C_IWUSR
            | C_IXUSR
            | C_IRGRP
            | C_IWGRP
            | C_IXGRP
            | C_IROTH
            | C_IWOTH
            | C_IXOTH;
        assert_eq!(mode, 0o120777);
    }
}

//! `<tar.h>` — POSIX tar archive format constants.
//!
//! Defines the magic strings, type flags, and mode bits used in
//! POSIX-format tar archives (IEEE Std 1003.1, `ustar` format).

// ---------------------------------------------------------------------------
// Magic and version
// ---------------------------------------------------------------------------

/// POSIX tar magic string (6 bytes including null).
///
/// The ustar magic is "ustar\0" in the header at offset 257.
pub const TMAGIC: &[u8] = b"ustar\0";

/// Length of `TMAGIC` including the trailing null.
pub const TMAGLEN: usize = 6;

/// POSIX tar version string (2 bytes, no null).
///
/// The version field is "00" at offset 263.
pub const TVERSION: &[u8] = b"00";

/// Length of `TVERSION`.
pub const TVERSLEN: usize = 2;

// ---------------------------------------------------------------------------
// Type flags (typeflag field in the header)
// ---------------------------------------------------------------------------

/// Regular file.
pub const REGTYPE: u8 = b'0';

/// Regular file (alternate, NUL byte — old-style tar).
pub const AREGTYPE: u8 = b'\0';

/// Link (hard link).
pub const LNKTYPE: u8 = b'1';

/// Symbolic link.
pub const SYMTYPE: u8 = b'2';

/// Character special device.
pub const CHRTYPE: u8 = b'3';

/// Block special device.
pub const BLKTYPE: u8 = b'4';

/// Directory.
pub const DIRTYPE: u8 = b'5';

/// FIFO (named pipe).
pub const FIFOTYPE: u8 = b'6';

/// Contiguous file (reserved, rarely used).
pub const CONTTYPE: u8 = b'7';

// ---------------------------------------------------------------------------
// Mode bits (stored in octal ASCII in the header)
// ---------------------------------------------------------------------------

/// Set UID on execution.
pub const TSUID: u32 = 0o4000;

/// Set GID on execution.
pub const TSGID: u32 = 0o2000;

/// Sticky bit (restricted deletion flag).
pub const TSVTX: u32 = 0o1000;

/// Owner read.
pub const TUREAD: u32 = 0o0400;

/// Owner write.
pub const TUWRITE: u32 = 0o0200;

/// Owner execute.
pub const TUEXEC: u32 = 0o0100;

/// Group read.
pub const TGREAD: u32 = 0o0040;

/// Group write.
pub const TGWRITE: u32 = 0o0020;

/// Group execute.
pub const TGEXEC: u32 = 0o0010;

/// Other read.
pub const TOREAD: u32 = 0o0004;

/// Other write.
pub const TOWRITE: u32 = 0o0002;

/// Other execute.
pub const TOEXEC: u32 = 0o0001;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Magic and version
    // -----------------------------------------------------------------------

    #[test]
    fn test_tmagic() {
        assert_eq!(TMAGIC, b"ustar\0");
        assert_eq!(TMAGIC.len(), TMAGLEN);
    }

    #[test]
    fn test_tversion() {
        assert_eq!(TVERSION, b"00");
        assert_eq!(TVERSION.len(), TVERSLEN);
    }

    #[test]
    fn test_tmaglen() {
        assert_eq!(TMAGLEN, 6);
    }

    #[test]
    fn test_tverslen() {
        assert_eq!(TVERSLEN, 2);
    }

    // -----------------------------------------------------------------------
    // Type flags
    // -----------------------------------------------------------------------

    #[test]
    fn test_type_flags_ascii() {
        assert_eq!(REGTYPE, b'0');
        assert_eq!(AREGTYPE, 0);
        assert_eq!(LNKTYPE, b'1');
        assert_eq!(SYMTYPE, b'2');
        assert_eq!(CHRTYPE, b'3');
        assert_eq!(BLKTYPE, b'4');
        assert_eq!(DIRTYPE, b'5');
        assert_eq!(FIFOTYPE, b'6');
        assert_eq!(CONTTYPE, b'7');
    }

    #[test]
    fn test_type_flags_sequential() {
        // '0' through '7' are sequential ASCII.
        assert_eq!(LNKTYPE, REGTYPE + 1);
        assert_eq!(SYMTYPE, REGTYPE + 2);
        assert_eq!(CHRTYPE, REGTYPE + 3);
        assert_eq!(BLKTYPE, REGTYPE + 4);
        assert_eq!(DIRTYPE, REGTYPE + 5);
        assert_eq!(FIFOTYPE, REGTYPE + 6);
        assert_eq!(CONTTYPE, REGTYPE + 7);
    }

    #[test]
    fn test_type_flags_distinct() {
        let flags = [
            REGTYPE, AREGTYPE, LNKTYPE, SYMTYPE, CHRTYPE, BLKTYPE, DIRTYPE, FIFOTYPE, CONTTYPE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                // AREGTYPE (0) != REGTYPE ('0'), so all are distinct.
                assert_ne!(
                    flags[i], flags[j],
                    "type flags should be distinct: {} vs {}",
                    flags[i], flags[j]
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Mode bits
    // -----------------------------------------------------------------------

    #[test]
    fn test_setuid_setgid_sticky() {
        assert_eq!(TSUID, 0o4000);
        assert_eq!(TSGID, 0o2000);
        assert_eq!(TSVTX, 0o1000);
    }

    #[test]
    fn test_owner_modes() {
        assert_eq!(TUREAD, 0o0400);
        assert_eq!(TUWRITE, 0o0200);
        assert_eq!(TUEXEC, 0o0100);
    }

    #[test]
    fn test_group_modes() {
        assert_eq!(TGREAD, 0o0040);
        assert_eq!(TGWRITE, 0o0020);
        assert_eq!(TGEXEC, 0o0010);
    }

    #[test]
    fn test_other_modes() {
        assert_eq!(TOREAD, 0o0004);
        assert_eq!(TOWRITE, 0o0002);
        assert_eq!(TOEXEC, 0o0001);
    }

    #[test]
    fn test_modes_are_single_bits_or_octal_groups() {
        // Each mode constant should have no overlapping bits with others
        // in its category.
        assert_eq!(TUREAD & TUWRITE, 0);
        assert_eq!(TUREAD & TUEXEC, 0);
        assert_eq!(TUWRITE & TUEXEC, 0);

        assert_eq!(TGREAD & TGWRITE, 0);
        assert_eq!(TGREAD & TGEXEC, 0);
        assert_eq!(TGWRITE & TGEXEC, 0);

        assert_eq!(TOREAD & TOWRITE, 0);
        assert_eq!(TOREAD & TOEXEC, 0);
        assert_eq!(TOWRITE & TOEXEC, 0);
    }

    #[test]
    fn test_full_permission_mask() {
        // 0o7777 covers all mode bits.
        let all = TSUID
            | TSGID
            | TSVTX
            | TUREAD
            | TUWRITE
            | TUEXEC
            | TGREAD
            | TGWRITE
            | TGEXEC
            | TOREAD
            | TOWRITE
            | TOEXEC;
        assert_eq!(all, 0o7777);
    }

    #[test]
    fn test_owner_group_other_alignment() {
        // Group bits are owner bits shifted right by 3.
        assert_eq!(TGREAD, TUREAD >> 3);
        assert_eq!(TGWRITE, TUWRITE >> 3);
        assert_eq!(TGEXEC, TUEXEC >> 3);

        // Other bits are group bits shifted right by 3.
        assert_eq!(TOREAD, TGREAD >> 3);
        assert_eq!(TOWRITE, TGWRITE >> 3);
        assert_eq!(TOEXEC, TGEXEC >> 3);
    }

    #[test]
    fn test_common_permission_combos() {
        // 0o755 = rwxr-xr-x (directories, executables).
        let mode_755 = TUREAD | TUWRITE | TUEXEC | TGREAD | TGEXEC | TOREAD | TOEXEC;
        assert_eq!(mode_755, 0o0755);

        // 0o644 = rw-r--r-- (regular files).
        let mode_644 = TUREAD | TUWRITE | TGREAD | TOREAD;
        assert_eq!(mode_644, 0o0644);

        // 0o1755 = sticky + rwxr-xr-x (/tmp).
        let mode_1755 = TSVTX | mode_755;
        assert_eq!(mode_1755, 0o1755);

        // 0o4755 = setuid + rwxr-xr-x (sudo, passwd).
        let mode_4755 = TSUID | mode_755;
        assert_eq!(mode_4755, 0o4755);
    }
}

//! The attribute flags both programs name: their numbers
//! (`lib/ext2fs/ext2_fs.h`), the letters and long names `lsattr` prints them
//! with (`lib/e2p/pf.c`), and the letters `chattr` takes (`misc/chattr.c`'s
//! `flags_array`).
//!
//! Nothing here touches a file, so unlike the rest of the crate it compiles
//! on every host, and its tests run in the workspace's suite and under
//! mutation testing there.

use cstdio::Output;

pub const EXT2_SECRM_FL: u64 = 0x0000_0001;
pub const EXT2_UNRM_FL: u64 = 0x0000_0002;
pub const EXT2_COMPR_FL: u64 = 0x0000_0004;
pub const EXT2_SYNC_FL: u64 = 0x0000_0008;
pub const EXT2_IMMUTABLE_FL: u64 = 0x0000_0010;
pub const EXT2_APPEND_FL: u64 = 0x0000_0020;
pub const EXT2_NODUMP_FL: u64 = 0x0000_0040;
pub const EXT2_NOATIME_FL: u64 = 0x0000_0080;
pub const EXT2_NOCOMPR_FL: u64 = 0x0000_0400;
pub const EXT4_ENCRYPT_FL: u64 = 0x0000_0800;
pub const EXT2_INDEX_FL: u64 = 0x0000_1000;
pub const EXT3_JOURNAL_DATA_FL: u64 = 0x0000_4000;
pub const EXT2_NOTAIL_FL: u64 = 0x0000_8000;
pub const EXT2_DIRSYNC_FL: u64 = 0x0001_0000;
pub const EXT2_TOPDIR_FL: u64 = 0x0002_0000;
pub const EXT4_EXTENTS_FL: u64 = 0x0008_0000;
pub const EXT4_VERITY_FL: u64 = 0x0010_0000;
pub const FS_NOCOW_FL: u64 = 0x0080_0000;
pub const FS_DAX_FL: u64 = 0x0200_0000;
pub const EXT4_INLINE_DATA_FL: u64 = 0x1000_0000;
pub const EXT4_PROJINHERIT_FL: u64 = 0x2000_0000;
pub const EXT4_CASEFOLD_FL: u64 = 0x4000_0000;

/// `pf.c`'s `flags_array`: each flag's letter and long name, in the order
/// `lsattr` prints them.
const PRINTED: [(u64, &str, &str); 22] = [
    (EXT2_SECRM_FL, "s", "Secure_Deletion"),
    (EXT2_UNRM_FL, "u", "Undelete"),
    (EXT2_SYNC_FL, "S", "Synchronous_Updates"),
    (EXT2_DIRSYNC_FL, "D", "Synchronous_Directory_Updates"),
    (EXT2_IMMUTABLE_FL, "i", "Immutable"),
    (EXT2_APPEND_FL, "a", "Append_Only"),
    (EXT2_NODUMP_FL, "d", "No_Dump"),
    (EXT2_NOATIME_FL, "A", "No_Atime"),
    (EXT2_COMPR_FL, "c", "Compression_Requested"),
    (EXT4_ENCRYPT_FL, "E", "Encrypted"),
    (EXT3_JOURNAL_DATA_FL, "j", "Journaled_Data"),
    (EXT2_INDEX_FL, "I", "Indexed_directory"),
    (EXT2_NOTAIL_FL, "t", "No_Tailmerging"),
    (EXT2_TOPDIR_FL, "T", "Top_of_Directory_Hierarchies"),
    (EXT4_EXTENTS_FL, "e", "Extents"),
    (FS_NOCOW_FL, "C", "No_COW"),
    (FS_DAX_FL, "x", "DAX"),
    (EXT4_CASEFOLD_FL, "F", "Casefold"),
    (EXT4_INLINE_DATA_FL, "N", "Inline_Data"),
    (EXT4_PROJINHERIT_FL, "P", "Project_Hierarchy"),
    (EXT4_VERITY_FL, "V", "Verity"),
    (EXT2_NOCOMPR_FL, "m", "Dont_Compress"),
];

/// `chattr.c`'s `flags_array`: the letters `chattr` takes, and their flags.
const SETTABLE: [(u64, u8); 18] = [
    (EXT2_NOATIME_FL, b'A'),
    (EXT2_SYNC_FL, b'S'),
    (EXT2_DIRSYNC_FL, b'D'),
    (EXT2_APPEND_FL, b'a'),
    (EXT2_COMPR_FL, b'c'),
    (EXT2_NOCOMPR_FL, b'm'),
    (EXT2_NODUMP_FL, b'd'),
    (EXT4_EXTENTS_FL, b'e'),
    (EXT2_IMMUTABLE_FL, b'i'),
    (EXT3_JOURNAL_DATA_FL, b'j'),
    (EXT4_PROJINHERIT_FL, b'P'),
    (EXT2_SECRM_FL, b's'),
    (EXT2_UNRM_FL, b'u'),
    (EXT2_NOTAIL_FL, b't'),
    (EXT2_TOPDIR_FL, b'T'),
    (FS_NOCOW_FL, b'C'),
    (FS_DAX_FL, b'x'),
    (EXT4_CASEFOLD_FL, b'F'),
];

/// `print_flags (f, flags, options)`: a letter or a `-` for each flag, or
/// (`long`) the long names of those set, `, `-separated, `---` for none.
pub fn print_flags(out: &mut Output, flags: u64, long: bool) {
    let mut first = true;
    for (flag, short, long_name) in PRINTED {
        if flags & flag != 0 {
            if long {
                if first {
                    first = false;
                } else {
                    out.write(b", ");
                }
                out.write(long_name.as_bytes());
            } else {
                out.write(short.as_bytes());
            }
        } else if !long {
            out.write(b"-");
        }
    }
    if long && first {
        out.write(b"---");
    }
}

/// `chattr.c`'s `get_flag (c)`: the flag `chattr` sets for the letter `c`,
/// 0 for a letter that is no flag.
#[must_use]
pub fn get_flag(c: u8) -> u64 {
    SETTABLE
        .iter()
        .find(|&&(_, letter)| letter == c)
        .map_or(0, |&(flag, _)| flag)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use cstdio::Sink;

    fn flags(f: u64, long: bool) -> Vec<u8> {
        let mut out = Output::new(Sink::Memory(Vec::new()), false, false);
        print_flags(&mut out, f, long);
        out.take_written()
    }

    #[test]
    fn flags_print_as_lsattr_prints_them() {
        assert_eq!(flags(0, false), b"----------------------");
        assert_eq!(flags(EXT4_EXTENTS_FL, false), b"--------------e-------");
        assert_eq!(
            flags(EXT2_IMMUTABLE_FL | EXT2_NOCOMPR_FL, false),
            b"----i----------------m"
        );
        assert_eq!(flags(0, true), b"---");
        assert_eq!(flags(EXT2_SECRM_FL, true), b"Secure_Deletion");
        assert_eq!(
            flags(EXT2_APPEND_FL | EXT4_EXTENTS_FL, true),
            b"Append_Only, Extents"
        );
    }

    #[test]
    fn every_printed_flag_has_its_own_bit_and_place() {
        let mut all = 0u64;
        for (i, (flag, _, _)) in PRINTED.iter().enumerate() {
            assert_eq!(flag.count_ones(), 1);
            assert_eq!(all & flag, 0, "a flag printed twice");
            all |= flag;
            let mut want = vec![b'-'; PRINTED.len()];
            want[i] = PRINTED[i].1.as_bytes()[0];
            assert_eq!(flags(*flag, false), want);
        }
    }

    #[test]
    fn each_letter_is_its_flag_and_others_are_none() {
        assert_eq!(get_flag(b'i'), EXT2_IMMUTABLE_FL);
        assert_eq!(get_flag(b'm'), EXT2_NOCOMPR_FL);
        assert_eq!(get_flag(b'F'), EXT4_CASEFOLD_FL);
        assert_eq!(get_flag(b'A'), EXT2_NOATIME_FL);
        assert_eq!(get_flag(b'E'), 0, "lsattr shows it; chattr cannot set it");
        assert_eq!(get_flag(b'z'), 0);
        // Every letter chattr takes is the letter lsattr prints for it.
        for (flag, letter) in SETTABLE {
            let printed = PRINTED.iter().find(|p| p.0 == flag).unwrap();
            assert_eq!(printed.1.as_bytes(), [letter]);
        }
    }
}

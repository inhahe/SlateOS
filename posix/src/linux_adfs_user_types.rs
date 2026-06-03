//! `<linux/adfs_fs.h>` — Acorn ADFS filesystem.
//!
//! ADFS is the native filesystem of Acorn Archimedes / RiscPC machines
//! (1987–2000). Linux includes a read-mostly driver for archaeology
//! and emulator integration (Arculator, RPCEmu).

// ---------------------------------------------------------------------------
// Filesystem name and magic
// ---------------------------------------------------------------------------

pub const ADFS_FS_NAME: &str = "adfs";

/// `statfs.f_type` for ADFS. Picked from the historical `_LINUX_MAGIC_H`
/// allocation in `include/uapi/linux/magic.h`.
pub const ADFS_SUPER_MAGIC: u32 = 0xADF5;

// ---------------------------------------------------------------------------
// Sector / block layout
// ---------------------------------------------------------------------------

/// ADFS uses 256-byte hardware sectors on the original media.
pub const ADFS_SECTOR_SIZE: usize = 256;

/// Disc record is 60 bytes at the end of sector 1.
pub const ADFS_DISCRECORD_SIZE: usize = 60;

/// Big-directory name length (max bytes in `dir.name`).
pub const ADFS_F_NAME_LEN: usize = 10;

/// E-format directory name length (after extension).
pub const ADFS_E_NAME_LEN: usize = 10;

/// The maximum entries an old-format directory may contain.
pub const ADFS_OLDDIR_MAX_ENTRIES: usize = 77;
/// The maximum entries a new-format directory may contain.
pub const ADFS_NEWDIR_MAX_ENTRIES: usize = 56;

// ---------------------------------------------------------------------------
// Disc-record magic ("Acorn") at fixed offset
// ---------------------------------------------------------------------------

pub const ADFS_DR_HUGO: &[u8; 4] = b"Hugo"; // old-style dir start
pub const ADFS_DR_NICK: &[u8; 4] = b"Nick"; // new-style dir start

// ---------------------------------------------------------------------------
// File-attribute bits in `inode.loadaddr` (RISC OS encoding)
// ---------------------------------------------------------------------------

pub const ADFS_ATTR_R: u8 = 0x01; // owner read
pub const ADFS_ATTR_W: u8 = 0x02; // owner write
pub const ADFS_ATTR_L: u8 = 0x04; // locked
pub const ADFS_ATTR_D: u8 = 0x08; // directory
pub const ADFS_ATTR_E: u8 = 0x10; // executable
pub const ADFS_ATTR_OR: u8 = 0x20; // public read
pub const ADFS_ATTR_OW: u8 = 0x40; // public write
pub const ADFS_ATTR_HIDDEN: u8 = 0x80;

// ---------------------------------------------------------------------------
// Mount options accepted by the Linux ADFS driver
// ---------------------------------------------------------------------------

pub const ADFS_OPT_UID: &str = "uid=";
pub const ADFS_OPT_GID: &str = "gid=";
pub const ADFS_OPT_OWNMASK: &str = "ownmask=";
pub const ADFS_OPT_OTHMASK: &str = "othmask=";
pub const ADFS_OPT_FTSUFFIX: &str = "ftsuffix=";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_name_and_magic() {
        assert_eq!(ADFS_FS_NAME, "adfs");
        assert_eq!(ADFS_SUPER_MAGIC, 0xADF5);
    }

    #[test]
    fn test_sector_and_discrecord_sizes() {
        // 256-byte sectors; disc record fits at the tail of sector 1.
        assert_eq!(ADFS_SECTOR_SIZE, 256);
        assert_eq!(ADFS_DISCRECORD_SIZE, 60);
        // Disc record fits in a sector with room to spare.
        assert!(ADFS_DISCRECORD_SIZE < ADFS_SECTOR_SIZE);
    }

    #[test]
    fn test_name_lengths_equal_10() {
        // Both F and E directory formats cap names at 10 bytes.
        assert_eq!(ADFS_F_NAME_LEN, 10);
        assert_eq!(ADFS_E_NAME_LEN, 10);
    }

    #[test]
    fn test_dir_entry_caps_old_new_distinct() {
        // The new directory format is bigger per entry, so fewer fit.
        assert!(ADFS_OLDDIR_MAX_ENTRIES > ADFS_NEWDIR_MAX_ENTRIES);
        assert_eq!(ADFS_OLDDIR_MAX_ENTRIES, 77);
        assert_eq!(ADFS_NEWDIR_MAX_ENTRIES, 56);
    }

    #[test]
    fn test_dir_start_magics_are_4_bytes() {
        assert_eq!(ADFS_DR_HUGO, b"Hugo");
        assert_eq!(ADFS_DR_NICK, b"Nick");
        assert_ne!(ADFS_DR_HUGO, ADFS_DR_NICK);
    }

    #[test]
    fn test_attr_bits_single_full_byte() {
        let a = [
            ADFS_ATTR_R,
            ADFS_ATTR_W,
            ADFS_ATTR_L,
            ADFS_ATTR_D,
            ADFS_ATTR_E,
            ADFS_ATTR_OR,
            ADFS_ATTR_OW,
            ADFS_ATTR_HIDDEN,
        ];
        let mut or = 0u8;
        for v in a {
            assert!(v.is_power_of_two());
            or |= v;
        }
        // 8 bits spanning the whole byte.
        assert_eq!(or, 0xFF);
    }

    #[test]
    fn test_mount_options_end_with_equals() {
        for o in [
            ADFS_OPT_UID,
            ADFS_OPT_GID,
            ADFS_OPT_OWNMASK,
            ADFS_OPT_OTHMASK,
            ADFS_OPT_FTSUFFIX,
        ] {
            assert!(o.ends_with('='));
        }
    }
}

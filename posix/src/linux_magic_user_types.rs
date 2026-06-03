//! `<linux/magic.h>` — filesystem and pseudo-filesystem magic numbers.
//!
//! Every `statfs(2)` reply carries an `f_type` field whose value is one
//! of the magics below. `df`, `mount`, container runtimes, and security
//! tools (e.g. AppArmor's `dac_overrideable` check) read this field to
//! identify the underlying filesystem without opening the device.

// ---------------------------------------------------------------------------
// On-disk filesystems
// ---------------------------------------------------------------------------

pub const EXT2_SUPER_MAGIC: u32 = 0xEF53;
pub const EXT3_SUPER_MAGIC: u32 = 0xEF53;
pub const EXT4_SUPER_MAGIC: u32 = 0xEF53;
pub const BTRFS_SUPER_MAGIC: u32 = 0x9123_683E;
pub const XFS_SUPER_MAGIC: u32 = 0x5846_5342;
pub const FAT_SUPER_MAGIC: u32 = 0x4D44;
pub const MSDOS_SUPER_MAGIC: u32 = 0x4D44;
pub const NTFS_SB_MAGIC: u32 = 0x5346_544E;
pub const ISOFS_SUPER_MAGIC: u32 = 0x9660;
pub const SQUASHFS_MAGIC: u32 = 0x7371_7368;
pub const F2FS_SUPER_MAGIC: u32 = 0xF2F5_2010;
pub const NILFS_SUPER_MAGIC: u32 = 0x3434;
pub const UDF_SUPER_MAGIC: u32 = 0x1554_1296;

// ---------------------------------------------------------------------------
// Pseudo-filesystems
// ---------------------------------------------------------------------------

pub const PROC_SUPER_MAGIC: u32 = 0x9FA0;
pub const SYSFS_MAGIC: u32 = 0x6265_7973;
pub const DEVPTS_SUPER_MAGIC: u32 = 0x1CD1;
pub const TMPFS_MAGIC: u32 = 0x0102_1994;
pub const RAMFS_MAGIC: u32 = 0x8584_58F6;
pub const OVERLAYFS_SUPER_MAGIC: u32 = 0x794C_7630;
pub const FUSE_SUPER_MAGIC: u32 = 0x6573_5546;
pub const CGROUP_SUPER_MAGIC: u32 = 0x0027_E0EB;
pub const CGROUP2_SUPER_MAGIC: u32 = 0x6367_7270;
pub const SELINUX_MAGIC: u32 = 0xF97C_FF8C;
pub const SMACK_MAGIC: u32 = 0x4341_5D53;
pub const SECURITYFS_MAGIC: u32 = 0x7365_6375;
pub const BPF_FS_MAGIC: u32 = 0xCAFE_4A11;
pub const TRACEFS_MAGIC: u32 = 0x7472_6163;
pub const DEBUGFS_MAGIC: u32 = 0x6465_6275;
pub const NSFS_MAGIC: u32 = 0x6E73_6673;

// ---------------------------------------------------------------------------
// Network/distributed filesystems
// ---------------------------------------------------------------------------

pub const NFS_SUPER_MAGIC: u32 = 0x6969;
pub const CIFS_MAGIC_NUMBER: u32 = 0xFF53_4D42;
pub const SMB_SUPER_MAGIC: u32 = 0x517B;
pub const CEPH_SUPER_MAGIC: u32 = 0x00C3_6400;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ext_family_shares_magic() {
        // ext2/3/4 all share the same magic — the kernel uses feature
        // bits to tell them apart.
        assert_eq!(EXT2_SUPER_MAGIC, 0xEF53);
        assert_eq!(EXT3_SUPER_MAGIC, EXT2_SUPER_MAGIC);
        assert_eq!(EXT4_SUPER_MAGIC, EXT2_SUPER_MAGIC);
    }

    #[test]
    fn test_ascii_packed_magics() {
        // Several magics are 4 ASCII characters packed little-endian.
        // "XFSB" = 0x58_46_53_42.
        assert_eq!(XFS_SUPER_MAGIC, 0x5846_5342);
        // "sysB" — sysfs magic packs the string "sysfs"-derived bytes.
        assert_eq!(SYSFS_MAGIC, 0x6265_7973);
        // "FUSE".
        assert_eq!(FUSE_SUPER_MAGIC, 0x6573_5546);
        // "NTFS".
        assert_eq!(NTFS_SB_MAGIC, 0x5346_544E);
    }

    #[test]
    fn test_pseudo_filesystem_magics_distinct() {
        let p = [
            PROC_SUPER_MAGIC,
            SYSFS_MAGIC,
            DEVPTS_SUPER_MAGIC,
            TMPFS_MAGIC,
            RAMFS_MAGIC,
            OVERLAYFS_SUPER_MAGIC,
            FUSE_SUPER_MAGIC,
            CGROUP_SUPER_MAGIC,
            CGROUP2_SUPER_MAGIC,
            SELINUX_MAGIC,
            SECURITYFS_MAGIC,
            BPF_FS_MAGIC,
            TRACEFS_MAGIC,
            DEBUGFS_MAGIC,
            NSFS_MAGIC,
        ];
        for i in 0..p.len() {
            for j in (i + 1)..p.len() {
                assert_ne!(p[i], p[j]);
            }
        }
    }

    #[test]
    fn test_on_disk_magics_distinct() {
        // Among the non-ext family, magics must differ.
        let on_disk = [
            EXT4_SUPER_MAGIC,
            BTRFS_SUPER_MAGIC,
            XFS_SUPER_MAGIC,
            FAT_SUPER_MAGIC,
            NTFS_SB_MAGIC,
            ISOFS_SUPER_MAGIC,
            SQUASHFS_MAGIC,
            F2FS_SUPER_MAGIC,
            NILFS_SUPER_MAGIC,
            UDF_SUPER_MAGIC,
        ];
        for i in 0..on_disk.len() {
            for j in (i + 1)..on_disk.len() {
                assert_ne!(on_disk[i], on_disk[j]);
            }
        }
    }

    #[test]
    fn test_well_known_specific_values() {
        // 0x9FA0 is procfs from time immemorial.
        assert_eq!(PROC_SUPER_MAGIC, 0x9FA0);
        // 0x6969 is NFS — the smallest magic in the kernel.
        assert_eq!(NFS_SUPER_MAGIC, 0x6969);
        // 0x0102_1994 is tmpfs — date of one of the maintainers' birthdays.
        assert_eq!(TMPFS_MAGIC, 0x0102_1994);
        // 0xCAFE_4A11 — "cafe-jail" for BPF FS.
        assert_eq!(BPF_FS_MAGIC, 0xCAFE_4A11);
    }
}

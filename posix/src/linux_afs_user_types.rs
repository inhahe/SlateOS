//! `<linux/afs.h>` — Andrew File System / OpenAFS / Auristor.
//!
//! AFS is a network filesystem with global namespaces (`/afs/cern.ch`,
//! `/afs/cs.cmu.edu`). Linux's in-tree client (`fs/afs/`) and the
//! OpenAFS userspace tools share these protocol constants.

// ---------------------------------------------------------------------------
// Filesystem name and magic
// ---------------------------------------------------------------------------

pub const AFS_FS_NAME: &str = "afs";

/// `statfs.f_type` for AFS. Linux kernel allocation.
pub const AFS_SUPER_MAGIC: u32 = 0x5346_414F; // "AFSO" little-endian on disk

// ---------------------------------------------------------------------------
// Global root and cell mount points
// ---------------------------------------------------------------------------

pub const AFS_ROOT_DIR: &str = "/afs";
pub const AFS_CELL_CONFIG: &str = "/etc/openafs/cellservdb";
pub const AFS_THISCELL_CONFIG: &str = "/etc/openafs/ThisCell";

// ---------------------------------------------------------------------------
// RPC port numbers (UDP — Rx/RPC protocol)
// ---------------------------------------------------------------------------

pub const AFS_FS_PORT: u16 = 7000;
pub const AFS_CB_PORT: u16 = 7001;
pub const AFS_PROT_PORT: u16 = 7002; // pts (protection server)
pub const AFS_VLDB_PORT: u16 = 7003;
pub const AFS_KAUTH_PORT: u16 = 7004;
pub const AFS_VOLSER_PORT: u16 = 7005;
pub const AFS_ERR_PORT: u16 = 7006;
pub const AFS_BOS_PORT: u16 = 7007;
pub const AFS_UPDATE_PORT: u16 = 7008;
pub const AFS_RMTSYS_PORT: u16 = 7009;
pub const AFS_BACKUP_PORT: u16 = 7021;

// ---------------------------------------------------------------------------
// File-id components
// ---------------------------------------------------------------------------

/// `afs_fid` = (volume(32), vnode(64), unique(32)) — 16 bytes on the wire.
pub const AFS_FID_SIZE: usize = 16;

/// Maximum length of an AFS cell name (`afsd -cellname`).
pub const AFS_MAXCELLLEN: usize = 64;

/// Volume names are capped at 32 bytes (`vos rename` enforces this).
pub const AFS_MAXVOLNAME: usize = 32;

// ---------------------------------------------------------------------------
// Volume types
// ---------------------------------------------------------------------------

pub const AFS_VOL_TYPE_RW: u8 = 0;
pub const AFS_VOL_TYPE_RO: u8 = 1;
pub const AFS_VOL_TYPE_BACKUP: u8 = 2;

// ---------------------------------------------------------------------------
// File types in an AFS callback
// ---------------------------------------------------------------------------

pub const AFS_FT_INVALID: u32 = 0;
pub const AFS_FT_FILE: u32 = 1;
pub const AFS_FT_DIR: u32 = 2;
pub const AFS_FT_SYMLINK: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_name_and_root() {
        assert_eq!(AFS_FS_NAME, "afs");
        assert_eq!(AFS_ROOT_DIR, "/afs");
    }

    #[test]
    fn test_rpc_ports_in_7000_range() {
        let p = [
            AFS_FS_PORT,
            AFS_CB_PORT,
            AFS_PROT_PORT,
            AFS_VLDB_PORT,
            AFS_KAUTH_PORT,
            AFS_VOLSER_PORT,
            AFS_ERR_PORT,
            AFS_BOS_PORT,
            AFS_UPDATE_PORT,
            AFS_RMTSYS_PORT,
            AFS_BACKUP_PORT,
        ];
        for v in p {
            assert!((7000..=7099).contains(&v));
        }
        // 7000-7009 dense, then jumps to 7021.
        for i in 0..10 {
            assert_eq!(p[i], 7000 + i as u16);
        }
    }

    #[test]
    fn test_fid_size_16_bytes() {
        // 4 + 8 + 4 = 16.
        assert_eq!(AFS_FID_SIZE, 16);
        assert_eq!(AFS_FID_SIZE, 4 + 8 + 4);
    }

    #[test]
    fn test_name_lengths_sensible() {
        assert_eq!(AFS_MAXCELLLEN, 64);
        assert_eq!(AFS_MAXVOLNAME, 32);
        // Volume names must fit inside a cell label.
        assert!(AFS_MAXVOLNAME < AFS_MAXCELLLEN);
    }

    #[test]
    fn test_volume_types_dense_0_to_2() {
        let v = [AFS_VOL_TYPE_RW, AFS_VOL_TYPE_RO, AFS_VOL_TYPE_BACKUP];
        for (i, &x) in v.iter().enumerate() {
            assert_eq!(x as usize, i);
        }
    }

    #[test]
    fn test_file_types_dense_0_to_3() {
        let t = [AFS_FT_INVALID, AFS_FT_FILE, AFS_FT_DIR, AFS_FT_SYMLINK];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_super_magic_value() {
        // 0x53464100 ('SFAO' big-endian, 'OAFS' little-endian).
        assert_eq!(AFS_SUPER_MAGIC.to_be_bytes(), *b"SFAO");
    }
}

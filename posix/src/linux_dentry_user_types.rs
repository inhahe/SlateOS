//! `<linux/dcache.h>` — dcache (directory entry cache) user-visible knobs.
//!
//! The dentry cache caches name → inode lookups. Most of it is kernel
//! internal, but a few constants leak into userspace via getdents,
//! readdir, and /proc/sys/fs/dentry-state.

// ---------------------------------------------------------------------------
// Name length limits (used by both VFS and userspace)
// ---------------------------------------------------------------------------

/// Inline name buffer in struct dentry (DNAME_INLINE_LEN).
pub const DNAME_INLINE_LEN: usize = 40;
/// Maximum component length (POSIX NAME_MAX).
pub const DENTRY_NAME_MAX: usize = 255;

// ---------------------------------------------------------------------------
// dentry flags (subset visible via /proc + tracepoints)
// ---------------------------------------------------------------------------

pub const DCACHE_OP_HASH: u32 = 0x0001;
pub const DCACHE_OP_COMPARE: u32 = 0x0002;
pub const DCACHE_OP_REVALIDATE: u32 = 0x0004;
pub const DCACHE_OP_DELETE: u32 = 0x0008;
pub const DCACHE_OP_PRUNE: u32 = 0x0010;
pub const DCACHE_DISCONNECTED: u32 = 0x0020;
pub const DCACHE_REFERENCED: u32 = 0x0040;
pub const DCACHE_RCUACCESS: u32 = 0x0080;
pub const DCACHE_CANT_MOUNT: u32 = 0x0100;
pub const DCACHE_GENOCIDE: u32 = 0x0200;
pub const DCACHE_SHRINK_LIST: u32 = 0x0400;
pub const DCACHE_MOUNTED: u32 = 0x10000;
pub const DCACHE_NEED_AUTOMOUNT: u32 = 0x20000;
pub const DCACHE_MANAGE_TRANSIT: u32 = 0x40000;
pub const DCACHE_MANAGED_DENTRY: u32 =
    DCACHE_MOUNTED | DCACHE_NEED_AUTOMOUNT | DCACHE_MANAGE_TRANSIT;

// ---------------------------------------------------------------------------
// /proc/sys/fs/dentry-state field count
// ---------------------------------------------------------------------------

/// /proc/sys/fs/dentry-state reports 6 u32 fields:
/// nr_dentry, nr_unused, age_limit, want_pages, nr_negative, dummy.
pub const DENTRY_STATE_FIELD_COUNT: usize = 6;

// ---------------------------------------------------------------------------
// File types in struct linux_dirent64::d_type (matches stat.h DT_*)
// ---------------------------------------------------------------------------

pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO: u8 = 1;
pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_BLK: u8 = 6;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;
pub const DT_SOCK: u8 = 12;
pub const DT_WHT: u8 = 14;

// ---------------------------------------------------------------------------
// struct linux_dirent64 field offsets
// ---------------------------------------------------------------------------

pub const DIRENT64_OFF_INO: usize = 0;
pub const DIRENT64_OFF_OFF: usize = 8;
pub const DIRENT64_OFF_RECLEN: usize = 16;
pub const DIRENT64_OFF_TYPE: usize = 18;
pub const DIRENT64_OFF_NAME: usize = 19;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_limits() {
        assert_eq!(DENTRY_NAME_MAX, 255);
        assert_eq!(DNAME_INLINE_LEN, 40);
        assert!(DENTRY_NAME_MAX > DNAME_INLINE_LEN);
    }

    #[test]
    fn test_dcache_op_flags_single_bit_distinct() {
        let f = [
            DCACHE_OP_HASH,
            DCACHE_OP_COMPARE,
            DCACHE_OP_REVALIDATE,
            DCACHE_OP_DELETE,
            DCACHE_OP_PRUNE,
            DCACHE_DISCONNECTED,
            DCACHE_REFERENCED,
            DCACHE_RCUACCESS,
            DCACHE_CANT_MOUNT,
            DCACHE_GENOCIDE,
            DCACHE_SHRINK_LIST,
        ];
        let mut or_all = 0u32;
        for &v in &f {
            assert!(v.is_power_of_two());
            or_all |= v;
        }
        // All eleven low-byte flags packed into bits 0..10.
        assert_eq!(or_all, 0x7FF);
    }

    #[test]
    fn test_managed_dentry_is_or_of_three() {
        assert_eq!(
            DCACHE_MANAGED_DENTRY,
            DCACHE_MOUNTED | DCACHE_NEED_AUTOMOUNT | DCACHE_MANAGE_TRANSIT
        );
        // The three are pairwise disjoint.
        assert_eq!(DCACHE_MOUNTED & DCACHE_NEED_AUTOMOUNT, 0);
        assert_eq!(DCACHE_NEED_AUTOMOUNT & DCACHE_MANAGE_TRANSIT, 0);
        assert_eq!(DCACHE_MOUNTED & DCACHE_MANAGE_TRANSIT, 0);
    }

    #[test]
    fn test_dt_values_match_st_mode_shift() {
        // DT_* = (st_mode >> 12) for the corresponding file type.
        // S_IFDIR = 0o40000 → DT_DIR = 4.
        // Use u32 literals because the S_IF* values are 16+ bits before the shift.
        assert_eq!(DT_DIR, (0o40000_u32 >> 12) as u8);
        assert_eq!(DT_REG, (0o100000_u32 >> 12) as u8);
        assert_eq!(DT_LNK, (0o120000_u32 >> 12) as u8);
        assert_eq!(DT_FIFO, (0o10000_u32 >> 12) as u8);
        assert_eq!(DT_CHR, (0o20000_u32 >> 12) as u8);
        assert_eq!(DT_BLK, (0o60000_u32 >> 12) as u8);
        assert_eq!(DT_SOCK, (0o140000_u32 >> 12) as u8);
    }

    #[test]
    fn test_dt_unknown_is_zero() {
        assert_eq!(DT_UNKNOWN, 0);
    }

    #[test]
    fn test_dt_values_all_even_except_unknown() {
        // The Linux DT_ values are all even (low bit of file-type nibble is 0).
        for t in [DT_FIFO, DT_CHR, DT_DIR, DT_BLK, DT_REG, DT_LNK, DT_SOCK, DT_WHT] {
            assert_eq!(t % 2, 0);
        }
    }

    #[test]
    fn test_dirent64_offsets_strictly_increasing() {
        let off = [
            DIRENT64_OFF_INO,
            DIRENT64_OFF_OFF,
            DIRENT64_OFF_RECLEN,
            DIRENT64_OFF_TYPE,
            DIRENT64_OFF_NAME,
        ];
        for w in off.windows(2) {
            assert!(w[1] > w[0]);
        }
        // d_ino + d_off = 16 bytes; d_reclen = u16 + d_type = u8 + d_name follows.
        assert_eq!(DIRENT64_OFF_OFF - DIRENT64_OFF_INO, 8);
        assert_eq!(DIRENT64_OFF_RECLEN - DIRENT64_OFF_OFF, 8);
        assert_eq!(DIRENT64_OFF_TYPE - DIRENT64_OFF_RECLEN, 2);
        assert_eq!(DIRENT64_OFF_NAME - DIRENT64_OFF_TYPE, 1);
    }

    #[test]
    fn test_dentry_state_field_count_is_6() {
        assert_eq!(DENTRY_STATE_FIELD_COUNT, 6);
    }
}

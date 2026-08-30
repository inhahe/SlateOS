//! `<linux/blkpg.h>` — partition-table manipulation ioctls.
//!
//! `BLKPG` is the kernel's "atomic" interface for adding, deleting,
//! and resizing partitions on a live block device without unmounting
//! it. `parted`, `fdisk -u`, and `partprobe` all funnel through here.

// ---------------------------------------------------------------------------
// BLKPG ioctl base number
// ---------------------------------------------------------------------------

/// `_IO(0x12, 0x69)` — the umbrella partition-table ioctl. The
/// op-code in `blkpg_ioctl_arg.op` selects sub-operation.
pub const BLKPG: u32 = 0x1269;

// ---------------------------------------------------------------------------
// `blkpg_ioctl_arg.op` sub-operations
// ---------------------------------------------------------------------------

pub const BLKPG_ADD_PARTITION: i32 = 1;
pub const BLKPG_DEL_PARTITION: i32 = 2;
pub const BLKPG_RESIZE_PARTITION: i32 = 3;

// ---------------------------------------------------------------------------
// Partition name / volname size in `blkpg_partition`
// ---------------------------------------------------------------------------

pub const BLKPG_DEVNAMELTH: usize = 64;
pub const BLKPG_VOLNAMELTH: usize = 64;

// ---------------------------------------------------------------------------
// Partition-flag bits used in `blkpg_partition.flags` (extension)
// ---------------------------------------------------------------------------

pub const BLKPG_FLAG_BOOT: u32 = 1 << 0;
pub const BLKPG_FLAG_HIDDEN: u32 = 1 << 1;
pub const BLKPG_FLAG_READONLY: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Related (`<linux/fs.h>`) ioctls in the same numeric block
// ---------------------------------------------------------------------------

pub const BLKPG_NEXT_NR: u32 = 0x126A;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blkpg_ioctl_nr_encoding() {
        // _IO(0x12, 0x69).
        assert_eq!(BLKPG >> 8, 0x12);
        assert_eq!(BLKPG & 0xFF, 0x69);
    }

    #[test]
    fn test_subop_codes_dense_1_to_3() {
        let s = [
            BLKPG_ADD_PARTITION,
            BLKPG_DEL_PARTITION,
            BLKPG_RESIZE_PARTITION,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
        // 0 is unused (reserved for "no op").
        assert!(BLKPG_ADD_PARTITION > 0);
    }

    #[test]
    fn test_name_lengths_equal_and_aligned() {
        assert_eq!(BLKPG_DEVNAMELTH, 64);
        assert_eq!(BLKPG_VOLNAMELTH, 64);
        assert_eq!(BLKPG_DEVNAMELTH, BLKPG_VOLNAMELTH);
        assert!(BLKPG_DEVNAMELTH.is_power_of_two());
    }

    #[test]
    fn test_partition_flags_each_single_bit() {
        let f = [BLKPG_FLAG_BOOT, BLKPG_FLAG_HIDDEN, BLKPG_FLAG_READONLY];
        let mut or = 0u32;
        for &v in &f {
            assert!(v.is_power_of_two());
            or |= v;
        }
        // Low three bits.
        assert_eq!(or, 0b111);
        // Pairwise disjoint.
        for (i, &a) in f.iter().enumerate() {
            for &b in &f[i + 1..] {
                assert_eq!(a & b, 0);
            }
        }
    }

    #[test]
    fn test_blkpg_next_nr_follows_blkpg() {
        // The "next-partition" probe sits exactly one ioctl above BLKPG.
        assert_eq!(BLKPG_NEXT_NR, BLKPG + 1);
        assert_eq!(BLKPG_NEXT_NR & 0xFF, 0x6A);
    }
}

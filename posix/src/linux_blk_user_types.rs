//! `<linux/blkdev.h>` — generic block-layer user-facing operations
//! and completion-tag encoding (`blk_qc_t`).
//!
//! These are the small primitives every block consumer touches:
//! the READ/WRITE direction codes, the `blk_qc_t` cookie that
//! `submit_bio_noacct` returns, and the queue-feature flag bits
//! exposed through sysfs as `/sys/block/<dev>/queue/<feature>`.

// ---------------------------------------------------------------------------
// Generic READ / WRITE direction codes (`<linux/fs.h>` overlap)
// ---------------------------------------------------------------------------

pub const READ: u32 = 0;
pub const WRITE: u32 = 1;

/// Legacy read-ahead op (kept for ABI compatibility).
pub const READA: u32 = 2;

// ---------------------------------------------------------------------------
// `blk_qc_t` completion-cookie encoding
// ---------------------------------------------------------------------------

/// Sentinel "no cookie" / "synchronous completion".
pub const BLK_QC_T_NONE: u64 = u64::MAX;

/// Bits used to store the hardware-queue index in the cookie.
pub const BLK_QC_T_HW_QUEUE_BITS: u32 = 16;

/// Mask covering the hardware-queue index.
pub const BLK_QC_T_HW_QUEUE_MASK: u64 = (1 << BLK_QC_T_HW_QUEUE_BITS) - 1;

/// Bit position where the request-tag stops and the hwq index begins.
pub const BLK_QC_T_HW_QUEUE_SHIFT: u32 = 16;

// ---------------------------------------------------------------------------
// Generic queue-feature flag bits (`QUEUE_FLAG_*`)
// ---------------------------------------------------------------------------

pub const QUEUE_FLAG_STOPPED: u32 = 1 << 0;
pub const QUEUE_FLAG_DYING: u32 = 1 << 1;
pub const QUEUE_FLAG_NOMERGES: u32 = 1 << 2;
pub const QUEUE_FLAG_SAME_COMP: u32 = 1 << 3;
pub const QUEUE_FLAG_FAIL_IO: u32 = 1 << 4;
pub const QUEUE_FLAG_NONROT: u32 = 1 << 5;
pub const QUEUE_FLAG_IO_STAT: u32 = 1 << 6;
pub const QUEUE_FLAG_NOXATTRS: u32 = 1 << 7;
pub const QUEUE_FLAG_ADD_RANDOM: u32 = 1 << 8;
pub const QUEUE_FLAG_SYNCHRONOUS: u32 = 1 << 9;
pub const QUEUE_FLAG_SAME_FORCE: u32 = 1 << 10;
pub const QUEUE_FLAG_INIT_DONE: u32 = 1 << 11;
pub const QUEUE_FLAG_STABLE_WRITES: u32 = 1 << 12;
pub const QUEUE_FLAG_POLL: u32 = 1 << 13;
pub const QUEUE_FLAG_WC: u32 = 1 << 14;
pub const QUEUE_FLAG_FUA: u32 = 1 << 15;
pub const QUEUE_FLAG_DAX: u32 = 1 << 16;

// ---------------------------------------------------------------------------
// Segment-size limits
// ---------------------------------------------------------------------------

/// Default maximum segment size (64 KiB).
pub const BLK_MAX_SEGMENT_SIZE: u32 = 65_536;

/// Minimum allowed segment size (one page on 4 KiB hardware).
pub const BLK_MIN_SEGMENT_SIZE: u32 = 4_096;

/// Maximum segments per request.
pub const BLK_MAX_SEGMENTS: u32 = 128;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direction_codes_dense_0_to_2() {
        assert_eq!(READ, 0);
        assert_eq!(WRITE, 1);
        assert_eq!(READA, 2);
    }

    #[test]
    fn test_qc_cookie_geometry() {
        // _NONE is the all-ones sentinel.
        assert_eq!(BLK_QC_T_NONE, u64::MAX);
        // 16 bits for the hwq index.
        assert_eq!(BLK_QC_T_HW_QUEUE_BITS, 16);
        assert_eq!(BLK_QC_T_HW_QUEUE_MASK, 0xFFFF);
        assert_eq!(BLK_QC_T_HW_QUEUE_SHIFT, 16);
        // Mask must be 2^bits - 1.
        assert_eq!(
            BLK_QC_T_HW_QUEUE_MASK,
            (1u64 << BLK_QC_T_HW_QUEUE_BITS) - 1
        );
    }

    #[test]
    fn test_queue_flags_each_single_bit_distinct() {
        let f = [
            QUEUE_FLAG_STOPPED,
            QUEUE_FLAG_DYING,
            QUEUE_FLAG_NOMERGES,
            QUEUE_FLAG_SAME_COMP,
            QUEUE_FLAG_FAIL_IO,
            QUEUE_FLAG_NONROT,
            QUEUE_FLAG_IO_STAT,
            QUEUE_FLAG_NOXATTRS,
            QUEUE_FLAG_ADD_RANDOM,
            QUEUE_FLAG_SYNCHRONOUS,
            QUEUE_FLAG_SAME_FORCE,
            QUEUE_FLAG_INIT_DONE,
            QUEUE_FLAG_STABLE_WRITES,
            QUEUE_FLAG_POLL,
            QUEUE_FLAG_WC,
            QUEUE_FLAG_FUA,
            QUEUE_FLAG_DAX,
        ];
        let mut or = 0u32;
        for (i, &v) in f.iter().enumerate() {
            assert!(v.is_power_of_two());
            // Dense bits 0..=16 (17 flags).
            assert_eq!(v, 1u32 << i);
            or |= v;
        }
        // Low 17 bits.
        assert_eq!(or, 0x1_FFFF);
    }

    #[test]
    fn test_segment_size_limits() {
        assert_eq!(BLK_MAX_SEGMENT_SIZE, 65_536);
        assert_eq!(BLK_MIN_SEGMENT_SIZE, 4_096);
        assert_eq!(BLK_MAX_SEGMENTS, 128);
        assert!(BLK_MIN_SEGMENT_SIZE < BLK_MAX_SEGMENT_SIZE);
        assert!(BLK_MAX_SEGMENT_SIZE.is_power_of_two());
        assert!(BLK_MIN_SEGMENT_SIZE.is_power_of_two());
        assert!(BLK_MAX_SEGMENTS.is_power_of_two());
        // 16x size ratio.
        assert_eq!(BLK_MAX_SEGMENT_SIZE / BLK_MIN_SEGMENT_SIZE, 16);
    }

    #[test]
    fn test_wc_and_fua_pair() {
        // Write-cache (WC) and force-unit-access (FUA) form a natural
        // pair — they're adjacent bits (14, 15).
        assert_eq!(QUEUE_FLAG_FUA, QUEUE_FLAG_WC << 1);
    }
}

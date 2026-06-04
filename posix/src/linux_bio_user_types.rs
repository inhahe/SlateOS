//! `<linux/bio.h>` — block-I/O `bio` request flags exposed to userspace.
//!
//! The block layer reports per-bio statistics and ioctl-visible flags
//! that mirror the in-kernel `bi_opf` / `bi_flags` fields. `iostat`
//! and `iotop` decode these directly.

// ---------------------------------------------------------------------------
// Operation classifiers (low byte of `req_opf`)
// ---------------------------------------------------------------------------

pub const REQ_OP_READ: u32 = 0;
pub const REQ_OP_WRITE: u32 = 1;
pub const REQ_OP_FLUSH: u32 = 2;
pub const REQ_OP_DISCARD: u32 = 3;
pub const REQ_OP_SECURE_ERASE: u32 = 5;
pub const REQ_OP_WRITE_ZEROES: u32 = 9;
pub const REQ_OP_ZONE_OPEN: u32 = 10;
pub const REQ_OP_ZONE_CLOSE: u32 = 11;
pub const REQ_OP_ZONE_FINISH: u32 = 12;
pub const REQ_OP_ZONE_APPEND: u32 = 13;
pub const REQ_OP_ZONE_RESET: u32 = 15;
pub const REQ_OP_ZONE_RESET_ALL: u32 = 17;
pub const REQ_OP_DRV_IN: u32 = 34;
pub const REQ_OP_DRV_OUT: u32 = 35;

pub const REQ_OP_LAST: u32 = 36;
pub const REQ_OP_MASK: u32 = 0xFF;

// ---------------------------------------------------------------------------
// Request flags (high bits of `req_flags_t`)
// ---------------------------------------------------------------------------

pub const REQ_FAILFAST_DEV: u32 = 1 << 8;
pub const REQ_FAILFAST_TRANSPORT: u32 = 1 << 9;
pub const REQ_FAILFAST_DRIVER: u32 = 1 << 10;
pub const REQ_SYNC: u32 = 1 << 11;
pub const REQ_META: u32 = 1 << 12;
pub const REQ_PRIO: u32 = 1 << 13;
pub const REQ_NOMERGE: u32 = 1 << 14;
pub const REQ_IDLE: u32 = 1 << 15;
pub const REQ_INTEGRITY: u32 = 1 << 16;
pub const REQ_FUA: u32 = 1 << 17;
pub const REQ_PREFLUSH: u32 = 1 << 18;
pub const REQ_RAHEAD: u32 = 1 << 19;
pub const REQ_BACKGROUND: u32 = 1 << 20;
pub const REQ_NOWAIT: u32 = 1 << 21;
pub const REQ_POLLED: u32 = 1 << 24;

// ---------------------------------------------------------------------------
// Convenience composites
// ---------------------------------------------------------------------------

pub const REQ_FAILFAST_MASK: u32 =
    REQ_FAILFAST_DEV | REQ_FAILFAST_TRANSPORT | REQ_FAILFAST_DRIVER;

pub const REQ_NOMERGE_FLAGS: u32 = REQ_NOMERGE | REQ_PREFLUSH | REQ_FUA;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_codes_distinct_and_in_low_byte() {
        let ops = [
            REQ_OP_READ,
            REQ_OP_WRITE,
            REQ_OP_FLUSH,
            REQ_OP_DISCARD,
            REQ_OP_SECURE_ERASE,
            REQ_OP_WRITE_ZEROES,
            REQ_OP_ZONE_OPEN,
            REQ_OP_ZONE_CLOSE,
            REQ_OP_ZONE_FINISH,
            REQ_OP_ZONE_APPEND,
            REQ_OP_ZONE_RESET,
            REQ_OP_ZONE_RESET_ALL,
            REQ_OP_DRV_IN,
            REQ_OP_DRV_OUT,
        ];
        for &v in &ops {
            assert!(v <= REQ_OP_MASK);
        }
        for (i, &a) in ops.iter().enumerate() {
            for &b in &ops[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // Reads are op 0 — the most common, hot-path code.
        assert_eq!(REQ_OP_READ, 0);
        // Writes are op 1, paired with reads.
        assert_eq!(REQ_OP_WRITE, 1);
        // Last is exclusive upper bound > all opcodes.
        for &v in &ops {
            assert!(v < REQ_OP_LAST);
        }
    }

    #[test]
    fn test_op_mask_is_byte_wide() {
        assert_eq!(REQ_OP_MASK, 0xFF);
        // No defined opcode collides with a flag bit (8 lowest bits only).
        assert_eq!(REQ_OP_MASK & REQ_SYNC, 0);
    }

    #[test]
    fn test_zone_ops_clustered_10_to_17() {
        // The append/reset family lives in 10..18 with two gaps (14, 16).
        for &v in &[
            REQ_OP_ZONE_OPEN,
            REQ_OP_ZONE_CLOSE,
            REQ_OP_ZONE_FINISH,
            REQ_OP_ZONE_APPEND,
            REQ_OP_ZONE_RESET,
            REQ_OP_ZONE_RESET_ALL,
        ] {
            assert!((10..=17).contains(&v));
        }
        // OPEN/CLOSE/FINISH/APPEND are dense 10..=13.
        assert_eq!(REQ_OP_ZONE_CLOSE - REQ_OP_ZONE_OPEN, 1);
        assert_eq!(REQ_OP_ZONE_FINISH - REQ_OP_ZONE_CLOSE, 1);
        assert_eq!(REQ_OP_ZONE_APPEND - REQ_OP_ZONE_FINISH, 1);
    }

    #[test]
    fn test_flag_bits_each_single_bit() {
        let f = [
            REQ_FAILFAST_DEV,
            REQ_FAILFAST_TRANSPORT,
            REQ_FAILFAST_DRIVER,
            REQ_SYNC,
            REQ_META,
            REQ_PRIO,
            REQ_NOMERGE,
            REQ_IDLE,
            REQ_INTEGRITY,
            REQ_FUA,
            REQ_PREFLUSH,
            REQ_RAHEAD,
            REQ_BACKGROUND,
            REQ_NOWAIT,
            REQ_POLLED,
        ];
        for &v in &f {
            assert!(v.is_power_of_two());
            // All flag bits sit in bits 8..=31.
            assert!(v >= 1 << 8);
        }
    }

    #[test]
    fn test_failfast_mask_unions_three_bits() {
        assert_eq!(
            REQ_FAILFAST_MASK,
            REQ_FAILFAST_DEV | REQ_FAILFAST_TRANSPORT | REQ_FAILFAST_DRIVER
        );
        assert_eq!(REQ_FAILFAST_MASK.count_ones(), 3);
    }

    #[test]
    fn test_nomerge_flags_block_three_bits() {
        assert_eq!(
            REQ_NOMERGE_FLAGS,
            REQ_NOMERGE | REQ_PREFLUSH | REQ_FUA
        );
        // Any of these three blocks the block layer from merging the
        // request with adjacent I/O.
        assert_eq!(REQ_NOMERGE_FLAGS.count_ones(), 3);
    }
}

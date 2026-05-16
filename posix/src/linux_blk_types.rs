//! `<linux/blk_types.h>` — Block I/O request type constants.
//!
//! Defines operation codes and flags for block I/O requests (struct bio).
//! Used by block drivers, device mapper, and the I/O scheduler.

// ---------------------------------------------------------------------------
// Block operation types (REQ_OP_*)
// ---------------------------------------------------------------------------

/// Read operation.
pub const REQ_OP_READ: u32 = 0;
/// Write operation.
pub const REQ_OP_WRITE: u32 = 1;
/// Flush volatile write cache.
pub const REQ_OP_FLUSH: u32 = 2;
/// Discard sectors.
pub const REQ_OP_DISCARD: u32 = 3;
/// Secure erase.
pub const REQ_OP_SECURE_ERASE: u32 = 5;
/// Write same data to multiple sectors.
pub const REQ_OP_WRITE_SAME: u32 = 7;
/// Write zeroes.
pub const REQ_OP_WRITE_ZEROES: u32 = 9;
/// Zone open.
pub const REQ_OP_ZONE_OPEN: u32 = 10;
/// Zone close.
pub const REQ_OP_ZONE_CLOSE: u32 = 11;
/// Zone finish.
pub const REQ_OP_ZONE_FINISH: u32 = 12;
/// Zone append.
pub const REQ_OP_ZONE_APPEND: u32 = 13;
/// Zone reset.
pub const REQ_OP_ZONE_RESET: u32 = 15;
/// Zone reset all.
pub const REQ_OP_ZONE_RESET_ALL: u32 = 17;
/// SCSI passthrough.
pub const REQ_OP_DRV_IN: u32 = 34;
/// SCSI passthrough (write direction).
pub const REQ_OP_DRV_OUT: u32 = 35;

/// Operation mask.
pub const REQ_OP_MASK: u32 = 0xFF;

// ---------------------------------------------------------------------------
// Request flags (ORed with operation)
// ---------------------------------------------------------------------------

/// Fail fast on device error.
pub const REQ_FAILFAST_DEV: u32 = 1 << 8;
/// Fail fast on transport error.
pub const REQ_FAILFAST_TRANSPORT: u32 = 1 << 9;
/// Fail fast on driver error.
pub const REQ_FAILFAST_DRIVER: u32 = 1 << 10;
/// Synchronous I/O.
pub const REQ_SYNC: u32 = 1 << 11;
/// Metadata I/O.
pub const REQ_META: u32 = 1 << 12;
/// Prio boost.
pub const REQ_PRIO: u32 = 1 << 13;
/// No merge with other requests.
pub const REQ_NOMERGE: u32 = 1 << 14;
/// Idle priority I/O.
pub const REQ_IDLE: u32 = 1 << 15;
/// Integrity protected.
pub const REQ_INTEGRITY: u32 = 1 << 16;
/// FUA (force unit access — bypass write cache).
pub const REQ_FUA: u32 = 1 << 17;
/// Preflush.
pub const REQ_PREFLUSH: u32 = 1 << 18;
/// Read ahead.
pub const REQ_RAHEAD: u32 = 1 << 19;
/// Background I/O.
pub const REQ_BACKGROUND: u32 = 1 << 20;
/// No wait (return error instead of blocking).
pub const REQ_NOWAIT: u32 = 1 << 21;
/// Polled I/O completion.
pub const REQ_POLLED: u32 = 1 << 22;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_distinct() {
        let ops = [
            REQ_OP_READ, REQ_OP_WRITE, REQ_OP_FLUSH, REQ_OP_DISCARD,
            REQ_OP_SECURE_ERASE, REQ_OP_WRITE_SAME, REQ_OP_WRITE_ZEROES,
            REQ_OP_ZONE_OPEN, REQ_OP_ZONE_CLOSE, REQ_OP_ZONE_FINISH,
            REQ_OP_ZONE_APPEND, REQ_OP_ZONE_RESET, REQ_OP_ZONE_RESET_ALL,
            REQ_OP_DRV_IN, REQ_OP_DRV_OUT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_ops_within_mask() {
        let ops = [
            REQ_OP_READ, REQ_OP_WRITE, REQ_OP_FLUSH, REQ_OP_DISCARD,
            REQ_OP_WRITE_ZEROES, REQ_OP_DRV_IN, REQ_OP_DRV_OUT,
        ];
        for op in &ops {
            assert_eq!(op & REQ_OP_MASK, *op);
        }
    }

    #[test]
    fn test_flags_are_powers_of_two() {
        let flags = [
            REQ_FAILFAST_DEV, REQ_FAILFAST_TRANSPORT, REQ_FAILFAST_DRIVER,
            REQ_SYNC, REQ_META, REQ_PRIO, REQ_NOMERGE, REQ_IDLE,
            REQ_INTEGRITY, REQ_FUA, REQ_PREFLUSH, REQ_RAHEAD,
            REQ_BACKGROUND, REQ_NOWAIT, REQ_POLLED,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_flags_above_op_mask() {
        let flags = [REQ_FAILFAST_DEV, REQ_SYNC, REQ_FUA, REQ_POLLED];
        for flag in &flags {
            assert_eq!(flag & REQ_OP_MASK, 0, "flag 0x{:x} overlaps op mask", flag);
        }
    }
}

//! `<linux/blk_types.h>` — Block I/O request type and flag constants.
//!
//! Block I/O requests (bios) carry data between the filesystem layer
//! and block device drivers. These constants define the operation
//! types and modifier flags that describe each request.

// ---------------------------------------------------------------------------
// Block I/O operation types (REQ_OP_*)
// ---------------------------------------------------------------------------

/// Read data from device.
pub const REQ_OP_READ: u32 = 0;
/// Write data to device.
pub const REQ_OP_WRITE: u32 = 1;
/// Flush volatile write cache.
pub const REQ_OP_FLUSH: u32 = 2;
/// Discard/trim sectors (SSD TRIM).
pub const REQ_OP_DISCARD: u32 = 3;
/// Secure erase sectors.
pub const REQ_OP_SECURE_ERASE: u32 = 5;
/// Write same data to multiple sectors.
pub const REQ_OP_WRITE_SAME: u32 = 7;
/// Write zeroes to sectors.
pub const REQ_OP_WRITE_ZEROES: u32 = 9;
/// Zone reset (zoned block devices).
pub const REQ_OP_ZONE_RESET: u32 = 15;
/// Zone open (zoned block devices).
pub const REQ_OP_ZONE_OPEN: u32 = 10;
/// Zone close (zoned block devices).
pub const REQ_OP_ZONE_CLOSE: u32 = 11;
/// Zone finish (zoned block devices).
pub const REQ_OP_ZONE_FINISH: u32 = 12;
/// Zone append write (zoned block devices).
pub const REQ_OP_ZONE_APPEND: u32 = 13;
/// SCSI passthrough.
pub const REQ_OP_DRV_IN: u32 = 34;
/// SCSI passthrough (output).
pub const REQ_OP_DRV_OUT: u32 = 35;

// ---------------------------------------------------------------------------
// Block I/O request flags
// ---------------------------------------------------------------------------

/// Request is synchronous.
pub const REQ_SYNC: u32 = 1 << 12;
/// Request has metadata.
pub const REQ_META: u32 = 1 << 13;
/// Request is for paging I/O.
pub const REQ_PRIO: u32 = 1 << 14;
/// Don't merge with other requests.
pub const REQ_NOMERGE: u32 = 1 << 15;
/// Request came from idle I/O scheduler.
pub const REQ_IDLE: u32 = 1 << 16;
/// Integrity payload attached.
pub const REQ_INTEGRITY: u32 = 1 << 17;
/// Force Unit Access (write through cache).
pub const REQ_FUA: u32 = 1 << 18;
/// Request requires preflush.
pub const REQ_PREFLUSH: u32 = 1 << 19;
/// Fail fast on device error.
pub const REQ_FAILFAST_DEV: u32 = 1 << 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_distinct() {
        let ops = [
            REQ_OP_READ, REQ_OP_WRITE, REQ_OP_FLUSH,
            REQ_OP_DISCARD, REQ_OP_SECURE_ERASE, REQ_OP_WRITE_SAME,
            REQ_OP_WRITE_ZEROES, REQ_OP_ZONE_RESET,
            REQ_OP_ZONE_OPEN, REQ_OP_ZONE_CLOSE,
            REQ_OP_ZONE_FINISH, REQ_OP_ZONE_APPEND,
            REQ_OP_DRV_IN, REQ_OP_DRV_OUT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_read_is_zero() {
        assert_eq!(REQ_OP_READ, 0);
    }

    #[test]
    fn test_flags_power_of_two() {
        let flags = [
            REQ_SYNC, REQ_META, REQ_PRIO, REQ_NOMERGE,
            REQ_IDLE, REQ_INTEGRITY, REQ_FUA, REQ_PREFLUSH,
            REQ_FAILFAST_DEV,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            REQ_SYNC, REQ_META, REQ_PRIO, REQ_NOMERGE,
            REQ_IDLE, REQ_INTEGRITY, REQ_FUA, REQ_PREFLUSH,
            REQ_FAILFAST_DEV,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

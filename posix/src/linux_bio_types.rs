//! `<linux/bio.h>` — Block I/O (bio) structure constants.
//!
//! A bio (block I/O) represents a single I/O request to a block
//! device. It contains a list of memory segments (bio_vec) and
//! metadata about the operation (read/write/discard, sector offset,
//! flags). Multiple bios can be merged by the I/O scheduler into
//! a single request for efficiency. The bio is the fundamental
//! unit of block I/O in the kernel, sitting between the filesystem
//! and the block device driver.

// ---------------------------------------------------------------------------
// bio operation types (bi_opf field, upper bits)
// ---------------------------------------------------------------------------

/// Read operation.
pub const REQ_OP_READ: u32 = 0;
/// Write operation.
pub const REQ_OP_WRITE: u32 = 1;
/// Flush volatile write cache.
pub const REQ_OP_FLUSH: u32 = 2;
/// Discard sectors (TRIM/UNMAP).
pub const REQ_OP_DISCARD: u32 = 3;
/// Write same data to multiple sectors.
pub const REQ_OP_WRITE_SAME: u32 = 7;
/// Write zeroes to sectors.
pub const REQ_OP_WRITE_ZEROES: u32 = 9;
/// Zone management (open/close/finish/reset zone).
pub const REQ_OP_ZONE_RESET: u32 = 15;

// ---------------------------------------------------------------------------
// bio flags (bi_opf field, lower bits)
// ---------------------------------------------------------------------------

/// Synchronous I/O (wait for completion).
pub const REQ_SYNC: u32 = 0x0000_0800;
/// Metadata I/O (filesystem metadata, not user data).
pub const REQ_META: u32 = 0x0000_1000;
/// Preflush: flush cache before this I/O.
pub const REQ_PREFLUSH: u32 = 0x0000_2000;
/// I/O should not be merged with others.
pub const REQ_NOMERGE: u32 = 0x0000_4000;
/// I/O is idle priority (background).
pub const REQ_IDLE: u32 = 0x0000_8000;
/// Force Unit Access (write must reach persistent storage).
pub const REQ_FUA: u32 = 0x0001_0000;
/// Completion should use the fast path.
pub const REQ_NOWAIT: u32 = 0x0010_0000;
/// Read-ahead I/O (speculative, can be dropped).
pub const REQ_RAHEAD: u32 = 0x0002_0000;

// ---------------------------------------------------------------------------
// bio status values
// ---------------------------------------------------------------------------

/// I/O completed successfully.
pub const BLK_STS_OK: u32 = 0;
/// Generic I/O error.
pub const BLK_STS_IOERR: u32 = 1;
/// Device not ready.
pub const BLK_STS_NOTSUPP: u32 = 2;
/// Timeout.
pub const BLK_STS_TIMEOUT: u32 = 3;
/// No space left on device.
pub const BLK_STS_NOSPC: u32 = 4;
/// Transport error.
pub const BLK_STS_TRANSPORT: u32 = 5;
/// Target device error.
pub const BLK_STS_TARGET: u32 = 6;
/// Resource temporarily unavailable.
pub const BLK_STS_AGAIN: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_types_distinct() {
        let ops = [
            REQ_OP_READ,
            REQ_OP_WRITE,
            REQ_OP_FLUSH,
            REQ_OP_DISCARD,
            REQ_OP_WRITE_SAME,
            REQ_OP_WRITE_ZEROES,
            REQ_OP_ZONE_RESET,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            REQ_SYNC,
            REQ_META,
            REQ_PREFLUSH,
            REQ_NOMERGE,
            REQ_IDLE,
            REQ_FUA,
            REQ_NOWAIT,
            REQ_RAHEAD,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_status_values_distinct() {
        let statuses = [
            BLK_STS_OK,
            BLK_STS_IOERR,
            BLK_STS_NOTSUPP,
            BLK_STS_TIMEOUT,
            BLK_STS_NOSPC,
            BLK_STS_TRANSPORT,
            BLK_STS_TARGET,
            BLK_STS_AGAIN,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }
}

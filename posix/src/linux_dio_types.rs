//! `<linux/fs.h>` (Direct I/O subset) — Direct I/O constants.
//!
//! Direct I/O bypasses the page cache, transferring data directly
//! between user buffers and the storage device. This eliminates
//! double-copying and cache pollution for applications that manage
//! their own caching (databases, video editors). Requires aligned
//! buffers and aligned file offsets. O_DIRECT is the flag; these
//! constants define alignment requirements and internal DIO states.

// ---------------------------------------------------------------------------
// O_DIRECT alignment requirements
// ---------------------------------------------------------------------------

/// Minimum buffer alignment for direct I/O (512 bytes, sector size).
pub const DIO_ALIGN_512: u32 = 512;
/// 4 KiB alignment (filesystem block size on many setups).
pub const DIO_ALIGN_4K: u32 = 4096;

// ---------------------------------------------------------------------------
// Direct I/O operation types
// ---------------------------------------------------------------------------

/// Direct I/O read operation.
pub const DIO_READ: u32 = 0;
/// Direct I/O write operation.
pub const DIO_WRITE: u32 = 1;

// ---------------------------------------------------------------------------
// Direct I/O flags (kernel internal iocb flags subset)
// ---------------------------------------------------------------------------

/// I/O should be synchronous (wait for completion).
pub const IOCB_FLAG_SYNC: u32 = 0x0000_0001;
/// I/O should use direct path (bypass page cache).
pub const IOCB_FLAG_DIRECT: u32 = 0x0000_0002;
/// I/O uses registered buffers (io_uring).
pub const IOCB_FLAG_REGISTERED: u32 = 0x0000_0004;
/// Append mode (write at end of file).
pub const IOCB_FLAG_APPEND: u32 = 0x0000_0008;
/// Don't post completion event (io_uring).
pub const IOCB_FLAG_SKIP_CQE: u32 = 0x0000_0010;

// ---------------------------------------------------------------------------
// DIO completion states
// ---------------------------------------------------------------------------

/// DIO operation in progress.
pub const DIO_STATE_IN_PROGRESS: u32 = 0;
/// DIO operation completed successfully.
pub const DIO_STATE_COMPLETE: u32 = 1;
/// DIO operation failed.
pub const DIO_STATE_ERROR: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alignment_powers_of_two() {
        assert!(DIO_ALIGN_512.is_power_of_two());
        assert!(DIO_ALIGN_4K.is_power_of_two());
        assert!(DIO_ALIGN_4K > DIO_ALIGN_512);
    }

    #[test]
    fn test_operation_types_distinct() {
        assert_ne!(DIO_READ, DIO_WRITE);
    }

    #[test]
    fn test_iocb_flags_no_overlap() {
        let flags = [
            IOCB_FLAG_SYNC, IOCB_FLAG_DIRECT, IOCB_FLAG_REGISTERED,
            IOCB_FLAG_APPEND, IOCB_FLAG_SKIP_CQE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_dio_states_distinct() {
        let states = [DIO_STATE_IN_PROGRESS, DIO_STATE_COMPLETE, DIO_STATE_ERROR];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}

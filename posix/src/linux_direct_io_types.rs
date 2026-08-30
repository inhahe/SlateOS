//! `<linux/fs.h>` — Direct I/O and O_DIRECT flag constants.
//!
//! Direct I/O bypasses the kernel page cache, reading/writing
//! directly between userspace buffers and the block device. These
//! constants define alignment requirements, flags, and limits.

// ---------------------------------------------------------------------------
// O_DIRECT flag value
// ---------------------------------------------------------------------------

/// Direct I/O flag for open().
pub const O_DIRECT: u32 = 0o40000;

// ---------------------------------------------------------------------------
// Direct I/O alignment requirements
// ---------------------------------------------------------------------------

/// Minimum alignment for O_DIRECT buffers (512 bytes, sector size).
pub const DIO_ALIGN_MIN: u32 = 512;
/// Typical filesystem block alignment (4096).
pub const DIO_ALIGN_BLOCK: u32 = 4096;

// ---------------------------------------------------------------------------
// AIO (Asynchronous I/O) context limits
// ---------------------------------------------------------------------------

/// Default max AIO events per context.
pub const AIO_MAX_NR_DEFAULT: u32 = 65536;
/// AIO events per call limit (io_submit batch).
pub const AIO_MAX_BATCH: u32 = 128;

// ---------------------------------------------------------------------------
// preadv2/pwritev2 flags (RWF_*)
// ---------------------------------------------------------------------------

/// High priority I/O.
pub const RWF_HIPRI: u32 = 0x01;
/// Initiate data sync write.
pub const RWF_DSYNC: u32 = 0x02;
/// Initiate file sync write.
pub const RWF_SYNC: u32 = 0x04;
/// Don't wait for I/O completion.
pub const RWF_NOWAIT: u32 = 0x08;
/// Append data (atomic, at end of file).
pub const RWF_APPEND: u32 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_o_direct() {
        assert_eq!(O_DIRECT, 0o40000);
    }

    #[test]
    fn test_alignment() {
        assert!(DIO_ALIGN_MIN.is_power_of_two());
        assert!(DIO_ALIGN_BLOCK.is_power_of_two());
        assert!(DIO_ALIGN_MIN <= DIO_ALIGN_BLOCK);
    }

    #[test]
    fn test_rwf_flags_no_overlap() {
        let flags = [RWF_HIPRI, RWF_DSYNC, RWF_SYNC, RWF_NOWAIT, RWF_APPEND];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_rwf_flags_power_of_two() {
        assert!(RWF_HIPRI.is_power_of_two());
        assert!(RWF_DSYNC.is_power_of_two());
        assert!(RWF_SYNC.is_power_of_two());
        assert!(RWF_NOWAIT.is_power_of_two());
        assert!(RWF_APPEND.is_power_of_two());
    }

    #[test]
    fn test_aio_limits() {
        assert_eq!(AIO_MAX_NR_DEFAULT, 65536);
        assert_eq!(AIO_MAX_BATCH, 128);
    }
}

//! `<linux/aio_abi.h>` — Additional AIO constants (part 3).
//!
//! Supplementary asynchronous I/O constants covering IOCB commands,
//! flags, and result status codes.

// ---------------------------------------------------------------------------
// IOCB command types
// ---------------------------------------------------------------------------

/// Read.
pub const IOCB_CMD_PREAD: u16 = 0;
/// Write.
pub const IOCB_CMD_PWRITE: u16 = 1;
/// Fsync.
pub const IOCB_CMD_FSYNC: u16 = 2;
/// Fdsync.
pub const IOCB_CMD_FDSYNC: u16 = 3;
/// Poll.
pub const IOCB_CMD_POLL: u16 = 5;
/// No-op.
pub const IOCB_CMD_NOOP: u16 = 6;
/// Vectored read.
pub const IOCB_CMD_PREADV: u16 = 7;
/// Vectored write.
pub const IOCB_CMD_PWRITEV: u16 = 8;

// ---------------------------------------------------------------------------
// IOCB flags
// ---------------------------------------------------------------------------

/// Resfd is valid.
pub const IOCB_FLAG_RESFD: u32 = 1 << 0;
/// Use io_pgetevents() for io cancellation.
pub const IOCB_FLAG_IOPRIO: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// AIO ring magic
// ---------------------------------------------------------------------------

/// AIO ring buffer magic number.
pub const AIO_RING_MAGIC: u32 = 0xa10a10a1;
/// AIO ring compatibility version.
pub const AIO_RING_COMPAT_FEATURES: u32 = 1;
/// AIO ring incompatibility features (none).
pub const AIO_RING_INCOMPAT_FEATURES: u32 = 0;

// ---------------------------------------------------------------------------
// AIO context limits
// ---------------------------------------------------------------------------

/// Maximum number of events per context (default).
pub const AIO_MAX_NR_DEFAULT: u32 = 0x10000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_types_distinct() {
        let cmds = [
            IOCB_CMD_PREAD,
            IOCB_CMD_PWRITE,
            IOCB_CMD_FSYNC,
            IOCB_CMD_FDSYNC,
            IOCB_CMD_POLL,
            IOCB_CMD_NOOP,
            IOCB_CMD_PREADV,
            IOCB_CMD_PWRITEV,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(IOCB_FLAG_RESFD & IOCB_FLAG_IOPRIO, 0);
    }

    #[test]
    fn test_ring_magic() {
        assert_ne!(AIO_RING_MAGIC, 0);
    }

    #[test]
    fn test_max_nr_power_of_two() {
        assert!(AIO_MAX_NR_DEFAULT.is_power_of_two());
    }
}

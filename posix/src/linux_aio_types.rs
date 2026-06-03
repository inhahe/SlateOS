//! `<linux/aio_abi.h>` — Linux AIO (kernel async I/O) constants.
//!
//! Linux native AIO (not POSIX aio) provides async disk I/O through
//! io_setup/io_submit/io_getevents syscalls. Operations are submitted
//! as iocb (I/O control block) structures and completed asynchronously.
//! This is the older interface; io_uring is the modern replacement with
//! better performance and broader operation support.

// ---------------------------------------------------------------------------
// AIO commands (aio_lio_opcode in struct iocb)
// ---------------------------------------------------------------------------

/// Read from file.
pub const IOCB_CMD_PREAD: u16 = 0;
/// Write to file.
pub const IOCB_CMD_PWRITE: u16 = 1;
/// Sync file data (fdatasync equivalent).
pub const IOCB_CMD_FSYNC: u16 = 2;
/// Sync file data + metadata (fsync equivalent).
pub const IOCB_CMD_FDSYNC: u16 = 3;
/// No-op (for testing/benchmarking).
pub const IOCB_CMD_NOOP: u16 = 6;
/// Vectored read (readv equivalent).
pub const IOCB_CMD_PREADV: u16 = 7;
/// Vectored write (writev equivalent).
pub const IOCB_CMD_PWRITEV: u16 = 8;

// ---------------------------------------------------------------------------
// IOCB flags (aio_flags field)
// ---------------------------------------------------------------------------

/// Resfd is valid (deliver completion via eventfd).
pub const IOCB_FLAG_RESFD: u32 = 1 << 0;
/// Use iocb priority field for ioprio.
pub const IOCB_FLAG_IOPRIO: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// AIO context limits
// ---------------------------------------------------------------------------

/// Default maximum number of concurrent AIO requests (/proc/sys/fs/aio-max-nr).
pub const AIO_MAX_NR_DEFAULT: u32 = 65536;
/// Minimum AIO ring size.
pub const AIO_RING_PAGES_MIN: u32 = 1;

// ---------------------------------------------------------------------------
// AIO event structure sizes
// ---------------------------------------------------------------------------

/// Size of struct io_event (result from io_getevents).
pub const IO_EVENT_SIZE: u32 = 32;
/// Size of struct iocb (submission).
pub const IOCB_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            IOCB_CMD_PREAD,
            IOCB_CMD_PWRITE,
            IOCB_CMD_FSYNC,
            IOCB_CMD_FDSYNC,
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
    fn test_iocb_flags_no_overlap() {
        assert_eq!(IOCB_FLAG_RESFD & IOCB_FLAG_IOPRIO, 0);
        assert!(IOCB_FLAG_RESFD.is_power_of_two());
        assert!(IOCB_FLAG_IOPRIO.is_power_of_two());
    }

    #[test]
    fn test_struct_sizes() {
        assert_eq!(IO_EVENT_SIZE, 32);
        assert_eq!(IOCB_SIZE, 64);
    }

    #[test]
    fn test_max_nr_positive() {
        assert!(AIO_MAX_NR_DEFAULT > 0);
    }
}

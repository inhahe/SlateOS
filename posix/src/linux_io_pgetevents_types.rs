//! `<linux/aio_abi.h>` — Linux AIO (Asynchronous I/O) ABI constants.
//!
//! Linux kernel AIO (not POSIX AIO) provides an asynchronous I/O
//! interface via io_setup/io_submit/io_getevents syscalls. AIO
//! control blocks describe I/O operations; events are reaped from
//! the completion ring. Largely superseded by io_uring for new code,
//! but still used by databases (MySQL, PostgreSQL) and storage
//! engines.

// ---------------------------------------------------------------------------
// AIO opcodes (iocb_cmd)
// ---------------------------------------------------------------------------

/// Read operation.
pub const IOCB_CMD_PREAD: u32 = 0;
/// Write operation.
pub const IOCB_CMD_PWRITE: u32 = 1;
/// Fsync operation.
pub const IOCB_CMD_FSYNC: u32 = 2;
/// Fdatasync operation.
pub const IOCB_CMD_FDSYNC: u32 = 3;
/// Poll operation (wait for fd readiness).
pub const IOCB_CMD_POLL: u32 = 5;
/// No-op (for testing).
pub const IOCB_CMD_NOOP: u32 = 6;
/// Vectored read (preadv).
pub const IOCB_CMD_PREADV: u32 = 7;
/// Vectored write (pwritev).
pub const IOCB_CMD_PWRITEV: u32 = 8;

// ---------------------------------------------------------------------------
// IOCB flags (aio_flags field)
// ---------------------------------------------------------------------------

/// Use eventfd for completion notification.
pub const IOCB_FLAG_RESFD: u32 = 1 << 0;
/// Use registered ioctx (io_uring compat).
pub const IOCB_FLAG_IOPRIO: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// AIO ring buffer constants
// ---------------------------------------------------------------------------

/// Magic number in AIO ring header.
pub const AIO_RING_MAGIC: u32 = 0xA10A_10A1;
/// AIO ring compatibility version.
pub const AIO_RING_COMPAT_FEATURES: u32 = 1;
/// Maximum number of events per io_getevents call.
pub const AIO_MAX_NR_DEFAULT: u32 = 0x0001_0000;

// ---------------------------------------------------------------------------
// io_pgetevents flags (Linux 4.18+)
// ---------------------------------------------------------------------------

/// No special flags.
pub const IOCB_PGETEVENTS_NO_FLAGS: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcodes_distinct() {
        let ops = [
            IOCB_CMD_PREAD,
            IOCB_CMD_PWRITE,
            IOCB_CMD_FSYNC,
            IOCB_CMD_FDSYNC,
            IOCB_CMD_POLL,
            IOCB_CMD_NOOP,
            IOCB_CMD_PREADV,
            IOCB_CMD_PWRITEV,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(IOCB_FLAG_RESFD & IOCB_FLAG_IOPRIO, 0);
        assert!(IOCB_FLAG_RESFD.is_power_of_two());
        assert!(IOCB_FLAG_IOPRIO.is_power_of_two());
    }

    #[test]
    fn test_ring_magic() {
        assert_ne!(AIO_RING_MAGIC, 0);
    }

    #[test]
    fn test_max_nr() {
        assert!(AIO_MAX_NR_DEFAULT > 0);
        assert!(AIO_MAX_NR_DEFAULT.is_power_of_two());
    }
}

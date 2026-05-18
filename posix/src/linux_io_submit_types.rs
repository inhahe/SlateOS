//! `<linux/aio_abi.h>` — Linux kernel AIO submission constants.
//!
//! Linux kernel AIO (`io_setup`, `io_submit`, `io_getevents`,
//! `io_destroy`) provides asynchronous I/O at the kernel level.
//! These constants define the iocb opcodes, flags, and context
//! settings.

// ---------------------------------------------------------------------------
// iocb opcodes (IOCB_CMD_*)
// ---------------------------------------------------------------------------

/// Asynchronous read.
pub const IOCB_CMD_PREAD: u32 = 0;
/// Asynchronous write.
pub const IOCB_CMD_PWRITE: u32 = 1;
/// Asynchronous fsync.
pub const IOCB_CMD_FSYNC: u32 = 2;
/// Asynchronous fdatasync.
pub const IOCB_CMD_FDSYNC: u32 = 3;
/// No-op (deprecated).
pub const IOCB_CMD_NOOP: u32 = 6;
/// Asynchronous vectored read.
pub const IOCB_CMD_PREADV: u32 = 7;
/// Asynchronous vectored write.
pub const IOCB_CMD_PWRITEV: u32 = 8;

// ---------------------------------------------------------------------------
// iocb flags (IOCB_FLAG_*)
// ---------------------------------------------------------------------------

/// Use resfd for completion notification.
pub const IOCB_FLAG_RESFD: u32 = 1 << 0;
/// Use io_priority (IOPRIO).
pub const IOCB_FLAG_IOPRIO: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// struct iocb field offsets (Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of aio_data (user data / cookie) in struct iocb.
pub const IOCB_OFF_DATA: u32 = 0;
/// Offset of aio_key in struct iocb.
pub const IOCB_OFF_KEY: u32 = 8;
/// Offset of aio_rw_flags in struct iocb.
pub const IOCB_OFF_RW_FLAGS: u32 = 12;
/// Offset of aio_lio_opcode in struct iocb.
pub const IOCB_OFF_OPCODE: u32 = 16;
/// Offset of aio_reqprio in struct iocb.
pub const IOCB_OFF_REQPRIO: u32 = 18;
/// Offset of aio_fildes (file descriptor) in struct iocb.
pub const IOCB_OFF_FD: u32 = 20;
/// Offset of aio_buf (buffer address) in struct iocb.
pub const IOCB_OFF_BUF: u32 = 24;
/// Offset of aio_nbytes (byte count) in struct iocb.
pub const IOCB_OFF_NBYTES: u32 = 32;
/// Offset of aio_offset (file offset) in struct iocb.
pub const IOCB_OFF_OFFSET: u32 = 40;

/// Size of struct iocb (bytes).
pub const IOCB_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// io_setup limits
// ---------------------------------------------------------------------------

/// Maximum number of AIO contexts per process (default).
pub const AIO_MAX_NR_DEFAULT: u32 = 65536;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcodes_distinct() {
        let ops = [
            IOCB_CMD_PREAD, IOCB_CMD_PWRITE, IOCB_CMD_FSYNC,
            IOCB_CMD_FDSYNC, IOCB_CMD_NOOP,
            IOCB_CMD_PREADV, IOCB_CMD_PWRITEV,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_pread_is_zero() {
        assert_eq!(IOCB_CMD_PREAD, 0);
    }

    #[test]
    fn test_flags_powers_of_two() {
        assert!(IOCB_FLAG_RESFD.is_power_of_two());
        assert!(IOCB_FLAG_IOPRIO.is_power_of_two());
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(IOCB_FLAG_RESFD & IOCB_FLAG_IOPRIO, 0);
    }

    #[test]
    fn test_offsets_ascending() {
        let offsets = [
            IOCB_OFF_DATA, IOCB_OFF_KEY, IOCB_OFF_RW_FLAGS,
            IOCB_OFF_OPCODE, IOCB_OFF_REQPRIO, IOCB_OFF_FD,
            IOCB_OFF_BUF, IOCB_OFF_NBYTES, IOCB_OFF_OFFSET,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_offsets_within_struct() {
        assert!(IOCB_OFF_OFFSET < IOCB_SIZE);
    }

    #[test]
    fn test_iocb_size() {
        assert_eq!(IOCB_SIZE, 64);
    }

    #[test]
    fn test_aio_max() {
        assert_eq!(AIO_MAX_NR_DEFAULT, 65536);
    }
}

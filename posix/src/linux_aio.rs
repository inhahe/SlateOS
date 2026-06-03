//! `<linux/aio_abi.h>` — Linux kernel AIO (Asynchronous I/O) constants.
//!
//! The kernel AIO interface (io_setup/io_submit/io_getevents)
//! provides asynchronous I/O for direct (O_DIRECT) file access.
//! Largely superseded by io_uring but still used by databases
//! and legacy applications.

// ---------------------------------------------------------------------------
// AIO commands (iocb_cmd)
// ---------------------------------------------------------------------------

/// Read operation.
pub const IOCB_CMD_PREAD: u16 = 0;
/// Write operation.
pub const IOCB_CMD_PWRITE: u16 = 1;
/// fsync operation.
pub const IOCB_CMD_FSYNC: u16 = 2;
/// fdatasync operation.
pub const IOCB_CMD_FDSYNC: u16 = 3;
/// Noop (for testing).
pub const IOCB_CMD_NOOP: u16 = 6;
/// Vectored read (readv).
pub const IOCB_CMD_PREADV: u16 = 7;
/// Vectored write (writev).
pub const IOCB_CMD_PWRITEV: u16 = 8;

// ---------------------------------------------------------------------------
// IOCB flags
// ---------------------------------------------------------------------------

/// Set if eventfd notification is desired.
pub const IOCB_FLAG_RESFD: u32 = 1 << 0;
/// Use registered IO priority class.
pub const IOCB_FLAG_IOPRIO: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Context limits
// ---------------------------------------------------------------------------

/// Default maximum AIO events per context.
pub const AIO_MAX_NR_DEFAULT: u32 = 65536;

/// Maximum AIO events system-wide (sysctl aio-max-nr).
pub const AIO_MAX_NR_SYSTEM: u32 = 1_048_576;

// ---------------------------------------------------------------------------
// Sysctl paths
// ---------------------------------------------------------------------------

/// Maximum outstanding AIO requests system-wide.
pub const SYSCTL_AIO_MAX_NR: &str = "fs.aio-max-nr";
/// Current number of AIO contexts.
pub const SYSCTL_AIO_NR: &str = "fs.aio-nr";

// ---------------------------------------------------------------------------
// io_event structure field offsets (for reference)
// ---------------------------------------------------------------------------

/// Size of struct io_event (64 bytes on 64-bit).
pub const IO_EVENT_SIZE: usize = 32;

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
    fn test_flags_powers_of_two() {
        assert!(IOCB_FLAG_RESFD.is_power_of_two());
        assert!(IOCB_FLAG_IOPRIO.is_power_of_two());
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(IOCB_FLAG_RESFD & IOCB_FLAG_IOPRIO, 0);
    }

    #[test]
    fn test_limits() {
        assert!(AIO_MAX_NR_DEFAULT > 0);
        assert!(AIO_MAX_NR_SYSTEM >= AIO_MAX_NR_DEFAULT);
    }

    #[test]
    fn test_sysctl_paths_distinct() {
        assert_ne!(SYSCTL_AIO_MAX_NR, SYSCTL_AIO_NR);
    }
}

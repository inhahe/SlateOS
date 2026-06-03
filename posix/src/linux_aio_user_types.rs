//! `<libaio.h>` / `<linux/aio_abi.h>` — POSIX-style async I/O.
//!
//! `io_setup`/`io_submit`/`io_getevents`/`io_destroy` are the original
//! Linux async-I/O API. Largely superseded by `io_uring`, but still
//! used by databases (Oracle, PostgreSQL with effective_io_concurrency)
//! and by Java's `AsynchronousFileChannel` on Linux.

// ---------------------------------------------------------------------------
// IOCB command numbers (`aio_lio_opcode`)
// ---------------------------------------------------------------------------

pub const IOCB_CMD_PREAD: u16 = 0;
pub const IOCB_CMD_PWRITE: u16 = 1;
pub const IOCB_CMD_FSYNC: u16 = 2;
pub const IOCB_CMD_FDSYNC: u16 = 3;
pub const IOCB_CMD_POLL: u16 = 5;
pub const IOCB_CMD_NOOP: u16 = 6;
pub const IOCB_CMD_PREADV: u16 = 7;
pub const IOCB_CMD_PWRITEV: u16 = 8;

// ---------------------------------------------------------------------------
// IOCB flags (`aio_flags`)
// ---------------------------------------------------------------------------

pub const IOCB_FLAG_RESFD: u32 = 1 << 0;
pub const IOCB_FLAG_IOPRIO: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_IO_SETUP: u32 = 206;
pub const NR_IO_DESTROY: u32 = 207;
pub const NR_IO_GETEVENTS: u32 = 208;
pub const NR_IO_SUBMIT: u32 = 209;
pub const NR_IO_CANCEL: u32 = 210;
pub const NR_IO_PGETEVENTS: u32 = 333;

// ---------------------------------------------------------------------------
// Reasonable limits
// ---------------------------------------------------------------------------

/// `/proc/sys/fs/aio-max-nr` default.
pub const AIO_MAX_NR_DEFAULT: u32 = 65_536;

/// Soft cap on the number of concurrent IOCBs per context (per-user limit).
pub const AIO_RING_MAGIC: u32 = 0xA10A_10A1;

// ---------------------------------------------------------------------------
// Sysctl paths
// ---------------------------------------------------------------------------

pub const SYSCTL_AIO_NR: &str = "/proc/sys/fs/aio-nr";
pub const SYSCTL_AIO_MAX_NR: &str = "/proc/sys/fs/aio-max-nr";

// ---------------------------------------------------------------------------
// `io_event.res` "error" sentinel — negative errno on failure
// ---------------------------------------------------------------------------

pub const IO_EVENT_RES_OK: i64 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iocb_cmds_mostly_dense_with_gap_at_4() {
        // The kernel skipped value 4 (was a withdrawn PREAD_AHEAD).
        assert_eq!(IOCB_CMD_PREAD, 0);
        assert_eq!(IOCB_CMD_PWRITE, 1);
        assert_eq!(IOCB_CMD_FSYNC, 2);
        assert_eq!(IOCB_CMD_FDSYNC, 3);
        assert_eq!(IOCB_CMD_POLL, 5);
        assert_eq!(IOCB_CMD_NOOP, 6);
        assert_eq!(IOCB_CMD_PREADV, 7);
        assert_eq!(IOCB_CMD_PWRITEV, 8);
    }

    #[test]
    fn test_iocb_flags_single_bits() {
        assert!(IOCB_FLAG_RESFD.is_power_of_two());
        assert!(IOCB_FLAG_IOPRIO.is_power_of_two());
        assert_eq!(IOCB_FLAG_RESFD | IOCB_FLAG_IOPRIO, 0x03);
        assert_eq!(IOCB_FLAG_RESFD & IOCB_FLAG_IOPRIO, 0);
    }

    #[test]
    fn test_syscall_numbers_dense_206_to_210() {
        let n = [
            NR_IO_SETUP,
            NR_IO_DESTROY,
            NR_IO_GETEVENTS,
            NR_IO_SUBMIT,
            NR_IO_CANCEL,
        ];
        for (i, &v) in n.iter().enumerate() {
            assert_eq!(v as usize, 206 + i);
        }
        // io_pgetevents was added much later (333).
        assert_eq!(NR_IO_PGETEVENTS, 333);
        assert!(NR_IO_PGETEVENTS > NR_IO_CANCEL);
    }

    #[test]
    fn test_aio_max_nr_default_power_of_two() {
        assert_eq!(AIO_MAX_NR_DEFAULT, 65_536);
        assert!(AIO_MAX_NR_DEFAULT.is_power_of_two());
    }

    #[test]
    fn test_aio_ring_magic() {
        // Sentinel that lets userspace recognise an io_ring mmap.
        assert_eq!(AIO_RING_MAGIC, 0xA10A_10A1);
    }

    #[test]
    fn test_sysctl_paths_under_fs() {
        assert!(SYSCTL_AIO_NR.starts_with("/proc/sys/fs/"));
        assert!(SYSCTL_AIO_MAX_NR.starts_with("/proc/sys/fs/"));
        assert_ne!(SYSCTL_AIO_NR, SYSCTL_AIO_MAX_NR);
    }

    #[test]
    fn test_io_event_res_ok_is_zero() {
        assert_eq!(IO_EVENT_RES_OK, 0);
    }
}

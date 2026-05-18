//! `<aio.h>` — POSIX asynchronous I/O constants.
//!
//! POSIX AIO (`aio_read`, `aio_write`, `aio_error`, `aio_return`,
//! `lio_listio`) provides asynchronous file I/O.  These constants
//! define operation codes, return values, and notification modes.

// ---------------------------------------------------------------------------
// lio_listio() operation codes (aio_lio_opcode)
// ---------------------------------------------------------------------------

/// Read operation.
pub const LIO_READ: u32 = 0;
/// Write operation.
pub const LIO_WRITE: u32 = 1;
/// No operation (skip this entry).
pub const LIO_NOP: u32 = 2;

// ---------------------------------------------------------------------------
// lio_listio() mode parameter
// ---------------------------------------------------------------------------

/// Wait for all operations to complete.
pub const LIO_WAIT: u32 = 0;
/// Return immediately (non-blocking).
pub const LIO_NOWAIT: u32 = 1;

// ---------------------------------------------------------------------------
// aio_cancel() return values
// ---------------------------------------------------------------------------

/// All operations cancelled successfully.
pub const AIO_CANCELED: i32 = 0;
/// Some operations could not be cancelled.
pub const AIO_NOTCANCELED: i32 = 1;
/// All specified operations had already completed.
pub const AIO_ALLDONE: i32 = 2;

// ---------------------------------------------------------------------------
// aio_error() return values
// ---------------------------------------------------------------------------

/// Operation completed successfully.
pub const AIO_OK: i32 = 0;
/// Operation is still in progress.
pub const AIO_INPROGRESS: i32 = -1;

// ---------------------------------------------------------------------------
// aio_suspend() / aio notification
// ---------------------------------------------------------------------------

/// Notify via signal (SIGEV_SIGNAL in sigevent).
pub const AIO_SIGEV_SIGNAL: u32 = 0;
/// Notify via thread (SIGEV_THREAD).
pub const AIO_SIGEV_THREAD: u32 = 2;
/// No notification.
pub const AIO_SIGEV_NONE: u32 = 1;

// ---------------------------------------------------------------------------
// AIO limits
// ---------------------------------------------------------------------------

/// Maximum number of outstanding AIO requests (system default).
pub const AIO_MAX_DEFAULT: u32 = 65536;
/// Maximum priority for AIO requests.
pub const AIO_PRIO_MAX: u32 = 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lio_opcodes_distinct() {
        let ops = [LIO_READ, LIO_WRITE, LIO_NOP];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_read_is_zero() {
        assert_eq!(LIO_READ, 0);
    }

    #[test]
    fn test_lio_modes_distinct() {
        assert_ne!(LIO_WAIT, LIO_NOWAIT);
    }

    #[test]
    fn test_cancel_results_distinct() {
        let results = [AIO_CANCELED, AIO_NOTCANCELED, AIO_ALLDONE];
        for i in 0..results.len() {
            for j in (i + 1)..results.len() {
                assert_ne!(results[i], results[j]);
            }
        }
    }

    #[test]
    fn test_canceled_is_zero() {
        assert_eq!(AIO_CANCELED, 0);
    }

    #[test]
    fn test_inprogress_is_negative() {
        assert!(AIO_INPROGRESS < 0);
    }

    #[test]
    fn test_sigev_types_distinct() {
        let types = [AIO_SIGEV_SIGNAL, AIO_SIGEV_THREAD, AIO_SIGEV_NONE];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_aio_max() {
        assert_eq!(AIO_MAX_DEFAULT, 65536);
    }
}

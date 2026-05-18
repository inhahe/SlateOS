//! `<linux/aio_abi.h>` — Linux kernel AIO cancellation and event constants.
//!
//! `io_cancel()` cancels outstanding AIO requests and
//! `io_getevents()` retrieves completion events.  These
//! constants define the event structure layout and
//! timeout-related values.

// ---------------------------------------------------------------------------
// struct io_event field offsets (Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of data (user cookie from iocb) in struct io_event.
pub const IO_EVENT_OFF_DATA: u32 = 0;
/// Offset of obj (iocb pointer) in struct io_event.
pub const IO_EVENT_OFF_OBJ: u32 = 8;
/// Offset of res (result / bytes transferred) in struct io_event.
pub const IO_EVENT_OFF_RES: u32 = 16;
/// Offset of res2 (secondary result / errno) in struct io_event.
pub const IO_EVENT_OFF_RES2: u32 = 24;

/// Size of struct io_event (bytes).
pub const IO_EVENT_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// io_getevents limits
// ---------------------------------------------------------------------------

/// Maximum number of events per io_getevents call.
pub const IO_GETEVENTS_MAX_NR: u32 = 65536;
/// Minimum number of events to request.
pub const IO_GETEVENTS_MIN_NR: u32 = 1;

// ---------------------------------------------------------------------------
// io_cancel return values
// ---------------------------------------------------------------------------

/// Cancellation succeeded.
pub const IO_CANCEL_OK: i32 = 0;
/// Request was not found (already completed or invalid).
pub const IO_CANCEL_EAGAIN: i32 = -11;

// ---------------------------------------------------------------------------
// AIO context state
// ---------------------------------------------------------------------------

/// Context is active and accepting submissions.
pub const AIO_CTX_ACTIVE: u32 = 0;
/// Context is being destroyed.
pub const AIO_CTX_DEAD: u32 = 1;

// ---------------------------------------------------------------------------
// io_pgetevents (extended version with signal mask)
// ---------------------------------------------------------------------------

/// Size of aio_sigset structure (sigset_t pointer + size).
pub const AIO_SIGSET_SIZE: u32 = 16;

// ---------------------------------------------------------------------------
// Kernel AIO ring buffer constants
// ---------------------------------------------------------------------------

/// Magic number for AIO ring buffer header.
pub const AIO_RING_MAGIC: u32 = 0xa10a10a1;
/// AIO ring compatibility version.
pub const AIO_RING_COMPAT_VERSION: u32 = 1;
/// Size of AIO ring header (bytes).
pub const AIO_RING_HEADER_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_offsets_ascending() {
        let offsets = [
            IO_EVENT_OFF_DATA, IO_EVENT_OFF_OBJ,
            IO_EVENT_OFF_RES, IO_EVENT_OFF_RES2,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_event_offsets_within_struct() {
        assert!(IO_EVENT_OFF_RES2 < IO_EVENT_SIZE);
    }

    #[test]
    fn test_event_size() {
        assert_eq!(IO_EVENT_SIZE, 32);
    }

    #[test]
    fn test_data_at_start() {
        assert_eq!(IO_EVENT_OFF_DATA, 0);
    }

    #[test]
    fn test_getevents_limits() {
        assert!(IO_GETEVENTS_MAX_NR >= IO_GETEVENTS_MIN_NR);
    }

    #[test]
    fn test_cancel_ok() {
        assert_eq!(IO_CANCEL_OK, 0);
    }

    #[test]
    fn test_cancel_eagain() {
        assert_eq!(IO_CANCEL_EAGAIN, -11);
    }

    #[test]
    fn test_ctx_states_distinct() {
        assert_ne!(AIO_CTX_ACTIVE, AIO_CTX_DEAD);
    }

    #[test]
    fn test_ring_magic() {
        assert_eq!(AIO_RING_MAGIC, 0xa10a10a1);
    }

    #[test]
    fn test_ring_version() {
        assert_eq!(AIO_RING_COMPAT_VERSION, 1);
    }

    #[test]
    fn test_ring_header_size() {
        assert_eq!(AIO_RING_HEADER_SIZE, 32);
    }
}

//! AIO error handling and event-FD integration.
//!
//! This module covers the post-2.5 additions to Linux AIO: eventfd
//! notification, the AIO context-id type, and reasonable cancel-result
//! handling. The original `aio_user_types` module covers the bare
//! syscall ABI; `aio2_user_types` covers wire layout; this module
//! covers the runtime semantics.

// ---------------------------------------------------------------------------
// AIO context identifier — opaque u64 in userspace
// ---------------------------------------------------------------------------

pub const AIO_CONTEXT_NULL: u64 = 0;

// ---------------------------------------------------------------------------
// io_cancel(2) result codes (returned through `io_event.res`)
// ---------------------------------------------------------------------------

pub const AIO_CANCEL_OK: i64 = 0;
/// Equivalent to `-EINPROGRESS` — request was already running.
pub const AIO_CANCEL_INPROGRESS: i64 = -115;
/// Equivalent to `-ENOENT`.
pub const AIO_CANCEL_NOT_FOUND: i64 = -2;
/// Equivalent to `-EINVAL`.
pub const AIO_CANCEL_INVAL: i64 = -22;

// ---------------------------------------------------------------------------
// eventfd notification (`IOCB_FLAG_RESFD`)
// ---------------------------------------------------------------------------

/// Each completed event posts a u64 increment to the eventfd.
pub const AIO_EVENTFD_INCREMENT: u64 = 1;

/// Maximum events one eventfd read can drain (a single u64).
pub const AIO_EVENTFD_READ_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// Cancellation status from io_event.res when an IOCB was cancelled
// ---------------------------------------------------------------------------

pub const AIO_RES_CANCELLED: i64 = -125; // -ECANCELED

// ---------------------------------------------------------------------------
// Stream-priority encoding (`reqprio` field; matches ioprio_set(2))
// ---------------------------------------------------------------------------

pub const IOPRIO_CLASS_NONE: u16 = 0;
pub const IOPRIO_CLASS_RT: u16 = 1;
pub const IOPRIO_CLASS_BE: u16 = 2;
pub const IOPRIO_CLASS_IDLE: u16 = 3;

pub const IOPRIO_CLASS_SHIFT: u16 = 13;
pub const IOPRIO_LEVEL_MASK: u16 = (1 << IOPRIO_CLASS_SHIFT) - 1;

#[must_use]
pub const fn ioprio_encode(class: u16, level: u16) -> u16 {
    (class << IOPRIO_CLASS_SHIFT) | (level & IOPRIO_LEVEL_MASK)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_context_zero() {
        assert_eq!(AIO_CONTEXT_NULL, 0);
    }

    #[test]
    fn test_cancel_result_codes_negative_errno() {
        // All non-OK results are negative errno values.
        for v in [
            AIO_CANCEL_INPROGRESS,
            AIO_CANCEL_NOT_FOUND,
            AIO_CANCEL_INVAL,
            AIO_RES_CANCELLED,
        ] {
            assert!(v < 0);
        }
        assert_eq!(AIO_CANCEL_OK, 0);
        // -EINPROGRESS, -ENOENT, -EINVAL, -ECANCELED.
        assert_eq!(AIO_CANCEL_INPROGRESS, -115);
        assert_eq!(AIO_CANCEL_NOT_FOUND, -2);
        assert_eq!(AIO_CANCEL_INVAL, -22);
        assert_eq!(AIO_RES_CANCELLED, -125);
    }

    #[test]
    fn test_eventfd_constants() {
        // Each completed AIO event posts +1.
        assert_eq!(AIO_EVENTFD_INCREMENT, 1);
        // eventfd read returns a u64 — 8 bytes.
        assert_eq!(AIO_EVENTFD_READ_SIZE, 8);
    }

    #[test]
    fn test_ioprio_classes_dense() {
        let c = [
            IOPRIO_CLASS_NONE,
            IOPRIO_CLASS_RT,
            IOPRIO_CLASS_BE,
            IOPRIO_CLASS_IDLE,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_ioprio_class_shift_and_mask() {
        // Class lives in bits 13..15, level in bits 0..12.
        assert_eq!(IOPRIO_CLASS_SHIFT, 13);
        assert_eq!(IOPRIO_LEVEL_MASK, 0x1FFF);
        // Class and level masks are disjoint within a u16.
        let class_mask = !IOPRIO_LEVEL_MASK & 0xFFFF;
        assert_eq!(class_mask & IOPRIO_LEVEL_MASK, 0);
    }

    #[test]
    fn test_ioprio_encode_round_trip() {
        // Best-effort, level 4.
        let v = ioprio_encode(IOPRIO_CLASS_BE, 4);
        assert_eq!(v >> IOPRIO_CLASS_SHIFT, IOPRIO_CLASS_BE);
        assert_eq!(v & IOPRIO_LEVEL_MASK, 4);
        // Real-time, level 0.
        let v = ioprio_encode(IOPRIO_CLASS_RT, 0);
        assert_eq!(v, IOPRIO_CLASS_RT << IOPRIO_CLASS_SHIFT);
        // Level is truncated to 13 bits — bit 13 should be discarded.
        let v = ioprio_encode(IOPRIO_CLASS_NONE, 0x2000);
        assert_eq!(v, 0);
    }
}

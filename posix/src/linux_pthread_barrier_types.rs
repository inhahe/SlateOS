//! `<pthread.h>` — Pthread barrier constants.
//!
//! Barriers synchronize a fixed number of threads at a
//! rendezvous point.  These constants define the serial
//! thread return value, process-sharing, and layout.

// ---------------------------------------------------------------------------
// Barrier return values
// ---------------------------------------------------------------------------

/// Returned to exactly one thread (the "serial" thread) at the barrier.
pub const PTHREAD_BARRIER_SERIAL_THREAD: i32 = -1;

// ---------------------------------------------------------------------------
// Process-shared attribute
// ---------------------------------------------------------------------------

/// Barrier is private to the process (default).
pub const PTHREAD_BARRIER_PRIVATE: u32 = 0;
/// Barrier is shared between processes.
pub const PTHREAD_BARRIER_SHARED: u32 = 1;

// ---------------------------------------------------------------------------
// Internal layout (glibc x86_64)
// ---------------------------------------------------------------------------

/// Size of pthread_barrier_t on Linux x86_64 (bytes).
pub const PTHREAD_BARRIER_T_SIZE: u32 = 32;
/// Alignment of pthread_barrier_t (bytes).
pub const PTHREAD_BARRIER_T_ALIGN: u32 = 8;

/// Offset of __in (current epoch count) in pthread_barrier_t.
pub const BARRIER_OFF_IN: u32 = 0;
/// Offset of __current_round in pthread_barrier_t.
pub const BARRIER_OFF_CURRENT_ROUND: u32 = 4;
/// Offset of __count (total threads) in pthread_barrier_t.
pub const BARRIER_OFF_COUNT: u32 = 8;
/// Offset of __shared (process-shared flag) in pthread_barrier_t.
pub const BARRIER_OFF_SHARED: u32 = 12;
/// Offset of __out (leaving epoch count) in pthread_barrier_t.
pub const BARRIER_OFF_OUT: u32 = 16;

/// Size of pthread_barrierattr_t (bytes).
pub const PTHREAD_BARRIERATTR_T_SIZE: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serial_thread_is_negative() {
        assert_eq!(PTHREAD_BARRIER_SERIAL_THREAD, -1);
    }

    #[test]
    fn test_process_shared_distinct() {
        assert_ne!(PTHREAD_BARRIER_PRIVATE, PTHREAD_BARRIER_SHARED);
    }

    #[test]
    fn test_private_is_zero() {
        assert_eq!(PTHREAD_BARRIER_PRIVATE, 0);
    }

    #[test]
    fn test_barrier_t_size() {
        assert_eq!(PTHREAD_BARRIER_T_SIZE, 32);
    }

    #[test]
    fn test_barrier_t_align() {
        assert!(PTHREAD_BARRIER_T_ALIGN.is_power_of_two());
    }

    #[test]
    fn test_offsets_ascending() {
        let offsets = [
            BARRIER_OFF_IN,
            BARRIER_OFF_CURRENT_ROUND,
            BARRIER_OFF_COUNT,
            BARRIER_OFF_SHARED,
            BARRIER_OFF_OUT,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_offsets_within_struct() {
        assert!(BARRIER_OFF_OUT < PTHREAD_BARRIER_T_SIZE);
    }

    #[test]
    fn test_barrierattr_size() {
        assert_eq!(PTHREAD_BARRIERATTR_T_SIZE, 4);
    }
}

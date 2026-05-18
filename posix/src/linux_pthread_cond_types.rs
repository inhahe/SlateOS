//! `<pthread.h>` — Pthread condition variable constants.
//!
//! Condition variables allow threads to block until a
//! predicate becomes true.  These constants define clock
//! selection, process-sharing, and internal layout.

// ---------------------------------------------------------------------------
// Condition variable clock (pthread_condattr_setclock)
// ---------------------------------------------------------------------------

/// Use CLOCK_REALTIME (default, wall-clock time).
pub const PTHREAD_COND_CLOCK_REALTIME: u32 = 0;
/// Use CLOCK_MONOTONIC (immune to time-of-day changes).
pub const PTHREAD_COND_CLOCK_MONOTONIC: u32 = 1;

// ---------------------------------------------------------------------------
// Process-shared attribute
// ---------------------------------------------------------------------------

/// Private to process (default).
pub const PTHREAD_COND_PRIVATE: u32 = 0;
/// Shared between processes.
pub const PTHREAD_COND_SHARED: u32 = 1;

// ---------------------------------------------------------------------------
// Internal layout (glibc x86_64, __pthread_cond_s)
// ---------------------------------------------------------------------------

/// Size of pthread_cond_t on Linux x86_64 (bytes).
pub const PTHREAD_COND_T_SIZE: u32 = 48;
/// Alignment of pthread_cond_t (bytes).
pub const PTHREAD_COND_T_ALIGN: u32 = 8;

/// Offset of __wseq (waiter sequence counter) in pthread_cond_t.
pub const COND_OFF_WSEQ: u32 = 0;
/// Offset of __g1_start in pthread_cond_t.
pub const COND_OFF_G1_START: u32 = 8;
/// Offset of __g_refs[0] in pthread_cond_t.
pub const COND_OFF_G_REFS_0: u32 = 16;
/// Offset of __g_refs[1] in pthread_cond_t.
pub const COND_OFF_G_REFS_1: u32 = 20;
/// Offset of __g_size[0] in pthread_cond_t.
pub const COND_OFF_G_SIZE_0: u32 = 24;
/// Offset of __g_size[1] in pthread_cond_t.
pub const COND_OFF_G_SIZE_1: u32 = 28;
/// Offset of __g1_orig_size in pthread_cond_t.
pub const COND_OFF_G1_ORIG_SIZE: u32 = 32;
/// Offset of __wrefs in pthread_cond_t.
pub const COND_OFF_WREFS: u32 = 36;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clocks_distinct() {
        assert_ne!(PTHREAD_COND_CLOCK_REALTIME, PTHREAD_COND_CLOCK_MONOTONIC);
    }

    #[test]
    fn test_realtime_is_zero() {
        assert_eq!(PTHREAD_COND_CLOCK_REALTIME, 0);
    }

    #[test]
    fn test_process_shared_distinct() {
        assert_ne!(PTHREAD_COND_PRIVATE, PTHREAD_COND_SHARED);
    }

    #[test]
    fn test_cond_t_size() {
        assert_eq!(PTHREAD_COND_T_SIZE, 48);
    }

    #[test]
    fn test_cond_t_align() {
        assert!(PTHREAD_COND_T_ALIGN.is_power_of_two());
    }

    #[test]
    fn test_offsets_ascending() {
        let offsets = [
            COND_OFF_WSEQ, COND_OFF_G1_START,
            COND_OFF_G_REFS_0, COND_OFF_G_REFS_1,
            COND_OFF_G_SIZE_0, COND_OFF_G_SIZE_1,
            COND_OFF_G1_ORIG_SIZE, COND_OFF_WREFS,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_offsets_within_struct() {
        assert!(COND_OFF_WREFS < PTHREAD_COND_T_SIZE);
    }

    #[test]
    fn test_wseq_at_start() {
        assert_eq!(COND_OFF_WSEQ, 0);
    }
}

//! `<pthread.h>` — Pthread spinlock constants.
//!
//! Spinlocks provide busy-waiting mutual exclusion suitable
//! for very short critical sections.  These constants define
//! the lock states and process-sharing attributes.

// ---------------------------------------------------------------------------
// Spinlock states
// ---------------------------------------------------------------------------

/// Spinlock is unlocked.
pub const PTHREAD_SPINLOCK_UNLOCKED: u32 = 0;
/// Spinlock is locked.
pub const PTHREAD_SPINLOCK_LOCKED: u32 = 1;

// ---------------------------------------------------------------------------
// Process-shared attribute
// ---------------------------------------------------------------------------

/// Spinlock is private to the process (default).
pub const PTHREAD_SPINLOCK_PRIVATE: u32 = 0;
/// Spinlock is shared between processes.
pub const PTHREAD_SPINLOCK_SHARED: u32 = 1;

// ---------------------------------------------------------------------------
// Internal layout
// ---------------------------------------------------------------------------

/// Size of pthread_spinlock_t on Linux x86_64 (bytes).
pub const PTHREAD_SPINLOCK_T_SIZE: u32 = 4;
/// Alignment of pthread_spinlock_t (bytes).
pub const PTHREAD_SPINLOCK_T_ALIGN: u32 = 4;

// ---------------------------------------------------------------------------
// Spin limits (implementation hints)
// ---------------------------------------------------------------------------

/// Default number of spins before yielding (glibc adaptive mutexes).
pub const PTHREAD_SPIN_COUNT_DEFAULT: u32 = 100;
/// Maximum spin count for adaptive mutexes.
pub const PTHREAD_SPIN_COUNT_MAX: u32 = 1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        assert_ne!(PTHREAD_SPINLOCK_UNLOCKED, PTHREAD_SPINLOCK_LOCKED);
    }

    #[test]
    fn test_unlocked_is_zero() {
        assert_eq!(PTHREAD_SPINLOCK_UNLOCKED, 0);
    }

    #[test]
    fn test_locked_is_one() {
        assert_eq!(PTHREAD_SPINLOCK_LOCKED, 1);
    }

    #[test]
    fn test_process_shared_distinct() {
        assert_ne!(PTHREAD_SPINLOCK_PRIVATE, PTHREAD_SPINLOCK_SHARED);
    }

    #[test]
    fn test_spinlock_t_size() {
        assert_eq!(PTHREAD_SPINLOCK_T_SIZE, 4);
    }

    #[test]
    fn test_spinlock_t_align() {
        assert!(PTHREAD_SPINLOCK_T_ALIGN.is_power_of_two());
    }

    #[test]
    fn test_spin_count_default() {
        assert_eq!(PTHREAD_SPIN_COUNT_DEFAULT, 100);
    }

    #[test]
    fn test_spin_count_max_gte_default() {
        assert!(PTHREAD_SPIN_COUNT_MAX >= PTHREAD_SPIN_COUNT_DEFAULT);
    }
}

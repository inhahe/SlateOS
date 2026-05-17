//! `<semaphore.h>` — POSIX semaphore constants.
//!
//! POSIX semaphores come in two forms: named semaphores (created
//! with sem_open, identified by name in /dev/shm) and unnamed
//! semaphores (created with sem_init, placed in shared memory or
//! on the stack). Unlike System V semaphores, POSIX semaphores
//! are individual counting semaphores (not arrays) and have a
//! simpler API. They are the preferred semaphore mechanism for
//! new code, especially for thread synchronization.

// ---------------------------------------------------------------------------
// POSIX semaphore value limits
// ---------------------------------------------------------------------------

/// Maximum value a POSIX semaphore can hold.
pub const SEM_VALUE_MAX: u32 = 0x7FFF_FFFF;

// ---------------------------------------------------------------------------
// POSIX semaphore name limits
// ---------------------------------------------------------------------------

/// Maximum length of a named semaphore name (including leading /).
pub const SEM_NAME_MAX: u32 = 251;
/// Prefix for named semaphores in the filesystem.
pub const SEM_NAME_PREFIX_LEN: u32 = 9;

// ---------------------------------------------------------------------------
// sem_open() flags
// ---------------------------------------------------------------------------

/// Create semaphore if it doesn't exist.
pub const SEM_O_CREAT: u32 = 0o100;
/// Fail if semaphore exists (with O_CREAT).
pub const SEM_O_EXCL: u32 = 0o200;

// ---------------------------------------------------------------------------
// sem_init() pshared values
// ---------------------------------------------------------------------------

/// Semaphore is shared between threads (not processes).
pub const SEM_PROCESS_PRIVATE: u32 = 0;
/// Semaphore is shared between processes.
pub const SEM_PROCESS_SHARED: u32 = 1;

// ---------------------------------------------------------------------------
// sem_timedwait() / sem_trywait() return indicators
// ---------------------------------------------------------------------------

/// Semaphore operation succeeded.
pub const SEM_OK: u32 = 0;
/// sem_trywait would block (semaphore value is 0).
pub const SEM_WOULD_BLOCK: u32 = 1;
/// sem_timedwait timed out.
pub const SEM_TIMED_OUT: u32 = 2;
/// Semaphore was interrupted by signal.
pub const SEM_INTERRUPTED: u32 = 3;

// ---------------------------------------------------------------------------
// Futex-based implementation constants (Linux internal)
// ---------------------------------------------------------------------------

/// Semaphore value field bit width in futex word.
pub const SEM_VALUE_BITS: u32 = 31;
/// Contention bit (waiters are blocked on this semaphore).
pub const SEM_CONTENTION_BIT: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_max() {
        assert_eq!(SEM_VALUE_MAX, i32::MAX as u32);
    }

    #[test]
    fn test_open_flags_no_overlap() {
        assert_eq!(SEM_O_CREAT & SEM_O_EXCL, 0);
    }

    #[test]
    fn test_pshared_distinct() {
        assert_ne!(SEM_PROCESS_PRIVATE, SEM_PROCESS_SHARED);
    }

    #[test]
    fn test_return_values_distinct() {
        let vals = [SEM_OK, SEM_WOULD_BLOCK, SEM_TIMED_OUT, SEM_INTERRUPTED];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_contention_bit_separate() {
        // Contention bit doesn't overlap with value bits
        assert_eq!(SEM_CONTENTION_BIT >> SEM_VALUE_BITS, 1);
    }

    #[test]
    fn test_name_max_positive() {
        assert!(SEM_NAME_MAX > 0);
    }
}

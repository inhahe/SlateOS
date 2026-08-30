//! `<semaphore.h>` — POSIX named/unnamed semaphore constants.
//!
//! POSIX semaphores (`sem_init`, `sem_open`, `sem_wait`, `sem_post`)
//! provide inter-process and inter-thread synchronization.  These
//! constants define limits, error values, and flags.

// ---------------------------------------------------------------------------
// Semaphore limits
// ---------------------------------------------------------------------------

/// Maximum value a semaphore may have.
pub const SEM_VALUE_MAX: u32 = 0x7FFFFFFF;
/// Maximum number of named semaphores per process (implementation limit).
pub const SEM_NSEMS_MAX: u32 = 256;

// ---------------------------------------------------------------------------
// sem_open() flags
// ---------------------------------------------------------------------------

/// Create semaphore if it does not exist.
pub const SEM_O_CREAT: u32 = 0o100;
/// Fail if semaphore exists (used with O_CREAT).
pub const SEM_O_EXCL: u32 = 0o200;

// ---------------------------------------------------------------------------
// sem_open() error return
// ---------------------------------------------------------------------------

/// Failed sem_open() return value (SEM_FAILED, usually (sem_t*)-1).
pub const SEM_FAILED_VALUE: usize = usize::MAX;

// ---------------------------------------------------------------------------
// Internal sem_t layout constants (glibc x86_64)
// ---------------------------------------------------------------------------

/// Size of sem_t on Linux x86_64 (bytes).
pub const SEM_T_SIZE: u32 = 32;
/// Alignment of sem_t (bytes).
pub const SEM_T_ALIGN: u32 = 8;
/// Offset of the value field in sem_t.
pub const SEM_OFF_VALUE: u32 = 0;
/// Offset of the private flag in sem_t.
pub const SEM_OFF_PRIVATE: u32 = 4;
/// Offset of the nwaiters field in sem_t.
pub const SEM_OFF_NWAITERS: u32 = 8;

// ---------------------------------------------------------------------------
// Process-shared flag
// ---------------------------------------------------------------------------

/// Semaphore is shared between processes.
pub const SEM_PROCESS_SHARED: u32 = 1;
/// Semaphore is private to the process (default).
pub const SEM_PROCESS_PRIVATE: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_max() {
        assert_eq!(SEM_VALUE_MAX, 0x7FFFFFFF);
    }

    #[test]
    fn test_nsems_max() {
        assert_eq!(SEM_NSEMS_MAX, 256);
    }

    #[test]
    fn test_open_flags_no_overlap() {
        assert_eq!(SEM_O_CREAT & SEM_O_EXCL, 0);
    }

    #[test]
    fn test_failed_value() {
        assert_eq!(SEM_FAILED_VALUE, usize::MAX);
    }

    #[test]
    fn test_sem_t_size() {
        assert_eq!(SEM_T_SIZE, 32);
    }

    #[test]
    fn test_sem_t_align() {
        assert!(SEM_T_ALIGN.is_power_of_two());
    }

    #[test]
    fn test_offsets_ascending() {
        let offsets = [SEM_OFF_VALUE, SEM_OFF_PRIVATE, SEM_OFF_NWAITERS];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_offsets_within_struct() {
        assert!(SEM_OFF_NWAITERS < SEM_T_SIZE);
    }

    #[test]
    fn test_process_shared_distinct() {
        assert_ne!(SEM_PROCESS_SHARED, SEM_PROCESS_PRIVATE);
    }

    #[test]
    fn test_process_private_is_zero() {
        assert_eq!(SEM_PROCESS_PRIVATE, 0);
    }
}

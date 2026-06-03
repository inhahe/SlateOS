//! `<pthread.h>` — Pthread read-write lock constants.
//!
//! Read-write locks allow concurrent readers or exclusive writer
//! access.  These constants define preference policies and the
//! internal layout of `pthread_rwlock_t`.

// ---------------------------------------------------------------------------
// rwlock kind / preference (pthread_rwlockattr_setkind_np, GNU extension)
// ---------------------------------------------------------------------------

/// Prefer readers (default) — readers are never blocked by writers.
pub const PTHREAD_RWLOCK_PREFER_READER_NP: u32 = 0;
/// Prefer writers — pending writers block new readers.
pub const PTHREAD_RWLOCK_PREFER_WRITER_NP: u32 = 1;
/// Prefer writers (non-recursive) — avoids writer starvation.
pub const PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP: u32 = 2;
/// Default rwlock kind (same as prefer reader).
pub const PTHREAD_RWLOCK_DEFAULT_NP: u32 = 0;

// ---------------------------------------------------------------------------
// rwlock process-shared attribute
// ---------------------------------------------------------------------------

/// Private to process (default).
pub const PTHREAD_RWLOCK_PRIVATE: u32 = 0;
/// Shared between processes.
pub const PTHREAD_RWLOCK_SHARED: u32 = 1;

// ---------------------------------------------------------------------------
// Internal layout (glibc x86_64, __pthread_rwlock_arch_t)
// ---------------------------------------------------------------------------

/// Size of pthread_rwlock_t on Linux x86_64 (bytes).
pub const PTHREAD_RWLOCK_T_SIZE: u32 = 56;
/// Alignment of pthread_rwlock_t (bytes).
pub const PTHREAD_RWLOCK_T_ALIGN: u32 = 8;

/// Offset of __readers (reader count) in pthread_rwlock_t.
pub const RWLOCK_OFF_READERS: u32 = 0;
/// Offset of __writers (writer count) in pthread_rwlock_t.
pub const RWLOCK_OFF_WRITERS: u32 = 4;
/// Offset of __wrphase_futex in pthread_rwlock_t.
pub const RWLOCK_OFF_WRPHASE: u32 = 8;
/// Offset of __writers_futex in pthread_rwlock_t.
pub const RWLOCK_OFF_WRITERS_FUTEX: u32 = 12;
/// Offset of __flags in pthread_rwlock_t.
pub const RWLOCK_OFF_FLAGS: u32 = 48;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kinds_distinct() {
        let kinds = [
            PTHREAD_RWLOCK_PREFER_READER_NP,
            PTHREAD_RWLOCK_PREFER_WRITER_NP,
            PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP,
        ];
        for i in 0..kinds.len() {
            for j in (i + 1)..kinds.len() {
                assert_ne!(kinds[i], kinds[j]);
            }
        }
    }

    #[test]
    fn test_default_is_prefer_reader() {
        assert_eq!(PTHREAD_RWLOCK_DEFAULT_NP, PTHREAD_RWLOCK_PREFER_READER_NP);
    }

    #[test]
    fn test_process_shared_distinct() {
        assert_ne!(PTHREAD_RWLOCK_PRIVATE, PTHREAD_RWLOCK_SHARED);
    }

    #[test]
    fn test_rwlock_t_size() {
        assert_eq!(PTHREAD_RWLOCK_T_SIZE, 56);
    }

    #[test]
    fn test_rwlock_t_align() {
        assert!(PTHREAD_RWLOCK_T_ALIGN.is_power_of_two());
    }

    #[test]
    fn test_offsets_ascending() {
        let offsets = [
            RWLOCK_OFF_READERS,
            RWLOCK_OFF_WRITERS,
            RWLOCK_OFF_WRPHASE,
            RWLOCK_OFF_WRITERS_FUTEX,
            RWLOCK_OFF_FLAGS,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_offsets_within_struct() {
        assert!(RWLOCK_OFF_FLAGS < PTHREAD_RWLOCK_T_SIZE);
    }
}

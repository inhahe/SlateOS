//! `<pthread.h>` — Pthread mutex constants.
//!
//! Mutex types, protocol flags, robustness settings, and
//! process-sharing attributes for POSIX thread mutexes.

// ---------------------------------------------------------------------------
// Mutex types (pthread_mutexattr_settype)
// ---------------------------------------------------------------------------

/// Normal (default) mutex — no error checking, no recursion.
pub const PTHREAD_MUTEX_NORMAL: u32 = 0;
/// Recursive mutex — same thread may lock multiple times.
pub const PTHREAD_MUTEX_RECURSIVE: u32 = 1;
/// Error-checking mutex — returns error on double lock.
pub const PTHREAD_MUTEX_ERRORCHECK: u32 = 2;
/// Default mutex type (alias for NORMAL on Linux).
pub const PTHREAD_MUTEX_DEFAULT: u32 = 0;

// ---------------------------------------------------------------------------
// Mutex protocol (pthread_mutexattr_setprotocol)
// ---------------------------------------------------------------------------

/// No priority protocol.
pub const PTHREAD_PRIO_NONE: u32 = 0;
/// Priority inheritance protocol.
pub const PTHREAD_PRIO_INHERIT: u32 = 1;
/// Priority ceiling protocol.
pub const PTHREAD_PRIO_PROTECT: u32 = 2;

// ---------------------------------------------------------------------------
// Mutex robustness (pthread_mutexattr_setrobust)
// ---------------------------------------------------------------------------

/// Stale mutex (default) — undefined behaviour if owner dies.
pub const PTHREAD_MUTEX_STALLED: u32 = 0;
/// Robust mutex — returns EOWNERDEAD if owner dies.
pub const PTHREAD_MUTEX_ROBUST: u32 = 1;

// ---------------------------------------------------------------------------
// Process-shared attribute
// ---------------------------------------------------------------------------

/// Mutex is private to the process (default).
pub const PTHREAD_PROCESS_PRIVATE: u32 = 0;
/// Mutex is shared between processes.
pub const PTHREAD_PROCESS_SHARED: u32 = 1;

// ---------------------------------------------------------------------------
// Mutex initializer sentinels (internal values)
// ---------------------------------------------------------------------------

/// Static initializer value for normal mutex.
pub const PTHREAD_MUTEX_INIT_KIND_NORMAL: u32 = 0;
/// Static initializer value for recursive mutex.
pub const PTHREAD_MUTEX_INIT_KIND_RECURSIVE: u32 = 1;
/// Static initializer value for errorcheck mutex.
pub const PTHREAD_MUTEX_INIT_KIND_ERRORCHECK: u32 = 2;

// ---------------------------------------------------------------------------
// Mutex internal layout (glibc x86_64, __pthread_mutex_s)
// ---------------------------------------------------------------------------

/// Size of pthread_mutex_t on Linux x86_64 (bytes).
pub const PTHREAD_MUTEX_T_SIZE: u32 = 40;
/// Alignment of pthread_mutex_t (bytes).
pub const PTHREAD_MUTEX_T_ALIGN: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutex_types_distinct() {
        let types = [
            PTHREAD_MUTEX_NORMAL, PTHREAD_MUTEX_RECURSIVE,
            PTHREAD_MUTEX_ERRORCHECK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_normal_is_zero() {
        assert_eq!(PTHREAD_MUTEX_NORMAL, 0);
    }

    #[test]
    fn test_default_is_normal() {
        assert_eq!(PTHREAD_MUTEX_DEFAULT, PTHREAD_MUTEX_NORMAL);
    }

    #[test]
    fn test_protocols_distinct() {
        let protos = [PTHREAD_PRIO_NONE, PTHREAD_PRIO_INHERIT, PTHREAD_PRIO_PROTECT];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_robustness_distinct() {
        assert_ne!(PTHREAD_MUTEX_STALLED, PTHREAD_MUTEX_ROBUST);
    }

    #[test]
    fn test_process_shared_distinct() {
        assert_ne!(PTHREAD_PROCESS_PRIVATE, PTHREAD_PROCESS_SHARED);
    }

    #[test]
    fn test_mutex_t_size() {
        assert_eq!(PTHREAD_MUTEX_T_SIZE, 40);
    }

    #[test]
    fn test_mutex_t_align() {
        assert!(PTHREAD_MUTEX_T_ALIGN.is_power_of_two());
    }
}

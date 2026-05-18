//! `<pthread.h>` — Pthread thread-specific data (TSD) constants.
//!
//! Thread-specific data allows each thread to have its own
//! copy of a variable, keyed by a `pthread_key_t`.  These
//! constants define the key limits and internal layout.

// ---------------------------------------------------------------------------
// Key limits
// ---------------------------------------------------------------------------

/// Maximum number of thread-specific data keys.
pub const PTHREAD_KEYS_MAX: u32 = 1024;
/// Maximum number of destructor iterations at thread exit.
pub const PTHREAD_DESTRUCTOR_ITERATIONS: u32 = 4;

// ---------------------------------------------------------------------------
// Thread creation attributes
// ---------------------------------------------------------------------------

/// Create thread in joinable state (default).
pub const PTHREAD_CREATE_JOINABLE: u32 = 0;
/// Create thread in detached state.
pub const PTHREAD_CREATE_DETACHED: u32 = 1;

// ---------------------------------------------------------------------------
// Thread scheduling scope
// ---------------------------------------------------------------------------

/// System-wide scheduling scope.
pub const PTHREAD_SCOPE_SYSTEM: u32 = 0;
/// Process-local scheduling scope (not supported on Linux).
pub const PTHREAD_SCOPE_PROCESS: u32 = 1;

// ---------------------------------------------------------------------------
// Thread scheduling inheritance
// ---------------------------------------------------------------------------

/// Inherit scheduling attributes from creating thread.
pub const PTHREAD_INHERIT_SCHED: u32 = 0;
/// Use explicit scheduling attributes.
pub const PTHREAD_EXPLICIT_SCHED: u32 = 1;

// ---------------------------------------------------------------------------
// Thread cancellation
// ---------------------------------------------------------------------------

/// Cancellation is enabled (default).
pub const PTHREAD_CANCEL_ENABLE: u32 = 0;
/// Cancellation is disabled.
pub const PTHREAD_CANCEL_DISABLE: u32 = 1;
/// Deferred cancellation (at cancellation points, default).
pub const PTHREAD_CANCEL_DEFERRED: u32 = 0;
/// Asynchronous cancellation (immediate).
pub const PTHREAD_CANCEL_ASYNCHRONOUS: u32 = 1;
/// Return value indicating thread was cancelled.
pub const PTHREAD_CANCELED: usize = usize::MAX; // (void*)-1

// ---------------------------------------------------------------------------
// Thread stack limits
// ---------------------------------------------------------------------------

/// Minimum thread stack size (bytes, PTHREAD_STACK_MIN on Linux).
pub const PTHREAD_STACK_MIN: u32 = 16384;
/// Default thread stack size (bytes, glibc default).
pub const PTHREAD_STACK_DEFAULT: u32 = 8388608; // 8 MiB
/// Default thread guard page size (bytes).
pub const PTHREAD_GUARD_DEFAULT: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keys_max() {
        assert_eq!(PTHREAD_KEYS_MAX, 1024);
    }

    #[test]
    fn test_destructor_iterations() {
        assert_eq!(PTHREAD_DESTRUCTOR_ITERATIONS, 4);
    }

    #[test]
    fn test_create_states_distinct() {
        assert_ne!(PTHREAD_CREATE_JOINABLE, PTHREAD_CREATE_DETACHED);
    }

    #[test]
    fn test_joinable_is_zero() {
        assert_eq!(PTHREAD_CREATE_JOINABLE, 0);
    }

    #[test]
    fn test_scope_distinct() {
        assert_ne!(PTHREAD_SCOPE_SYSTEM, PTHREAD_SCOPE_PROCESS);
    }

    #[test]
    fn test_sched_inherit_distinct() {
        assert_ne!(PTHREAD_INHERIT_SCHED, PTHREAD_EXPLICIT_SCHED);
    }

    #[test]
    fn test_cancel_enable_distinct() {
        assert_ne!(PTHREAD_CANCEL_ENABLE, PTHREAD_CANCEL_DISABLE);
    }

    #[test]
    fn test_cancel_type_distinct() {
        assert_ne!(PTHREAD_CANCEL_DEFERRED, PTHREAD_CANCEL_ASYNCHRONOUS);
    }

    #[test]
    fn test_canceled_is_max() {
        assert_eq!(PTHREAD_CANCELED, usize::MAX);
    }

    #[test]
    fn test_stack_min() {
        assert_eq!(PTHREAD_STACK_MIN, 16384);
    }

    #[test]
    fn test_stack_default_gt_min() {
        assert!(PTHREAD_STACK_DEFAULT > PTHREAD_STACK_MIN);
    }

    #[test]
    fn test_guard_default() {
        assert_eq!(PTHREAD_GUARD_DEFAULT, 4096);
    }
}

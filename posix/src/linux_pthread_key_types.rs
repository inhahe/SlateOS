//! `<pthread.h>` — Pthread thread-specific data (TSD) constants.
//!
//! Thread-specific data allows each thread to have its own
//! copy of a variable, keyed by a `pthread_key_t`.  These
//! constants define the key limits and internal layout.

// ---------------------------------------------------------------------------
// Key limits
// ---------------------------------------------------------------------------

/// Maximum number of thread-specific data keys: musl's 128, which is what
/// `pthread_key_create` allows (`pthread.rs`'s `KEYS_MAX`) and what the
/// header a C program compiles against says.  It was glibc's 1024 until
/// 2026-09-27, promising eight times the keys the library hands out.
pub const PTHREAD_KEYS_MAX: u32 = 128;
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

/// Minimum thread stack size in bytes: musl's 2048.  A C program's
/// `<limits.h>` says 2048, so `pthread_attr_setstacksize(&a,
/// PTHREAD_STACK_MIN + margin)` must succeed; it was glibc's 16384 until
/// 2026-09-27, which refused every such request below 16 KiB.  A stack is
/// still at least one 16 KiB page: `pthread_create` rounds the size up.
pub const PTHREAD_STACK_MIN: u32 = 2048;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// musl's `<limits.h>` (probed 2026-09-27).
    #[test]
    fn test_keys_max() {
        assert_eq!(PTHREAD_KEYS_MAX, 128);
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

    /// musl's `<limits.h>` (probed 2026-09-27).
    #[test]
    fn test_stack_min() {
        assert_eq!(PTHREAD_STACK_MIN, 2048);
    }
}

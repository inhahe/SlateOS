//! `<linux/futex.h>` — fast userspace locking primitives.
//!
//! Provides futex operation constants and the `futex()` system call
//! wrapper.  Futexes are the building block for userspace
//! synchronization primitives (mutexes, condition variables, etc.).

use crate::errno;
use crate::types::TimeT;

// ---------------------------------------------------------------------------
// Futex operations
// ---------------------------------------------------------------------------

/// Wait if `*uaddr == val`.
pub const FUTEX_WAIT: i32 = 0;

/// Wake up to `val` waiters on `uaddr`.
pub const FUTEX_WAKE: i32 = 1;

/// Requeue waiters from `uaddr` to `uaddr2`.
pub const FUTEX_REQUEUE: i32 = 3;

/// Conditional requeue (atomically check before requeuing).
pub const FUTEX_CMP_REQUEUE: i32 = 4;

/// Wake one waiter and set lock value atomically.
pub const FUTEX_WAKE_OP: i32 = 5;

/// Wait on a bitset.
pub const FUTEX_WAIT_BITSET: i32 = 9;

/// Wake on a bitset.
pub const FUTEX_WAKE_BITSET: i32 = 10;

/// Lock a PI futex (priority-inheritance).
pub const FUTEX_LOCK_PI: i32 = 6;

/// Unlock a PI futex.
pub const FUTEX_UNLOCK_PI: i32 = 7;

/// Try lock a PI futex.
pub const FUTEX_TRYLOCK_PI: i32 = 8;

/// Wait on a PI futex with requeue.
pub const FUTEX_WAIT_REQUEUE_PI: i32 = 11;

/// Requeue PI waiters.
pub const FUTEX_CMP_REQUEUE_PI: i32 = 12;

// ---------------------------------------------------------------------------
// Futex flags (OR with operation)
// ---------------------------------------------------------------------------

/// Use `CLOCK_REALTIME` instead of `CLOCK_MONOTONIC` for timeouts.
pub const FUTEX_CLOCK_REALTIME: i32 = 256;

/// Use private futex (process-local, not shared).
pub const FUTEX_PRIVATE_FLAG: i32 = 128;

// ---------------------------------------------------------------------------
// Convenience combined values
// ---------------------------------------------------------------------------

/// Private wait.
pub const FUTEX_WAIT_PRIVATE: i32 = FUTEX_WAIT | FUTEX_PRIVATE_FLAG;

/// Private wake.
pub const FUTEX_WAKE_PRIVATE: i32 = FUTEX_WAKE | FUTEX_PRIVATE_FLAG;

/// Wait on all bits.
pub const FUTEX_BITSET_MATCH_ANY: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// futex()
// ---------------------------------------------------------------------------

/// Futex system call.
///
/// Stub — returns -1 with `ENOSYS`.  On the real kernel this would
/// be a direct syscall.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn futex(
    _uaddr: *mut u32,
    _futex_op: i32,
    _val: u32,
    _timeout: *const u8, // struct timespec *
    _uaddr2: *mut u32,
    _val3: u32,
) -> i64 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_futex_ops_distinct() {
        let ops = [
            FUTEX_WAIT, FUTEX_WAKE, FUTEX_REQUEUE,
            FUTEX_CMP_REQUEUE, FUTEX_WAKE_OP,
            FUTEX_LOCK_PI, FUTEX_UNLOCK_PI, FUTEX_TRYLOCK_PI,
            FUTEX_WAIT_BITSET, FUTEX_WAKE_BITSET,
            FUTEX_WAIT_REQUEUE_PI, FUTEX_CMP_REQUEUE_PI,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j], "futex ops must be distinct");
            }
        }
    }

    #[test]
    fn test_futex_wait_wake_values() {
        assert_eq!(FUTEX_WAIT, 0);
        assert_eq!(FUTEX_WAKE, 1);
    }

    #[test]
    fn test_futex_private_flag() {
        assert_eq!(FUTEX_PRIVATE_FLAG, 128);
        assert_eq!(FUTEX_WAIT_PRIVATE, 128);
        assert_eq!(FUTEX_WAKE_PRIVATE, 129);
    }

    #[test]
    fn test_futex_clock_realtime() {
        assert_eq!(FUTEX_CLOCK_REALTIME, 256);
    }

    #[test]
    fn test_futex_bitset_match_any() {
        assert_eq!(FUTEX_BITSET_MATCH_ANY, 0xFFFF_FFFF);
    }

    #[test]
    fn test_futex_stub() {
        let ret = futex(
            core::ptr::null_mut(),
            FUTEX_WAIT,
            0,
            core::ptr::null(),
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_futex_pi_ops_distinct() {
        assert_ne!(FUTEX_LOCK_PI, FUTEX_UNLOCK_PI);
        assert_ne!(FUTEX_UNLOCK_PI, FUTEX_TRYLOCK_PI);
    }
}

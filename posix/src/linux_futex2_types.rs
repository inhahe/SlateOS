//! `<linux/futex.h>` (extended) — Futex operation constants.
//!
//! Futexes (Fast Userspace muTEXes) are the building block for all
//! userspace synchronization: mutexes, condition variables, rwlocks,
//! barriers, and semaphores. The uncontended fast path is pure
//! userspace (atomic CAS, no syscall). The kernel is only involved
//! when a thread must block (FUTEX_WAIT) or wake waiters (FUTEX_WAKE).
//! This module covers the full set of futex operations including
//! priority-inheritance and the newer futex2 interface.

// ---------------------------------------------------------------------------
// Futex operations
// ---------------------------------------------------------------------------

/// Wait if *uaddr == val (block until woken).
pub const FUTEX_WAIT: u32 = 0;
/// Wake up to val waiters on uaddr.
pub const FUTEX_WAKE: u32 = 1;
/// Wake val waiters on uaddr, requeue rest on uaddr2.
pub const FUTEX_REQUEUE: u32 = 3;
/// CMP + requeue (atomic compare-and-requeue).
pub const FUTEX_CMP_REQUEUE: u32 = 4;
/// Wake one waiter and set new value atomically.
pub const FUTEX_WAKE_OP: u32 = 5;
/// Wait with PI (priority inheritance) mutex semantics.
pub const FUTEX_LOCK_PI: u32 = 6;
/// Unlock PI mutex and wake top waiter.
pub const FUTEX_UNLOCK_PI: u32 = 7;
/// Try to lock PI mutex (non-blocking).
pub const FUTEX_TRYLOCK_PI: u32 = 8;
/// Wait with bitset (selective wake).
pub const FUTEX_WAIT_BITSET: u32 = 9;
/// Wake waiters matching bitset.
pub const FUTEX_WAKE_BITSET: u32 = 10;
/// Wait for PI requeue.
pub const FUTEX_WAIT_REQUEUE_PI: u32 = 11;
/// CMP + requeue with PI.
pub const FUTEX_CMP_REQUEUE_PI: u32 = 12;
/// Lock PI mutex with timeout.
pub const FUTEX_LOCK_PI2: u32 = 13;

// ---------------------------------------------------------------------------
// Futex flags (OR'd with operation)
// ---------------------------------------------------------------------------

/// Use per-process private futex (not shared between processes).
pub const FUTEX_PRIVATE_FLAG: u32 = 0x80;
/// Use CLOCK_REALTIME for timeout (default is CLOCK_MONOTONIC).
pub const FUTEX_CLOCK_REALTIME: u32 = 0x100;

// ---------------------------------------------------------------------------
// Futex bitset
// ---------------------------------------------------------------------------

/// Match all waiters (equivalent to no bitset filtering).
pub const FUTEX_BITSET_MATCH_ANY: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Futex wake_op operations (for FUTEX_WAKE_OP)
// ---------------------------------------------------------------------------

/// Set: *uaddr2 = oparg.
pub const FUTEX_OP_SET: u32 = 0;
/// Add: *uaddr2 += oparg.
pub const FUTEX_OP_ADD: u32 = 1;
/// Or: *uaddr2 |= oparg.
pub const FUTEX_OP_OR: u32 = 2;
/// Andn: *uaddr2 &= ~oparg.
pub const FUTEX_OP_ANDN: u32 = 3;
/// Xor: *uaddr2 ^= oparg.
pub const FUTEX_OP_XOR: u32 = 4;

// ---------------------------------------------------------------------------
// Futex wake_op comparison operations
// ---------------------------------------------------------------------------

/// Compare equal.
pub const FUTEX_OP_CMP_EQ: u32 = 0;
/// Compare not equal.
pub const FUTEX_OP_CMP_NE: u32 = 1;
/// Compare less than.
pub const FUTEX_OP_CMP_LT: u32 = 2;
/// Compare less or equal.
pub const FUTEX_OP_CMP_LE: u32 = 3;
/// Compare greater than.
pub const FUTEX_OP_CMP_GT: u32 = 4;
/// Compare greater or equal.
pub const FUTEX_OP_CMP_GE: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operations_distinct() {
        let ops = [
            FUTEX_WAIT, FUTEX_WAKE, FUTEX_REQUEUE,
            FUTEX_CMP_REQUEUE, FUTEX_WAKE_OP, FUTEX_LOCK_PI,
            FUTEX_UNLOCK_PI, FUTEX_TRYLOCK_PI, FUTEX_WAIT_BITSET,
            FUTEX_WAKE_BITSET, FUTEX_WAIT_REQUEUE_PI,
            FUTEX_CMP_REQUEUE_PI, FUTEX_LOCK_PI2,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(FUTEX_PRIVATE_FLAG & FUTEX_CLOCK_REALTIME, 0);
        assert!(FUTEX_PRIVATE_FLAG.is_power_of_two());
        assert!(FUTEX_CLOCK_REALTIME.is_power_of_two());
    }

    #[test]
    fn test_wake_ops_distinct() {
        let ops = [
            FUTEX_OP_SET, FUTEX_OP_ADD, FUTEX_OP_OR,
            FUTEX_OP_ANDN, FUTEX_OP_XOR,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_cmp_ops_distinct() {
        let cmps = [
            FUTEX_OP_CMP_EQ, FUTEX_OP_CMP_NE, FUTEX_OP_CMP_LT,
            FUTEX_OP_CMP_LE, FUTEX_OP_CMP_GT, FUTEX_OP_CMP_GE,
        ];
        for i in 0..cmps.len() {
            for j in (i + 1)..cmps.len() {
                assert_ne!(cmps[i], cmps[j]);
            }
        }
    }

    #[test]
    fn test_bitset_match_any() {
        assert_eq!(FUTEX_BITSET_MATCH_ANY, u32::MAX);
    }
}

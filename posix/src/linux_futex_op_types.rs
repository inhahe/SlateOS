//! `<linux/futex.h>` — Futex operation and flag constants.
//!
//! Futexes (Fast Userspace muTEXes) are the building block for
//! userspace synchronization primitives. The kernel provides
//! wait/wake operations; the fast path is pure userspace atomics.

// ---------------------------------------------------------------------------
// Futex operations
// ---------------------------------------------------------------------------

/// Wait if *uaddr == val.
pub const FUTEX_WAIT: u32 = 0;
/// Wake up to val waiters.
pub const FUTEX_WAKE: u32 = 1;
/// Requeue waiters from uaddr to uaddr2.
pub const FUTEX_REQUEUE: u32 = 3;
/// Conditional requeue (compare first).
pub const FUTEX_CMP_REQUEUE: u32 = 4;
/// Wake one waiter + set val at uaddr.
pub const FUTEX_WAKE_OP: u32 = 5;
/// Wait on a bitset.
pub const FUTEX_WAIT_BITSET: u32 = 9;
/// Wake waiters matching bitset.
pub const FUTEX_WAKE_BITSET: u32 = 10;
/// Lock PI futex.
pub const FUTEX_LOCK_PI: u32 = 6;
/// Unlock PI futex.
pub const FUTEX_UNLOCK_PI: u32 = 7;
/// Try to lock PI futex.
pub const FUTEX_TRYLOCK_PI: u32 = 8;
/// Wait on PI futex with requeue.
pub const FUTEX_WAIT_REQUEUE_PI: u32 = 11;
/// Requeue PI futex.
pub const FUTEX_CMP_REQUEUE_PI: u32 = 12;
/// Lock PI futex (v2 with flags).
pub const FUTEX_LOCK_PI2: u32 = 13;

// ---------------------------------------------------------------------------
// Futex flags (OR'd with operation)
// ---------------------------------------------------------------------------

/// Use CLOCK_REALTIME for timeout.
pub const FUTEX_CLOCK_REALTIME: u32 = 256;
/// Private futex (not shared across processes).
pub const FUTEX_PRIVATE_FLAG: u32 = 128;

// ---------------------------------------------------------------------------
// Futex bitset
// ---------------------------------------------------------------------------

/// Match all bitsets (wake/wait all).
pub const FUTEX_BITSET_MATCH_ANY: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// FUTEX_WAKE_OP operations (encoded in val3)
// ---------------------------------------------------------------------------

/// Set: *uaddr2 = oparg.
pub const FUTEX_OP_SET: u32 = 0;
/// Add: *uaddr2 += oparg.
pub const FUTEX_OP_ADD: u32 = 1;
/// Or: *uaddr2 |= oparg.
pub const FUTEX_OP_OR: u32 = 2;
/// And-not: *uaddr2 &= ~oparg.
pub const FUTEX_OP_ANDN: u32 = 3;
/// Xor: *uaddr2 ^= oparg.
pub const FUTEX_OP_XOR: u32 = 4;

// ---------------------------------------------------------------------------
// FUTEX_WAKE_OP comparison operators
// ---------------------------------------------------------------------------

/// Equal.
pub const FUTEX_OP_CMP_EQ: u32 = 0;
/// Not equal.
pub const FUTEX_OP_CMP_NE: u32 = 1;
/// Less than.
pub const FUTEX_OP_CMP_LT: u32 = 2;
/// Less than or equal.
pub const FUTEX_OP_CMP_LE: u32 = 3;
/// Greater than.
pub const FUTEX_OP_CMP_GT: u32 = 4;
/// Greater than or equal.
pub const FUTEX_OP_CMP_GE: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_distinct() {
        let ops = [
            FUTEX_WAIT, FUTEX_WAKE, FUTEX_REQUEUE,
            FUTEX_CMP_REQUEUE, FUTEX_WAKE_OP,
            FUTEX_LOCK_PI, FUTEX_UNLOCK_PI, FUTEX_TRYLOCK_PI,
            FUTEX_WAIT_BITSET, FUTEX_WAKE_BITSET,
            FUTEX_WAIT_REQUEUE_PI, FUTEX_CMP_REQUEUE_PI,
            FUTEX_LOCK_PI2,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_wait_is_zero() {
        assert_eq!(FUTEX_WAIT, 0);
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(FUTEX_CLOCK_REALTIME & FUTEX_PRIVATE_FLAG, 0);
    }

    #[test]
    fn test_bitset_match_any() {
        assert_eq!(FUTEX_BITSET_MATCH_ANY, u32::MAX);
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
            FUTEX_OP_CMP_EQ, FUTEX_OP_CMP_NE,
            FUTEX_OP_CMP_LT, FUTEX_OP_CMP_LE,
            FUTEX_OP_CMP_GT, FUTEX_OP_CMP_GE,
        ];
        for i in 0..cmps.len() {
            for j in (i + 1)..cmps.len() {
                assert_ne!(cmps[i], cmps[j]);
            }
        }
    }
}

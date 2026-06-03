//! `<linux/futex.h>` — fast userspace mutex primitives.
//!
//! Every glibc/pthread mutex, std::sync::Mutex (Rust), and tokio
//! `Notify` lowers into `futex()` system calls. The operation codes
//! below split the syscall by behavior; flags toggle private vs
//! shared and absolute vs relative timeouts.

// ---------------------------------------------------------------------------
// Operation codes (futex_op argument)
// ---------------------------------------------------------------------------

/// `FUTEX_WAIT`.
pub const FUTEX_WAIT: u32 = 0;
/// `FUTEX_WAKE`.
pub const FUTEX_WAKE: u32 = 1;
/// `FUTEX_FD` (deprecated).
pub const FUTEX_FD: u32 = 2;
/// `FUTEX_REQUEUE`.
pub const FUTEX_REQUEUE: u32 = 3;
/// `FUTEX_CMP_REQUEUE`.
pub const FUTEX_CMP_REQUEUE: u32 = 4;
/// `FUTEX_WAKE_OP` — wake one futex and run an op on another.
pub const FUTEX_WAKE_OP: u32 = 5;
/// `FUTEX_LOCK_PI` — priority-inheriting lock.
pub const FUTEX_LOCK_PI: u32 = 6;
/// `FUTEX_UNLOCK_PI`.
pub const FUTEX_UNLOCK_PI: u32 = 7;
/// `FUTEX_TRYLOCK_PI`.
pub const FUTEX_TRYLOCK_PI: u32 = 8;
/// `FUTEX_WAIT_BITSET` — wait with a per-wake bitmask.
pub const FUTEX_WAIT_BITSET: u32 = 9;
/// `FUTEX_WAKE_BITSET`.
pub const FUTEX_WAKE_BITSET: u32 = 10;
/// `FUTEX_WAIT_REQUEUE_PI`.
pub const FUTEX_WAIT_REQUEUE_PI: u32 = 11;
/// `FUTEX_CMP_REQUEUE_PI`.
pub const FUTEX_CMP_REQUEUE_PI: u32 = 12;
/// `FUTEX_LOCK_PI2` — PI lock with new clock semantics.
pub const FUTEX_LOCK_PI2: u32 = 13;

// ---------------------------------------------------------------------------
// Operation flags (OR'd into futex_op)
// ---------------------------------------------------------------------------

/// Operation is private to the calling process.
pub const FUTEX_PRIVATE_FLAG: u32 = 128;
/// Operation uses CLOCK_REALTIME (default is CLOCK_MONOTONIC).
pub const FUTEX_CLOCK_REALTIME: u32 = 256;
/// Mask covering the actual op number (low 7 bits).
pub const FUTEX_CMD_MASK: u32 = !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);

// ---------------------------------------------------------------------------
// FUTEX_WAKE_OP encoded "op" field constants
// ---------------------------------------------------------------------------

/// uaddr2 := oparg.
pub const FUTEX_OP_SET: u32 = 0;
/// uaddr2 += oparg.
pub const FUTEX_OP_ADD: u32 = 1;
/// uaddr2 |= oparg.
pub const FUTEX_OP_OR: u32 = 2;
/// uaddr2 &= ~oparg.
pub const FUTEX_OP_ANDN: u32 = 3;
/// uaddr2 ^= oparg.
pub const FUTEX_OP_XOR: u32 = 4;
/// Shift oparg before applying.
pub const FUTEX_OP_OPARG_SHIFT: u32 = 8;

// ---------------------------------------------------------------------------
// Comparison operations
// ---------------------------------------------------------------------------

/// ==.
pub const FUTEX_OP_CMP_EQ: u32 = 0;
/// !=.
pub const FUTEX_OP_CMP_NE: u32 = 1;
/// <.
pub const FUTEX_OP_CMP_LT: u32 = 2;
/// <=.
pub const FUTEX_OP_CMP_LE: u32 = 3;
/// >.
pub const FUTEX_OP_CMP_GT: u32 = 4;
/// >=.
pub const FUTEX_OP_CMP_GE: u32 = 5;

// ---------------------------------------------------------------------------
// PI-futex value layout
// ---------------------------------------------------------------------------

/// Value indicating waiters present (PI futex).
pub const FUTEX_WAITERS: u32 = 0x8000_0000;
/// Owner died bit (PI futex).
pub const FUTEX_OWNER_DIED: u32 = 0x4000_0000;
/// TID mask within PI futex value.
pub const FUTEX_TID_MASK: u32 = 0x3FFF_FFFF;

// ---------------------------------------------------------------------------
// Convenience: wait on any bit
// ---------------------------------------------------------------------------

/// `FUTEX_BITSET_MATCH_ANY` — match any bit (default bitmask).
pub const FUTEX_BITSET_MATCH_ANY: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_codes_dense_through_13() {
        let o = [
            FUTEX_WAIT,
            FUTEX_WAKE,
            FUTEX_FD,
            FUTEX_REQUEUE,
            FUTEX_CMP_REQUEUE,
            FUTEX_WAKE_OP,
            FUTEX_LOCK_PI,
            FUTEX_UNLOCK_PI,
            FUTEX_TRYLOCK_PI,
            FUTEX_WAIT_BITSET,
            FUTEX_WAKE_BITSET,
            FUTEX_WAIT_REQUEUE_PI,
            FUTEX_CMP_REQUEUE_PI,
            FUTEX_LOCK_PI2,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_flags_and_mask() {
        // The flag bits live above the op field (bit 7+).
        assert_eq!(FUTEX_PRIVATE_FLAG, 0x80);
        assert_eq!(FUTEX_CLOCK_REALTIME, 0x100);
        // CMD_MASK must mask out the two flag bits and leave op intact.
        assert_eq!(FUTEX_CMD_MASK & FUTEX_PRIVATE_FLAG, 0);
        assert_eq!(FUTEX_CMD_MASK & FUTEX_CLOCK_REALTIME, 0);
        for op in 0..=FUTEX_LOCK_PI2 {
            assert_eq!(op & FUTEX_CMD_MASK, op);
        }
    }

    #[test]
    fn test_wake_op_codes_dense() {
        let a = [
            FUTEX_OP_SET,
            FUTEX_OP_ADD,
            FUTEX_OP_OR,
            FUTEX_OP_ANDN,
            FUTEX_OP_XOR,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // Shift flag must be distinct.
        assert_ne!(FUTEX_OP_OPARG_SHIFT, FUTEX_OP_XOR);
    }

    #[test]
    fn test_compare_op_codes_dense() {
        let c = [
            FUTEX_OP_CMP_EQ,
            FUTEX_OP_CMP_NE,
            FUTEX_OP_CMP_LT,
            FUTEX_OP_CMP_LE,
            FUTEX_OP_CMP_GT,
            FUTEX_OP_CMP_GE,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_pi_value_bits_distinct() {
        // Waiters bit / owner-died bit / TID mask must not overlap.
        assert_eq!(FUTEX_WAITERS & FUTEX_OWNER_DIED, 0);
        assert_eq!(FUTEX_WAITERS & FUTEX_TID_MASK, 0);
        assert_eq!(FUTEX_OWNER_DIED & FUTEX_TID_MASK, 0);
        // The three together must cover all 32 bits.
        assert_eq!(
            FUTEX_WAITERS | FUTEX_OWNER_DIED | FUTEX_TID_MASK,
            u32::MAX
        );
    }

    #[test]
    fn test_bitset_match_any() {
        // The "match any" bitmask must be all-ones, not zero.
        assert_eq!(FUTEX_BITSET_MATCH_ANY, !0u32);
    }
}

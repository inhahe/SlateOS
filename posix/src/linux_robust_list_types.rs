//! `<linux/futex.h>` (robust list subset) — Robust futex list constants.
//!
//! Robust futexes solve the "dead lock holder" problem: if a thread
//! dies while holding a futex-based mutex, other waiters would be
//! stuck forever. Each thread maintains a robust list of futexes it
//! holds. On thread death, the kernel walks this list and marks each
//! futex with FUTEX_OWNER_DIED, waking one waiter who can then
//! recover the mutex (possibly with inconsistent protected state).

// ---------------------------------------------------------------------------
// Robust futex special values
// ---------------------------------------------------------------------------

/// Owner died bit (set in futex word when holder exits without unlock).
pub const FUTEX_OWNER_DIED: u32 = 0x4000_0000;
/// TID mask (lower 30 bits hold the owning thread's TID).
pub const FUTEX_TID_MASK: u32 = 0x3FFF_FFFF;
/// Waiters bit (set when there are threads waiting on the futex).
pub const FUTEX_WAITERS: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Robust list operations
// ---------------------------------------------------------------------------

/// Register robust list with kernel (set_robust_list).
pub const ROBUST_LIST_SET: u32 = 0;
/// Retrieve robust list (get_robust_list).
pub const ROBUST_LIST_GET: u32 = 1;

// ---------------------------------------------------------------------------
// Robust list limits
// ---------------------------------------------------------------------------

/// Maximum entries the kernel will walk on thread death.
pub const ROBUST_LIST_LIMIT: u32 = 2048;
/// Size of robust_list_head structure (for set_robust_list len parameter).
pub const ROBUST_LIST_HEAD_SIZE: u32 = 24; // 3 pointers on 64-bit

// ---------------------------------------------------------------------------
// Futex PI recovery states
// ---------------------------------------------------------------------------

/// PI mutex is clean (no recovery needed).
pub const FUTEX_PI_CLEAN: u32 = 0;
/// PI mutex needs recovery (owner died).
pub const FUTEX_PI_OWNER_DIED: u32 = 1;
/// PI mutex is being recovered by new owner.
pub const FUTEX_PI_RECOVERING: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_special_bits_no_overlap() {
        // OWNER_DIED, TID_MASK, and WAITERS partition the 32-bit word
        assert_eq!(FUTEX_OWNER_DIED & FUTEX_TID_MASK, 0);
        assert_eq!(FUTEX_WAITERS & FUTEX_TID_MASK, 0);
        assert_eq!(FUTEX_WAITERS & FUTEX_OWNER_DIED, 0);
        // Together they cover all 32 bits
        assert_eq!(FUTEX_WAITERS | FUTEX_OWNER_DIED | FUTEX_TID_MASK, 0xFFFF_FFFF);
    }

    #[test]
    fn test_tid_mask_width() {
        // TID mask should be 30 bits
        assert_eq!(FUTEX_TID_MASK.count_ones(), 30);
    }

    #[test]
    fn test_robust_list_ops_distinct() {
        assert_ne!(ROBUST_LIST_SET, ROBUST_LIST_GET);
    }

    #[test]
    fn test_pi_recovery_states_distinct() {
        let states = [FUTEX_PI_CLEAN, FUTEX_PI_OWNER_DIED, FUTEX_PI_RECOVERING];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_limits() {
        assert!(ROBUST_LIST_LIMIT > 0);
        assert!(ROBUST_LIST_HEAD_SIZE > 0);
    }
}

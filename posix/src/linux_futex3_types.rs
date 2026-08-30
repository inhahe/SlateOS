//! `<linux/futex.h>` — Additional futex constants (batch 3).
//!
//! Supplementary futex constants covering futex2 operations,
//! futex size encodings, and NUMA-aware futex flags.

// ---------------------------------------------------------------------------
// futex2 operations (FUTEX2_*)
// ---------------------------------------------------------------------------

/// futex2: wait operation.
pub const FUTEX2_WAIT: u32 = 0;
/// futex2: wake operation.
pub const FUTEX2_WAKE: u32 = 1;
/// futex2: requeue operation.
pub const FUTEX2_REQUEUE: u32 = 2;

// ---------------------------------------------------------------------------
// futex2 size flags
// ---------------------------------------------------------------------------

/// 8-bit futex.
pub const FUTEX2_SIZE_U8: u32 = 0x00;
/// 16-bit futex.
pub const FUTEX2_SIZE_U16: u32 = 0x01;
/// 32-bit futex.
pub const FUTEX2_SIZE_U32: u32 = 0x02;
/// 64-bit futex.
pub const FUTEX2_SIZE_U64: u32 = 0x03;
/// Size mask.
pub const FUTEX2_SIZE_MASK: u32 = 0x03;

// ---------------------------------------------------------------------------
// futex2 flags
// ---------------------------------------------------------------------------

/// Private futex (not shared between processes).
pub const FUTEX2_PRIVATE: u32 = 1 << 7;
/// NUMA-aware futex.
pub const FUTEX2_NUMA: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Robust futex list
// ---------------------------------------------------------------------------

/// Robust list: entry is pending.
pub const FUTEX_WAITERS_RL: u32 = 0x40000000;
/// Robust list: entry was owned by exited thread.
pub const FUTEX_OWNER_DIED_RL: u32 = 0x40000000;
/// Robust list: TID mask.
pub const FUTEX_TID_MASK_RL: u32 = 0x3FFFFFFF;
/// Robust list: magic for PI futex.
pub const FUTEX_PI_FLAG: u32 = 0x80000000;

/// Robust list head size (64-bit).
pub const ROBUST_LIST_HEAD_SIZE: u32 = 24;
/// Robust list entry size (64-bit).
pub const ROBUST_LIST_ENTRY_SIZE: u32 = 24;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_futex2_ops_distinct() {
        let ops = [FUTEX2_WAIT, FUTEX2_WAKE, FUTEX2_REQUEUE];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_futex2_sizes_distinct() {
        let sizes = [
            FUTEX2_SIZE_U8,
            FUTEX2_SIZE_U16,
            FUTEX2_SIZE_U32,
            FUTEX2_SIZE_U64,
        ];
        for i in 0..sizes.len() {
            for j in (i + 1)..sizes.len() {
                assert_ne!(sizes[i], sizes[j]);
            }
        }
    }

    #[test]
    fn test_size_mask() {
        assert_eq!(FUTEX2_SIZE_U8 & FUTEX2_SIZE_MASK, FUTEX2_SIZE_U8);
        assert_eq!(FUTEX2_SIZE_U64 & FUTEX2_SIZE_MASK, FUTEX2_SIZE_U64);
    }

    #[test]
    fn test_futex2_flags_no_overlap() {
        assert_eq!(FUTEX2_PRIVATE & FUTEX2_NUMA, 0);
    }

    #[test]
    fn test_tid_mask_and_flags() {
        // TID mask + PI flag should cover all 32 bits with waiters
        assert_eq!(FUTEX_TID_MASK_RL | FUTEX_WAITERS_RL, 0x7FFFFFFF);
    }

    #[test]
    fn test_pi_flag_is_top_bit() {
        assert_eq!(FUTEX_PI_FLAG, 1 << 31);
    }

    #[test]
    fn test_robust_list_sizes() {
        assert_eq!(ROBUST_LIST_HEAD_SIZE, 24);
        assert_eq!(ROBUST_LIST_ENTRY_SIZE, 24);
    }
}

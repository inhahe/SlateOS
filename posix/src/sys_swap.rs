//! `<sys/swap.h>` — swap management.
//!
//! Re-exports `swapon()` and `swapoff()` from the `unistd` module
//! and defines `SWAP_FLAG_*` constants.

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use crate::unistd::swapoff;
pub use crate::unistd::swapon;

// ---------------------------------------------------------------------------
// Swap flags
// ---------------------------------------------------------------------------

/// Prefer this swap area.
pub const SWAP_FLAG_PREFER: i32 = 0x8000;

/// Discard freed swap pages.
pub const SWAP_FLAG_DISCARD: i32 = 0x10000;

/// Discard pages once (on swapon).
pub const SWAP_FLAG_DISCARD_ONCE: i32 = 0x20000;

/// Discard pages on each swap-out.
pub const SWAP_FLAG_DISCARD_PAGES: i32 = 0x40000;

/// Mask for the priority field in the flags argument.
pub const SWAP_FLAG_PRIO_MASK: i32 = 0x7FFF;

/// Shift for the priority field.
pub const SWAP_FLAG_PRIO_SHIFT: i32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_flag_prefer() {
        assert_eq!(SWAP_FLAG_PREFER, 0x8000);
    }

    #[test]
    fn test_swap_flag_discard() {
        assert_eq!(SWAP_FLAG_DISCARD, 0x10000);
    }

    #[test]
    fn test_flags_distinct() {
        let flags = [
            SWAP_FLAG_PREFER,
            SWAP_FLAG_DISCARD,
            SWAP_FLAG_DISCARD_ONCE,
            SWAP_FLAG_DISCARD_PAGES,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_prio_mask() {
        assert_eq!(SWAP_FLAG_PRIO_MASK, 0x7FFF);
        // Priority is stored in the lower 15 bits.
        assert_eq!(SWAP_FLAG_PRIO_MASK & SWAP_FLAG_PREFER, 0);
    }

    #[test]
    fn test_swapon_stub() {
        let ret = swapon(b"/dev/sda2\0".as_ptr(), 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_swapoff_stub() {
        let ret = swapoff(b"/dev/sda2\0".as_ptr());
        assert_eq!(ret, -1);
    }
}

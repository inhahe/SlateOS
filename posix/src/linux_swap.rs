//! `<linux/swap.h>` — Swap management constants.
//!
//! Constants for `swapon()` and `swapoff()` syscalls and swap device
//! management flags.

// ---------------------------------------------------------------------------
// swapon flags
// ---------------------------------------------------------------------------

/// Use specified priority.
pub const SWAP_FLAG_PREFER: u32 = 0x8000;
/// Priority mask (lower 15 bits).
pub const SWAP_FLAG_PRIO_MASK: u32 = 0x7FFF;
/// Priority shift.
pub const SWAP_FLAG_PRIO_SHIFT: u32 = 0;
/// Discard freed swap pages.
pub const SWAP_FLAG_DISCARD: u32 = 0x10000;
/// Discard once (on swapon only).
pub const SWAP_FLAG_DISCARD_ONCE: u32 = 0x20000;
/// Discard on each swap page free.
pub const SWAP_FLAG_DISCARD_PAGES: u32 = 0x40000;

// ---------------------------------------------------------------------------
// Swap space limits
// ---------------------------------------------------------------------------

/// Maximum number of swap areas.
pub const MAX_SWAPFILES: u32 = 30;

// ---------------------------------------------------------------------------
// Swap magic
// ---------------------------------------------------------------------------

/// Swap space magic string (at end of first page).
pub const SWAP_MAGIC: &[u8] = b"SWAPSPACE2";

/// Magic offset in swap header (4086 for 4K pages, but varies).
pub const SWAP_MAGIC_OFFSET_4K: usize = 4086;

// ---------------------------------------------------------------------------
// Re-exports from unistd
// ---------------------------------------------------------------------------

pub use crate::unistd::swapoff;
pub use crate::unistd::swapon;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_flags() {
        assert_eq!(SWAP_FLAG_PREFER, 0x8000);
        assert_eq!(SWAP_FLAG_PRIO_MASK, 0x7FFF);
        assert_eq!(SWAP_FLAG_DISCARD, 0x10000);
    }

    #[test]
    fn test_discard_flags_distinct() {
        assert_ne!(SWAP_FLAG_DISCARD, SWAP_FLAG_DISCARD_ONCE);
        assert_ne!(SWAP_FLAG_DISCARD, SWAP_FLAG_DISCARD_PAGES);
        assert_ne!(SWAP_FLAG_DISCARD_ONCE, SWAP_FLAG_DISCARD_PAGES);
    }

    #[test]
    fn test_max_swapfiles() {
        assert_eq!(MAX_SWAPFILES, 30);
    }

    #[test]
    fn test_swap_magic() {
        assert_eq!(SWAP_MAGIC, b"SWAPSPACE2");
    }

    #[test]
    fn test_swapon_stub() {
        let ret = swapon(core::ptr::null(), 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_swapoff_stub() {
        let ret = swapoff(core::ptr::null());
        assert_eq!(ret, -1);
    }
}

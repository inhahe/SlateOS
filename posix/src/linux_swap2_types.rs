//! `<linux/swap.h>` — Swap subsystem constants (extended).
//!
//! Extended swap constants covering swapon flags, swap
//! priority parameters, and swap cluster sizes.

// ---------------------------------------------------------------------------
// swapon() flags
// ---------------------------------------------------------------------------

/// Prefer this swap area.
pub const SWAP_FLAG_PREFER: u32 = 0x8000;
/// Priority mask (lower 15 bits).
pub const SWAP_FLAG_PRIO_MASK: u32 = 0x7FFF;
/// Priority shift.
pub const SWAP_FLAG_PRIO_SHIFT: u32 = 0;
/// Discard (TRIM) freed swap pages.
pub const SWAP_FLAG_DISCARD: u32 = 0x10000;
/// Discard once at swapon.
pub const SWAP_FLAG_DISCARD_ONCE: u32 = 0x20000;
/// Discard individual freed pages.
pub const SWAP_FLAG_DISCARD_PAGES: u32 = 0x40000;

// ---------------------------------------------------------------------------
// Swap priority range
// ---------------------------------------------------------------------------

/// Minimum swap priority.
pub const SWAP_PRIO_MIN: i32 = -1;
/// Maximum swap priority.
pub const SWAP_PRIO_MAX: i32 = 32767;
/// Default swap priority (no explicit priority).
pub const SWAP_PRIO_DEFAULT: i32 = -1;

// ---------------------------------------------------------------------------
// Swap cluster sizes
// ---------------------------------------------------------------------------

/// Default swap cluster size (pages).
pub const SWAP_CLUSTER_DEFAULT: u32 = 32;
/// Maximum swap cluster size.
pub const SWAP_CLUSTER_MAX: u32 = 256;

// ---------------------------------------------------------------------------
// Swap magic strings (offsets in swap header)
// ---------------------------------------------------------------------------

/// Swap magic offset (last 10 bytes of first page).
pub const SWAP_MAGIC_OFFSET: u32 = 4086;
/// Swap UUID offset.
pub const SWAP_UUID_OFFSET: u32 = 1036;
/// Swap UUID length.
pub const SWAP_UUID_LEN: u32 = 16;
/// Swap label offset.
pub const SWAP_LABEL_OFFSET: u32 = 1052;
/// Swap label max length.
pub const SWAP_LABEL_LEN: u32 = 16;

// ---------------------------------------------------------------------------
// Maximum swap areas
// ---------------------------------------------------------------------------

/// Maximum number of swap areas.
pub const MAX_SWAPFILES: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_prefer_flag() {
        assert_eq!(SWAP_FLAG_PREFER, 0x8000);
    }

    #[test]
    fn test_prio_mask() {
        assert_eq!(SWAP_FLAG_PRIO_MASK, 0x7FFF);
    }

    #[test]
    fn test_prio_range() {
        assert!(SWAP_PRIO_MIN <= SWAP_PRIO_MAX);
    }

    #[test]
    fn test_cluster_sizes() {
        assert!(SWAP_CLUSTER_DEFAULT <= SWAP_CLUSTER_MAX);
    }

    #[test]
    fn test_uuid_len() {
        assert_eq!(SWAP_UUID_LEN, 16);
    }

    #[test]
    fn test_label_len() {
        assert_eq!(SWAP_LABEL_LEN, 16);
    }

    #[test]
    fn test_max_swapfiles() {
        assert_eq!(MAX_SWAPFILES, 32);
    }

    #[test]
    fn test_magic_offset() {
        assert_eq!(SWAP_MAGIC_OFFSET, 4086);
    }
}

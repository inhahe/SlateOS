//! `<linux/swap.h>` — `swapon(2)`/`swapoff(2)` userspace constants.
//!
//! mkswap, swapon, swapoff, and systemd's swap unit code use the
//! flag bits and on-disk magic below to enable a swap file or
//! partition and to validate its header.

// ---------------------------------------------------------------------------
// swapon(2) flags
// ---------------------------------------------------------------------------

/// Set a priority for the swap area (low 16 bits of the flag arg).
pub const SWAP_FLAG_PREFER: u32 = 1 << 15;
/// Mask of valid priority bits paired with `SWAP_FLAG_PREFER`.
pub const SWAP_FLAG_PRIO_MASK: u32 = 0x7fff;
/// Shift to encode a 0..32767 priority in the flag word.
pub const SWAP_FLAG_PRIO_SHIFT: u32 = 0;
/// Discard freed swap slots (TRIM-style).
pub const SWAP_FLAG_DISCARD: u32 = 1 << 16;
/// Issue a single discard at swapon time.
pub const SWAP_FLAG_DISCARD_ONCE: u32 = 1 << 17;
/// Issue discards per-page as the kernel frees them.
pub const SWAP_FLAG_DISCARD_PAGES: u32 = 1 << 18;

/// Mask of every userspace-settable swap flag.
pub const SWAP_FLAGS_VALID: u32 = SWAP_FLAG_PRIO_MASK
    | SWAP_FLAG_PREFER
    | SWAP_FLAG_DISCARD
    | SWAP_FLAG_DISCARD_ONCE
    | SWAP_FLAG_DISCARD_PAGES;

// ---------------------------------------------------------------------------
// On-disk header constants (struct swap_header)
// ---------------------------------------------------------------------------

/// Magic string at the tail of page 0 — `"SWAPSPACE2"`.
pub const SWAP_HEADER_MAGIC: &[u8; 10] = b"SWAPSPACE2";
/// Length of the magic string.
pub const SWAP_HEADER_MAGIC_LEN: u32 = 10;
/// Header layout version (always 1).
pub const SWAP_HEADER_VERSION: u32 = 1;
/// Length of the user-visible swap label.
pub const SWAP_HEADER_LABEL_LEN: u32 = 16;
/// Length of the UUID.
pub const SWAP_HEADER_UUID_LEN: u32 = 16;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of simultaneously enabled swap areas.
pub const MAX_SWAPFILES: u32 = 32;
/// Maximum swap-area priority value.
pub const SWAP_FLAG_PRIO_MAX: u32 = 32_767;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prio_mask_fits_in_15_bits() {
        // PRIO mask must be exactly the low 15 bits — PREFER is bit 15.
        assert_eq!(SWAP_FLAG_PRIO_MASK, 0x7fff);
        assert_eq!(SWAP_FLAG_PRIO_MASK & SWAP_FLAG_PREFER, 0);
        assert!(SWAP_FLAG_PREFER.is_power_of_two());
        assert_eq!(SWAP_FLAG_PRIO_MAX, SWAP_FLAG_PRIO_MASK);
    }

    #[test]
    fn test_discard_flags_distinct_pow2() {
        let d = [
            SWAP_FLAG_DISCARD,
            SWAP_FLAG_DISCARD_ONCE,
            SWAP_FLAG_DISCARD_PAGES,
        ];
        for &b in &d {
            assert!(b.is_power_of_two());
            // Discard flags must sit above the PRIO/PREFER bits.
            assert!(b > SWAP_FLAG_PREFER);
        }
        for i in 0..d.len() {
            for j in (i + 1)..d.len() {
                assert_ne!(d[i], d[j]);
            }
        }
    }

    #[test]
    fn test_valid_flag_mask_covers_all() {
        // Every defined flag bit must be in the VALID mask, otherwise
        // swapon(2) would reject legitimate inputs.
        let all = [
            SWAP_FLAG_PREFER,
            SWAP_FLAG_DISCARD,
            SWAP_FLAG_DISCARD_ONCE,
            SWAP_FLAG_DISCARD_PAGES,
        ];
        for &b in &all {
            assert_eq!(SWAP_FLAGS_VALID & b, b);
        }
        assert_eq!(SWAP_FLAGS_VALID & SWAP_FLAG_PRIO_MASK, SWAP_FLAG_PRIO_MASK);
    }

    #[test]
    fn test_header_magic_and_sizes() {
        // Magic string is the literal "SWAPSPACE2" — userspace
        // mkswap/swapon writes/checks these exact bytes at the end
        // of page 0.
        assert_eq!(SWAP_HEADER_MAGIC, b"SWAPSPACE2");
        assert_eq!(SWAP_HEADER_MAGIC.len() as u32, SWAP_HEADER_MAGIC_LEN);
        assert_eq!(SWAP_HEADER_VERSION, 1);
        assert_eq!(SWAP_HEADER_LABEL_LEN, 16);
        assert_eq!(SWAP_HEADER_UUID_LEN, 16);
    }

    #[test]
    fn test_max_swapfiles_in_reasonable_range() {
        // Linux historically caps simultaneous swap areas at 32.
        assert_eq!(MAX_SWAPFILES, 32);
        assert!(MAX_SWAPFILES.is_power_of_two());
    }
}

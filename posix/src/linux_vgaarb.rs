//! `<linux/vgaarb.h>` — VGA arbiter constants.
//!
//! The VGA arbiter manages access to legacy VGA I/O ports and
//! memory regions when multiple graphics cards are present.
//! Only one card can own VGA resources at a time.

// ---------------------------------------------------------------------------
// VGA resource flags
// ---------------------------------------------------------------------------

/// VGA I/O resources.
pub const VGA_RSRC_LEGACY_IO: u32 = 1 << 0;
/// VGA memory resources.
pub const VGA_RSRC_LEGACY_MEM: u32 = 1 << 1;
/// Normal I/O.
pub const VGA_RSRC_NORMAL_IO: u32 = 1 << 2;
/// Normal memory.
pub const VGA_RSRC_NORMAL_MEM: u32 = 1 << 3;

/// All legacy resources.
pub const VGA_RSRC_LEGACY_MASK: u32 =
    VGA_RSRC_LEGACY_IO | VGA_RSRC_LEGACY_MEM;

// ---------------------------------------------------------------------------
// VGA decode flags
// ---------------------------------------------------------------------------

/// Legacy decode enabled.
pub const VGA_DEFAULT_DEVICE: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// VGA arbiter state
// ---------------------------------------------------------------------------

/// Resources not locked.
pub const VGA_STATE_UNLOCKED: u32 = 0;
/// Resources locked.
pub const VGA_STATE_LOCKED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rsrc_flags_powers_of_two() {
        let flags = [
            VGA_RSRC_LEGACY_IO, VGA_RSRC_LEGACY_MEM,
            VGA_RSRC_NORMAL_IO, VGA_RSRC_NORMAL_MEM,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_rsrc_flags_no_overlap() {
        let flags = [
            VGA_RSRC_LEGACY_IO, VGA_RSRC_LEGACY_MEM,
            VGA_RSRC_NORMAL_IO, VGA_RSRC_NORMAL_MEM,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_legacy_mask() {
        assert_eq!(VGA_RSRC_LEGACY_MASK, 0x03);
    }

    #[test]
    fn test_state_values() {
        assert_ne!(VGA_STATE_UNLOCKED, VGA_STATE_LOCKED);
    }
}

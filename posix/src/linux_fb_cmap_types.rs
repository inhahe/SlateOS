//! `<linux/fb.h>` — framebuffer colour-map constants.
//!
//! Colour-map related constants used by the legacy framebuffer
//! `FBIOPUTCMAP` / `FBIOGETCMAP` ioctls. The full `<linux/fb.h>` is
//! covered by `linux_fb_types`; this module isolates the colour map
//! subset for clarity.

// ---------------------------------------------------------------------------
// Colour-map size limits
// ---------------------------------------------------------------------------

/// Maximum number of entries in an fb_cmap (matches Linux uapi).
pub const FB_CMAP_MAX_ENTRIES: u32 = 256;
/// Width in bits of each colour component when the framebuffer reports
/// a 16-bit-per-channel colour map (i.e. R, G, B and transp each occupy
/// 16 bits).
pub const FB_CMAP_COMPONENT_BITS: u32 = 16;

// ---------------------------------------------------------------------------
// Colour-map "kind" hints
// ---------------------------------------------------------------------------

/// Standard, indexed colour map.
pub const FB_CMAP_STANDARD: u32 = 0;
/// Truecolor colour map (R/G/B masks describe the pixel layout).
pub const FB_CMAP_TRUECOLOR: u32 = 1;
/// Direct-color colour map (palette per channel).
pub const FB_CMAP_DIRECTCOLOR: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_entries_sensible() {
        assert_eq!(FB_CMAP_MAX_ENTRIES, 256);
        assert!(FB_CMAP_MAX_ENTRIES.is_power_of_two());
    }

    #[test]
    fn test_component_bits() {
        assert_eq!(FB_CMAP_COMPONENT_BITS, 16);
    }

    #[test]
    fn test_kinds_distinct() {
        let kinds = [FB_CMAP_STANDARD, FB_CMAP_TRUECOLOR, FB_CMAP_DIRECTCOLOR];
        for i in 0..kinds.len() {
            for j in (i + 1)..kinds.len() {
                assert_ne!(kinds[i], kinds[j]);
            }
        }
    }
}

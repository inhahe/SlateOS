//! `<linux/aperture.h>` — Aperture helpers for framebuffer handoff.
//!
//! When transitioning from firmware framebuffer (efifb, simplefb)
//! to a native GPU driver, the aperture helpers manage the handoff
//! of the framebuffer memory region. This prevents both drivers
//! from accessing the same memory simultaneously.

// ---------------------------------------------------------------------------
// Aperture flags
// ---------------------------------------------------------------------------

/// Platform device (firmware framebuffer).
pub const APERTURE_PLATFORM: u32 = 1 << 0;
/// Primary aperture (the one currently displayed).
pub const APERTURE_PRIMARY: u32 = 1 << 1;
/// Aperture is system RAM (not VRAM).
pub const APERTURE_SYSMEM: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Well-known firmware framebuffer names
// ---------------------------------------------------------------------------

/// EFI framebuffer.
pub const APERTURE_EFIFB: &str = "efifb";
/// Simple framebuffer.
pub const APERTURE_SIMPLEFB: &str = "simple-framebuffer";
/// VESA framebuffer.
pub const APERTURE_VESAFB: &str = "vesafb";
/// Offscreen framebuffer.
pub const APERTURE_OFFB: &str = "offb";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [APERTURE_PLATFORM, APERTURE_PRIMARY, APERTURE_SYSMEM];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [APERTURE_PLATFORM, APERTURE_PRIMARY, APERTURE_SYSMEM];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_fb_names_distinct() {
        let names = [
            APERTURE_EFIFB, APERTURE_SIMPLEFB,
            APERTURE_VESAFB, APERTURE_OFFB,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }
}

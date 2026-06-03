//! `<linux/if_slip.h>` — SLIP (Serial Line Internet Protocol) constants.
//!
//! SLIP is a simple encapsulation for IP over serial lines.
//! These constants define SLIP IOCTL commands, modes, and
//! special byte values used in SLIP framing.

// ---------------------------------------------------------------------------
// SLIP special characters
// ---------------------------------------------------------------------------

/// End of frame marker.
pub const SLIP_END: u8 = 0xC0;
/// Escape character.
pub const SLIP_ESC: u8 = 0xDB;
/// Escaped END byte.
pub const SLIP_ESC_END: u8 = 0xDC;
/// Escaped ESC byte.
pub const SLIP_ESC_ESC: u8 = 0xDD;

// ---------------------------------------------------------------------------
// SLIP IOCTL commands
// ---------------------------------------------------------------------------

/// Set SLIP mode/discipline.
pub const SIOCSKEEPALIVE: u32 = 0x894B;
/// Get SLIP keepalive.
pub const SIOCGKEEPALIVE: u32 = 0x894C;
/// Set SLIP outfill.
pub const SIOCSOUTFILL: u32 = 0x894D;
/// Get SLIP outfill.
pub const SIOCGOUTFILL: u32 = 0x894E;
/// Set SLIP mode.
pub const SIOCSSLIPMODE: u32 = 0x894F;

// ---------------------------------------------------------------------------
// SLIP modes
// ---------------------------------------------------------------------------

/// Normal SLIP.
pub const SL_MODE_SLIP: u32 = 0;
/// CSLIP (Compressed SLIP, Van Jacobson header compression).
pub const SL_MODE_CSLIP: u32 = 1;
/// SLIP6 (6-bit encoding).
pub const SL_MODE_SLIP6: u32 = 2;
/// CSLIP6.
pub const SL_MODE_CSLIP6: u32 = 3;
/// Adaptive SLIP.
pub const SL_MODE_AX25: u32 = 4;

// ---------------------------------------------------------------------------
// SLIP flags
// ---------------------------------------------------------------------------

/// Compressed headers enabled.
pub const SLF_COMPRESS: u32 = 1 << 0;
/// Auto-detect header compression.
pub const SLF_AUTOCOMP: u32 = 1 << 1;
/// Keep device alive.
pub const SLF_KEEPTEST: u32 = 1 << 2;
/// Outfill timer active.
pub const SLF_OUTFILL: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// SLIP MTU/buffer sizes
// ---------------------------------------------------------------------------

/// Default SLIP MTU.
pub const SLIP_MTU: u32 = 296;
/// Maximum SLIP MTU.
pub const SLIP_MTU_MAX: u32 = 65534;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_special_chars_distinct() {
        let chars = [SLIP_END, SLIP_ESC, SLIP_ESC_END, SLIP_ESC_ESC];
        for i in 0..chars.len() {
            for j in (i + 1)..chars.len() {
                assert_ne!(chars[i], chars[j]);
            }
        }
    }

    #[test]
    fn test_end_value() {
        assert_eq!(SLIP_END, 0xC0);
    }

    #[test]
    fn test_esc_value() {
        assert_eq!(SLIP_ESC, 0xDB);
    }

    #[test]
    fn test_modes_distinct() {
        let modes = [
            SL_MODE_SLIP,
            SL_MODE_CSLIP,
            SL_MODE_SLIP6,
            SL_MODE_CSLIP6,
            SL_MODE_AX25,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [SLF_COMPRESS, SLF_AUTOCOMP, SLF_KEEPTEST, SLF_OUTFILL];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [SLF_COMPRESS, SLF_AUTOCOMP, SLF_KEEPTEST, SLF_OUTFILL];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [
            SIOCSKEEPALIVE,
            SIOCGKEEPALIVE,
            SIOCSOUTFILL,
            SIOCGOUTFILL,
            SIOCSSLIPMODE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_default_mtu() {
        assert_eq!(SLIP_MTU, 296);
    }

    #[test]
    fn test_max_mtu() {
        assert!(SLIP_MTU_MAX > SLIP_MTU);
    }
}

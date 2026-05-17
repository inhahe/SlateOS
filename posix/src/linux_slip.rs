//! `<linux/slip.h>` — Serial Line Internet Protocol constants.
//!
//! SLIP (RFC 1055) is one of the oldest methods for sending IP
//! datagrams over serial lines. While largely superseded by PPP,
//! SLIP remains useful for simple embedded systems and legacy
//! equipment. CSLIP adds Van Jacobson TCP/IP header compression.

// ---------------------------------------------------------------------------
// SLIP line disciplines
// ---------------------------------------------------------------------------

/// SLIP line discipline number.
pub const N_SLIP: u32 = 1;
/// CSLIP (compressed SLIP) — same discipline, compression enabled.
pub const N_CSLIP: u32 = 1;

// ---------------------------------------------------------------------------
// SLIP framing bytes
// ---------------------------------------------------------------------------

/// Frame end delimiter.
pub const SLIP_END: u8 = 0xC0;
/// Frame escape byte.
pub const SLIP_ESC: u8 = 0xDB;
/// Escaped END (ESC + ESC_END = literal 0xC0 in data).
pub const SLIP_ESC_END: u8 = 0xDC;
/// Escaped ESC (ESC + ESC_ESC = literal 0xDB in data).
pub const SLIP_ESC_ESC: u8 = 0xDD;

// ---------------------------------------------------------------------------
// SLIP mode flags (ioctl)
// ---------------------------------------------------------------------------

/// Normal SLIP mode.
pub const SL_MODE_SLIP: u32 = 0;
/// Compressed SLIP (Van Jacobson).
pub const SL_MODE_CSLIP: u32 = 1 << 0;
/// SLIP6 (6-bit encoding for noisy lines).
pub const SL_MODE_SLIP6: u32 = 1 << 1;
/// CSLIP6 (compressed + 6-bit).
pub const SL_MODE_CSLIP6: u32 = (1 << 0) | (1 << 1);
/// Adaptive mode (auto-detect compression).
pub const SL_MODE_ADAPTIVE: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// SLIP ioctl commands
// ---------------------------------------------------------------------------

/// Set SLIP mode/flags.
pub const SIOCSLMODE: u32 = 0x89E0;
/// Get SLIP mode/flags.
pub const SIOCGLMODE: u32 = 0x89E1;
/// Set keepalive interval.
pub const SIOCSKEEPALIVE: u32 = 0x89E2;
/// Get keepalive interval.
pub const SIOCGKEEPALIVE: u32 = 0x89E3;
/// Set outfill interval.
pub const SIOCSOUTFILL: u32 = 0x89E4;
/// Get outfill interval.
pub const SIOCGOUTFILL: u32 = 0x89E5;

// ---------------------------------------------------------------------------
// SLIP limits
// ---------------------------------------------------------------------------

/// Maximum SLIP MTU.
pub const SLIP_MAX_MTU: u16 = 65534;
/// Default SLIP MTU.
pub const SLIP_DEFAULT_MTU: u16 = 296;
/// Maximum number of SLIP interfaces.
pub const SLIP_MAX_DEV: u8 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_framing_bytes_distinct() {
        let bytes = [SLIP_END, SLIP_ESC, SLIP_ESC_END, SLIP_ESC_ESC];
        for i in 0..bytes.len() {
            for j in (i + 1)..bytes.len() {
                assert_ne!(bytes[i], bytes[j]);
            }
        }
    }

    #[test]
    fn test_framing_values() {
        assert_eq!(SLIP_END, 0xC0);
        assert_eq!(SLIP_ESC, 0xDB);
        assert_eq!(SLIP_ESC_END, 0xDC);
        assert_eq!(SLIP_ESC_ESC, 0xDD);
    }

    #[test]
    fn test_mode_flags() {
        // CSLIP6 is the combination of CSLIP and SLIP6
        assert_eq!(SL_MODE_CSLIP6, SL_MODE_CSLIP | SL_MODE_SLIP6);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            SIOCSLMODE, SIOCGLMODE, SIOCSKEEPALIVE,
            SIOCGKEEPALIVE, SIOCSOUTFILL, SIOCGOUTFILL,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_mtu_limits() {
        assert!(SLIP_DEFAULT_MTU < SLIP_MAX_MTU);
    }
}

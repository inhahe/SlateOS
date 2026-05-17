//! `<linux/parport.h>` — Parallel port subsystem constants.
//!
//! The parport subsystem manages IEEE 1284 parallel ports. While
//! largely obsolete for printing, parallel ports are still used in
//! industrial control, embedded systems, and retro computing.
//! Parport provides shared access to the port with priority-based
//! arbitration between multiple drivers (lp, ppdev, etc.).

// ---------------------------------------------------------------------------
// Port modes (IEEE 1284)
// ---------------------------------------------------------------------------

/// Compatibility mode (Centronics, original printer mode).
pub const PARPORT_MODE_COMPAT: u32 = 1 << 0;
/// Nibble mode (4-bit reverse channel).
pub const PARPORT_MODE_NIBBLE: u32 = 1 << 1;
/// Byte mode (8-bit reverse channel).
pub const PARPORT_MODE_BYTE: u32 = 1 << 2;
/// EPP mode (Enhanced Parallel Port).
pub const PARPORT_MODE_EPP: u32 = 1 << 3;
/// ECP mode (Extended Capability Port).
pub const PARPORT_MODE_ECP: u32 = 1 << 4;
/// DMA capable.
pub const PARPORT_MODE_DMA: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Parport control register bits
// ---------------------------------------------------------------------------

/// Strobe (active low, triggers data latch).
pub const PARPORT_CONTROL_STROBE: u32 = 1 << 0;
/// Auto linefeed.
pub const PARPORT_CONTROL_AUTOFD: u32 = 1 << 1;
/// Initialize printer (active low reset).
pub const PARPORT_CONTROL_INIT: u32 = 1 << 2;
/// Select printer.
pub const PARPORT_CONTROL_SELECT: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Parport status register bits
// ---------------------------------------------------------------------------

/// Error (active low).
pub const PARPORT_STATUS_ERROR: u32 = 1 << 3;
/// Select in (printer online).
pub const PARPORT_STATUS_SELECT: u32 = 1 << 4;
/// Paper out.
pub const PARPORT_STATUS_PAPEROUT: u32 = 1 << 5;
/// Acknowledge (data received).
pub const PARPORT_STATUS_ACK: u32 = 1 << 6;
/// Busy (active low, printer processing).
pub const PARPORT_STATUS_BUSY: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Claim/release flags
// ---------------------------------------------------------------------------

/// Exclusive access (block other users).
pub const PARPORT_FLAG_EXCL: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_no_overlap() {
        let modes = [
            PARPORT_MODE_COMPAT, PARPORT_MODE_NIBBLE,
            PARPORT_MODE_BYTE, PARPORT_MODE_EPP,
            PARPORT_MODE_ECP, PARPORT_MODE_DMA,
        ];
        for i in 0..modes.len() {
            assert!(modes[i].is_power_of_two());
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }

    #[test]
    fn test_control_bits_no_overlap() {
        let bits = [
            PARPORT_CONTROL_STROBE, PARPORT_CONTROL_AUTOFD,
            PARPORT_CONTROL_INIT, PARPORT_CONTROL_SELECT,
        ];
        for i in 0..bits.len() {
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_status_bits_no_overlap() {
        let bits = [
            PARPORT_STATUS_ERROR, PARPORT_STATUS_SELECT,
            PARPORT_STATUS_PAPEROUT, PARPORT_STATUS_ACK,
            PARPORT_STATUS_BUSY,
        ];
        for i in 0..bits.len() {
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_excl_flag() {
        assert_eq!(PARPORT_FLAG_EXCL, 1);
    }
}

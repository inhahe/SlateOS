//! `<linux/usb/ch11.h>` — USB hub constants.
//!
//! USB hubs expand the number of available ports. Root hubs are built
//! into the host controller; external hubs are separate devices.
//! Hubs manage port power, detect device connect/disconnect events,
//! perform port reset (speed negotiation), and route packets between
//! upstream and downstream ports. USB 3.0+ hubs have separate
//! transaction translators for USB 2.0 backward compatibility.

// ---------------------------------------------------------------------------
// Hub port status bits (wPortStatus)
// ---------------------------------------------------------------------------

/// Device connected to port.
pub const USB_PORT_STAT_CONNECTION: u32 = 0x0001;
/// Port is enabled (data transfer possible).
pub const USB_PORT_STAT_ENABLE: u32 = 0x0002;
/// Port is suspended.
pub const USB_PORT_STAT_SUSPEND: u32 = 0x0004;
/// Overcurrent condition on port.
pub const USB_PORT_STAT_OVERCURRENT: u32 = 0x0008;
/// Port reset in progress.
pub const USB_PORT_STAT_RESET: u32 = 0x0010;
/// Port power is on.
pub const USB_PORT_STAT_POWER: u32 = 0x0100;
/// Low-speed device attached.
pub const USB_PORT_STAT_LOW_SPEED: u32 = 0x0200;
/// High-speed device attached.
pub const USB_PORT_STAT_HIGH_SPEED: u32 = 0x0400;

// ---------------------------------------------------------------------------
// Hub port status change bits (wPortChange)
// ---------------------------------------------------------------------------

/// Connection status changed.
pub const USB_PORT_STAT_C_CONNECTION: u32 = 0x0001;
/// Enable status changed.
pub const USB_PORT_STAT_C_ENABLE: u32 = 0x0002;
/// Suspend status changed.
pub const USB_PORT_STAT_C_SUSPEND: u32 = 0x0004;
/// Overcurrent status changed.
pub const USB_PORT_STAT_C_OVERCURRENT: u32 = 0x0008;
/// Reset complete.
pub const USB_PORT_STAT_C_RESET: u32 = 0x0010;

// ---------------------------------------------------------------------------
// Hub characteristics (wHubCharacteristics)
// ---------------------------------------------------------------------------

/// Gang power switching (all ports together).
pub const HUB_CHAR_GANG_POWER: u32 = 0x0000;
/// Individual port power switching.
pub const HUB_CHAR_INDIVIDUAL_POWER: u32 = 0x0001;
/// No power switching (always on).
pub const HUB_CHAR_NO_POWER: u32 = 0x0002;
/// Power switching mask.
pub const HUB_CHAR_POWER_MASK: u32 = 0x0003;
/// Hub is part of a compound device.
pub const HUB_CHAR_COMPOUND: u32 = 0x0004;
/// Global overcurrent protection.
pub const HUB_CHAR_OC_GLOBAL: u32 = 0x0000;
/// Individual port overcurrent protection.
pub const HUB_CHAR_OC_INDIVIDUAL: u32 = 0x0008;

// ---------------------------------------------------------------------------
// Hub limits
// ---------------------------------------------------------------------------

/// Maximum ports per hub.
pub const USB_MAXCHILDREN: u32 = 31;
/// Maximum hub depth (tiers).
pub const USB_MAXHUB_DEPTH: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_status_bits() {
        // Key status bits should be distinct bit positions
        let bits = [
            USB_PORT_STAT_CONNECTION,
            USB_PORT_STAT_ENABLE,
            USB_PORT_STAT_SUSPEND,
            USB_PORT_STAT_OVERCURRENT,
            USB_PORT_STAT_RESET,
        ];
        for i in 0..bits.len() {
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_change_bits() {
        let bits = [
            USB_PORT_STAT_C_CONNECTION,
            USB_PORT_STAT_C_ENABLE,
            USB_PORT_STAT_C_SUSPEND,
            USB_PORT_STAT_C_OVERCURRENT,
            USB_PORT_STAT_C_RESET,
        ];
        for i in 0..bits.len() {
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_limits() {
        assert!(USB_MAXCHILDREN > 0);
        assert!(USB_MAXHUB_DEPTH > 0);
    }
}

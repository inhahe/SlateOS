//! `<linux/gameport.h>` — legacy 15-pin gameport (joystick) device ABI.
//!
//! The gameport bus is largely historical (replaced by USB HID) but
//! still ships in the kernel and is used by retro-game emulators
//! talking to PCI gameport adapters and SoundBlaster on-board jacks.

// ---------------------------------------------------------------------------
// Port-mode register values
// ---------------------------------------------------------------------------

/// Raw 4-axis 4-button mode.
pub const GAMEPORT_MODE_RAW: u32 = 0;
/// Cooked mode (kernel decodes pulse widths).
pub const GAMEPORT_MODE_COOKED: u32 = 1;
/// External serial protocol mode.
pub const GAMEPORT_MODE_ON: u32 = 2;

// ---------------------------------------------------------------------------
// Joystick API limits (input-driven; preserved for compatibility)
// ---------------------------------------------------------------------------

/// Maximum number of axes per gameport device.
pub const GAMEPORT_MAX_AXES: u32 = 4;
/// Maximum number of buttons per gameport device.
pub const GAMEPORT_MAX_BUTTONS: u32 = 4;
/// Maximum poll interval (ms).
pub const GAMEPORT_MAX_POLL_INTERVAL: u32 = 1000;

// ---------------------------------------------------------------------------
// Device-node paths
// ---------------------------------------------------------------------------

/// Joystick character device prefix.
pub const GAMEPORT_JS_PREFIX: &str = "/dev/input/js";
/// Event character device prefix.
pub const GAMEPORT_EVENT_PREFIX: &str = "/dev/input/event";

// ---------------------------------------------------------------------------
// Bus and ID types (struct gameport.id.bustype)
// ---------------------------------------------------------------------------

/// ISA bus host.
pub const GAMEPORT_BUS_ISA: u32 = 0x01;
/// PCI bus host.
pub const GAMEPORT_BUS_PCI: u32 = 0x02;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_dense() {
        assert_eq!(GAMEPORT_MODE_RAW, 0);
        assert_eq!(GAMEPORT_MODE_COOKED, 1);
        assert_eq!(GAMEPORT_MODE_ON, 2);
    }

    #[test]
    fn test_limits_match_dpos() {
        // 4 axes / 4 buttons is the original gameport spec.
        assert_eq!(GAMEPORT_MAX_AXES, 4);
        assert_eq!(GAMEPORT_MAX_BUTTONS, 4);
        // Poll interval at most one second (kernel safety).
        assert_eq!(GAMEPORT_MAX_POLL_INTERVAL, 1000);
    }

    #[test]
    fn test_device_paths() {
        assert_eq!(GAMEPORT_JS_PREFIX, "/dev/input/js");
        assert_eq!(GAMEPORT_EVENT_PREFIX, "/dev/input/event");
        assert_ne!(GAMEPORT_JS_PREFIX, GAMEPORT_EVENT_PREFIX);
    }

    #[test]
    fn test_bus_types_distinct() {
        assert_ne!(GAMEPORT_BUS_ISA, GAMEPORT_BUS_PCI);
        assert_eq!(GAMEPORT_BUS_ISA, 1);
        assert_eq!(GAMEPORT_BUS_PCI, 2);
    }
}

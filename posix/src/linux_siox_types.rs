//! `<linux/siox.h>` — SIOX (Safety Input/Output eXtension) bus constants.
//!
//! SIOX is a serial communication bus for safety-critical industrial
//! I/O. It connects modules in a daisy chain using a shift-register
//! protocol with CRC error detection. Each cycle, the master shifts
//! data out to all modules simultaneously and reads back their
//! responses. Used in industrial automation where deterministic
//! timing and error detection are required.

// ---------------------------------------------------------------------------
// SIOX device types
// ---------------------------------------------------------------------------

/// Digital input module (reads switches/sensors).
pub const SIOX_TYPE_DIGITAL_IN: u32 = 0;
/// Digital output module (drives relays/actuators).
pub const SIOX_TYPE_DIGITAL_OUT: u32 = 1;
/// Analog input module (reads ADC values).
pub const SIOX_TYPE_ANALOG_IN: u32 = 2;
/// Analog output module (drives DAC values).
pub const SIOX_TYPE_ANALOG_OUT: u32 = 3;
/// Combo module (mixed I/O).
pub const SIOX_TYPE_COMBO: u32 = 4;

// ---------------------------------------------------------------------------
// SIOX bus states
// ---------------------------------------------------------------------------

/// Bus is idle (no active communication).
pub const SIOX_BUS_IDLE: u32 = 0;
/// Bus is active (cyclic communication running).
pub const SIOX_BUS_ACTIVE: u32 = 1;
/// Bus error (CRC mismatch or timeout).
pub const SIOX_BUS_ERROR: u32 = 2;

// ---------------------------------------------------------------------------
// SIOX status flags
// ---------------------------------------------------------------------------

/// Module watchdog is active.
pub const SIOX_STATUS_WATCHDOG: u32 = 1 << 0;
/// Module detected a CRC error.
pub const SIOX_STATUS_CRC_ERROR: u32 = 1 << 1;
/// Module is connected.
pub const SIOX_STATUS_CONNECTED: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_types_distinct() {
        let types = [
            SIOX_TYPE_DIGITAL_IN,
            SIOX_TYPE_DIGITAL_OUT,
            SIOX_TYPE_ANALOG_IN,
            SIOX_TYPE_ANALOG_OUT,
            SIOX_TYPE_COMBO,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_bus_states_distinct() {
        let states = [SIOX_BUS_IDLE, SIOX_BUS_ACTIVE, SIOX_BUS_ERROR];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_status_flags_no_overlap() {
        let flags = [
            SIOX_STATUS_WATCHDOG,
            SIOX_STATUS_CRC_ERROR,
            SIOX_STATUS_CONNECTED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

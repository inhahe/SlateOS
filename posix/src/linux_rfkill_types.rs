//! `<linux/rfkill.h>` — RF kill switch constants.
//!
//! The rfkill subsystem manages radio transmitters (WiFi, Bluetooth,
//! NFC, cellular, GPS, etc.). Radios can be blocked by software
//! (soft-block, reversible) or hardware (hard-block, physical switch).
//! Userspace reads /dev/rfkill for state change events and writes to
//! it to toggle soft-block state.

// ---------------------------------------------------------------------------
// Radio types
// ---------------------------------------------------------------------------

/// All radios.
pub const RFKILL_TYPE_ALL: u32 = 0;
/// WiFi / WLAN.
pub const RFKILL_TYPE_WLAN: u32 = 1;
/// Bluetooth.
pub const RFKILL_TYPE_BLUETOOTH: u32 = 2;
/// Ultra-Wideband (UWB).
pub const RFKILL_TYPE_UWB: u32 = 3;
/// WiMAX.
pub const RFKILL_TYPE_WIMAX: u32 = 4;
/// Mobile broadband (3G/4G/5G).
pub const RFKILL_TYPE_WWAN: u32 = 5;
/// GPS.
pub const RFKILL_TYPE_GPS: u32 = 6;
/// FM radio.
pub const RFKILL_TYPE_FM: u32 = 7;
/// NFC (Near Field Communication).
pub const RFKILL_TYPE_NFC: u32 = 8;
/// Number of defined types.
pub const RFKILL_TYPE_NUM: u32 = 9;

// ---------------------------------------------------------------------------
// rfkill operations
// ---------------------------------------------------------------------------

/// Add a new rfkill device (kernel → userspace event).
pub const RFKILL_OP_ADD: u32 = 0;
/// Remove an rfkill device.
pub const RFKILL_OP_DEL: u32 = 1;
/// State changed.
pub const RFKILL_OP_CHANGE: u32 = 2;
/// Change all devices of a type (userspace → kernel).
pub const RFKILL_OP_CHANGE_ALL: u32 = 3;

// ---------------------------------------------------------------------------
// rfkill states
// ---------------------------------------------------------------------------

/// Radio is unblocked (transmitting allowed).
pub const RFKILL_STATE_UNBLOCKED: u32 = 0;
/// Radio is soft-blocked (software disabled).
pub const RFKILL_STATE_SOFT_BLOCKED: u32 = 1;
/// Radio is hard-blocked (hardware switch off).
pub const RFKILL_STATE_HARD_BLOCKED: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radio_types_distinct() {
        let types = [
            RFKILL_TYPE_ALL,
            RFKILL_TYPE_WLAN,
            RFKILL_TYPE_BLUETOOTH,
            RFKILL_TYPE_UWB,
            RFKILL_TYPE_WIMAX,
            RFKILL_TYPE_WWAN,
            RFKILL_TYPE_GPS,
            RFKILL_TYPE_FM,
            RFKILL_TYPE_NFC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_operations_distinct() {
        let ops = [
            RFKILL_OP_ADD,
            RFKILL_OP_DEL,
            RFKILL_OP_CHANGE,
            RFKILL_OP_CHANGE_ALL,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            RFKILL_STATE_UNBLOCKED,
            RFKILL_STATE_SOFT_BLOCKED,
            RFKILL_STATE_HARD_BLOCKED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_type_count() {
        assert_eq!(RFKILL_TYPE_NUM, 9);
        assert!(RFKILL_TYPE_NFC < RFKILL_TYPE_NUM);
    }
}

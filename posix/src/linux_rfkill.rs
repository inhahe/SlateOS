//! `<linux/rfkill.h>` — Radio frequency kill switch constants.
//!
//! rfkill provides a unified interface for enabling/disabling
//! wireless transmitters (WiFi, Bluetooth, NFC, GPS, etc.).
//! Userspace reads /dev/rfkill and controls via sysfs.

// ---------------------------------------------------------------------------
// rfkill types
// ---------------------------------------------------------------------------

/// All radio types.
pub const RFKILL_TYPE_ALL: u32 = 0;
/// WLAN (WiFi).
pub const RFKILL_TYPE_WLAN: u32 = 1;
/// Bluetooth.
pub const RFKILL_TYPE_BLUETOOTH: u32 = 2;
/// Ultra-wideband (UWB).
pub const RFKILL_TYPE_UWB: u32 = 3;
/// WiMAX.
pub const RFKILL_TYPE_WIMAX: u32 = 4;
/// WWAN (mobile broadband).
pub const RFKILL_TYPE_WWAN: u32 = 5;
/// GPS.
pub const RFKILL_TYPE_GPS: u32 = 6;
/// FM radio.
pub const RFKILL_TYPE_FM: u32 = 7;
/// NFC.
pub const RFKILL_TYPE_NFC: u32 = 8;

/// Number of rfkill types.
pub const NUM_RFKILL_TYPES: u32 = 9;

// ---------------------------------------------------------------------------
// rfkill operations
// ---------------------------------------------------------------------------

/// Add a new rfkill device.
pub const RFKILL_OP_ADD: u32 = 0;
/// Delete an rfkill device.
pub const RFKILL_OP_DEL: u32 = 1;
/// Change state.
pub const RFKILL_OP_CHANGE: u32 = 2;
/// Change all devices of a type.
pub const RFKILL_OP_CHANGE_ALL: u32 = 3;

// ---------------------------------------------------------------------------
// rfkill states
// ---------------------------------------------------------------------------

/// Radio is unblocked (enabled).
pub const RFKILL_STATE_UNBLOCKED: u32 = 0;
/// Soft-blocked (software disable).
pub const RFKILL_STATE_SOFT_BLOCKED: u32 = 1;
/// Hard-blocked (hardware switch).
pub const RFKILL_STATE_HARD_BLOCKED: u32 = 2;

// ---------------------------------------------------------------------------
// rfkill event structure
// ---------------------------------------------------------------------------

/// rfkill event (read from /dev/rfkill).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RfkillEvent {
    /// Device index.
    pub idx: u32,
    /// Device type.
    pub type_: u8,
    /// Operation.
    pub op: u8,
    /// Soft block state.
    pub soft: u8,
    /// Hard block state.
    pub hard: u8,
}

impl RfkillEvent {
    /// Create a zeroed rfkill event.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            RFKILL_TYPE_ALL, RFKILL_TYPE_WLAN, RFKILL_TYPE_BLUETOOTH,
            RFKILL_TYPE_UWB, RFKILL_TYPE_WIMAX, RFKILL_TYPE_WWAN,
            RFKILL_TYPE_GPS, RFKILL_TYPE_FM, RFKILL_TYPE_NFC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ops_distinct() {
        let ops = [
            RFKILL_OP_ADD, RFKILL_OP_DEL,
            RFKILL_OP_CHANGE, RFKILL_OP_CHANGE_ALL,
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
            RFKILL_STATE_UNBLOCKED, RFKILL_STATE_SOFT_BLOCKED,
            RFKILL_STATE_HARD_BLOCKED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_event_size() {
        assert_eq!(core::mem::size_of::<RfkillEvent>(), 8);
    }

    #[test]
    fn test_num_types() {
        assert_eq!(NUM_RFKILL_TYPES, RFKILL_TYPE_NFC + 1);
    }
}

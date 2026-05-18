//! `<linux/rfkill.h>` — Additional rfkill constants.
//!
//! Supplementary rfkill constants covering device types,
//! operations, and state values for wireless kill switch.

// ---------------------------------------------------------------------------
// rfkill types (RFKILL_TYPE_*)
// ---------------------------------------------------------------------------

/// All radio types.
pub const RFKILL_TYPE_ALL: u32 = 0;
/// WLAN (Wi-Fi).
pub const RFKILL_TYPE_WLAN: u32 = 1;
/// Bluetooth.
pub const RFKILL_TYPE_BLUETOOTH: u32 = 2;
/// Ultra-Wideband.
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
/// Number of types.
pub const RFKILL_NUM_TYPES: u32 = 9;

// ---------------------------------------------------------------------------
// rfkill operations (RFKILL_OP_*)
// ---------------------------------------------------------------------------

/// Add device.
pub const RFKILL_OP_ADD: u32 = 0;
/// Delete device.
pub const RFKILL_OP_DEL: u32 = 1;
/// Change state.
pub const RFKILL_OP_CHANGE: u32 = 2;
/// Change all devices of type.
pub const RFKILL_OP_CHANGE_ALL: u32 = 3;

// ---------------------------------------------------------------------------
// rfkill states (RFKILL_STATE_*)
// ---------------------------------------------------------------------------

/// Unblocked.
pub const RFKILL_STATE_UNBLOCKED: u32 = 0;
/// Soft blocked.
pub const RFKILL_STATE_SOFT_BLOCKED: u32 = 1;
/// Hard blocked.
pub const RFKILL_STATE_HARD_BLOCKED: u32 = 2;

// ---------------------------------------------------------------------------
// rfkill IOCTL
// ---------------------------------------------------------------------------

/// IOCTL magic.
pub const RFKILL_IOC_MAGIC: u8 = b'R';
/// Max event size.
pub const RFKILL_EVENT_SIZE_V1: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_sequential() {
        assert_eq!(RFKILL_TYPE_ALL, 0);
        assert_eq!(RFKILL_TYPE_WLAN, 1);
        assert_eq!(RFKILL_TYPE_NFC, 8);
        assert_eq!(RFKILL_NUM_TYPES, 9);
    }

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
    fn test_ops_sequential() {
        assert_eq!(RFKILL_OP_ADD, 0);
        assert_eq!(RFKILL_OP_DEL, 1);
        assert_eq!(RFKILL_OP_CHANGE, 2);
        assert_eq!(RFKILL_OP_CHANGE_ALL, 3);
    }

    #[test]
    fn test_states_sequential() {
        assert_eq!(RFKILL_STATE_UNBLOCKED, 0);
        assert_eq!(RFKILL_STATE_SOFT_BLOCKED, 1);
        assert_eq!(RFKILL_STATE_HARD_BLOCKED, 2);
    }

    #[test]
    fn test_ioc_magic() {
        assert_eq!(RFKILL_IOC_MAGIC, b'R');
    }

    #[test]
    fn test_event_size() {
        assert_eq!(RFKILL_EVENT_SIZE_V1, 8);
    }
}

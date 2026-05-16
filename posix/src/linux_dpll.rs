//! `<linux/dpll.h>` — Digital Phase-Locked Loop subsystem constants.
//!
//! The DPLL subsystem manages clock synchronization hardware on
//! network equipment. DPLLs lock to reference signals (GPS, SyncE,
//! PPS) to produce stable output clocks for PTP and telecom.

// ---------------------------------------------------------------------------
// DPLL netlink commands
// ---------------------------------------------------------------------------

/// Unspecified.
pub const DPLL_CMD_UNSPEC: u8 = 0;
/// Get device info.
pub const DPLL_CMD_DEVICE_ID_GET: u8 = 1;
/// Get device.
pub const DPLL_CMD_DEVICE_GET: u8 = 2;
/// Set device.
pub const DPLL_CMD_DEVICE_SET: u8 = 3;
/// Create device notification.
pub const DPLL_CMD_DEVICE_CREATE_NTF: u8 = 4;
/// Delete device notification.
pub const DPLL_CMD_DEVICE_DELETE_NTF: u8 = 5;
/// Change device notification.
pub const DPLL_CMD_DEVICE_CHANGE_NTF: u8 = 6;
/// Get pin.
pub const DPLL_CMD_PIN_ID_GET: u8 = 7;
/// Get pin info.
pub const DPLL_CMD_PIN_GET: u8 = 8;
/// Set pin.
pub const DPLL_CMD_PIN_SET: u8 = 9;
/// Pin create notification.
pub const DPLL_CMD_PIN_CREATE_NTF: u8 = 10;
/// Pin delete notification.
pub const DPLL_CMD_PIN_DELETE_NTF: u8 = 11;
/// Pin change notification.
pub const DPLL_CMD_PIN_CHANGE_NTF: u8 = 12;

// ---------------------------------------------------------------------------
// DPLL modes
// ---------------------------------------------------------------------------

/// Manual mode (freerunning).
pub const DPLL_MODE_MANUAL: u32 = 0;
/// Automatic mode (auto-select reference).
pub const DPLL_MODE_AUTOMATIC: u32 = 1;

// ---------------------------------------------------------------------------
// DPLL lock status
// ---------------------------------------------------------------------------

/// Unlocked (no valid reference).
pub const DPLL_LOCK_STATUS_UNLOCKED: u32 = 0;
/// Locked (tracking reference).
pub const DPLL_LOCK_STATUS_LOCKED: u32 = 1;
/// Locked, holdover capable.
pub const DPLL_LOCK_STATUS_LOCKED_HO_ACQ: u32 = 2;
/// Holdover (lost reference, using stored freq).
pub const DPLL_LOCK_STATUS_HOLDOVER: u32 = 3;

// ---------------------------------------------------------------------------
// Pin types
// ---------------------------------------------------------------------------

/// MUX pin.
pub const DPLL_PIN_TYPE_MUX: u32 = 1;
/// External pin.
pub const DPLL_PIN_TYPE_EXT: u32 = 2;
/// SyncE (Synchronous Ethernet).
pub const DPLL_PIN_TYPE_SYNCE_ETH_PORT: u32 = 3;
/// Internal oscillator.
pub const DPLL_PIN_TYPE_INT_OSCILLATOR: u32 = 4;
/// GNSS (GPS/GLONASS).
pub const DPLL_PIN_TYPE_GNSS: u32 = 5;

// ---------------------------------------------------------------------------
// Pin state
// ---------------------------------------------------------------------------

/// Pin connected.
pub const DPLL_PIN_STATE_CONNECTED: u32 = 0;
/// Pin disconnected.
pub const DPLL_PIN_STATE_DISCONNECTED: u32 = 1;
/// Pin is selected reference.
pub const DPLL_PIN_STATE_SELECTABLE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            DPLL_CMD_UNSPEC, DPLL_CMD_DEVICE_ID_GET, DPLL_CMD_DEVICE_GET,
            DPLL_CMD_DEVICE_SET, DPLL_CMD_DEVICE_CREATE_NTF,
            DPLL_CMD_DEVICE_DELETE_NTF, DPLL_CMD_DEVICE_CHANGE_NTF,
            DPLL_CMD_PIN_ID_GET, DPLL_CMD_PIN_GET, DPLL_CMD_PIN_SET,
            DPLL_CMD_PIN_CREATE_NTF, DPLL_CMD_PIN_DELETE_NTF,
            DPLL_CMD_PIN_CHANGE_NTF,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_modes_distinct() {
        assert_ne!(DPLL_MODE_MANUAL, DPLL_MODE_AUTOMATIC);
    }

    #[test]
    fn test_lock_status_distinct() {
        let statuses = [
            DPLL_LOCK_STATUS_UNLOCKED, DPLL_LOCK_STATUS_LOCKED,
            DPLL_LOCK_STATUS_LOCKED_HO_ACQ, DPLL_LOCK_STATUS_HOLDOVER,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_pin_types_distinct() {
        let types = [
            DPLL_PIN_TYPE_MUX, DPLL_PIN_TYPE_EXT,
            DPLL_PIN_TYPE_SYNCE_ETH_PORT, DPLL_PIN_TYPE_INT_OSCILLATOR,
            DPLL_PIN_TYPE_GNSS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_pin_states_distinct() {
        let states = [
            DPLL_PIN_STATE_CONNECTED, DPLL_PIN_STATE_DISCONNECTED,
            DPLL_PIN_STATE_SELECTABLE,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}

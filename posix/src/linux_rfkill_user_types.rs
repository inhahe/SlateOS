//! `<linux/rfkill.h>` — `/dev/rfkill` event-stream interface.
//!
//! `/dev/rfkill` delivers `struct rfkill_event` records describing
//! every wireless device's hardware/software kill state. NetworkManager
//! and `rfkill` CLI open it and call `read()` for state changes or
//! `write()` to block/unblock devices.

// ---------------------------------------------------------------------------
// Operations (rfkill_event.op)
// ---------------------------------------------------------------------------

/// Device added (announced at open time and on hotplug).
pub const RFKILL_OP_ADD: u8 = 0;
/// Device removed.
pub const RFKILL_OP_DEL: u8 = 1;
/// Device state changed (block/unblock).
pub const RFKILL_OP_CHANGE: u8 = 2;
/// Bulk state change — apply to every device of the given type.
pub const RFKILL_OP_CHANGE_ALL: u8 = 3;

// ---------------------------------------------------------------------------
// Wireless types (rfkill_event.type)
// ---------------------------------------------------------------------------

/// "any" type — matches all rfkill devices in CHANGE_ALL.
pub const RFKILL_TYPE_ALL: u8 = 0;
/// Wi-Fi (802.11).
pub const RFKILL_TYPE_WLAN: u8 = 1;
/// Bluetooth.
pub const RFKILL_TYPE_BLUETOOTH: u8 = 2;
/// Ultra-wideband.
pub const RFKILL_TYPE_UWB: u8 = 3;
/// WiMAX.
pub const RFKILL_TYPE_WIMAX: u8 = 4;
/// WWAN (cellular).
pub const RFKILL_TYPE_WWAN: u8 = 5;
/// GPS receiver.
pub const RFKILL_TYPE_GPS: u8 = 6;
/// FM radio.
pub const RFKILL_TYPE_FM: u8 = 7;
/// NFC reader/tag.
pub const RFKILL_TYPE_NFC: u8 = 8;

/// Number of defined wireless types (sentinel — not a valid type).
pub const NUM_RFKILL_TYPES: u8 = 9;

// ---------------------------------------------------------------------------
// Hard/soft block fields (rfkill_event.soft / .hard are u8 booleans)
// ---------------------------------------------------------------------------

/// "unblocked" (radio may run).
pub const RFKILL_UNBLOCKED: u8 = 0;
/// "blocked" (radio held off).
pub const RFKILL_BLOCKED: u8 = 1;

// ---------------------------------------------------------------------------
// Event size
// ---------------------------------------------------------------------------

/// Size of `struct rfkill_event` in bytes: idx u32 + type u8 + op u8 +
/// soft u8 + hard u8 = 8 bytes. Used to validate short reads.
pub const RFKILL_EVENT_SIZE_V1: usize = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_dense() {
        let v = [
            RFKILL_OP_ADD,
            RFKILL_OP_DEL,
            RFKILL_OP_CHANGE,
            RFKILL_OP_CHANGE_ALL,
        ];
        for (i, &op) in v.iter().enumerate() {
            assert_eq!(op as usize, i);
        }
    }

    #[test]
    fn test_types_dense_and_num_sentinel() {
        let v = [
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
        for (i, &t) in v.iter().enumerate() {
            assert_eq!(t as usize, i);
        }
        // NUM_RFKILL_TYPES is the count, not a valid type.
        assert_eq!(NUM_RFKILL_TYPES as usize, v.len());
    }

    #[test]
    fn test_block_state_is_bool_like() {
        assert_eq!(RFKILL_UNBLOCKED, 0);
        assert_eq!(RFKILL_BLOCKED, 1);
    }

    #[test]
    fn test_event_size_v1() {
        // Wire format is 8 bytes; v2 adds reserved bytes but the
        // kernel always accepts the 8-byte short form.
        assert_eq!(RFKILL_EVENT_SIZE_V1, 8);
    }
}

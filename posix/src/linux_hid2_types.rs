//! `<linux/hid.h>` — Additional HID constants.
//!
//! Supplementary HID constants covering report types,
//! usage pages, collection types, and global items.

// ---------------------------------------------------------------------------
// Report types
// ---------------------------------------------------------------------------

/// Input report.
pub const HID_INPUT_REPORT: u32 = 0;
/// Output report.
pub const HID_OUTPUT_REPORT: u32 = 1;
/// Feature report.
pub const HID_FEATURE_REPORT: u32 = 2;

// ---------------------------------------------------------------------------
// Usage pages (HID_UP_*)
// ---------------------------------------------------------------------------

/// Undefined.
pub const HID_UP_UNDEFINED: u32 = 0x00000;
/// Generic desktop.
pub const HID_UP_GENDESK: u32 = 0x00010;
/// Simulation controls.
pub const HID_UP_SIMULATION: u32 = 0x00020;
/// VR controls.
pub const HID_UP_VR: u32 = 0x00030;
/// Sport controls.
pub const HID_UP_SPORT: u32 = 0x00040;
/// Game controls.
pub const HID_UP_GAME: u32 = 0x00050;
/// Generic device.
pub const HID_UP_GENDEVCTRLS: u32 = 0x00060;
/// Keyboard.
pub const HID_UP_KEYBOARD: u32 = 0x00070;
/// LED.
pub const HID_UP_LED: u32 = 0x00080;
/// Button.
pub const HID_UP_BUTTON: u32 = 0x00090;
/// Ordinal.
pub const HID_UP_ORDINAL: u32 = 0x000A0;
/// Telephony.
pub const HID_UP_TELEPHONY: u32 = 0x000B0;
/// Consumer.
pub const HID_UP_CONSUMER: u32 = 0x000C0;
/// Digitizer.
pub const HID_UP_DIGITIZER: u32 = 0x000D0;
/// PID.
pub const HID_UP_PID: u32 = 0x000F0;

// ---------------------------------------------------------------------------
// Collection types
// ---------------------------------------------------------------------------

/// Physical collection.
pub const HID_COLLECTION_PHYSICAL: u32 = 0;
/// Application collection.
pub const HID_COLLECTION_APPLICATION: u32 = 1;
/// Logical collection.
pub const HID_COLLECTION_LOGICAL: u32 = 2;
/// Report collection.
pub const HID_COLLECTION_REPORT: u32 = 3;
/// Named array.
pub const HID_COLLECTION_NAMED_ARRAY: u32 = 4;
/// Usage switch.
pub const HID_COLLECTION_USAGE_SWITCH: u32 = 5;
/// Usage modifier.
pub const HID_COLLECTION_USAGE_MODIFIER: u32 = 6;

// ---------------------------------------------------------------------------
// Main items
// ---------------------------------------------------------------------------

/// Input.
pub const HID_MAIN_ITEM_INPUT: u32 = 0x80;
/// Output.
pub const HID_MAIN_ITEM_OUTPUT: u32 = 0x90;
/// Feature.
pub const HID_MAIN_ITEM_FEATURE: u32 = 0xB0;
/// Collection.
pub const HID_MAIN_ITEM_COLLECTION: u32 = 0xA0;
/// End collection.
pub const HID_MAIN_ITEM_END_COLLECTION: u32 = 0xC0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_types_sequential() {
        assert_eq!(HID_INPUT_REPORT, 0);
        assert_eq!(HID_OUTPUT_REPORT, 1);
        assert_eq!(HID_FEATURE_REPORT, 2);
    }

    #[test]
    fn test_usage_pages_distinct() {
        let pages = [
            HID_UP_UNDEFINED, HID_UP_GENDESK, HID_UP_SIMULATION,
            HID_UP_VR, HID_UP_SPORT, HID_UP_GAME,
            HID_UP_GENDEVCTRLS, HID_UP_KEYBOARD, HID_UP_LED,
            HID_UP_BUTTON, HID_UP_ORDINAL, HID_UP_TELEPHONY,
            HID_UP_CONSUMER, HID_UP_DIGITIZER, HID_UP_PID,
        ];
        for i in 0..pages.len() {
            for j in (i + 1)..pages.len() {
                assert_ne!(pages[i], pages[j]);
            }
        }
    }

    #[test]
    fn test_collection_types_sequential() {
        assert_eq!(HID_COLLECTION_PHYSICAL, 0);
        assert_eq!(HID_COLLECTION_APPLICATION, 1);
        assert_eq!(HID_COLLECTION_USAGE_MODIFIER, 6);
    }

    #[test]
    fn test_main_items_distinct() {
        let items = [
            HID_MAIN_ITEM_INPUT, HID_MAIN_ITEM_OUTPUT,
            HID_MAIN_ITEM_FEATURE, HID_MAIN_ITEM_COLLECTION,
            HID_MAIN_ITEM_END_COLLECTION,
        ];
        for i in 0..items.len() {
            for j in (i + 1)..items.len() {
                assert_ne!(items[i], items[j]);
            }
        }
    }
}

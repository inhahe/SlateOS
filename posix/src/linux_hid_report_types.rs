//! `<linux/hid.h>` — HID report descriptor item constants.
//!
//! HID report descriptors are byte streams of items that describe
//! the format of input/output/feature reports. Items are classified
//! as Main (Input/Output/Feature/Collection), Global (Usage Page,
//! Logical Min/Max), and Local (Usage, Usage Min/Max) items.

// ---------------------------------------------------------------------------
// HID item types (bits 3:2 of short-item prefix byte)
// ---------------------------------------------------------------------------

/// Main item type.
pub const HID_ITEM_TYPE_MAIN: u8 = 0;
/// Global item type.
pub const HID_ITEM_TYPE_GLOBAL: u8 = 1;
/// Local item type.
pub const HID_ITEM_TYPE_LOCAL: u8 = 2;
/// Reserved item type.
pub const HID_ITEM_TYPE_RESERVED: u8 = 3;

// ---------------------------------------------------------------------------
// Main item tags (bits 7:4)
// ---------------------------------------------------------------------------

/// Input item (data from device).
pub const HID_MAIN_ITEM_TAG_INPUT: u8 = 0x08;
/// Output item (data to device).
pub const HID_MAIN_ITEM_TAG_OUTPUT: u8 = 0x09;
/// Feature item (device configuration).
pub const HID_MAIN_ITEM_TAG_FEATURE: u8 = 0x0B;
/// Begin collection.
pub const HID_MAIN_ITEM_TAG_BEGIN_COLLECTION: u8 = 0x0A;
/// End collection.
pub const HID_MAIN_ITEM_TAG_END_COLLECTION: u8 = 0x0C;

// ---------------------------------------------------------------------------
// Global item tags
// ---------------------------------------------------------------------------

/// Usage Page.
pub const HID_GLOBAL_ITEM_TAG_USAGE_PAGE: u8 = 0x00;
/// Logical Minimum.
pub const HID_GLOBAL_ITEM_TAG_LOGICAL_MIN: u8 = 0x01;
/// Logical Maximum.
pub const HID_GLOBAL_ITEM_TAG_LOGICAL_MAX: u8 = 0x02;
/// Physical Minimum.
pub const HID_GLOBAL_ITEM_TAG_PHYSICAL_MIN: u8 = 0x03;
/// Physical Maximum.
pub const HID_GLOBAL_ITEM_TAG_PHYSICAL_MAX: u8 = 0x04;
/// Report Size (bits per field).
pub const HID_GLOBAL_ITEM_TAG_REPORT_SIZE: u8 = 0x07;
/// Report ID.
pub const HID_GLOBAL_ITEM_TAG_REPORT_ID: u8 = 0x08;
/// Report Count (number of fields).
pub const HID_GLOBAL_ITEM_TAG_REPORT_COUNT: u8 = 0x09;

// ---------------------------------------------------------------------------
// Collection types
// ---------------------------------------------------------------------------

/// Physical collection (group of axes).
pub const HID_COLLECTION_PHYSICAL: u8 = 0x00;
/// Application collection (top-level, e.g., mouse).
pub const HID_COLLECTION_APPLICATION: u8 = 0x01;
/// Logical collection (related items).
pub const HID_COLLECTION_LOGICAL: u8 = 0x02;
/// Report collection.
pub const HID_COLLECTION_REPORT: u8 = 0x03;
/// Named array collection.
pub const HID_COLLECTION_NAMED_ARRAY: u8 = 0x04;
/// Usage switch collection.
pub const HID_COLLECTION_USAGE_SWITCH: u8 = 0x05;
/// Usage modifier collection.
pub const HID_COLLECTION_USAGE_MODIFIER: u8 = 0x06;

// ---------------------------------------------------------------------------
// Input/Output/Feature item flags (bits in data byte)
// ---------------------------------------------------------------------------

/// Data (0) vs Constant (1).
pub const HID_IOF_CONSTANT: u32 = 1 << 0;
/// Array (0) vs Variable (1).
pub const HID_IOF_VARIABLE: u32 = 1 << 1;
/// Absolute (0) vs Relative (1).
pub const HID_IOF_RELATIVE: u32 = 1 << 2;
/// No Wrap (0) vs Wrap (1).
pub const HID_IOF_WRAP: u32 = 1 << 3;
/// Linear (0) vs Non-Linear (1).
pub const HID_IOF_NON_LINEAR: u32 = 1 << 4;
/// Preferred State (0) vs No Preferred (1).
pub const HID_IOF_NO_PREFERRED: u32 = 1 << 5;
/// No Null (0) vs Null State (1).
pub const HID_IOF_NULL_STATE: u32 = 1 << 6;
/// Bit Field (0) vs Buffered Bytes (1).
pub const HID_IOF_BUFFERED_BYTES: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_types_distinct() {
        let types = [
            HID_ITEM_TYPE_MAIN, HID_ITEM_TYPE_GLOBAL,
            HID_ITEM_TYPE_LOCAL, HID_ITEM_TYPE_RESERVED,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_main_tags_distinct() {
        let tags = [
            HID_MAIN_ITEM_TAG_INPUT, HID_MAIN_ITEM_TAG_OUTPUT,
            HID_MAIN_ITEM_TAG_FEATURE, HID_MAIN_ITEM_TAG_BEGIN_COLLECTION,
            HID_MAIN_ITEM_TAG_END_COLLECTION,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j]);
            }
        }
    }

    #[test]
    fn test_collection_types_distinct() {
        let cols = [
            HID_COLLECTION_PHYSICAL, HID_COLLECTION_APPLICATION,
            HID_COLLECTION_LOGICAL, HID_COLLECTION_REPORT,
            HID_COLLECTION_NAMED_ARRAY, HID_COLLECTION_USAGE_SWITCH,
            HID_COLLECTION_USAGE_MODIFIER,
        ];
        for i in 0..cols.len() {
            for j in (i + 1)..cols.len() {
                assert_ne!(cols[i], cols[j]);
            }
        }
    }

    #[test]
    fn test_iof_flags_no_overlap() {
        let flags = [
            HID_IOF_CONSTANT, HID_IOF_VARIABLE, HID_IOF_RELATIVE,
            HID_IOF_WRAP, HID_IOF_NON_LINEAR, HID_IOF_NO_PREFERRED,
            HID_IOF_NULL_STATE, HID_IOF_BUFFERED_BYTES,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

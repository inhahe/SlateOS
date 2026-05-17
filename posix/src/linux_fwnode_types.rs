//! `<linux/fwnode.h>` — Firmware node abstraction constants.
//!
//! Fwnode provides a unified abstraction over device tree (OF),
//! ACPI, and software nodes for device description. Instead of
//! drivers checking whether they're on a DT or ACPI platform,
//! they use the fwnode API to read properties, find children,
//! and follow references. This enables truly platform-agnostic
//! drivers. Software nodes (swnode) allow creating fwnode
//! hierarchies entirely in code for devices without firmware
//! description.

// ---------------------------------------------------------------------------
// Firmware node types
// ---------------------------------------------------------------------------

/// Device Tree node (OF).
pub const FWNODE_TYPE_OF: u32 = 0;
/// ACPI device node.
pub const FWNODE_TYPE_ACPI: u32 = 1;
/// ACPI data node (package data, not device).
pub const FWNODE_TYPE_ACPI_DATA: u32 = 2;
/// Software node (created in code).
pub const FWNODE_TYPE_SWNODE: u32 = 3;
/// PCI device node.
pub const FWNODE_TYPE_PCI: u32 = 4;
/// Named child fwnode.
pub const FWNODE_TYPE_NAMED_CHILD: u32 = 5;

// ---------------------------------------------------------------------------
// Firmware node property types (value types)
// ---------------------------------------------------------------------------

/// Property is a u8 value.
pub const FWNODE_PROP_U8: u32 = 0;
/// Property is a u16 value.
pub const FWNODE_PROP_U16: u32 = 1;
/// Property is a u32 value.
pub const FWNODE_PROP_U32: u32 = 2;
/// Property is a u64 value.
pub const FWNODE_PROP_U64: u32 = 3;
/// Property is a string.
pub const FWNODE_PROP_STRING: u32 = 4;
/// Property is a reference (phandle/ACPI ref).
pub const FWNODE_PROP_REFERENCE: u32 = 5;

// ---------------------------------------------------------------------------
// Firmware node flags
// ---------------------------------------------------------------------------

/// Node is the primary fwnode for the device.
pub const FWNODE_FLAG_PRIMARY: u32 = 1 << 0;
/// Node links have been resolved.
pub const FWNODE_FLAG_LINKS_RESOLVED: u32 = 1 << 1;
/// Node is being removed.
pub const FWNODE_FLAG_REMOVING: u32 = 1 << 2;
/// Node was created by firmware (not software).
pub const FWNODE_FLAG_FW_CREATED: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Software node property entry types
// ---------------------------------------------------------------------------

/// Integer array property.
pub const SWNODE_PROP_INT_ARRAY: u32 = 0;
/// String property.
pub const SWNODE_PROP_STRING: u32 = 1;
/// String array property.
pub const SWNODE_PROP_STRING_ARRAY: u32 = 2;
/// Reference property.
pub const SWNODE_PROP_REFERENCE: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_types_distinct() {
        let types = [
            FWNODE_TYPE_OF, FWNODE_TYPE_ACPI,
            FWNODE_TYPE_ACPI_DATA, FWNODE_TYPE_SWNODE,
            FWNODE_TYPE_PCI, FWNODE_TYPE_NAMED_CHILD,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_property_types_distinct() {
        let types = [
            FWNODE_PROP_U8, FWNODE_PROP_U16, FWNODE_PROP_U32,
            FWNODE_PROP_U64, FWNODE_PROP_STRING, FWNODE_PROP_REFERENCE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            FWNODE_FLAG_PRIMARY, FWNODE_FLAG_LINKS_RESOLVED,
            FWNODE_FLAG_REMOVING, FWNODE_FLAG_FW_CREATED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_swnode_prop_types_distinct() {
        let types = [
            SWNODE_PROP_INT_ARRAY, SWNODE_PROP_STRING,
            SWNODE_PROP_STRING_ARRAY, SWNODE_PROP_REFERENCE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}

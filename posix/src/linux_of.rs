//! `<linux/of.h>` — Open Firmware / Device Tree constants.
//!
//! Device Tree is the standard mechanism for describing hardware
//! on ARM, RISC-V, and PowerPC platforms. The kernel parses the
//! Flattened Device Tree (FDT) blob at boot and builds an in-memory
//! tree of device nodes and properties.

// ---------------------------------------------------------------------------
// FDT (Flattened Device Tree) structure tags
// ---------------------------------------------------------------------------

/// Begin node token.
pub const FDT_BEGIN_NODE: u32 = 0x0000_0001;
/// End node token.
pub const FDT_END_NODE: u32 = 0x0000_0002;
/// Property token.
pub const FDT_PROP: u32 = 0x0000_0003;
/// NOP token (padding).
pub const FDT_NOP: u32 = 0x0000_0004;
/// End of structure block.
pub const FDT_END: u32 = 0x0000_0009;

// ---------------------------------------------------------------------------
// FDT header magic
// ---------------------------------------------------------------------------

/// FDT magic number.
pub const FDT_MAGIC: u32 = 0xD00D_FEED;

// ---------------------------------------------------------------------------
// Standard property names
// ---------------------------------------------------------------------------

/// Compatible string.
pub const OF_PROP_COMPATIBLE: &str = "compatible";
/// Model string.
pub const OF_PROP_MODEL: &str = "model";
/// Phandle.
pub const OF_PROP_PHANDLE: &str = "phandle";
/// Status.
pub const OF_PROP_STATUS: &str = "status";
/// Reg (address+size pairs).
pub const OF_PROP_REG: &str = "reg";
/// Address cells.
pub const OF_PROP_ADDRESS_CELLS: &str = "#address-cells";
/// Size cells.
pub const OF_PROP_SIZE_CELLS: &str = "#size-cells";
/// Interrupt cells.
pub const OF_PROP_INTERRUPT_CELLS: &str = "#interrupt-cells";
/// Interrupt parent.
pub const OF_PROP_INTERRUPT_PARENT: &str = "interrupt-parent";
/// Interrupts.
pub const OF_PROP_INTERRUPTS: &str = "interrupts";
/// Clock names.
pub const OF_PROP_CLOCK_NAMES: &str = "clock-names";
/// Clocks.
pub const OF_PROP_CLOCKS: &str = "clocks";

// ---------------------------------------------------------------------------
// Status property values
// ---------------------------------------------------------------------------

/// Device is operational.
pub const OF_STATUS_OKAY: &str = "okay";
/// Device is disabled.
pub const OF_STATUS_DISABLED: &str = "disabled";
/// Device is reserved.
pub const OF_STATUS_RESERVED: &str = "reserved";
/// Device has failed.
pub const OF_STATUS_FAIL: &str = "fail";

// ---------------------------------------------------------------------------
// FDT version
// ---------------------------------------------------------------------------

/// Current FDT version.
pub const FDT_LAST_SUPPORTED_VERSION: u32 = 17;
/// Minimum compatible version.
pub const FDT_FIRST_SUPPORTED_VERSION: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fdt_tags_distinct() {
        let tags = [FDT_BEGIN_NODE, FDT_END_NODE, FDT_PROP, FDT_NOP, FDT_END];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j]);
            }
        }
    }

    #[test]
    fn test_fdt_magic() {
        assert_eq!(FDT_MAGIC, 0xD00D_FEED);
    }

    #[test]
    fn test_property_names_distinct() {
        let props = [
            OF_PROP_COMPATIBLE,
            OF_PROP_MODEL,
            OF_PROP_PHANDLE,
            OF_PROP_STATUS,
            OF_PROP_REG,
            OF_PROP_ADDRESS_CELLS,
            OF_PROP_SIZE_CELLS,
            OF_PROP_INTERRUPT_CELLS,
            OF_PROP_INTERRUPT_PARENT,
            OF_PROP_INTERRUPTS,
            OF_PROP_CLOCK_NAMES,
            OF_PROP_CLOCKS,
        ];
        for i in 0..props.len() {
            for j in (i + 1)..props.len() {
                assert_ne!(props[i], props[j]);
            }
        }
    }

    #[test]
    fn test_status_values_distinct() {
        let statuses = [
            OF_STATUS_OKAY,
            OF_STATUS_DISABLED,
            OF_STATUS_RESERVED,
            OF_STATUS_FAIL,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_fdt_versions() {
        assert!(FDT_FIRST_SUPPORTED_VERSION <= FDT_LAST_SUPPORTED_VERSION);
    }
}

//! `<linux/property.h>` — Unified device property API constants.
//!
//! The property API provides a firmware-agnostic interface for
//! reading device properties from Device Tree, ACPI, or software
//! nodes. Drivers use `device_property_read_*()` instead of
//! DT-specific or ACPI-specific functions.

// ---------------------------------------------------------------------------
// Property entry types
// ---------------------------------------------------------------------------

/// Boolean property (presence = true).
pub const DEV_PROP_BOOL: u32 = 0;
/// Unsigned 8-bit integer.
pub const DEV_PROP_U8: u32 = 1;
/// Unsigned 16-bit integer.
pub const DEV_PROP_U16: u32 = 2;
/// Unsigned 32-bit integer.
pub const DEV_PROP_U32: u32 = 3;
/// Unsigned 64-bit integer.
pub const DEV_PROP_U64: u32 = 4;
/// String.
pub const DEV_PROP_STRING: u32 = 5;
/// Reference (phandle / ACPI reference).
pub const DEV_PROP_REF: u32 = 6;

// ---------------------------------------------------------------------------
// Connection types (for fwnode_connection)
// ---------------------------------------------------------------------------

/// GPIO connection.
pub const FWNODE_CONN_GPIO: u32 = 0;
/// Clock connection.
pub const FWNODE_CONN_CLK: u32 = 1;
/// Regulator connection.
pub const FWNODE_CONN_REGULATOR: u32 = 2;
/// PHY connection.
pub const FWNODE_CONN_PHY: u32 = 3;
/// PWM connection.
pub const FWNODE_CONN_PWM: u32 = 4;

// ---------------------------------------------------------------------------
// Firmware node types
// ---------------------------------------------------------------------------

/// Device tree node.
pub const FWNODE_TYPE_OF: u32 = 0;
/// ACPI node.
pub const FWNODE_TYPE_ACPI: u32 = 1;
/// Pointer to swnode data.
pub const FWNODE_TYPE_SWNODE: u32 = 2;
/// Platform data node.
pub const FWNODE_TYPE_PDATA: u32 = 3;

// ---------------------------------------------------------------------------
// Common property names (firmware-agnostic)
// ---------------------------------------------------------------------------

/// Rotation property.
pub const PROP_ROTATION: &str = "rotation";
/// Label.
pub const PROP_LABEL: &str = "label";
/// GPIO controller flag.
pub const PROP_GPIO_CONTROLLER: &str = "gpio-controller";
/// GPIO line names.
pub const PROP_GPIO_LINE_NAMES: &str = "gpio-line-names";
/// Wakeup source flag.
pub const PROP_WAKEUP_SOURCE: &str = "wakeup-source";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prop_types_distinct() {
        let types = [
            DEV_PROP_BOOL, DEV_PROP_U8, DEV_PROP_U16,
            DEV_PROP_U32, DEV_PROP_U64, DEV_PROP_STRING,
            DEV_PROP_REF,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_conn_types_distinct() {
        let types = [
            FWNODE_CONN_GPIO, FWNODE_CONN_CLK,
            FWNODE_CONN_REGULATOR, FWNODE_CONN_PHY,
            FWNODE_CONN_PWM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_fwnode_types_distinct() {
        let types = [
            FWNODE_TYPE_OF, FWNODE_TYPE_ACPI,
            FWNODE_TYPE_SWNODE, FWNODE_TYPE_PDATA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_prop_names_distinct() {
        let names = [
            PROP_ROTATION, PROP_LABEL, PROP_GPIO_CONTROLLER,
            PROP_GPIO_LINE_NAMES, PROP_WAKEUP_SOURCE,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }
}

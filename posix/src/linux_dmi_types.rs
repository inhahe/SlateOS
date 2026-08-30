//! `<linux/dmi.h>` — Desktop Management Interface constants.
//!
//! DMI (Desktop Management Interface) / SMBIOS provides hardware
//! inventory data in structured tables: system manufacturer, model,
//! serial number, BIOS version, chassis type, memory configuration,
//! processor info, etc. The kernel exposes this via /sys/class/dmi/id/
//! and makes it available for driver quirk matching (some drivers
//! need to identify specific hardware models to apply workarounds).

// ---------------------------------------------------------------------------
// DMI string indices (well-known identifiers)
// ---------------------------------------------------------------------------

/// BIOS vendor string.
pub const DMI_BIOS_VENDOR: u32 = 0;
/// BIOS version string.
pub const DMI_BIOS_VERSION: u32 = 1;
/// BIOS release date.
pub const DMI_BIOS_DATE: u32 = 2;
/// System manufacturer.
pub const DMI_SYS_VENDOR: u32 = 3;
/// System product name.
pub const DMI_PRODUCT_NAME: u32 = 4;
/// System version.
pub const DMI_PRODUCT_VERSION: u32 = 5;
/// System serial number.
pub const DMI_PRODUCT_SERIAL: u32 = 6;
/// System UUID.
pub const DMI_PRODUCT_UUID: u32 = 7;
/// System SKU number.
pub const DMI_PRODUCT_SKU: u32 = 8;
/// System family.
pub const DMI_PRODUCT_FAMILY: u32 = 9;
/// Board manufacturer.
pub const DMI_BOARD_VENDOR: u32 = 10;
/// Board product name.
pub const DMI_BOARD_NAME: u32 = 11;
/// Board version.
pub const DMI_BOARD_VERSION: u32 = 12;
/// Board serial number.
pub const DMI_BOARD_SERIAL: u32 = 13;
/// Chassis manufacturer.
pub const DMI_CHASSIS_VENDOR: u32 = 14;
/// Chassis type.
pub const DMI_CHASSIS_TYPE: u32 = 15;
/// Chassis version.
pub const DMI_CHASSIS_VERSION: u32 = 16;
/// Chassis serial number.
pub const DMI_CHASSIS_SERIAL: u32 = 17;
/// Maximum DMI string index.
pub const DMI_STRING_MAX: u32 = 18;

// ---------------------------------------------------------------------------
// DMI chassis types
// ---------------------------------------------------------------------------

/// Desktop.
pub const DMI_CHASSIS_DESKTOP: u32 = 3;
/// Low-profile desktop.
pub const DMI_CHASSIS_LOW_PROFILE: u32 = 4;
/// Pizza box.
pub const DMI_CHASSIS_PIZZA_BOX: u32 = 5;
/// Mini tower.
pub const DMI_CHASSIS_MINI_TOWER: u32 = 6;
/// Tower.
pub const DMI_CHASSIS_TOWER: u32 = 7;
/// Portable (laptop/notebook).
pub const DMI_CHASSIS_PORTABLE: u32 = 8;
/// Laptop.
pub const DMI_CHASSIS_LAPTOP: u32 = 9;
/// Notebook.
pub const DMI_CHASSIS_NOTEBOOK: u32 = 10;
/// Handheld.
pub const DMI_CHASSIS_HANDHELD: u32 = 11;
/// Rack mount server.
pub const DMI_CHASSIS_RACK_MOUNT: u32 = 17;
/// Tablet.
pub const DMI_CHASSIS_TABLET: u32 = 30;
/// Convertible.
pub const DMI_CHASSIS_CONVERTIBLE: u32 = 31;
/// Detachable.
pub const DMI_CHASSIS_DETACHABLE: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dmi_strings_ordered() {
        assert!(DMI_BIOS_VENDOR < DMI_STRING_MAX);
        assert!(DMI_PRODUCT_NAME < DMI_STRING_MAX);
        assert!(DMI_CHASSIS_SERIAL < DMI_STRING_MAX);
    }

    #[test]
    fn test_chassis_types_distinct() {
        let types = [
            DMI_CHASSIS_DESKTOP,
            DMI_CHASSIS_LOW_PROFILE,
            DMI_CHASSIS_PIZZA_BOX,
            DMI_CHASSIS_MINI_TOWER,
            DMI_CHASSIS_TOWER,
            DMI_CHASSIS_PORTABLE,
            DMI_CHASSIS_LAPTOP,
            DMI_CHASSIS_NOTEBOOK,
            DMI_CHASSIS_HANDHELD,
            DMI_CHASSIS_RACK_MOUNT,
            DMI_CHASSIS_TABLET,
            DMI_CHASSIS_CONVERTIBLE,
            DMI_CHASSIS_DETACHABLE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}

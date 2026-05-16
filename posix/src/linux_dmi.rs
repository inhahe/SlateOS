//! `<linux/dmi.h>` — DMI/SMBIOS table constants.
//!
//! The Desktop Management Interface (DMI) and System Management BIOS
//! (SMBIOS) provide structured firmware tables describing hardware
//! components (BIOS, system board, chassis, memory, etc.). The kernel
//! parses these at boot; drivers and userspace tools query them for
//! system identification.

// ---------------------------------------------------------------------------
// SMBIOS structure types (from SMBIOS 3.x specification)
// ---------------------------------------------------------------------------

/// BIOS information.
pub const DMI_ENTRY_BIOS: u8 = 0;
/// System information.
pub const DMI_ENTRY_SYSTEM: u8 = 1;
/// Baseboard (module) information.
pub const DMI_ENTRY_BASEBOARD: u8 = 2;
/// Chassis (enclosure) information.
pub const DMI_ENTRY_CHASSIS: u8 = 3;
/// Processor information.
pub const DMI_ENTRY_PROCESSOR: u8 = 4;
/// Memory controller (obsolete).
pub const DMI_ENTRY_MEM_CONTROLLER: u8 = 5;
/// Memory module (obsolete).
pub const DMI_ENTRY_MEM_MODULE: u8 = 6;
/// Cache information.
pub const DMI_ENTRY_CACHE: u8 = 7;
/// Port connector.
pub const DMI_ENTRY_PORT_CONNECTOR: u8 = 8;
/// System slots.
pub const DMI_ENTRY_SYSTEM_SLOT: u8 = 9;
/// On-board device (obsolete).
pub const DMI_ENTRY_ONBOARD_DEVICE: u8 = 10;
/// OEM strings.
pub const DMI_ENTRY_OEM_STRINGS: u8 = 11;
/// Physical memory array.
pub const DMI_ENTRY_PHYS_MEM_ARRAY: u8 = 16;
/// Memory device.
pub const DMI_ENTRY_MEM_DEVICE: u8 = 17;
/// Memory array mapped address.
pub const DMI_ENTRY_MEM_ARRAY_MAPPED_ADDR: u8 = 19;
/// Memory device mapped address.
pub const DMI_ENTRY_MEM_DEV_MAPPED_ADDR: u8 = 20;
/// System boot information.
pub const DMI_ENTRY_SYSTEM_BOOT: u8 = 32;
/// End of table.
pub const DMI_ENTRY_END_OF_TABLE: u8 = 127;

// ---------------------------------------------------------------------------
// DMI string identifiers (for dmi_get_system_info)
// ---------------------------------------------------------------------------

/// BIOS vendor.
pub const DMI_BIOS_VENDOR: u32 = 0;
/// BIOS version.
pub const DMI_BIOS_VERSION: u32 = 1;
/// BIOS date.
pub const DMI_BIOS_DATE: u32 = 2;
/// BIOS release.
pub const DMI_BIOS_RELEASE: u32 = 3;
/// EC firmware release.
pub const DMI_EC_FIRMWARE_RELEASE: u32 = 4;
/// System vendor.
pub const DMI_SYS_VENDOR: u32 = 5;
/// Product name.
pub const DMI_PRODUCT_NAME: u32 = 6;
/// Product version.
pub const DMI_PRODUCT_VERSION: u32 = 7;
/// Product serial number.
pub const DMI_PRODUCT_SERIAL: u32 = 8;
/// Product UUID.
pub const DMI_PRODUCT_UUID: u32 = 9;
/// Product SKU.
pub const DMI_PRODUCT_SKU: u32 = 10;
/// Product family.
pub const DMI_PRODUCT_FAMILY: u32 = 11;
/// Board vendor.
pub const DMI_BOARD_VENDOR: u32 = 12;
/// Board name.
pub const DMI_BOARD_NAME: u32 = 13;
/// Board version.
pub const DMI_BOARD_VERSION: u32 = 14;
/// Board serial number.
pub const DMI_BOARD_SERIAL: u32 = 15;
/// Board asset tag.
pub const DMI_BOARD_ASSET_TAG: u32 = 16;
/// Chassis vendor.
pub const DMI_CHASSIS_VENDOR: u32 = 17;
/// Chassis type.
pub const DMI_CHASSIS_TYPE: u32 = 18;
/// Chassis version.
pub const DMI_CHASSIS_VERSION: u32 = 19;
/// Chassis serial number.
pub const DMI_CHASSIS_SERIAL: u32 = 20;
/// Chassis asset tag.
pub const DMI_CHASSIS_ASSET_TAG: u32 = 21;
/// Total string count.
pub const DMI_STRING_MAX: u32 = 22;

// ---------------------------------------------------------------------------
// Chassis types (SMBIOS Table 17)
// ---------------------------------------------------------------------------

/// Other.
pub const DMI_CHASSIS_TYPE_OTHER: u8 = 1;
/// Unknown.
pub const DMI_CHASSIS_TYPE_UNKNOWN: u8 = 2;
/// Desktop.
pub const DMI_CHASSIS_TYPE_DESKTOP: u8 = 3;
/// Low-profile desktop.
pub const DMI_CHASSIS_TYPE_LOW_PROFILE_DESKTOP: u8 = 4;
/// Pizza box.
pub const DMI_CHASSIS_TYPE_PIZZA_BOX: u8 = 5;
/// Mini tower.
pub const DMI_CHASSIS_TYPE_MINI_TOWER: u8 = 6;
/// Tower.
pub const DMI_CHASSIS_TYPE_TOWER: u8 = 7;
/// Portable.
pub const DMI_CHASSIS_TYPE_PORTABLE: u8 = 8;
/// Laptop.
pub const DMI_CHASSIS_TYPE_LAPTOP: u8 = 9;
/// Notebook.
pub const DMI_CHASSIS_TYPE_NOTEBOOK: u8 = 10;
/// Hand-held.
pub const DMI_CHASSIS_TYPE_HAND_HELD: u8 = 11;
/// Rack mount.
pub const DMI_CHASSIS_TYPE_RACK_MOUNT: u8 = 23;
/// Blade.
pub const DMI_CHASSIS_TYPE_BLADE: u8 = 28;
/// Tablet.
pub const DMI_CHASSIS_TYPE_TABLET: u8 = 30;
/// Convertible.
pub const DMI_CHASSIS_TYPE_CONVERTIBLE: u8 = 31;
/// Detachable.
pub const DMI_CHASSIS_TYPE_DETACHABLE: u8 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_types_distinct() {
        let entries = [
            DMI_ENTRY_BIOS, DMI_ENTRY_SYSTEM, DMI_ENTRY_BASEBOARD,
            DMI_ENTRY_CHASSIS, DMI_ENTRY_PROCESSOR, DMI_ENTRY_MEM_CONTROLLER,
            DMI_ENTRY_MEM_MODULE, DMI_ENTRY_CACHE, DMI_ENTRY_PORT_CONNECTOR,
            DMI_ENTRY_SYSTEM_SLOT, DMI_ENTRY_ONBOARD_DEVICE,
            DMI_ENTRY_OEM_STRINGS, DMI_ENTRY_PHYS_MEM_ARRAY,
            DMI_ENTRY_MEM_DEVICE, DMI_ENTRY_MEM_ARRAY_MAPPED_ADDR,
            DMI_ENTRY_MEM_DEV_MAPPED_ADDR, DMI_ENTRY_SYSTEM_BOOT,
            DMI_ENTRY_END_OF_TABLE,
        ];
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                assert_ne!(entries[i], entries[j]);
            }
        }
    }

    #[test]
    fn test_string_ids_sequential() {
        assert_eq!(DMI_BIOS_VENDOR, 0);
        assert_eq!(DMI_STRING_MAX, 22);
        // All IDs should be less than the max.
        let ids = [
            DMI_BIOS_VENDOR, DMI_BIOS_VERSION, DMI_BIOS_DATE,
            DMI_BIOS_RELEASE, DMI_EC_FIRMWARE_RELEASE,
            DMI_SYS_VENDOR, DMI_PRODUCT_NAME, DMI_PRODUCT_VERSION,
            DMI_PRODUCT_SERIAL, DMI_PRODUCT_UUID, DMI_PRODUCT_SKU,
            DMI_PRODUCT_FAMILY, DMI_BOARD_VENDOR, DMI_BOARD_NAME,
            DMI_BOARD_VERSION, DMI_BOARD_SERIAL, DMI_BOARD_ASSET_TAG,
            DMI_CHASSIS_VENDOR, DMI_CHASSIS_TYPE, DMI_CHASSIS_VERSION,
            DMI_CHASSIS_SERIAL, DMI_CHASSIS_ASSET_TAG,
        ];
        for id in &ids {
            assert!(*id < DMI_STRING_MAX);
        }
    }

    #[test]
    fn test_string_ids_distinct() {
        let ids = [
            DMI_BIOS_VENDOR, DMI_BIOS_VERSION, DMI_BIOS_DATE,
            DMI_BIOS_RELEASE, DMI_EC_FIRMWARE_RELEASE,
            DMI_SYS_VENDOR, DMI_PRODUCT_NAME, DMI_PRODUCT_VERSION,
            DMI_PRODUCT_SERIAL, DMI_PRODUCT_UUID, DMI_PRODUCT_SKU,
            DMI_PRODUCT_FAMILY, DMI_BOARD_VENDOR, DMI_BOARD_NAME,
            DMI_BOARD_VERSION, DMI_BOARD_SERIAL, DMI_BOARD_ASSET_TAG,
            DMI_CHASSIS_VENDOR, DMI_CHASSIS_TYPE, DMI_CHASSIS_VERSION,
            DMI_CHASSIS_SERIAL, DMI_CHASSIS_ASSET_TAG,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_chassis_types_distinct() {
        let types = [
            DMI_CHASSIS_TYPE_OTHER, DMI_CHASSIS_TYPE_UNKNOWN,
            DMI_CHASSIS_TYPE_DESKTOP, DMI_CHASSIS_TYPE_LOW_PROFILE_DESKTOP,
            DMI_CHASSIS_TYPE_PIZZA_BOX, DMI_CHASSIS_TYPE_MINI_TOWER,
            DMI_CHASSIS_TYPE_TOWER, DMI_CHASSIS_TYPE_PORTABLE,
            DMI_CHASSIS_TYPE_LAPTOP, DMI_CHASSIS_TYPE_NOTEBOOK,
            DMI_CHASSIS_TYPE_HAND_HELD, DMI_CHASSIS_TYPE_RACK_MOUNT,
            DMI_CHASSIS_TYPE_BLADE, DMI_CHASSIS_TYPE_TABLET,
            DMI_CHASSIS_TYPE_CONVERTIBLE, DMI_CHASSIS_TYPE_DETACHABLE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_end_of_table() {
        assert_eq!(DMI_ENTRY_END_OF_TABLE, 127);
    }
}

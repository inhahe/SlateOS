//! ACPI NFIT — NVDIMM Firmware Interface Table (persistent memory).
//!
//! Linux exposes NFIT-described persistent memory (Intel Optane DC,
//! NVDIMM-N) via `/sys/bus/nd/devices/`. The `ndctl` tool reads these
//! structures to enumerate regions, namespaces, and DIMMs.

// ---------------------------------------------------------------------------
// Sysfs paths
// ---------------------------------------------------------------------------

pub const SYS_BUS_ND_DEVICES: &str = "/sys/bus/nd/devices";
pub const SYS_FIRMWARE_ACPI_TABLES_NFIT: &str = "/sys/firmware/acpi/tables/NFIT";

// ---------------------------------------------------------------------------
// NFIT table signature ("NFIT" — 4 ASCII bytes)
// ---------------------------------------------------------------------------

pub const ACPI_SIG_NFIT: &str = "NFIT";

// ---------------------------------------------------------------------------
// NFIT subtable type IDs (`enum acpi_nfit_type`)
// ---------------------------------------------------------------------------

pub const ACPI_NFIT_TYPE_SYSTEM_ADDRESS: u16 = 0;
pub const ACPI_NFIT_TYPE_MEMORY_MAP: u16 = 1;
pub const ACPI_NFIT_TYPE_INTERLEAVE: u16 = 2;
pub const ACPI_NFIT_TYPE_SMBIOS: u16 = 3;
pub const ACPI_NFIT_TYPE_CONTROL_REGION: u16 = 4;
pub const ACPI_NFIT_TYPE_DATA_REGION: u16 = 5;
pub const ACPI_NFIT_TYPE_FLUSH_ADDRESS: u16 = 6;
pub const ACPI_NFIT_TYPE_CAPABILITIES: u16 = 7;

// ---------------------------------------------------------------------------
// Memory-mapping flags (`flags` field of `acpi_nfit_memory_map`)
// ---------------------------------------------------------------------------

pub const ACPI_NFIT_MEM_SAVE_FAILED: u16 = 0x0001;
pub const ACPI_NFIT_MEM_RESTORE_FAILED: u16 = 0x0002;
pub const ACPI_NFIT_MEM_FLUSH_FAILED: u16 = 0x0004;
pub const ACPI_NFIT_MEM_NOT_ARMED: u16 = 0x0008;
pub const ACPI_NFIT_MEM_HEALTH_OBSERVED: u16 = 0x0010;
pub const ACPI_NFIT_MEM_HEALTH_ENABLED: u16 = 0x0020;
pub const ACPI_NFIT_MEM_MAP_FAILED: u16 = 0x0040;

// ---------------------------------------------------------------------------
// SPA Range "type GUID" categories (well-known GUIDs in `nfit.h`)
// ---------------------------------------------------------------------------

pub const ACPI_NFIT_GUID_VOLATILE_MEMORY: &str = "7305944F-FDDA-44E3-B16C-3F22D252E5D0";
pub const ACPI_NFIT_GUID_PERSISTENT_MEMORY: &str = "66F0D379-B4F3-4074-AC43-0D3318B78CDB";
pub const ACPI_NFIT_GUID_CONTROL_REGION: &str = "92F701F6-13B4-405D-910B-299367E8234C";
pub const ACPI_NFIT_GUID_DATA_REGION: &str = "91AF0530-5D86-470E-A6B0-0A2DB9408249";
pub const ACPI_NFIT_GUID_VOLATILE_VIRTUAL_DISK: &str = "77AB535A-45FC-624B-5560-F7B281D1F96E";
pub const ACPI_NFIT_GUID_PERSISTENT_VIRTUAL_DISK: &str = "5CEA02C9-4D07-69D3-269F-4496FBE096F9";

// ---------------------------------------------------------------------------
// Health bits (`get_smart` ND_CMD)
// ---------------------------------------------------------------------------

pub const ND_SMART_NON_CRITICAL_HEALTH: u8 = 0x00;
pub const ND_SMART_NON_CRIT_HEALTH: u8 = 0x01;
pub const ND_SMART_CRITICAL_HEALTH: u8 = 0x02;
pub const ND_SMART_FATAL_HEALTH: u8 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_under_sys_bus_and_firmware() {
        assert!(SYS_BUS_ND_DEVICES.starts_with("/sys/bus/nd/"));
        assert!(SYS_FIRMWARE_ACPI_TABLES_NFIT.ends_with("/NFIT"));
    }

    #[test]
    fn test_nfit_signature_4_chars() {
        assert_eq!(ACPI_SIG_NFIT, "NFIT");
        assert_eq!(ACPI_SIG_NFIT.len(), 4);
    }

    #[test]
    fn test_subtable_types_dense_0_to_7() {
        let t = [
            ACPI_NFIT_TYPE_SYSTEM_ADDRESS,
            ACPI_NFIT_TYPE_MEMORY_MAP,
            ACPI_NFIT_TYPE_INTERLEAVE,
            ACPI_NFIT_TYPE_SMBIOS,
            ACPI_NFIT_TYPE_CONTROL_REGION,
            ACPI_NFIT_TYPE_DATA_REGION,
            ACPI_NFIT_TYPE_FLUSH_ADDRESS,
            ACPI_NFIT_TYPE_CAPABILITIES,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_memory_map_flag_bits_single_low_7() {
        let f = [
            ACPI_NFIT_MEM_SAVE_FAILED,
            ACPI_NFIT_MEM_RESTORE_FAILED,
            ACPI_NFIT_MEM_FLUSH_FAILED,
            ACPI_NFIT_MEM_NOT_ARMED,
            ACPI_NFIT_MEM_HEALTH_OBSERVED,
            ACPI_NFIT_MEM_HEALTH_ENABLED,
            ACPI_NFIT_MEM_MAP_FAILED,
        ];
        let mut or = 0u16;
        for v in f {
            assert!(v.is_power_of_two());
            or |= v;
        }
        // 7 flags all live in the low byte (0x7F).
        assert_eq!(or, 0x7F);
    }

    #[test]
    fn test_guid_strings_canonical_36_chars() {
        let g = [
            ACPI_NFIT_GUID_VOLATILE_MEMORY,
            ACPI_NFIT_GUID_PERSISTENT_MEMORY,
            ACPI_NFIT_GUID_CONTROL_REGION,
            ACPI_NFIT_GUID_DATA_REGION,
            ACPI_NFIT_GUID_VOLATILE_VIRTUAL_DISK,
            ACPI_NFIT_GUID_PERSISTENT_VIRTUAL_DISK,
        ];
        for s in g {
            // 8-4-4-4-12 = 36 chars with hyphens.
            assert_eq!(s.len(), 36);
            assert_eq!(s.as_bytes()[8], b'-');
            assert_eq!(s.as_bytes()[13], b'-');
            assert_eq!(s.as_bytes()[18], b'-');
            assert_eq!(s.as_bytes()[23], b'-');
        }
        // GUIDs must be distinct.
        for i in 0..g.len() {
            for j in (i + 1)..g.len() {
                assert_ne!(g[i], g[j]);
            }
        }
    }

    #[test]
    fn test_smart_health_bits_low_3() {
        // NON_CRIT/CRITICAL/FATAL are power-of-two bits 0..2.
        assert_eq!(ND_SMART_NON_CRITICAL_HEALTH, 0);
        assert!(ND_SMART_NON_CRIT_HEALTH.is_power_of_two());
        assert!(ND_SMART_CRITICAL_HEALTH.is_power_of_two());
        assert!(ND_SMART_FATAL_HEALTH.is_power_of_two());
        assert_eq!(
            ND_SMART_NON_CRIT_HEALTH | ND_SMART_CRITICAL_HEALTH | ND_SMART_FATAL_HEALTH,
            0x07
        );
    }
}

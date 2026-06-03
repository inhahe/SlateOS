//! `<linux/ndctl.h>` — NVDIMM control ioctls.
//!
//! NVDIMMs (persistent memory) are configured through libnvdimm's
//! `/dev/ndctl*` and `/dev/nmem*` interfaces. `ndctl(1)`, `daxctl(1)`,
//! and PMDK use these ioctls to read SMART data, manage namespaces,
//! and pass through ACPI DSM vendor commands.

// ---------------------------------------------------------------------------
// `enum nd_cmd_type` — bus and DIMM commands
// ---------------------------------------------------------------------------

pub const ND_CMD_IMPLEMENTED: u32 = 0;

// Bus commands
pub const ND_CMD_ARS_CAP: u32 = 1;
pub const ND_CMD_ARS_START: u32 = 2;
pub const ND_CMD_ARS_STATUS: u32 = 3;
pub const ND_CMD_CLEAR_ERROR: u32 = 4;

// DIMM commands
pub const ND_CMD_SMART: u32 = 1;
pub const ND_CMD_SMART_THRESHOLD: u32 = 2;
pub const ND_CMD_DIMM_FLAGS: u32 = 3;
pub const ND_CMD_GET_CONFIG_SIZE: u32 = 4;
pub const ND_CMD_GET_CONFIG_DATA: u32 = 5;
pub const ND_CMD_SET_CONFIG_DATA: u32 = 6;
pub const ND_CMD_VENDOR_EFFECT_LOG_SIZE: u32 = 7;
pub const ND_CMD_VENDOR_EFFECT_LOG: u32 = 8;
pub const ND_CMD_VENDOR: u32 = 9;
pub const ND_CMD_CALL: u32 = 10;

// ---------------------------------------------------------------------------
// SMART health flags (`nd_smart_payload.flags`)
// ---------------------------------------------------------------------------

pub const ND_SMART_HEALTH_VALID: u32 = 1 << 0;
pub const ND_SMART_SPARES_VALID: u32 = 1 << 1;
pub const ND_SMART_USED_VALID: u32 = 1 << 2;
pub const ND_SMART_TEMP_VALID: u32 = 1 << 3;
pub const ND_SMART_CTEMP_VALID: u32 = 1 << 4;
pub const ND_SMART_ALARM_VALID: u32 = 1 << 9;
pub const ND_SMART_SHUTDOWN_VALID: u32 = 1 << 10;
pub const ND_SMART_SHUTDOWN_COUNT_VALID: u32 = 1 << 11;
pub const ND_SMART_VENDOR_VALID: u32 = 1 << 12;

// ---------------------------------------------------------------------------
// Health status codes (`nd_smart_payload.health`)
// ---------------------------------------------------------------------------

pub const ND_SMART_NON_CRITICAL_HEALTH: u32 = 1 << 0;
pub const ND_SMART_CRITICAL_HEALTH: u32 = 1 << 1;
pub const ND_SMART_FATAL_HEALTH: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Address range scrub (ARS) types
// ---------------------------------------------------------------------------

pub const ND_ARS_VOLATILE: u32 = 1 << 0;
pub const ND_ARS_PERSISTENT: u32 = 1 << 1;
pub const ND_ARS_RETURN_PREV_DATA: u32 = 1 << 2;
pub const ND_CONFIG_LOCKED: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Sysfs interface paths
// ---------------------------------------------------------------------------

pub const SYS_BUS_NDCTL_DEVICES: &str = "/sys/bus/nd/devices";
pub const DEV_NDCTL_PREFIX: &str = "/dev/ndctl";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bus_commands_dense_1_to_4() {
        let b = [
            ND_CMD_ARS_CAP,
            ND_CMD_ARS_START,
            ND_CMD_ARS_STATUS,
            ND_CMD_CLEAR_ERROR,
        ];
        for (i, &v) in b.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_dimm_commands_dense_1_to_10() {
        let d = [
            ND_CMD_SMART,
            ND_CMD_SMART_THRESHOLD,
            ND_CMD_DIMM_FLAGS,
            ND_CMD_GET_CONFIG_SIZE,
            ND_CMD_GET_CONFIG_DATA,
            ND_CMD_SET_CONFIG_DATA,
            ND_CMD_VENDOR_EFFECT_LOG_SIZE,
            ND_CMD_VENDOR_EFFECT_LOG,
            ND_CMD_VENDOR,
            ND_CMD_CALL,
        ];
        for (i, &v) in d.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_smart_valid_flags_single_bit_and_distinct() {
        let s = [
            ND_SMART_HEALTH_VALID,
            ND_SMART_SPARES_VALID,
            ND_SMART_USED_VALID,
            ND_SMART_TEMP_VALID,
            ND_SMART_CTEMP_VALID,
            ND_SMART_ALARM_VALID,
            ND_SMART_SHUTDOWN_VALID,
            ND_SMART_SHUTDOWN_COUNT_VALID,
            ND_SMART_VENDOR_VALID,
        ];
        for v in s {
            assert!(v.is_power_of_two());
        }
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
    }

    #[test]
    fn test_health_severity_levels_single_bit() {
        // Three monotone severity bits 0..2.
        assert!(ND_SMART_NON_CRITICAL_HEALTH.is_power_of_two());
        assert!(ND_SMART_CRITICAL_HEALTH.is_power_of_two());
        assert!(ND_SMART_FATAL_HEALTH.is_power_of_two());
        assert!(ND_SMART_NON_CRITICAL_HEALTH < ND_SMART_CRITICAL_HEALTH);
        assert!(ND_SMART_CRITICAL_HEALTH < ND_SMART_FATAL_HEALTH);
        assert_eq!(
            ND_SMART_NON_CRITICAL_HEALTH
                | ND_SMART_CRITICAL_HEALTH
                | ND_SMART_FATAL_HEALTH,
            0x7
        );
    }

    #[test]
    fn test_ars_types() {
        // ARS scope flags are dense low bits.
        assert_eq!(ND_ARS_VOLATILE, 1);
        assert_eq!(ND_ARS_PERSISTENT, 2);
        assert_eq!(ND_ARS_RETURN_PREV_DATA, 4);
    }

    #[test]
    fn test_sysfs_paths() {
        assert_eq!(SYS_BUS_NDCTL_DEVICES, "/sys/bus/nd/devices");
        assert_eq!(DEV_NDCTL_PREFIX, "/dev/ndctl");
    }
}

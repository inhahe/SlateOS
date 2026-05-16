//! `<linux/ndctl.h>` — Non-Volatile DIMM (NVDIMM) control constants.
//!
//! ndctl manages persistent memory (Intel Optane DCPMM, CXL memory)
//! including namespaces, regions, labels, and health monitoring.

// ---------------------------------------------------------------------------
// NVDIMM commands
// ---------------------------------------------------------------------------

/// Smart health info.
pub const ND_CMD_SMART: u32 = 1;
/// Smart threshold.
pub const ND_CMD_SMART_THRESHOLD: u32 = 2;
/// DIMM flags.
pub const ND_CMD_DIMM_FLAGS: u32 = 3;
/// Get config size.
pub const ND_CMD_GET_CONFIG_SIZE: u32 = 4;
/// Get config data.
pub const ND_CMD_GET_CONFIG_DATA: u32 = 5;
/// Set config data.
pub const ND_CMD_SET_CONFIG_DATA: u32 = 6;
/// Vendor specific.
pub const ND_CMD_VENDOR: u32 = 7;
/// ARS (Address Range Scrub) cap.
pub const ND_CMD_ARS_CAP: u32 = 8;
/// ARS start.
pub const ND_CMD_ARS_START: u32 = 9;
/// ARS status.
pub const ND_CMD_ARS_STATUS: u32 = 10;
/// Clear error.
pub const ND_CMD_CLEAR_ERROR: u32 = 11;

// ---------------------------------------------------------------------------
// Namespace types
// ---------------------------------------------------------------------------

/// I/O mode (block device via BTT).
pub const ND_DEVICE_NAMESPACE_IO: u32 = 1;
/// Persistent memory mode.
pub const ND_DEVICE_NAMESPACE_PMEM: u32 = 2;
/// Block mode.
pub const ND_DEVICE_NAMESPACE_BLK: u32 = 3;
/// DAX device.
pub const ND_DEVICE_DAX_PMEM: u32 = 4;

// ---------------------------------------------------------------------------
// Region types
// ---------------------------------------------------------------------------

/// Persistent memory region.
pub const ND_REGION_PMEM: u32 = 1;
/// Block region.
pub const ND_REGION_BLK: u32 = 2;

// ---------------------------------------------------------------------------
// DIMM flags
// ---------------------------------------------------------------------------

/// DIMM is locked.
pub const ND_DIMM_FLAG_LOCKED: u32 = 1 << 0;
/// DIMM requires passphrase.
pub const ND_DIMM_FLAG_SECURE: u32 = 1 << 1;
/// DIMM is frozen (security state).
pub const ND_DIMM_FLAG_FROZEN: u32 = 1 << 2;
/// DIMM exceeded lifetime.
pub const ND_DIMM_FLAG_OVERTEMP: u32 = 1 << 3;
/// DIMM media is disabled.
pub const ND_DIMM_FLAG_MEDIA_DISABLED: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// ARS (Address Range Scrub) status
// ---------------------------------------------------------------------------

/// ARS not started.
pub const ND_ARS_STATUS_NONE: u32 = 0;
/// ARS in progress.
pub const ND_ARS_STATUS_INPROGRESS: u32 = 1;
/// ARS complete.
pub const ND_ARS_STATUS_COMPLETE: u32 = 2;

// ---------------------------------------------------------------------------
// SMART health flags
// ---------------------------------------------------------------------------

/// SMART alarm trip.
pub const ND_SMART_HEALTH_VALID: u32 = 1 << 0;
/// Spare capacity below threshold.
pub const ND_SMART_SPARES_VALID: u32 = 1 << 1;
/// Used life percentage valid.
pub const ND_SMART_USED_VALID: u32 = 1 << 2;
/// Temperature valid.
pub const ND_SMART_TEMP_VALID: u32 = 1 << 3;
/// Controller temperature valid.
pub const ND_SMART_CTEMP_VALID: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            ND_CMD_SMART, ND_CMD_SMART_THRESHOLD, ND_CMD_DIMM_FLAGS,
            ND_CMD_GET_CONFIG_SIZE, ND_CMD_GET_CONFIG_DATA,
            ND_CMD_SET_CONFIG_DATA, ND_CMD_VENDOR, ND_CMD_ARS_CAP,
            ND_CMD_ARS_START, ND_CMD_ARS_STATUS, ND_CMD_CLEAR_ERROR,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_ns_types_distinct() {
        let types = [
            ND_DEVICE_NAMESPACE_IO, ND_DEVICE_NAMESPACE_PMEM,
            ND_DEVICE_NAMESPACE_BLK, ND_DEVICE_DAX_PMEM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_dimm_flags_are_powers_of_two() {
        let flags = [
            ND_DIMM_FLAG_LOCKED, ND_DIMM_FLAG_SECURE,
            ND_DIMM_FLAG_FROZEN, ND_DIMM_FLAG_OVERTEMP,
            ND_DIMM_FLAG_MEDIA_DISABLED,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two());
        }
    }

    #[test]
    fn test_ars_status_distinct() {
        let statuses = [
            ND_ARS_STATUS_NONE, ND_ARS_STATUS_INPROGRESS,
            ND_ARS_STATUS_COMPLETE,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_smart_flags_are_powers_of_two() {
        let flags = [
            ND_SMART_HEALTH_VALID, ND_SMART_SPARES_VALID,
            ND_SMART_USED_VALID, ND_SMART_TEMP_VALID,
            ND_SMART_CTEMP_VALID,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two());
        }
    }

    #[test]
    fn test_region_types() {
        assert_ne!(ND_REGION_PMEM, ND_REGION_BLK);
    }
}

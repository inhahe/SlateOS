//! `<linux/cxl_mem.h>` — Compute Express Link memory constants.
//!
//! CXL is a high-bandwidth, low-latency interconnect for CPUs,
//! accelerators, and memory expanders. CXL.mem enables coherent
//! memory pooling and sharing across devices.

// ---------------------------------------------------------------------------
// CXL memory commands
// ---------------------------------------------------------------------------

/// Identify device.
pub const CXL_MEM_COMMAND_ID_IDENTIFY: u32 = 1;
/// Get health info.
pub const CXL_MEM_COMMAND_ID_GET_HEALTH_INFO: u32 = 2;
/// Get alert config.
pub const CXL_MEM_COMMAND_ID_GET_ALERT_CONFIG: u32 = 3;
/// Set alert config.
pub const CXL_MEM_COMMAND_ID_SET_ALERT_CONFIG: u32 = 4;
/// Get shutdown state.
pub const CXL_MEM_COMMAND_ID_GET_SHUTDOWN_STATE: u32 = 5;
/// Set shutdown state.
pub const CXL_MEM_COMMAND_ID_SET_SHUTDOWN_STATE: u32 = 6;
/// Get poison list.
pub const CXL_MEM_COMMAND_ID_GET_POISON: u32 = 7;
/// Inject poison.
pub const CXL_MEM_COMMAND_ID_INJECT_POISON: u32 = 8;
/// Clear poison.
pub const CXL_MEM_COMMAND_ID_CLEAR_POISON: u32 = 9;
/// Get scan media capabilities.
pub const CXL_MEM_COMMAND_ID_GET_SCAN_MEDIA_CAPS: u32 = 10;
/// Scan media.
pub const CXL_MEM_COMMAND_ID_SCAN_MEDIA: u32 = 11;
/// Get scan media results.
pub const CXL_MEM_COMMAND_ID_GET_SCAN_MEDIA_RESULTS: u32 = 12;

// ---------------------------------------------------------------------------
// CXL device types
// ---------------------------------------------------------------------------

/// Type 1: CXL.io (PCIe compatible).
pub const CXL_DEVTYPE_CLASSMEM: u32 = 1;
/// Type 2: CXL.io + CXL.mem (accelerator with host-managed device memory).
pub const CXL_DEVTYPE_ACCEL: u32 = 2;
/// Type 3: CXL.io + CXL.mem (memory expander).
pub const CXL_DEVTYPE_EXPANDER: u32 = 3;

// ---------------------------------------------------------------------------
// Health status
// ---------------------------------------------------------------------------

/// Normal health.
pub const CXL_HEALTH_NORMAL: u32 = 0;
/// Non-critical.
pub const CXL_HEALTH_NONCRITICAL: u32 = 1;
/// Critical.
pub const CXL_HEALTH_CRITICAL: u32 = 2;
/// Fatal.
pub const CXL_HEALTH_FATAL: u32 = 3;

// ---------------------------------------------------------------------------
// Media status
// ---------------------------------------------------------------------------

/// Media not ready.
pub const CXL_MEDIA_NOT_READY: u32 = 0;
/// Media ready.
pub const CXL_MEDIA_READY: u32 = 1;
/// Media error.
pub const CXL_MEDIA_ERROR: u32 = 2;
/// Media disabled.
pub const CXL_MEDIA_DISABLED: u32 = 3;

// ---------------------------------------------------------------------------
// Poison source types
// ---------------------------------------------------------------------------

/// Unknown source.
pub const CXL_POISON_SOURCE_UNKNOWN: u32 = 0;
/// External (host injected).
pub const CXL_POISON_SOURCE_EXTERNAL: u32 = 1;
/// Internal (device detected).
pub const CXL_POISON_SOURCE_INTERNAL: u32 = 2;
/// Injected (test/debug).
pub const CXL_POISON_SOURCE_INJECTED: u32 = 3;
/// Vendor-specific.
pub const CXL_POISON_SOURCE_VENDOR: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            CXL_MEM_COMMAND_ID_IDENTIFY,
            CXL_MEM_COMMAND_ID_GET_HEALTH_INFO,
            CXL_MEM_COMMAND_ID_GET_ALERT_CONFIG,
            CXL_MEM_COMMAND_ID_SET_ALERT_CONFIG,
            CXL_MEM_COMMAND_ID_GET_SHUTDOWN_STATE,
            CXL_MEM_COMMAND_ID_SET_SHUTDOWN_STATE,
            CXL_MEM_COMMAND_ID_GET_POISON,
            CXL_MEM_COMMAND_ID_INJECT_POISON,
            CXL_MEM_COMMAND_ID_CLEAR_POISON,
            CXL_MEM_COMMAND_ID_GET_SCAN_MEDIA_CAPS,
            CXL_MEM_COMMAND_ID_SCAN_MEDIA,
            CXL_MEM_COMMAND_ID_GET_SCAN_MEDIA_RESULTS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_dev_types_distinct() {
        let types = [
            CXL_DEVTYPE_CLASSMEM,
            CXL_DEVTYPE_ACCEL,
            CXL_DEVTYPE_EXPANDER,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_health_distinct() {
        let healths = [
            CXL_HEALTH_NORMAL,
            CXL_HEALTH_NONCRITICAL,
            CXL_HEALTH_CRITICAL,
            CXL_HEALTH_FATAL,
        ];
        for i in 0..healths.len() {
            for j in (i + 1)..healths.len() {
                assert_ne!(healths[i], healths[j]);
            }
        }
    }

    #[test]
    fn test_media_distinct() {
        let media = [
            CXL_MEDIA_NOT_READY,
            CXL_MEDIA_READY,
            CXL_MEDIA_ERROR,
            CXL_MEDIA_DISABLED,
        ];
        for i in 0..media.len() {
            for j in (i + 1)..media.len() {
                assert_ne!(media[i], media[j]);
            }
        }
    }

    #[test]
    fn test_poison_sources_distinct() {
        let sources = [
            CXL_POISON_SOURCE_UNKNOWN,
            CXL_POISON_SOURCE_EXTERNAL,
            CXL_POISON_SOURCE_INTERNAL,
            CXL_POISON_SOURCE_INJECTED,
            CXL_POISON_SOURCE_VENDOR,
        ];
        for i in 0..sources.len() {
            for j in (i + 1)..sources.len() {
                assert_ne!(sources[i], sources[j]);
            }
        }
    }
}

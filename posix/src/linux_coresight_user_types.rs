//! `<linux/coresight.h>` — ARM CoreSight tracing subsystem sysfs interface.
//!
//! CoreSight is ARM's on-chip debug & trace infrastructure: ETM/PTM
//! processors generate trace, sinks (ETF/ETR/TPIU) capture it, and
//! the kernel exposes the topology under /sys/bus/coresight.

// ---------------------------------------------------------------------------
// Sysfs root and bus
// ---------------------------------------------------------------------------

pub const CORESIGHT_SYSFS_BUS: &str = "/sys/bus/coresight";
pub const CORESIGHT_SYSFS_DEVICES: &str = "/sys/bus/coresight/devices";

// ---------------------------------------------------------------------------
// Device class strings (CoreSight component types)
// ---------------------------------------------------------------------------

pub const CORESIGHT_DEV_ETM: &str = "etm";
pub const CORESIGHT_DEV_ETB: &str = "etb";
pub const CORESIGHT_DEV_ETF: &str = "tmc_etf";
pub const CORESIGHT_DEV_ETR: &str = "tmc_etr";
pub const CORESIGHT_DEV_TPIU: &str = "tpiu";
pub const CORESIGHT_DEV_STM: &str = "stm";
pub const CORESIGHT_DEV_FUNNEL: &str = "funnel";
pub const CORESIGHT_DEV_REPLICATOR: &str = "replicator";
pub const CORESIGHT_DEV_CATU: &str = "catu";
pub const CORESIGHT_DEV_CTI: &str = "cti";

// ---------------------------------------------------------------------------
// Common sysfs attribute files on every component
// ---------------------------------------------------------------------------

pub const CORESIGHT_ATTR_ENABLE_SOURCE: &str = "enable_source";
pub const CORESIGHT_ATTR_ENABLE_SINK: &str = "enable_sink";
pub const CORESIGHT_ATTR_MGMT: &str = "mgmt";
pub const CORESIGHT_ATTR_CONNECTIONS: &str = "connections";

// ---------------------------------------------------------------------------
// CoreSight component types (numeric, used in perf records)
// ---------------------------------------------------------------------------

pub const CORESIGHT_DEV_TYPE_SOURCE: u32 = 1;
pub const CORESIGHT_DEV_TYPE_LINK: u32 = 2;
pub const CORESIGHT_DEV_TYPE_SINK: u32 = 3;
pub const CORESIGHT_DEV_TYPE_HELPER: u32 = 4;
pub const CORESIGHT_DEV_TYPE_ECT: u32 = 5;

// ---------------------------------------------------------------------------
// Subtypes
// ---------------------------------------------------------------------------

pub const CORESIGHT_DEV_SUBTYPE_SOURCE_PROC: u32 = 1;
pub const CORESIGHT_DEV_SUBTYPE_SOURCE_BUS: u32 = 2;
pub const CORESIGHT_DEV_SUBTYPE_SOURCE_SOFTWARE: u32 = 3;
pub const CORESIGHT_DEV_SUBTYPE_LINK_MERG: u32 = 1;
pub const CORESIGHT_DEV_SUBTYPE_LINK_SPLIT: u32 = 2;
pub const CORESIGHT_DEV_SUBTYPE_LINK_FIFO: u32 = 3;
pub const CORESIGHT_DEV_SUBTYPE_SINK_PORT: u32 = 1;
pub const CORESIGHT_DEV_SUBTYPE_SINK_BUFFER: u32 = 2;
pub const CORESIGHT_DEV_SUBTYPE_SINK_SYSMEM: u32 = 3;

// ---------------------------------------------------------------------------
// ETM trace ID range (7-bit value)
// ---------------------------------------------------------------------------

pub const CORESIGHT_TRACE_ID_MIN: u8 = 0x01;
pub const CORESIGHT_TRACE_ID_MAX: u8 = 0x70;
/// Reserved trace IDs (per ARMv8 spec): 0x00, 0x71-0x7F.
pub const CORESIGHT_TRACE_ID_RESERVED_LO: u8 = 0x00;
pub const CORESIGHT_TRACE_ID_RESERVED_HI_START: u8 = 0x71;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_paths_under_bus_coresight() {
        assert!(CORESIGHT_SYSFS_BUS.starts_with("/sys/bus/coresight"));
        assert!(CORESIGHT_SYSFS_DEVICES.starts_with(CORESIGHT_SYSFS_BUS));
    }

    #[test]
    fn test_dev_classes_distinct() {
        let d = [
            CORESIGHT_DEV_ETM,
            CORESIGHT_DEV_ETB,
            CORESIGHT_DEV_ETF,
            CORESIGHT_DEV_ETR,
            CORESIGHT_DEV_TPIU,
            CORESIGHT_DEV_STM,
            CORESIGHT_DEV_FUNNEL,
            CORESIGHT_DEV_REPLICATOR,
            CORESIGHT_DEV_CATU,
            CORESIGHT_DEV_CTI,
        ];
        for (i, &x) in d.iter().enumerate() {
            for &y in &d[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_attr_files_distinct() {
        let a = [
            CORESIGHT_ATTR_ENABLE_SOURCE,
            CORESIGHT_ATTR_ENABLE_SINK,
            CORESIGHT_ATTR_MGMT,
            CORESIGHT_ATTR_CONNECTIONS,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_dev_types_dense_1_to_5() {
        let t = [
            CORESIGHT_DEV_TYPE_SOURCE,
            CORESIGHT_DEV_TYPE_LINK,
            CORESIGHT_DEV_TYPE_SINK,
            CORESIGHT_DEV_TYPE_HELPER,
            CORESIGHT_DEV_TYPE_ECT,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_subtypes_dense_per_class() {
        // SOURCE subtypes 1..=3
        assert_eq!(CORESIGHT_DEV_SUBTYPE_SOURCE_PROC, 1);
        assert_eq!(CORESIGHT_DEV_SUBTYPE_SOURCE_BUS, 2);
        assert_eq!(CORESIGHT_DEV_SUBTYPE_SOURCE_SOFTWARE, 3);
        // LINK subtypes 1..=3
        assert_eq!(CORESIGHT_DEV_SUBTYPE_LINK_MERG, 1);
        assert_eq!(CORESIGHT_DEV_SUBTYPE_LINK_SPLIT, 2);
        assert_eq!(CORESIGHT_DEV_SUBTYPE_LINK_FIFO, 3);
        // SINK subtypes 1..=3
        assert_eq!(CORESIGHT_DEV_SUBTYPE_SINK_PORT, 1);
        assert_eq!(CORESIGHT_DEV_SUBTYPE_SINK_BUFFER, 2);
        assert_eq!(CORESIGHT_DEV_SUBTYPE_SINK_SYSMEM, 3);
    }

    #[test]
    fn test_trace_id_range_valid() {
        assert_eq!(CORESIGHT_TRACE_ID_MIN, 0x01);
        assert_eq!(CORESIGHT_TRACE_ID_MAX, 0x70);
        assert!(CORESIGHT_TRACE_ID_MIN > CORESIGHT_TRACE_ID_RESERVED_LO);
        assert!(CORESIGHT_TRACE_ID_MAX < CORESIGHT_TRACE_ID_RESERVED_HI_START);
        // Range size is 112 IDs.
        assert_eq!(
            (CORESIGHT_TRACE_ID_MAX - CORESIGHT_TRACE_ID_MIN + 1) as u32,
            112
        );
    }
}

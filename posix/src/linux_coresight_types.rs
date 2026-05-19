//! `<linux/coresight.h>` — CoreSight tracing constants.
//!
//! Constants for ARM CoreSight debug and trace infrastructure
//! covering trace modes, sink types, and source types.

// ---------------------------------------------------------------------------
// CoreSight trace modes
// ---------------------------------------------------------------------------

/// Trace mode: disabled.
pub const CS_MODE_DISABLED: u32 = 0;
/// Trace mode: sysfs (manual control).
pub const CS_MODE_SYSFS: u32 = 1;
/// Trace mode: perf (perf_event integration).
pub const CS_MODE_PERF: u32 = 2;

// ---------------------------------------------------------------------------
// CoreSight device types
// ---------------------------------------------------------------------------

/// Sink device.
pub const CORESIGHT_DEV_TYPE_SINK: u32 = 1;
/// Link device.
pub const CORESIGHT_DEV_TYPE_LINK: u32 = 2;
/// Linksink device.
pub const CORESIGHT_DEV_TYPE_LINKSINK: u32 = 3;
/// Source device.
pub const CORESIGHT_DEV_TYPE_SOURCE: u32 = 4;
/// Helper device.
pub const CORESIGHT_DEV_TYPE_HELPER: u32 = 5;

// ---------------------------------------------------------------------------
// CoreSight sink subtypes
// ---------------------------------------------------------------------------

/// ETB (Embedded Trace Buffer).
pub const CORESIGHT_DEV_SUBTYPE_SINK_ETB: u32 = 1;
/// TPIU (Trace Port Interface Unit).
pub const CORESIGHT_DEV_SUBTYPE_SINK_TPIU: u32 = 2;
/// TMC-ETR (Trace Memory Controller - ETR mode).
pub const CORESIGHT_DEV_SUBTYPE_SINK_TMC_ETR: u32 = 3;
/// TMC-ETB (Trace Memory Controller - ETB mode).
pub const CORESIGHT_DEV_SUBTYPE_SINK_TMC_ETB: u32 = 4;
/// TMC-ETF (Trace Memory Controller - ETF mode).
pub const CORESIGHT_DEV_SUBTYPE_SINK_TMC_ETF: u32 = 5;

// ---------------------------------------------------------------------------
// CoreSight source subtypes
// ---------------------------------------------------------------------------

/// ETM (Embedded Trace Macrocell) source.
pub const CORESIGHT_DEV_SUBTYPE_SOURCE_ETM: u32 = 1;
/// STM (System Trace Macrocell) source.
pub const CORESIGHT_DEV_SUBTYPE_SOURCE_STM: u32 = 2;
/// Software source.
pub const CORESIGHT_DEV_SUBTYPE_SOURCE_SW: u32 = 3;
/// ETE (Embedded Trace Extension) source.
pub const CORESIGHT_DEV_SUBTYPE_SOURCE_ETE: u32 = 4;
/// TRBE (Trace Buffer Extension) source.
pub const CORESIGHT_DEV_SUBTYPE_SOURCE_TRBE: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_modes_distinct() {
        let modes = [CS_MODE_DISABLED, CS_MODE_SYSFS, CS_MODE_PERF];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_dev_types_distinct() {
        let types = [
            CORESIGHT_DEV_TYPE_SINK, CORESIGHT_DEV_TYPE_LINK,
            CORESIGHT_DEV_TYPE_LINKSINK, CORESIGHT_DEV_TYPE_SOURCE,
            CORESIGHT_DEV_TYPE_HELPER,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_sink_subtypes_distinct() {
        let subs = [
            CORESIGHT_DEV_SUBTYPE_SINK_ETB,
            CORESIGHT_DEV_SUBTYPE_SINK_TPIU,
            CORESIGHT_DEV_SUBTYPE_SINK_TMC_ETR,
            CORESIGHT_DEV_SUBTYPE_SINK_TMC_ETB,
            CORESIGHT_DEV_SUBTYPE_SINK_TMC_ETF,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_source_subtypes_distinct() {
        let subs = [
            CORESIGHT_DEV_SUBTYPE_SOURCE_ETM,
            CORESIGHT_DEV_SUBTYPE_SOURCE_STM,
            CORESIGHT_DEV_SUBTYPE_SOURCE_SW,
            CORESIGHT_DEV_SUBTYPE_SOURCE_ETE,
            CORESIGHT_DEV_SUBTYPE_SOURCE_TRBE,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_disabled_is_zero() {
        assert_eq!(CS_MODE_DISABLED, 0);
    }
}

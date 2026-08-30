//! `<linux/coresight.h>` — ARM CoreSight trace constants.
//!
//! CoreSight is ARM's on-chip debug and trace infrastructure.
//! It provides hardware tracing of program execution (ETM),
//! system trace (STM), and cross-trigger synchronization (CTI).
//! Trace data flows through funnels and replicators to sinks
//! (ETB, TMC, TPIU).

// ---------------------------------------------------------------------------
// CoreSight device types
// ---------------------------------------------------------------------------

/// Trace sink (ETB, TMC in circular buffer mode).
pub const CORESIGHT_DEV_TYPE_SINK: u32 = 0;
/// Trace link (funnel, replicator).
pub const CORESIGHT_DEV_TYPE_LINK: u32 = 1;
/// Trace source (ETM, STM).
pub const CORESIGHT_DEV_TYPE_SOURCE: u32 = 2;
/// Helper device.
pub const CORESIGHT_DEV_TYPE_HELPER: u32 = 3;

// ---------------------------------------------------------------------------
// CoreSight device subtypes — sources
// ---------------------------------------------------------------------------

/// ETM (Embedded Trace Macrocell).
pub const CORESIGHT_DEV_SUBTYPE_SOURCE_ETM: u32 = 0;
/// STM (System Trace Macrocell).
pub const CORESIGHT_DEV_SUBTYPE_SOURCE_STM: u32 = 1;
/// Software source.
pub const CORESIGHT_DEV_SUBTYPE_SOURCE_SW: u32 = 2;

// ---------------------------------------------------------------------------
// CoreSight device subtypes — sinks
// ---------------------------------------------------------------------------

/// ETB (Embedded Trace Buffer).
pub const CORESIGHT_DEV_SUBTYPE_SINK_ETB: u32 = 0;
/// TPIU (Trace Port Interface Unit).
pub const CORESIGHT_DEV_SUBTYPE_SINK_TPIU: u32 = 1;
/// TMC-ETR (Trace Memory Controller in ETR mode).
pub const CORESIGHT_DEV_SUBTYPE_SINK_TMC_ETR: u32 = 2;
/// TMC-ETF (Trace Memory Controller as FIFO).
pub const CORESIGHT_DEV_SUBTYPE_SINK_TMC_ETF: u32 = 3;
/// Sysfs buffer sink.
pub const CORESIGHT_DEV_SUBTYPE_SINK_SYSMEM: u32 = 4;

// ---------------------------------------------------------------------------
// CoreSight device subtypes — links
// ---------------------------------------------------------------------------

/// Funnel (merge N inputs → 1 output).
pub const CORESIGHT_DEV_SUBTYPE_LINK_FUNNEL: u32 = 0;
/// Replicator (1 input → N outputs).
pub const CORESIGHT_DEV_SUBTYPE_LINK_REPLICATOR: u32 = 1;

// ---------------------------------------------------------------------------
// ETM configuration flags
// ---------------------------------------------------------------------------

/// Cycle-accurate tracing.
pub const ETM_OPT_CYCACC: u32 = 1 << 0;
/// Branch broadcast.
pub const ETM_OPT_BRANCH_BROADCAST: u32 = 1 << 1;
/// Return stack.
pub const ETM_OPT_RETURN_STACK: u32 = 1 << 2;
/// Timestamp events.
pub const ETM_OPT_TS: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_types_distinct() {
        let types = [
            CORESIGHT_DEV_TYPE_SINK,
            CORESIGHT_DEV_TYPE_LINK,
            CORESIGHT_DEV_TYPE_SOURCE,
            CORESIGHT_DEV_TYPE_HELPER,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_source_subtypes_distinct() {
        let subs = [
            CORESIGHT_DEV_SUBTYPE_SOURCE_ETM,
            CORESIGHT_DEV_SUBTYPE_SOURCE_STM,
            CORESIGHT_DEV_SUBTYPE_SOURCE_SW,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_sink_subtypes_distinct() {
        let subs = [
            CORESIGHT_DEV_SUBTYPE_SINK_ETB,
            CORESIGHT_DEV_SUBTYPE_SINK_TPIU,
            CORESIGHT_DEV_SUBTYPE_SINK_TMC_ETR,
            CORESIGHT_DEV_SUBTYPE_SINK_TMC_ETF,
            CORESIGHT_DEV_SUBTYPE_SINK_SYSMEM,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_link_subtypes_distinct() {
        assert_ne!(
            CORESIGHT_DEV_SUBTYPE_LINK_FUNNEL,
            CORESIGHT_DEV_SUBTYPE_LINK_REPLICATOR
        );
    }

    #[test]
    fn test_etm_opts_powers_of_two() {
        let opts = [
            ETM_OPT_CYCACC,
            ETM_OPT_BRANCH_BROADCAST,
            ETM_OPT_RETURN_STACK,
            ETM_OPT_TS,
        ];
        for opt in &opts {
            assert!(opt.is_power_of_two(), "0x{:x}", opt);
        }
    }

    #[test]
    fn test_etm_opts_no_overlap() {
        let opts = [
            ETM_OPT_CYCACC,
            ETM_OPT_BRANCH_BROADCAST,
            ETM_OPT_RETURN_STACK,
            ETM_OPT_TS,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_eq!(opts[i] & opts[j], 0);
            }
        }
    }
}

//! `<linux/interconnect.h>` — Interconnect framework constants.
//!
//! The interconnect framework models the on-chip communication
//! fabric (buses, crossbars, NoCs) as a graph of nodes. Drivers
//! request bandwidth and latency along paths. The framework
//! aggregates demands and programs bus QoS registers.

// ---------------------------------------------------------------------------
// Bandwidth units
// ---------------------------------------------------------------------------

/// Bytes per second (unit base).
pub const ICC_BW_UNIT_BPS: u32 = 0;
/// Kilobytes per second.
pub const ICC_BW_UNIT_KBPS: u32 = 1;
/// Megabytes per second.
pub const ICC_BW_UNIT_MBPS: u32 = 2;
/// Gigabytes per second.
pub const ICC_BW_UNIT_GBPS: u32 = 3;

// ---------------------------------------------------------------------------
// Node types
// ---------------------------------------------------------------------------

/// Master node (initiator).
pub const ICC_NODE_MASTER: u32 = 0;
/// Slave node (target).
pub const ICC_NODE_SLAVE: u32 = 1;

// ---------------------------------------------------------------------------
// Tag bits (for path differentiation)
// ---------------------------------------------------------------------------

/// Average bandwidth tag.
pub const ICC_TAG_AVG: u32 = 1 << 0;
/// Peak bandwidth tag.
pub const ICC_TAG_PEAK: u32 = 1 << 1;
/// Active-only bandwidth tag.
pub const ICC_TAG_ACTIVE_ONLY: u32 = 1 << 2;
/// Wake-only bandwidth tag.
pub const ICC_TAG_WAKE: u32 = 1 << 3;
/// Sleep bandwidth tag.
pub const ICC_TAG_SLEEP: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Aggregation modes
// ---------------------------------------------------------------------------

/// Sum aggregation.
pub const ICC_AGG_SUM: u32 = 0;
/// Max aggregation.
pub const ICC_AGG_MAX: u32 = 1;

// ---------------------------------------------------------------------------
// Default values
// ---------------------------------------------------------------------------

/// No bandwidth request.
pub const ICC_BW_NONE: u32 = 0;
/// Maximum bandwidth request.
pub const ICC_BW_MAX: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bw_units_distinct() {
        let units = [
            ICC_BW_UNIT_BPS,
            ICC_BW_UNIT_KBPS,
            ICC_BW_UNIT_MBPS,
            ICC_BW_UNIT_GBPS,
        ];
        for i in 0..units.len() {
            for j in (i + 1)..units.len() {
                assert_ne!(units[i], units[j]);
            }
        }
    }

    #[test]
    fn test_node_types_distinct() {
        assert_ne!(ICC_NODE_MASTER, ICC_NODE_SLAVE);
    }

    #[test]
    fn test_tag_bits_powers_of_two() {
        let tags = [
            ICC_TAG_AVG,
            ICC_TAG_PEAK,
            ICC_TAG_ACTIVE_ONLY,
            ICC_TAG_WAKE,
            ICC_TAG_SLEEP,
        ];
        for tag in &tags {
            assert!(tag.is_power_of_two(), "0x{:x}", tag);
        }
    }

    #[test]
    fn test_tag_bits_no_overlap() {
        let tags = [
            ICC_TAG_AVG,
            ICC_TAG_PEAK,
            ICC_TAG_ACTIVE_ONLY,
            ICC_TAG_WAKE,
            ICC_TAG_SLEEP,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_eq!(tags[i] & tags[j], 0);
            }
        }
    }

    #[test]
    fn test_agg_modes_distinct() {
        assert_ne!(ICC_AGG_SUM, ICC_AGG_MAX);
    }

    #[test]
    fn test_bw_defaults() {
        assert_eq!(ICC_BW_NONE, 0);
        assert_eq!(ICC_BW_MAX, u32::MAX);
    }
}

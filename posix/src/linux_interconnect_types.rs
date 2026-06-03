//! `<linux/interconnect.h>` — Interconnect framework constants.
//!
//! The interconnect framework models the on-chip network-on-chip
//! (NoC) fabric that connects SoC components (CPUs, GPUs, memory
//! controllers, peripherals). It allows drivers to request bandwidth
//! and latency QoS for their data paths. The framework aggregates
//! requests and configures interconnect hardware (bus frequencies,
//! link widths, priorities) to meet all active constraints while
//! minimizing power consumption.

// ---------------------------------------------------------------------------
// Interconnect path tags (bandwidth request tags)
// ---------------------------------------------------------------------------

/// Average bandwidth (sustained throughput).
pub const ICC_TAG_AVG: u32 = 0;
/// Peak bandwidth (maximum burst).
pub const ICC_TAG_PEAK: u32 = 1;

// ---------------------------------------------------------------------------
// Interconnect node types
// ---------------------------------------------------------------------------

/// Master node (initiator — CPU, GPU, DMA engine).
pub const ICC_NODE_MASTER: u32 = 0;
/// Slave node (target — memory controller, MMIO peripheral).
pub const ICC_NODE_SLAVE: u32 = 1;
/// Fabric node (switch/router in the NoC).
pub const ICC_NODE_FABRIC: u32 = 2;
/// Gateway node (bridge between different interconnects).
pub const ICC_NODE_GATEWAY: u32 = 3;

// ---------------------------------------------------------------------------
// Interconnect provider flags
// ---------------------------------------------------------------------------

/// Provider supports QoS (quality of service).
pub const ICC_PROVIDER_QOS: u32 = 1 << 0;
/// Provider supports priority levels.
pub const ICC_PROVIDER_PRIORITY: u32 = 1 << 1;
/// Provider data is pre-configured by firmware.
pub const ICC_PROVIDER_PRECONFIGURED: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Interconnect bandwidth units
// ---------------------------------------------------------------------------

/// Bandwidth in kilobytes per second.
pub const ICC_BW_UNIT_KBPS: u32 = 0;
/// Bandwidth in megabytes per second.
pub const ICC_BW_UNIT_MBPS: u32 = 1;

// ---------------------------------------------------------------------------
// Interconnect path states
// ---------------------------------------------------------------------------

/// Path is idle (no active bandwidth request).
pub const ICC_PATH_IDLE: u32 = 0;
/// Path is active (bandwidth allocated).
pub const ICC_PATH_ACTIVE: u32 = 1;
/// Path is being updated (reconfiguration in progress).
pub const ICC_PATH_UPDATING: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tags_distinct() {
        assert_ne!(ICC_TAG_AVG, ICC_TAG_PEAK);
    }

    #[test]
    fn test_node_types_distinct() {
        let types = [
            ICC_NODE_MASTER,
            ICC_NODE_SLAVE,
            ICC_NODE_FABRIC,
            ICC_NODE_GATEWAY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_provider_flags_no_overlap() {
        let flags = [
            ICC_PROVIDER_QOS,
            ICC_PROVIDER_PRIORITY,
            ICC_PROVIDER_PRECONFIGURED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_bw_units_distinct() {
        assert_ne!(ICC_BW_UNIT_KBPS, ICC_BW_UNIT_MBPS);
    }

    #[test]
    fn test_path_states_distinct() {
        let states = [ICC_PATH_IDLE, ICC_PATH_ACTIVE, ICC_PATH_UPDATING];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}

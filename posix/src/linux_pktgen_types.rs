//! `<linux/pktgen.h>` — Packet generator constants.
//!
//! pktgen is a kernel-level packet generator for network
//! performance testing.  These constants define pktgen
//! flags, packet distribution modes, and control commands.

// ---------------------------------------------------------------------------
// Pktgen flags (F_*)
// ---------------------------------------------------------------------------

/// Use IPv6 destination.
pub const F_IPV6: u32 = 1 << 0;
/// IPSRC random.
pub const F_IPSRC_RND: u32 = 1 << 1;
/// IPDST random.
pub const F_IPDST_RND: u32 = 1 << 2;
/// TX size random.
pub const F_TXSIZE_RND: u32 = 1 << 3;
/// UDP source port random.
pub const F_UDPSRC_RND: u32 = 1 << 4;
/// UDP dest port random.
pub const F_UDPDST_RND: u32 = 1 << 5;
/// MAC source random.
pub const F_MACSRC_RND: u32 = 1 << 6;
/// MAC dest random.
pub const F_MACDST_RND: u32 = 1 << 7;
/// MPLS random.
pub const F_MPLS_RND: u32 = 1 << 8;
/// VID (VLAN ID) random.
pub const F_VID_RND: u32 = 1 << 9;
/// SVID (Service VLAN ID) random.
pub const F_SVID_RND: u32 = 1 << 10;
/// Flow sequence.
pub const F_FLOW_SEQ: u32 = 1 << 11;
/// Use queue mapping.
pub const F_QUEUE_MAP_RND: u32 = 1 << 12;
/// Queue map CPU.
pub const F_QUEUE_MAP_CPU: u32 = 1 << 13;
/// Node allocation.
pub const F_NODE: u32 = 1 << 14;
/// UDP checksum.
pub const F_UDPCSUM: u32 = 1 << 15;
/// No timestamp.
pub const F_NO_TIMESTAMP: u32 = 1 << 16;
/// Share SKB.
pub const F_SHARED: u32 = 1 << 17;

// ---------------------------------------------------------------------------
// Pktgen thread commands
// ---------------------------------------------------------------------------

/// Remove device.
pub const PKTGEN_CMD_REMOVE: u32 = 0;
/// Add device.
pub const PKTGEN_CMD_ADD: u32 = 1;
/// Start.
pub const PKTGEN_CMD_START: u32 = 2;
/// Stop.
pub const PKTGEN_CMD_STOP: u32 = 3;
/// Reset.
pub const PKTGEN_CMD_RESET: u32 = 4;

// ---------------------------------------------------------------------------
// Pktgen config sizes
// ---------------------------------------------------------------------------

/// Maximum devices per thread.
pub const MAX_PKTGEN_DEVS: u32 = 32;
/// Maximum label stack depth.
pub const MAX_MPLS_LABELS: u32 = 16;
/// Maximum CFLOWS.
pub const MAX_CFLOWS: u32 = 65536;

// ---------------------------------------------------------------------------
// Pktgen distribution types
// ---------------------------------------------------------------------------

/// Uniform distribution.
pub const DIST_UNIFORM: u32 = 0;
/// Normal (Gaussian) distribution.
pub const DIST_NORMAL: u32 = 1;
/// Pareto distribution.
pub const DIST_PARETO: u32 = 2;
/// Paretonormal distribution.
pub const DIST_PARETONORMAL: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            F_IPV6, F_IPSRC_RND, F_IPDST_RND, F_TXSIZE_RND,
            F_UDPSRC_RND, F_UDPDST_RND, F_MACSRC_RND,
            F_MACDST_RND, F_MPLS_RND, F_VID_RND, F_SVID_RND,
            F_FLOW_SEQ, F_QUEUE_MAP_RND, F_QUEUE_MAP_CPU,
            F_NODE, F_UDPCSUM, F_NO_TIMESTAMP, F_SHARED,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} not power of two");
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            F_IPV6, F_IPSRC_RND, F_IPDST_RND, F_TXSIZE_RND,
            F_UDPSRC_RND, F_UDPDST_RND, F_MACSRC_RND,
            F_MACDST_RND, F_MPLS_RND, F_VID_RND, F_SVID_RND,
            F_FLOW_SEQ, F_QUEUE_MAP_RND, F_QUEUE_MAP_CPU,
            F_NODE, F_UDPCSUM, F_NO_TIMESTAMP, F_SHARED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            PKTGEN_CMD_REMOVE, PKTGEN_CMD_ADD,
            PKTGEN_CMD_START, PKTGEN_CMD_STOP, PKTGEN_CMD_RESET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_dists_distinct() {
        let dists = [DIST_UNIFORM, DIST_NORMAL, DIST_PARETO, DIST_PARETONORMAL];
        for i in 0..dists.len() {
            for j in (i + 1)..dists.len() {
                assert_ne!(dists[i], dists[j]);
            }
        }
    }

    #[test]
    fn test_max_cflows() {
        assert_eq!(MAX_CFLOWS, 65536);
    }

    #[test]
    fn test_uniform_is_zero() {
        assert_eq!(DIST_UNIFORM, 0);
    }
}

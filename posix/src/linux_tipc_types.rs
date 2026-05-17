//! `<linux/tipc.h>` — TIPC (Transparent Inter-Process Communication) constants.
//!
//! TIPC is a cluster networking protocol designed for intra-cluster
//! communication. It provides location-transparent messaging using
//! service addresses instead of IP:port pairs. Nodes discover each
//! other automatically on the same L2 network. Used in telecom
//! systems, high-availability clusters, and real-time applications.

// ---------------------------------------------------------------------------
// Address types
// ---------------------------------------------------------------------------

/// Service address (type, instance).
pub const TIPC_SERVICE_ADDR: u32 = 2;
/// Service range (type, lower, upper).
pub const TIPC_SERVICE_RANGE: u32 = 1;
/// Socket address (node, ref).
pub const TIPC_SOCKET_ADDR: u32 = 3;

// ---------------------------------------------------------------------------
// Socket types
// ---------------------------------------------------------------------------

/// Reliable datagram (unconnected, ordered within service).
pub const SOCK_RDM: u32 = 4;

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

/// TIPC address family.
pub const AF_TIPC: u32 = 30;
/// TIPC protocol family.
pub const PF_TIPC: u32 = 30;

// ---------------------------------------------------------------------------
// Well-known service types
// ---------------------------------------------------------------------------

/// Topology service (subscription notifications).
pub const TIPC_TOP_SRV: u32 = 1;
/// Configuration service.
pub const TIPC_CFG_SRV: u32 = 0;

// ---------------------------------------------------------------------------
// Socket options (SOL_TIPC level)
// ---------------------------------------------------------------------------

/// TIPC socket option level.
pub const SOL_TIPC: u32 = 271;
/// Set importance (priority).
pub const TIPC_IMPORTANCE: u32 = 127;
/// Set source droppable.
pub const TIPC_SRC_DROPPABLE: u32 = 128;
/// Set destination droppable.
pub const TIPC_DEST_DROPPABLE: u32 = 129;
/// Set connection timeout (ms).
pub const TIPC_CONN_TIMEOUT: u32 = 130;
/// Get node identity.
pub const TIPC_NODE_RECVQ_DEPTH: u32 = 131;
/// Get sock recv queue depth.
pub const TIPC_SOCK_RECVQ_DEPTH: u32 = 132;
/// Multicast loop (receive own multicasts).
pub const TIPC_MCAST_BROADCAST: u32 = 133;
/// Group join.
pub const TIPC_GROUP_JOIN: u32 = 135;
/// Group leave.
pub const TIPC_GROUP_LEAVE: u32 = 136;
/// Get sock recv queue used bytes.
pub const TIPC_SOCK_RECVQ_USED: u32 = 137;

// ---------------------------------------------------------------------------
// Message importance levels
// ---------------------------------------------------------------------------

/// Low importance (can be dropped under congestion).
pub const TIPC_LOW_IMPORTANCE: u32 = 0;
/// Medium importance.
pub const TIPC_MEDIUM_IMPORTANCE: u32 = 1;
/// High importance.
pub const TIPC_HIGH_IMPORTANCE: u32 = 2;
/// Critical importance (never dropped).
pub const TIPC_CRITICAL_IMPORTANCE: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_addr_types_distinct() {
        let types = [TIPC_SERVICE_RANGE, TIPC_SERVICE_ADDR, TIPC_SOCKET_ADDR];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_af_pf_match() {
        assert_eq!(AF_TIPC, PF_TIPC);
        assert_eq!(AF_TIPC, 30);
    }

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            TIPC_IMPORTANCE, TIPC_SRC_DROPPABLE,
            TIPC_DEST_DROPPABLE, TIPC_CONN_TIMEOUT,
            TIPC_NODE_RECVQ_DEPTH, TIPC_SOCK_RECVQ_DEPTH,
            TIPC_MCAST_BROADCAST, TIPC_GROUP_JOIN,
            TIPC_GROUP_LEAVE, TIPC_SOCK_RECVQ_USED,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_importance_ordered() {
        assert!(TIPC_LOW_IMPORTANCE < TIPC_MEDIUM_IMPORTANCE);
        assert!(TIPC_MEDIUM_IMPORTANCE < TIPC_HIGH_IMPORTANCE);
        assert!(TIPC_HIGH_IMPORTANCE < TIPC_CRITICAL_IMPORTANCE);
    }

    #[test]
    fn test_services_distinct() {
        assert_ne!(TIPC_TOP_SRV, TIPC_CFG_SRV);
    }
}

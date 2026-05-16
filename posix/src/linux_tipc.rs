//! `<linux/tipc.h>` — Transparent Inter-Process Communication.
//!
//! TIPC is a protocol for intra-cluster communication that provides
//! location-transparent addressing for services across nodes.

// ---------------------------------------------------------------------------
// TIPC address types
// ---------------------------------------------------------------------------

/// Service address.
pub const TIPC_ADDR_NAME: u8 = 1;
/// Service range (name sequence).
pub const TIPC_ADDR_NAMESEQ: u8 = 1;
/// Socket address (node:port).
pub const TIPC_ADDR_ID: u8 = 3;

// ---------------------------------------------------------------------------
// TIPC socket types
// ---------------------------------------------------------------------------

/// Reliable datagram.
pub const SOCK_RDM: i32 = 4;

// ---------------------------------------------------------------------------
// TIPC service types (well-known)
// ---------------------------------------------------------------------------

/// Topology server.
pub const TIPC_TOP_SRV: u32 = 1;
/// Configuration server.
pub const TIPC_CFG_SRV: u32 = 0;
/// Reserved range start.
pub const TIPC_RESERVED_TYPES: u32 = 64;

// ---------------------------------------------------------------------------
// TIPC socket options
// ---------------------------------------------------------------------------

/// TIPC protocol number.
pub const AF_TIPC: i32 = 30;

/// Connection timeout.
pub const TIPC_CONN_TIMEOUT: i32 = 130;
/// Importance level.
pub const TIPC_IMPORTANCE: i32 = 127;
/// Source drop notifications.
pub const TIPC_SRC_DROPPABLE: i32 = 128;
/// Destination drop notifications.
pub const TIPC_DEST_DROPPABLE: i32 = 129;
/// Node scope (local node only).
pub const TIPC_NODE_SCOPE: u32 = 3;
/// Cluster scope.
pub const TIPC_CLUSTER_SCOPE: u32 = 2;
/// Zone scope (not typically used).
pub const TIPC_ZONE_SCOPE: u32 = 1;

// ---------------------------------------------------------------------------
// TIPC importance levels
// ---------------------------------------------------------------------------

/// Low importance.
pub const TIPC_LOW_IMPORTANCE: u32 = 0;
/// Medium importance.
pub const TIPC_MEDIUM_IMPORTANCE: u32 = 1;
/// High importance.
pub const TIPC_HIGH_IMPORTANCE: u32 = 2;
/// Critical importance.
pub const TIPC_CRITICAL_IMPORTANCE: u32 = 3;

// ---------------------------------------------------------------------------
// TIPC topology subscription events
// ---------------------------------------------------------------------------

/// Service published.
pub const TIPC_PUBLISHED: u32 = 1;
/// Service withdrawn.
pub const TIPC_WITHDRAWN: u32 = 2;
/// Subscription timeout.
pub const TIPC_SUBSCR_TIMEOUT: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_tipc() {
        assert_eq!(AF_TIPC, 30);
    }

    #[test]
    fn test_scope_ordering() {
        assert!(TIPC_ZONE_SCOPE < TIPC_CLUSTER_SCOPE);
        assert!(TIPC_CLUSTER_SCOPE < TIPC_NODE_SCOPE);
    }

    #[test]
    fn test_importance_levels() {
        assert_eq!(TIPC_LOW_IMPORTANCE, 0);
        assert_eq!(TIPC_MEDIUM_IMPORTANCE, 1);
        assert_eq!(TIPC_HIGH_IMPORTANCE, 2);
        assert_eq!(TIPC_CRITICAL_IMPORTANCE, 3);
    }

    #[test]
    fn test_subscription_events() {
        assert_eq!(TIPC_PUBLISHED, 1);
        assert_eq!(TIPC_WITHDRAWN, 2);
        assert_eq!(TIPC_SUBSCR_TIMEOUT, 3);
    }

    #[test]
    fn test_socket_options_distinct() {
        let opts = [TIPC_CONN_TIMEOUT, TIPC_IMPORTANCE, TIPC_SRC_DROPPABLE, TIPC_DEST_DROPPABLE];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}

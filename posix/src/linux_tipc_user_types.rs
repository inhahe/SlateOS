//! `<linux/tipc.h>` — Transparent Inter-Process Communication socket API.
//!
//! TIPC is a cluster-aware datagram/stream protocol used by
//! telecom and HPC clusters (Ericsson, Wind River). Userspace
//! talks to TIPC via the `AF_TIPC` socket family using the
//! constants below for `connect(2)`, `bind(2)`, and `sendmsg(2)`
//! destination addressing.

// ---------------------------------------------------------------------------
// Socket address family number (must match linux/socket.h)
// ---------------------------------------------------------------------------

/// AF_TIPC address family.
pub const AF_TIPC: u32 = 30;
/// PF_TIPC protocol family (alias).
pub const PF_TIPC: u32 = AF_TIPC;

// ---------------------------------------------------------------------------
// Socket types
// ---------------------------------------------------------------------------

/// Connectionless / message socket.
pub const SOCK_RDM: u32 = 4;
/// Sequenced packet socket.
pub const SOCK_SEQPACKET: u32 = 5;
/// Connection-oriented stream socket.
pub const SOCK_STREAM: u32 = 1;
/// Datagram socket.
pub const SOCK_DGRAM: u32 = 2;

// ---------------------------------------------------------------------------
// TIPC name address types (struct sockaddr_tipc.addrtype)
// ---------------------------------------------------------------------------

/// Bind/send by service-id ({type,instance}).
pub const TIPC_SERVICE_RANGE: u32 = 1;
/// Bind/send by service address.
pub const TIPC_SERVICE_ADDR: u32 = 2;
/// Bind/send by socket address (low-level).
pub const TIPC_SOCKET_ADDR: u32 = 3;

// ---------------------------------------------------------------------------
// Scope (struct sockaddr_tipc.scope)
// ---------------------------------------------------------------------------

/// Node scope — only sockets on the same node match.
pub const TIPC_NODE_SCOPE: i32 = 3;
/// Cluster scope — sockets on any node in the cluster match.
pub const TIPC_CLUSTER_SCOPE: i32 = 2;
/// Zone scope (deprecated, kept for ABI stability).
pub const TIPC_ZONE_SCOPE: i32 = 1;

// ---------------------------------------------------------------------------
// Reserved well-known service types
// ---------------------------------------------------------------------------

/// Configuration server.
pub const TIPC_CFG_SRV: u32 = 0;
/// Topology service (publication / withdrawal events).
pub const TIPC_TOP_SRV: u32 = 1;
/// Link-state events.
pub const TIPC_LINK_STATE: u32 = 2;
/// Reserved high-end of system service types.
pub const TIPC_RESERVED_TYPES: u32 = 64;

// ---------------------------------------------------------------------------
// Socket options (setsockopt level = SOL_TIPC)
// ---------------------------------------------------------------------------

/// SOL_TIPC value.
pub const SOL_TIPC: u32 = 271;
/// Importance level (low/medium/high/critical).
pub const TIPC_IMPORTANCE: u32 = 127;
/// Source droppable (kernel may drop on receive-buffer overflow).
pub const TIPC_SRC_DROPPABLE: u32 = 128;
/// Destination droppable.
pub const TIPC_DEST_DROPPABLE: u32 = 129;
/// Connection-timeout in ms.
pub const TIPC_CONN_TIMEOUT: u32 = 130;
/// Get node id (read-only).
pub const TIPC_NODE_RECVQ_DEPTH: u32 = 131;
/// Get socket receive queue depth.
pub const TIPC_SOCK_RECVQ_DEPTH: u32 = 132;
/// Get MTU.
pub const TIPC_MCAST_BROADCAST: u32 = 133;
/// Use replicast not broadcast.
pub const TIPC_MCAST_REPLICAST: u32 = 134;
/// Get group join descriptor.
pub const TIPC_GROUP_JOIN: u32 = 135;
/// Leave group.
pub const TIPC_GROUP_LEAVE: u32 = 136;

// ---------------------------------------------------------------------------
// Importance levels
// ---------------------------------------------------------------------------

/// Low importance.
pub const TIPC_LOW_IMPORTANCE: u32 = 0;
/// Medium importance (default).
pub const TIPC_MEDIUM_IMPORTANCE: u32 = 1;
/// High importance.
pub const TIPC_HIGH_IMPORTANCE: u32 = 2;
/// Critical importance.
pub const TIPC_CRITICAL_IMPORTANCE: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_tipc_value() {
        // AF_TIPC has been 30 since Linux 2.6.16.
        assert_eq!(AF_TIPC, 30);
        assert_eq!(PF_TIPC, AF_TIPC);
    }

    #[test]
    fn test_addrtypes_distinct() {
        let a = [TIPC_SERVICE_RANGE, TIPC_SERVICE_ADDR, TIPC_SOCKET_ADDR];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
    }

    #[test]
    fn test_scope_values_distinct_and_node_widest() {
        // NODE > CLUSTER > ZONE — narrower-first ordering, since
        // smaller numerical scope means "more nodes can see this".
        assert!(TIPC_NODE_SCOPE > TIPC_CLUSTER_SCOPE);
        assert!(TIPC_CLUSTER_SCOPE > TIPC_ZONE_SCOPE);
    }

    #[test]
    fn test_reserved_services_distinct() {
        let s = [TIPC_CFG_SRV, TIPC_TOP_SRV, TIPC_LINK_STATE];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
            // Reserved services live in 0..64.
            assert!(s[i] < TIPC_RESERVED_TYPES);
        }
    }

    #[test]
    fn test_sockopts_distinct_and_above_127() {
        let o = [
            TIPC_IMPORTANCE,
            TIPC_SRC_DROPPABLE,
            TIPC_DEST_DROPPABLE,
            TIPC_CONN_TIMEOUT,
            TIPC_NODE_RECVQ_DEPTH,
            TIPC_SOCK_RECVQ_DEPTH,
            TIPC_MCAST_BROADCAST,
            TIPC_MCAST_REPLICAST,
            TIPC_GROUP_JOIN,
            TIPC_GROUP_LEAVE,
        ];
        for i in 0..o.len() {
            for j in (i + 1)..o.len() {
                assert_ne!(o[i], o[j]);
            }
            // TIPC socket option numbers start at 127 to avoid
            // collision with SOL_SOCKET options.
            assert!(o[i] >= 127);
        }
    }

    #[test]
    fn test_importance_levels_dense() {
        let imp = [
            TIPC_LOW_IMPORTANCE,
            TIPC_MEDIUM_IMPORTANCE,
            TIPC_HIGH_IMPORTANCE,
            TIPC_CRITICAL_IMPORTANCE,
        ];
        for (i, &v) in imp.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}

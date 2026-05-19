//! `<linux/tipc.h>` — Additional TIPC constants (batch 3).
//!
//! Supplementary TIPC constants covering node states,
//! link states, and bearer types.

// ---------------------------------------------------------------------------
// TIPC node states
// ---------------------------------------------------------------------------

/// Node is up.
pub const TIPC_NODE_UP: u32 = 0;
/// Node is down.
pub const TIPC_NODE_DOWN: u32 = 1;
/// Node state: coming up.
pub const TIPC_NODE_COMING_UP: u32 = 2;
/// Node state: going down.
pub const TIPC_NODE_GOING_DOWN: u32 = 3;

// ---------------------------------------------------------------------------
// TIPC link states
// ---------------------------------------------------------------------------

/// Link: working/active.
pub const TIPC_LINK_WORKING: u32 = 0;
/// Link: probing.
pub const TIPC_LINK_PROBING: u32 = 1;
/// Link: reset.
pub const TIPC_LINK_RESET: u32 = 2;
/// Link: activating.
pub const TIPC_LINK_ACTIVATING: u32 = 3;
/// Link: establishing.
pub const TIPC_LINK_ESTABLISHING: u32 = 4;

// ---------------------------------------------------------------------------
// TIPC bearer types
// ---------------------------------------------------------------------------

/// Ethernet bearer.
pub const TIPC_MEDIA_TYPE_ETH: u32 = 1;
/// InfiniBand bearer.
pub const TIPC_MEDIA_TYPE_IB: u32 = 2;
/// UDP bearer.
pub const TIPC_MEDIA_TYPE_UDP: u32 = 3;

// ---------------------------------------------------------------------------
// TIPC publication scope
// ---------------------------------------------------------------------------

/// Zone scope.
pub const TIPC_ZONE_SCOPE: u32 = 1;
/// Cluster scope.
pub const TIPC_CLUSTER_SCOPE: u32 = 2;
/// Node scope.
pub const TIPC_NODE_SCOPE: u32 = 3;

// ---------------------------------------------------------------------------
// TIPC group event types
// ---------------------------------------------------------------------------

/// Member joined.
pub const TIPC_GRP_JOIN_MSG: u32 = 0;
/// Member left.
pub const TIPC_GRP_LEAVE_MSG: u32 = 1;
/// Member reclaim.
pub const TIPC_GRP_RECLAIM_MSG: u32 = 2;

// ---------------------------------------------------------------------------
// TIPC socket options
// ---------------------------------------------------------------------------

/// Importance level.
pub const TIPC_IMPORTANCE: u32 = 127;
/// Source drop capable.
pub const TIPC_SRC_DROPPABLE: u32 = 128;
/// Destination drop capable.
pub const TIPC_DEST_DROPPABLE: u32 = 129;
/// Connection timeout.
pub const TIPC_CONN_TIMEOUT: u32 = 130;
/// Node delay.
pub const TIPC_NODE_RECVQ_DEPTH: u32 = 131;
/// Socket receive queue depth.
pub const TIPC_SOCK_RECVQ_DEPTH: u32 = 132;
/// Socket receive queue used.
pub const TIPC_SOCK_RECVQ_USED: u32 = 133;
/// Group join.
pub const TIPC_GROUP_JOIN: u32 = 135;
/// Group leave.
pub const TIPC_GROUP_LEAVE: u32 = 136;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_states_distinct() {
        let states = [
            TIPC_NODE_UP, TIPC_NODE_DOWN,
            TIPC_NODE_COMING_UP, TIPC_NODE_GOING_DOWN,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_link_states_distinct() {
        let states = [
            TIPC_LINK_WORKING, TIPC_LINK_PROBING,
            TIPC_LINK_RESET, TIPC_LINK_ACTIVATING,
            TIPC_LINK_ESTABLISHING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_media_types_distinct() {
        let types = [
            TIPC_MEDIA_TYPE_ETH, TIPC_MEDIA_TYPE_IB,
            TIPC_MEDIA_TYPE_UDP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_scopes_distinct() {
        let scopes = [TIPC_ZONE_SCOPE, TIPC_CLUSTER_SCOPE, TIPC_NODE_SCOPE];
        for i in 0..scopes.len() {
            for j in (i + 1)..scopes.len() {
                assert_ne!(scopes[i], scopes[j]);
            }
        }
    }

    #[test]
    fn test_group_events_distinct() {
        let events = [
            TIPC_GRP_JOIN_MSG, TIPC_GRP_LEAVE_MSG,
            TIPC_GRP_RECLAIM_MSG,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}

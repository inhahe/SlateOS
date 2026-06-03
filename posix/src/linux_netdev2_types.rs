//! `<linux/netdevice.h>` — Additional netdevice constants.
//!
//! Supplementary netdevice constants covering interface flags,
//! hardware features, and queue states.

// ---------------------------------------------------------------------------
// Net device interface flags (additional IFF_*)
// ---------------------------------------------------------------------------

/// Interface is up.
pub const IFF_UP2: u32 = 1 << 0;
/// Broadcast address valid.
pub const IFF_BROADCAST2: u32 = 1 << 1;
/// Debug flag.
pub const IFF_DEBUG2: u32 = 1 << 2;
/// Loopback.
pub const IFF_LOOPBACK2: u32 = 1 << 3;
/// Point-to-point link.
pub const IFF_POINTOPOINT2: u32 = 1 << 4;
/// No trailers.
pub const IFF_NOTRAILERS2: u32 = 1 << 5;
/// Running.
pub const IFF_RUNNING2: u32 = 1 << 6;
/// No ARP.
pub const IFF_NOARP2: u32 = 1 << 7;
/// Promiscuous mode.
pub const IFF_PROMISC2: u32 = 1 << 8;
/// All multicast.
pub const IFF_ALLMULTI2: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// Net device private flags
// ---------------------------------------------------------------------------

/// Device is master.
pub const IFF_XMIT_DST_RELEASE: u32 = 1 << 0;
/// Don't change features.
pub const IFF_DONT_BRIDGE: u32 = 1 << 1;
/// Live address change.
pub const IFF_LIVE_ADDR_CHANGE: u32 = 1 << 2;
/// Macvlan port.
pub const IFF_MACVLAN_PORT: u32 = 1 << 3;
/// Team port.
pub const IFF_TEAM_PORT: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Net device transmit queue states
// ---------------------------------------------------------------------------

/// Queue is stopped.
pub const NETDEV_QUEUE_STATE_DRV_XOFF: u32 = 0;
/// Stack stopped queue.
pub const NETDEV_QUEUE_STATE_STACK_XOFF: u32 = 1;
/// Frozen.
pub const NETDEV_QUEUE_STATE_FROZEN: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iff_flags_no_overlap() {
        let flags = [
            IFF_UP2,
            IFF_BROADCAST2,
            IFF_DEBUG2,
            IFF_LOOPBACK2,
            IFF_POINTOPOINT2,
            IFF_NOTRAILERS2,
            IFF_RUNNING2,
            IFF_NOARP2,
            IFF_PROMISC2,
            IFF_ALLMULTI2,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_priv_flags_no_overlap() {
        let flags = [
            IFF_XMIT_DST_RELEASE,
            IFF_DONT_BRIDGE,
            IFF_LIVE_ADDR_CHANGE,
            IFF_MACVLAN_PORT,
            IFF_TEAM_PORT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_queue_states_distinct() {
        let states = [
            NETDEV_QUEUE_STATE_DRV_XOFF,
            NETDEV_QUEUE_STATE_STACK_XOFF,
            NETDEV_QUEUE_STATE_FROZEN,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}

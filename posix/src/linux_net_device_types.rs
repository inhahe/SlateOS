//! `<linux/netdevice.h>` — Network device type and flag constants.
//!
//! Network devices (net_device) represent network interfaces in the
//! kernel. Each device has a type (Ethernet, loopback, tunnel, etc.),
//! flags (UP, RUNNING, PROMISC, etc.), and various operational states.
//! The net_device structure is one of the largest in the kernel,
//! containing statistics, queuing disciplines, hardware features,
//! and protocol-specific state.

// ---------------------------------------------------------------------------
// Network device types (ARPHRD_* from if_arp.h)
// ---------------------------------------------------------------------------

/// Ethernet (10/100/1000/10G/etc).
pub const ARPHRD_ETHER: u32 = 1;
/// Loopback device.
pub const ARPHRD_LOOPBACK: u32 = 772;
/// Point-to-point tunnel (IP-in-IP, GRE).
pub const ARPHRD_TUNNEL: u32 = 768;
/// IPv6-in-IPv4 tunnel.
pub const ARPHRD_TUNNEL6: u32 = 769;
/// IEEE 802.11 (WiFi).
pub const ARPHRD_IEEE80211: u32 = 801;
/// IEEE 802.15.4 (Zigbee/Thread/6LoWPAN).
pub const ARPHRD_IEEE802154: u32 = 804;
/// SLIP (Serial Line IP).
pub const ARPHRD_SLIP: u32 = 256;
/// PPP (Point-to-Point Protocol).
pub const ARPHRD_PPP: u32 = 512;
/// Virtual/software device (veth, bridge, bond).
pub const ARPHRD_NONE: u32 = 0xFFFE;

// ---------------------------------------------------------------------------
// Network device flags (IFF_*)
// ---------------------------------------------------------------------------

/// Interface is up (administratively enabled).
pub const IFF_UP: u32 = 0x0001;
/// Interface is running (carrier detected).
pub const IFF_RUNNING: u32 = 0x0040;
/// Broadcast address is valid.
pub const IFF_BROADCAST: u32 = 0x0002;
/// Interface is loopback.
pub const IFF_LOOPBACK: u32 = 0x0008;
/// Interface is point-to-point.
pub const IFF_POINTOPOINT: u32 = 0x0010;
/// No ARP (don't use ARP protocol).
pub const IFF_NOARP: u32 = 0x0080;
/// Promiscuous mode (receive all packets).
pub const IFF_PROMISC: u32 = 0x0100;
/// All-multicast mode (receive all multicast packets).
pub const IFF_ALLMULTI: u32 = 0x0200;
/// Master of a bonding/bridge group.
pub const IFF_MASTER: u32 = 0x0400;
/// Slave of a bonding/bridge group.
pub const IFF_SLAVE: u32 = 0x0800;
/// Supports multicast.
pub const IFF_MULTICAST: u32 = 0x1000;

// ---------------------------------------------------------------------------
// Network device operational states
// ---------------------------------------------------------------------------

/// Unknown state (just registered).
pub const IF_OPER_UNKNOWN: u32 = 0;
/// Not present (hardware removed).
pub const IF_OPER_NOTPRESENT: u32 = 1;
/// Down (administratively disabled).
pub const IF_OPER_DOWN: u32 = 2;
/// Lower layer is down (e.g., no carrier).
pub const IF_OPER_LOWERLAYERDOWN: u32 = 3;
/// Testing mode.
pub const IF_OPER_TESTING: u32 = 4;
/// Dormant (waiting for external event, 802.1X).
pub const IF_OPER_DORMANT: u32 = 5;
/// Up and operational.
pub const IF_OPER_UP: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_types_distinct() {
        let types = [
            ARPHRD_ETHER,
            ARPHRD_LOOPBACK,
            ARPHRD_TUNNEL,
            ARPHRD_TUNNEL6,
            ARPHRD_IEEE80211,
            ARPHRD_IEEE802154,
            ARPHRD_SLIP,
            ARPHRD_PPP,
            ARPHRD_NONE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_iff_flags_no_overlap() {
        let flags = [
            IFF_UP,
            IFF_BROADCAST,
            IFF_LOOPBACK,
            IFF_POINTOPOINT,
            IFF_RUNNING,
            IFF_NOARP,
            IFF_PROMISC,
            IFF_ALLMULTI,
            IFF_MASTER,
            IFF_SLAVE,
            IFF_MULTICAST,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_oper_states_distinct() {
        let states = [
            IF_OPER_UNKNOWN,
            IF_OPER_NOTPRESENT,
            IF_OPER_DOWN,
            IF_OPER_LOWERLAYERDOWN,
            IF_OPER_TESTING,
            IF_OPER_DORMANT,
            IF_OPER_UP,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}

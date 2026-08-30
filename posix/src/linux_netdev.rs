//! `<linux/netdevice.h>` supplemental — Network device constants.
//!
//! Flags and constants for network device management, supplementing
//! the existing if_link and rtnetlink modules with device-level
//! configuration constants.

// ---------------------------------------------------------------------------
// Net device flags (IFF_*)
// ---------------------------------------------------------------------------

/// Interface is up.
pub const IFF_UP: u32 = 1 << 0;
/// Broadcast capable.
pub const IFF_BROADCAST: u32 = 1 << 1;
/// Debug mode.
pub const IFF_DEBUG: u32 = 1 << 2;
/// Loopback.
pub const IFF_LOOPBACK: u32 = 1 << 3;
/// Point-to-point link.
pub const IFF_POINTOPOINT: u32 = 1 << 4;
/// Avoid trailers.
pub const IFF_NOTRAILERS: u32 = 1 << 5;
/// Resources allocated.
pub const IFF_RUNNING: u32 = 1 << 6;
/// No ARP.
pub const IFF_NOARP: u32 = 1 << 7;
/// Promiscuous mode.
pub const IFF_PROMISC: u32 = 1 << 8;
/// Receive all multicast.
pub const IFF_ALLMULTI: u32 = 1 << 9;
/// Master of a load balance cluster.
pub const IFF_MASTER: u32 = 1 << 10;
/// Slave of a load balance cluster.
pub const IFF_SLAVE: u32 = 1 << 11;
/// Supports multicast.
pub const IFF_MULTICAST: u32 = 1 << 12;
/// Can set media type.
pub const IFF_PORTSEL: u32 = 1 << 13;
/// Auto media selection active.
pub const IFF_AUTOMEDIA: u32 = 1 << 14;
/// Dial-on-demand.
pub const IFF_DYNAMIC: u32 = 1 << 15;
/// Lower up (L1 carrier).
pub const IFF_LOWER_UP: u32 = 1 << 16;
/// Driver signals dormant.
pub const IFF_DORMANT: u32 = 1 << 17;
/// Echo sent packets.
pub const IFF_ECHO: u32 = 1 << 18;

// ---------------------------------------------------------------------------
// Net device TX queue states
// ---------------------------------------------------------------------------

/// Queue is stopped.
pub const NETDEV_TX_OK: i32 = 0;
/// Queue is busy.
pub const NETDEV_TX_BUSY: i32 = 1;

// ---------------------------------------------------------------------------
// Maximum values
// ---------------------------------------------------------------------------

/// Maximum interface name length.
pub const IFNAMSIZ: usize = 16;
/// Maximum hardware address length.
pub const MAX_ADDR_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iff_flags_are_powers_of_two() {
        let flags = [
            IFF_UP,
            IFF_BROADCAST,
            IFF_DEBUG,
            IFF_LOOPBACK,
            IFF_POINTOPOINT,
            IFF_NOTRAILERS,
            IFF_RUNNING,
            IFF_NOARP,
            IFF_PROMISC,
            IFF_ALLMULTI,
            IFF_MASTER,
            IFF_SLAVE,
            IFF_MULTICAST,
            IFF_PORTSEL,
            IFF_AUTOMEDIA,
            IFF_DYNAMIC,
            IFF_LOWER_UP,
            IFF_DORMANT,
            IFF_ECHO,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_iff_flags_no_overlap() {
        let flags = [
            IFF_UP,
            IFF_BROADCAST,
            IFF_DEBUG,
            IFF_LOOPBACK,
            IFF_POINTOPOINT,
            IFF_NOTRAILERS,
            IFF_RUNNING,
            IFF_NOARP,
            IFF_PROMISC,
            IFF_ALLMULTI,
            IFF_MASTER,
            IFF_SLAVE,
            IFF_MULTICAST,
            IFF_PORTSEL,
            IFF_AUTOMEDIA,
            IFF_DYNAMIC,
            IFF_LOWER_UP,
            IFF_DORMANT,
            IFF_ECHO,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_tx_results() {
        assert_eq!(NETDEV_TX_OK, 0);
        assert_eq!(NETDEV_TX_BUSY, 1);
    }

    #[test]
    fn test_ifnamsiz() {
        assert_eq!(IFNAMSIZ, 16);
    }

    #[test]
    fn test_max_addr_len() {
        assert_eq!(MAX_ADDR_LEN, 32);
    }
}

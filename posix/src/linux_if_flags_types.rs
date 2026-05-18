//! `<net/if.h>` — Network interface flag constants (IFF_*).
//!
//! These flags describe the state and capabilities of a network
//! interface. They are returned by `ioctl(SIOCGIFFLAGS)` and
//! set by `ioctl(SIOCSIFFLAGS)`.

// ---------------------------------------------------------------------------
// Interface flags (IFF_*)
// ---------------------------------------------------------------------------

/// Interface is up (running).
pub const IFF_UP: u32 = 1 << 0;
/// Broadcast address valid.
pub const IFF_BROADCAST: u32 = 1 << 1;
/// Turn on debugging.
pub const IFF_DEBUG: u32 = 1 << 2;
/// Is a loopback interface.
pub const IFF_LOOPBACK: u32 = 1 << 3;
/// Is a point-to-point link.
pub const IFF_POINTOPOINT: u32 = 1 << 4;
/// Avoid use of trailers.
pub const IFF_NOTRAILERS: u32 = 1 << 5;
/// Interface is running.
pub const IFF_RUNNING: u32 = 1 << 6;
/// No ARP protocol.
pub const IFF_NOARP: u32 = 1 << 7;
/// Receive all packets (promiscuous mode).
pub const IFF_PROMISC: u32 = 1 << 8;
/// Receive all multicast packets.
pub const IFF_ALLMULTI: u32 = 1 << 9;
/// Master of a load balancer.
pub const IFF_MASTER: u32 = 1 << 10;
/// Slave of a load balancer.
pub const IFF_SLAVE: u32 = 1 << 11;
/// Supports multicast.
pub const IFF_MULTICAST: u32 = 1 << 12;
/// Can set media type.
pub const IFF_PORTSEL: u32 = 1 << 13;
/// Auto media select active.
pub const IFF_AUTOMEDIA: u32 = 1 << 14;
/// Dialup device with changing addresses.
pub const IFF_DYNAMIC: u32 = 1 << 15;
/// Driver signals L1 up.
pub const IFF_LOWER_UP: u32 = 1 << 16;
/// Driver signals dormant.
pub const IFF_DORMANT: u32 = 1 << 17;
/// Echo sent packets.
pub const IFF_ECHO: u32 = 1 << 18;

// ---------------------------------------------------------------------------
// Interface ioctl commands
// ---------------------------------------------------------------------------

/// Get interface flags.
pub const SIOCGIFFLAGS: u32 = 0x8913;
/// Set interface flags.
pub const SIOCSIFFLAGS: u32 = 0x8914;
/// Get interface address.
pub const SIOCGIFADDR: u32 = 0x8915;
/// Set interface address.
pub const SIOCSIFADDR: u32 = 0x8916;
/// Get interface netmask.
pub const SIOCGIFNETMASK: u32 = 0x891B;
/// Set interface netmask.
pub const SIOCSIFNETMASK: u32 = 0x891C;
/// Get interface hardware address.
pub const SIOCGIFHWADDR: u32 = 0x8927;
/// Get interface index.
pub const SIOCGIFINDEX: u32 = 0x8933;
/// Get interface name.
pub const SIOCGIFNAME: u32 = 0x8910;
/// Get interface MTU.
pub const SIOCGIFMTU: u32 = 0x8921;
/// Set interface MTU.
pub const SIOCSIFMTU: u32 = 0x8922;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            IFF_UP, IFF_BROADCAST, IFF_DEBUG, IFF_LOOPBACK,
            IFF_POINTOPOINT, IFF_NOTRAILERS, IFF_RUNNING,
            IFF_NOARP, IFF_PROMISC, IFF_ALLMULTI,
            IFF_MASTER, IFF_SLAVE, IFF_MULTICAST,
            IFF_PORTSEL, IFF_AUTOMEDIA, IFF_DYNAMIC,
            IFF_LOWER_UP, IFF_DORMANT, IFF_ECHO,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_flags_power_of_two() {
        let flags = [
            IFF_UP, IFF_BROADCAST, IFF_DEBUG, IFF_LOOPBACK,
            IFF_POINTOPOINT, IFF_NOTRAILERS, IFF_RUNNING,
            IFF_NOARP, IFF_PROMISC, IFF_ALLMULTI,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_iff_up() {
        assert_eq!(IFF_UP, 1);
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            SIOCGIFFLAGS, SIOCSIFFLAGS, SIOCGIFADDR, SIOCSIFADDR,
            SIOCGIFNETMASK, SIOCSIFNETMASK, SIOCGIFHWADDR,
            SIOCGIFINDEX, SIOCGIFNAME, SIOCGIFMTU, SIOCSIFMTU,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }
}

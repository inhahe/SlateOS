//! `<linux/sockios.h>` — socket ioctl numbers.
//!
//! Provides ioctl request codes used with network sockets and
//! interfaces.  These are the standard Linux socket ioctl numbers.

// ---------------------------------------------------------------------------
// Socket I/O control
// ---------------------------------------------------------------------------

/// Get/set interface list.
pub const SIOCGIFNAME: u64 = 0x8910;

/// Set interface flags.
pub const SIOCSIFFLAGS: u64 = 0x8914;

/// Get interface flags.
pub const SIOCGIFFLAGS: u64 = 0x8913;

/// Get interface address.
pub const SIOCGIFADDR: u64 = 0x8915;

/// Set interface address.
pub const SIOCSIFADDR: u64 = 0x8916;

/// Get destination address.
pub const SIOCGIFDSTADDR: u64 = 0x8917;

/// Set destination address.
pub const SIOCSIFDSTADDR: u64 = 0x8918;

/// Get broadcast address.
pub const SIOCGIFBRDADDR: u64 = 0x8919;

/// Set broadcast address.
pub const SIOCSIFBRDADDR: u64 = 0x891A;

/// Get network mask.
pub const SIOCGIFNETMASK: u64 = 0x891B;

/// Set network mask.
pub const SIOCSIFNETMASK: u64 = 0x891C;

/// Get hardware address.
pub const SIOCGIFHWADDR: u64 = 0x8927;

/// Set hardware address.
pub const SIOCSIFHWADDR: u64 = 0x8924;

/// Get MTU.
pub const SIOCGIFMTU: u64 = 0x8921;

/// Set MTU.
pub const SIOCSIFMTU: u64 = 0x8922;

/// Get interface index.
pub const SIOCGIFINDEX: u64 = 0x8933;

/// Get interface metric.
pub const SIOCGIFMETRIC: u64 = 0x891D;

/// Set interface metric.
pub const SIOCSIFMETRIC: u64 = 0x891E;

/// Get transmit queue length.
pub const SIOCGIFTXQLEN: u64 = 0x8942;

/// Set transmit queue length.
pub const SIOCSIFTXQLEN: u64 = 0x8943;

// ---------------------------------------------------------------------------
// Routing table ioctls
// ---------------------------------------------------------------------------

/// Add routing table entry.
pub const SIOCADDRT: u64 = 0x890B;

/// Delete routing table entry.
pub const SIOCDELRT: u64 = 0x890C;

// ---------------------------------------------------------------------------
// ARP cache ioctls
// ---------------------------------------------------------------------------

/// Get ARP entry.
pub const SIOCGARP: u64 = 0x8954;

/// Set ARP entry.
pub const SIOCSARP: u64 = 0x8955;

/// Delete ARP entry.
pub const SIOCDARP: u64 = 0x8953;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sioc_if_distinct() {
        let codes = [
            SIOCGIFNAME,
            SIOCGIFFLAGS,
            SIOCSIFFLAGS,
            SIOCGIFADDR,
            SIOCSIFADDR,
            SIOCGIFNETMASK,
            SIOCSIFNETMASK,
            SIOCGIFHWADDR,
            SIOCSIFHWADDR,
            SIOCGIFMTU,
            SIOCSIFMTU,
            SIOCGIFINDEX,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j], "SIOC codes must be distinct");
            }
        }
    }

    #[test]
    fn test_routing_ioctls() {
        assert_ne!(SIOCADDRT, SIOCDELRT);
    }

    #[test]
    fn test_arp_ioctls_distinct() {
        let arps = [SIOCGARP, SIOCSARP, SIOCDARP];
        for i in 0..arps.len() {
            for j in (i + 1)..arps.len() {
                assert_ne!(arps[i], arps[j]);
            }
        }
    }

    #[test]
    fn test_siocgifflags_value() {
        assert_eq!(SIOCGIFFLAGS, 0x8913);
    }

    #[test]
    fn test_siocgifaddr_value() {
        assert_eq!(SIOCGIFADDR, 0x8915);
    }
}

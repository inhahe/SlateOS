//! `<net/if.h>` — network interface definitions.
//!
//! Re-exports interface-related constants and functions from the
//! `socket` module and adds additional definitions that belong
//! specifically to the `<net/if.h>` header.

// ---------------------------------------------------------------------------
// Re-exports from socket
// ---------------------------------------------------------------------------

pub use crate::socket::IFF_BROADCAST;
pub use crate::socket::IFF_LOOPBACK;
pub use crate::socket::IFF_MULTICAST;
pub use crate::socket::IFF_RUNNING;
pub use crate::socket::IFF_UP;
pub use crate::socket::IfNameindex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum length of an interface name (including null terminator).
pub const IFNAMSIZ: usize = 16;

/// Maximum length of an interface name (alias for IFNAMSIZ).
pub const IF_NAMESIZE: usize = IFNAMSIZ;

// ---------------------------------------------------------------------------
// Additional IFF_* flags not in socket.rs
// ---------------------------------------------------------------------------

/// Interface is a loopback net (alias, kept for source compat).
pub const IFF_DEBUG: u32 = 4;

/// Interface receives all packets (promiscuous mode).
pub const IFF_PROMISC: u32 = 0x100;

/// No ARP protocol.
pub const IFF_NOARP: u32 = 0x80;

/// Receive all multicast packets.
pub const IFF_ALLMULTI: u32 = 0x200;

/// Interface is a point-to-point link.
pub const IFF_POINTOPOINT: u32 = 0x10;

/// No trailers.
pub const IFF_NOTRAILERS: u32 = 0x20;

/// Master of a load balancer.
pub const IFF_MASTER: u32 = 0x400;

/// Slave of a load balancer.
pub const IFF_SLAVE: u32 = 0x800;

/// Supports multicast (alias of IFF_MULTICAST for clarity).
pub const IFF_DYNAMIC: u32 = 0x8000;

/// Interface is in lower-layer-up state.
pub const IFF_LOWER_UP: u32 = 0x10000;

/// Driver signals dormant state.
pub const IFF_DORMANT: u32 = 0x20000;

// ---------------------------------------------------------------------------
// Ifreq structure
// ---------------------------------------------------------------------------

/// Interface request structure (for ioctl operations).
///
/// Used with `SIOCGIFADDR`, `SIOCSIFADDR`, `SIOCGIFFLAGS`, etc.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ifreq {
    /// Interface name (null-terminated).
    pub ifr_name: [u8; IFNAMSIZ],
    /// Union data (address, flags, metric, etc.).
    ///
    /// The largest member is a `sockaddr` (16 bytes on most
    /// platforms), so we use a 24-byte array to cover all variants
    /// with padding.
    pub ifr_data: [u8; 24],
}

// ---------------------------------------------------------------------------
// Functions (re-exported from socket, callable through this module)
// ---------------------------------------------------------------------------

/// Convert an interface name to its index.
///
/// # Safety
/// `ifname` must be a valid null-terminated string pointer.
#[inline]
pub unsafe fn if_nametoindex(ifname: *const u8) -> u32 {
    unsafe { crate::socket::if_nametoindex(ifname) }
}

/// Convert an interface index to its name.
///
/// # Safety
/// `ifname` must point to a buffer of at least `IFNAMSIZ` bytes.
#[inline]
pub unsafe fn if_indextoname(ifindex: u32, ifname: *mut u8) -> *mut u8 {
    unsafe { crate::socket::if_indextoname(ifindex, ifname) }
}

/// Return an array of all network interfaces.
#[inline]
pub fn if_nameindex() -> *mut IfNameindex {
    crate::socket::if_nameindex()
}

/// Free the array returned by `if_nameindex`.
#[inline]
pub fn if_freenameindex(ptr: *mut IfNameindex) {
    crate::socket::if_freenameindex(ptr);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_ifnamsiz() {
        assert_eq!(IFNAMSIZ, 16);
        assert_eq!(IF_NAMESIZE, IFNAMSIZ);
    }

    #[test]
    fn test_iff_flags_values() {
        assert_eq!(IFF_UP, 1);
        assert_eq!(IFF_BROADCAST, 2);
        assert_eq!(IFF_LOOPBACK, 8);
        assert_eq!(IFF_RUNNING, 0x40);
    }

    #[test]
    fn test_iff_flags_distinct() {
        let flags: [u32; 11] = [
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
            IFF_MULTICAST,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j], "IFF flags must be distinct");
            }
        }
    }

    #[test]
    fn test_iff_flags_are_bitmask() {
        let basic = [
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
        ];
        for &f in &basic {
            assert_ne!(f, 0);
            assert_eq!(f & (f - 1), 0, "IFF flag 0x{f:X} is not a power of two");
        }
    }

    // -----------------------------------------------------------------------
    // Ifreq struct
    // -----------------------------------------------------------------------

    #[test]
    fn test_ifreq_size() {
        assert_eq!(core::mem::size_of::<Ifreq>(), IFNAMSIZ + 24);
    }

    #[test]
    fn test_ifreq_name_fits() {
        let mut req = Ifreq {
            ifr_name: [0u8; IFNAMSIZ],
            ifr_data: [0u8; 24],
        };
        let name = b"eth0";
        req.ifr_name[..name.len()].copy_from_slice(name);
        assert_eq!(&req.ifr_name[..4], b"eth0");
        assert_eq!(req.ifr_name[4], 0); // null terminated
    }

    // -----------------------------------------------------------------------
    // Function re-exports
    // -----------------------------------------------------------------------

    #[test]
    fn test_if_nametoindex_via_module() {
        let idx = unsafe { if_nametoindex(b"lo\0".as_ptr()) };
        // lo is interface index 2 in our stub implementation.
        assert_ne!(idx, 0);
    }

    #[test]
    fn test_if_indextoname_via_module() {
        let mut buf = [0u8; IFNAMSIZ];
        let ret = unsafe { if_indextoname(1, buf.as_mut_ptr()) };
        assert!(!ret.is_null());
    }

    #[test]
    fn test_if_nameindex_via_module() {
        let table = if_nameindex();
        assert!(!table.is_null());
        if_freenameindex(table);
    }

    // -----------------------------------------------------------------------
    // Cross-module consistency
    // -----------------------------------------------------------------------

    #[test]
    fn test_flags_match_socket() {
        assert_eq!(IFF_UP, crate::socket::IFF_UP);
        assert_eq!(IFF_BROADCAST, crate::socket::IFF_BROADCAST);
        assert_eq!(IFF_LOOPBACK, crate::socket::IFF_LOOPBACK);
        assert_eq!(IFF_RUNNING, crate::socket::IFF_RUNNING);
        assert_eq!(IFF_MULTICAST, crate::socket::IFF_MULTICAST);
    }
}

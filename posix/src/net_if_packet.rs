//! `<netpacket/packet.h>` / `<linux/if_packet.h>` — packet socket definitions.
//!
//! Provides `SockaddrLl` and packet-level constants for raw packet
//! capture via `AF_PACKET` sockets.

// ---------------------------------------------------------------------------
// Packet types
// ---------------------------------------------------------------------------

/// Packet addressed to the local host.
pub const PACKET_HOST: u8 = 0;

/// Physical layer broadcast.
pub const PACKET_BROADCAST: u8 = 1;

/// Packet addressed to a multicast group.
pub const PACKET_MULTICAST: u8 = 2;

/// Packet addressed to another host (promiscuous).
pub const PACKET_OTHERHOST: u8 = 3;

/// Packet originated locally (loopback).
pub const PACKET_OUTGOING: u8 = 4;

// ---------------------------------------------------------------------------
// Socket types for AF_PACKET
// ---------------------------------------------------------------------------

/// Cooked (include fake header).
pub const PACKET_ADD_MEMBERSHIP: i32 = 1;

/// Drop membership.
pub const PACKET_DROP_MEMBERSHIP: i32 = 2;

/// Receive all multicast packets.
pub const PACKET_MR_MULTICAST: i32 = 0;

/// Promiscuous mode.
pub const PACKET_MR_PROMISC: i32 = 1;

/// All-multicast mode.
pub const PACKET_MR_ALLMULTI: i32 = 2;

// ---------------------------------------------------------------------------
// SockaddrLl — link-layer socket address
// ---------------------------------------------------------------------------

/// Link-layer socket address for `AF_PACKET`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockaddrLl {
    /// Always `AF_PACKET` (17).
    pub sll_family: u16,
    /// Physical layer protocol (network byte order).
    pub sll_protocol: u16,
    /// Interface index.
    pub sll_ifindex: i32,
    /// ARP hardware type.
    pub sll_hatype: u16,
    /// Packet type (`PACKET_*`).
    pub sll_pkttype: u8,
    /// Length of hardware address.
    pub sll_halen: u8,
    /// Hardware address (up to 8 bytes).
    pub sll_addr: [u8; 8],
}

/// `AF_PACKET` address family.
pub const AF_PACKET: i32 = 17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sockaddr_ll_size() {
        assert_eq!(core::mem::size_of::<SockaddrLl>(), 20);
    }

    #[test]
    fn test_packet_types_distinct() {
        let types = [
            PACKET_HOST,
            PACKET_BROADCAST,
            PACKET_MULTICAST,
            PACKET_OTHERHOST,
            PACKET_OUTGOING,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_packet_host_zero() {
        assert_eq!(PACKET_HOST, 0);
    }

    #[test]
    fn test_af_packet() {
        assert_eq!(AF_PACKET, 17);
    }

    #[test]
    fn test_mr_modes_distinct() {
        assert_ne!(PACKET_MR_MULTICAST, PACKET_MR_PROMISC);
        assert_ne!(PACKET_MR_PROMISC, PACKET_MR_ALLMULTI);
    }

    #[test]
    fn test_sockaddr_ll_init() {
        let addr = SockaddrLl {
            sll_family: AF_PACKET as u16,
            sll_protocol: 0,
            sll_ifindex: 0,
            sll_hatype: 0,
            sll_pkttype: PACKET_HOST,
            sll_halen: 0,
            sll_addr: [0; 8],
        };
        assert_eq!(addr.sll_family, 17);
        assert_eq!(addr.sll_pkttype, 0);
    }
}

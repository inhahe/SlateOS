//! `<linux/if_packet.h>` — Raw socket and packet-level constants.
//!
//! Raw sockets bypass the transport layer and give direct access
//! to link-layer or network-layer frames. These constants define
//! packet directions, socket types, and link-layer header types.

// ---------------------------------------------------------------------------
// Packet types (sll_pkttype in sockaddr_ll)
// ---------------------------------------------------------------------------

/// Packet addressed to the local host.
pub const PACKET_HOST: u8 = 0;
/// Broadcast packet.
pub const PACKET_BROADCAST: u8 = 1;
/// Multicast packet.
pub const PACKET_MULTICAST: u8 = 2;
/// Packet to another host (promiscuous capture).
pub const PACKET_OTHERHOST: u8 = 3;
/// Outgoing packet (loopback).
pub const PACKET_OUTGOING: u8 = 4;
/// Packet from the loopback device.
pub const PACKET_LOOPBACK: u8 = 5;
/// Packet from user space.
pub const PACKET_USER: u8 = 6;
/// Packet from the kernel.
pub const PACKET_KERNEL: u8 = 7;

// ---------------------------------------------------------------------------
// Packet socket options
// ---------------------------------------------------------------------------

/// Add/remove packet membership.
pub const PACKET_ADD_MEMBERSHIP: u32 = 1;
/// Drop membership.
pub const PACKET_DROP_MEMBERSHIP: u32 = 2;
/// Set receive buffer (ring).
pub const PACKET_RX_RING: u32 = 5;
/// Set transmit buffer (ring).
pub const PACKET_TX_RING: u32 = 13;
/// Copy threshold (packet truncation).
pub const PACKET_COPY_THRESH: u32 = 7;
/// Get statistics.
pub const PACKET_STATISTICS: u32 = 6;
/// Set packet version.
pub const PACKET_VERSION: u32 = 10;
/// Set fanout mode.
pub const PACKET_FANOUT: u32 = 18;
/// Lose packet rather than block.
pub const PACKET_LOSS: u32 = 14;
/// Enable VLAN offload info.
pub const PACKET_VNET_HDR: u32 = 15;
/// Set QDISC bypass.
pub const PACKET_QDISC_BYPASS: u32 = 20;

// ---------------------------------------------------------------------------
// Packet fanout modes
// ---------------------------------------------------------------------------

/// Hash-based fanout.
pub const PACKET_FANOUT_HASH: u32 = 0;
/// Load-balanced fanout.
pub const PACKET_FANOUT_LB: u32 = 1;
/// CPU-based fanout.
pub const PACKET_FANOUT_CPU: u32 = 2;
/// Rollover fanout.
pub const PACKET_FANOUT_ROLLOVER: u32 = 3;
/// Random fanout.
pub const PACKET_FANOUT_RND: u32 = 4;
/// QM (queue mapping) fanout.
pub const PACKET_FANOUT_QM: u32 = 5;
/// eBPF fanout.
pub const PACKET_FANOUT_CBPF: u32 = 6;
/// eBPF fanout.
pub const PACKET_FANOUT_EBPF: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkttype_distinct() {
        let types = [
            PACKET_HOST,
            PACKET_BROADCAST,
            PACKET_MULTICAST,
            PACKET_OTHERHOST,
            PACKET_OUTGOING,
            PACKET_LOOPBACK,
            PACKET_USER,
            PACKET_KERNEL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_sockopt_distinct() {
        let opts = [
            PACKET_ADD_MEMBERSHIP,
            PACKET_DROP_MEMBERSHIP,
            PACKET_RX_RING,
            PACKET_TX_RING,
            PACKET_COPY_THRESH,
            PACKET_STATISTICS,
            PACKET_VERSION,
            PACKET_FANOUT,
            PACKET_LOSS,
            PACKET_VNET_HDR,
            PACKET_QDISC_BYPASS,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_fanout_modes_distinct() {
        let modes = [
            PACKET_FANOUT_HASH,
            PACKET_FANOUT_LB,
            PACKET_FANOUT_CPU,
            PACKET_FANOUT_ROLLOVER,
            PACKET_FANOUT_RND,
            PACKET_FANOUT_QM,
            PACKET_FANOUT_CBPF,
            PACKET_FANOUT_EBPF,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_host_is_zero() {
        assert_eq!(PACKET_HOST, 0);
    }
}

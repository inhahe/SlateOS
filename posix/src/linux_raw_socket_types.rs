//! `<linux/if_packet.h>` (raw subset) — Raw/packet socket constants.
//!
//! Raw sockets bypass the kernel's protocol processing and give
//! userspace direct access to network frames. SOCK_RAW (IP level)
//! lets applications construct their own protocol headers. SOCK_PACKET
//! and AF_PACKET give access to link-layer frames (used by tcpdump,
//! Wireshark, dhcpd). Packet sockets support ring buffers (TPACKET)
//! for high-performance capture without per-packet system calls.

// ---------------------------------------------------------------------------
// Packet socket types
// ---------------------------------------------------------------------------

/// Cooked packet (stripped link-layer header, protocol info in sockaddr_ll).
pub const PACKET_HOST: u32 = 0;
/// Broadcast packet.
pub const PACKET_BROADCAST: u32 = 1;
/// Multicast packet.
pub const PACKET_MULTICAST: u32 = 2;
/// Packet sent by another host (promiscuous capture).
pub const PACKET_OTHERHOST: u32 = 3;
/// Packet originated from us (loopback).
pub const PACKET_OUTGOING: u32 = 4;
/// Kernel loopback packet.
pub const PACKET_LOOPBACK: u32 = 5;
/// Packet from user space (tun/tap).
pub const PACKET_USER: u32 = 6;
/// Kernel packet.
pub const PACKET_KERNEL: u32 = 7;

// ---------------------------------------------------------------------------
// TPACKET versions (ring buffer interface)
// ---------------------------------------------------------------------------

/// TPACKET v1 (original ring buffer).
pub const TPACKET_V1: u32 = 0;
/// TPACKET v2 (VLAN support, status flags).
pub const TPACKET_V2: u32 = 1;
/// TPACKET v3 (variable-length blocks, better batching).
pub const TPACKET_V3: u32 = 2;

// ---------------------------------------------------------------------------
// Packet socket options
// ---------------------------------------------------------------------------

/// Add membership (receive from multicast group).
pub const PACKET_ADD_MEMBERSHIP: u32 = 1;
/// Drop membership.
pub const PACKET_DROP_MEMBERSHIP: u32 = 2;
/// Get receive stats.
pub const PACKET_STATISTICS: u32 = 6;
/// Attach BPF filter.
pub const PACKET_FANOUT: u32 = 18;
/// Set TX ring.
pub const PACKET_TX_RING: u32 = 13;
/// Set RX ring.
pub const PACKET_RX_RING: u32 = 5;
/// Enable QDISC bypass for TX.
pub const PACKET_QDISC_BYPASS: u32 = 20;

// ---------------------------------------------------------------------------
// Fanout modes (distribute packets across sockets)
// ---------------------------------------------------------------------------

/// Hash on packet flow (5-tuple).
pub const PACKET_FANOUT_HASH: u32 = 0;
/// Round-robin distribution.
pub const PACKET_FANOUT_LB: u32 = 1;
/// CPU-based (each CPU gets its own socket).
pub const PACKET_FANOUT_CPU: u32 = 2;
/// Rollover (send to next socket when current is full).
pub const PACKET_FANOUT_ROLLOVER: u32 = 3;
/// Random distribution.
pub const PACKET_FANOUT_RND: u32 = 4;
/// Queue mapping based.
pub const PACKET_FANOUT_QM: u32 = 5;
/// eBPF program selects socket.
pub const PACKET_FANOUT_EBPF: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_types_distinct() {
        let types = [
            PACKET_HOST, PACKET_BROADCAST, PACKET_MULTICAST,
            PACKET_OTHERHOST, PACKET_OUTGOING, PACKET_LOOPBACK,
            PACKET_USER, PACKET_KERNEL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_tpacket_versions_distinct() {
        let versions = [TPACKET_V1, TPACKET_V2, TPACKET_V3];
        for i in 0..versions.len() {
            for j in (i + 1)..versions.len() {
                assert_ne!(versions[i], versions[j]);
            }
        }
    }

    #[test]
    fn test_fanout_modes_distinct() {
        let modes = [
            PACKET_FANOUT_HASH, PACKET_FANOUT_LB, PACKET_FANOUT_CPU,
            PACKET_FANOUT_ROLLOVER, PACKET_FANOUT_RND,
            PACKET_FANOUT_QM, PACKET_FANOUT_EBPF,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}

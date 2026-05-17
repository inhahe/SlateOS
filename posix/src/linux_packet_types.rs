//! `<linux/if_packet.h>` — AF_PACKET socket constants.
//!
//! AF_PACKET sockets provide raw access to network frames at the
//! link layer (Ethernet). They are used by packet capture tools
//! (tcpdump, Wireshark), network diagnostics, DHCP clients, and
//! userspace network stacks. PACKET_MMAP provides zero-copy access
//! via shared ring buffers.

// ---------------------------------------------------------------------------
// Packet socket types (socket() type argument for AF_PACKET)
// ---------------------------------------------------------------------------

/// Raw Ethernet frames (include link-layer header).
pub const PACKET_RAW: u32 = 3;
/// Cooked frames (link-layer header stripped, replaced with sockaddr_ll).
pub const PACKET_DGRAM: u32 = 2;

// ---------------------------------------------------------------------------
// Packet directions (sll_pkttype in sockaddr_ll)
// ---------------------------------------------------------------------------

/// Packet addressed to us.
pub const PACKET_HOST: u32 = 0;
/// Broadcast packet.
pub const PACKET_BROADCAST: u32 = 1;
/// Multicast packet.
pub const PACKET_MULTICAST: u32 = 2;
/// Packet to another host (promiscuous mode capture).
pub const PACKET_OTHERHOST: u32 = 3;
/// Locally generated packet (loopback).
pub const PACKET_OUTGOING: u32 = 4;

// ---------------------------------------------------------------------------
// Packet socket options (setsockopt level SOL_PACKET)
// ---------------------------------------------------------------------------

/// Add multicast membership.
pub const PACKET_ADD_MEMBERSHIP: u32 = 1;
/// Drop multicast membership.
pub const PACKET_DROP_MEMBERSHIP: u32 = 2;
/// Set receive ring buffer (v1/v2).
pub const PACKET_RX_RING: u32 = 5;
/// Set transmit ring buffer.
pub const PACKET_TX_RING: u32 = 13;
/// Set packet version (TPACKET_V1/V2/V3).
pub const PACKET_VERSION: u32 = 10;
/// Get statistics.
pub const PACKET_STATISTICS: u32 = 6;
/// Set fanout (load balancing across sockets).
pub const PACKET_FANOUT: u32 = 18;
/// Attach BPF filter.
pub const PACKET_AUXDATA: u32 = 8;

// ---------------------------------------------------------------------------
// TPACKET versions
// ---------------------------------------------------------------------------

/// Original TPACKET format.
pub const TPACKET_V1: u32 = 0;
/// TPACKET v2 (VLAN support, larger headers).
pub const TPACKET_V2: u32 = 1;
/// TPACKET v3 (variable-length blocks, best performance).
pub const TPACKET_V3: u32 = 2;

// ---------------------------------------------------------------------------
// Fanout modes
// ---------------------------------------------------------------------------

/// Hash-based distribution.
pub const PACKET_FANOUT_HASH: u32 = 0;
/// Round-robin distribution.
pub const PACKET_FANOUT_LB: u32 = 1;
/// CPU-based distribution.
pub const PACKET_FANOUT_CPU: u32 = 2;
/// Rollover to next socket on full.
pub const PACKET_FANOUT_ROLLOVER: u32 = 3;
/// Random distribution.
pub const PACKET_FANOUT_RND: u32 = 4;
/// BPF-based distribution.
pub const PACKET_FANOUT_CBPF: u32 = 6;
/// eBPF-based distribution.
pub const PACKET_FANOUT_EBPF: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkt_types_distinct() {
        let types = [
            PACKET_HOST, PACKET_BROADCAST, PACKET_MULTICAST,
            PACKET_OTHERHOST, PACKET_OUTGOING,
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
            PACKET_FANOUT_CBPF, PACKET_FANOUT_EBPF,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            PACKET_ADD_MEMBERSHIP, PACKET_DROP_MEMBERSHIP,
            PACKET_RX_RING, PACKET_TX_RING, PACKET_VERSION,
            PACKET_STATISTICS, PACKET_FANOUT, PACKET_AUXDATA,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}

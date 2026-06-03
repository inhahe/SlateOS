//! `<linux/if_packet.h>` — Packet socket (AF_PACKET) constants.
//!
//! AF_PACKET sockets allow sending and receiving raw link-layer
//! frames. Used by tcpdump, Wireshark, DHCP clients, and
//! network bridging. Supports mmap ring buffers for high-speed
//! packet capture.

// ---------------------------------------------------------------------------
// Packet types (sll_pkttype)
// ---------------------------------------------------------------------------

/// Packet addressed to local host.
pub const PACKET_HOST: u8 = 0;
/// Broadcast packet.
pub const PACKET_BROADCAST: u8 = 1;
/// Multicast packet.
pub const PACKET_MULTICAST: u8 = 2;
/// Packet for another host (promiscuous mode).
pub const PACKET_OTHERHOST: u8 = 3;
/// Outgoing packet (looped back from TX).
pub const PACKET_OUTGOING: u8 = 4;
/// Loopback packet.
pub const PACKET_LOOPBACK: u8 = 5;
/// User-space originated.
pub const PACKET_USER: u8 = 6;
/// Kernel-originated.
pub const PACKET_KERNEL: u8 = 7;

// ---------------------------------------------------------------------------
// Socket level options (SOL_PACKET)
// ---------------------------------------------------------------------------

/// Add membership (join multicast group).
pub const PACKET_ADD_MEMBERSHIP: u32 = 1;
/// Drop membership.
pub const PACKET_DROP_MEMBERSHIP: u32 = 2;
/// Receive buffer size.
pub const PACKET_RECV_OUTPUT: u32 = 3;
/// Receive statistics.
pub const PACKET_STATISTICS: u32 = 6;
/// Set packet version for mmap ring.
pub const PACKET_VERSION: u32 = 10;
/// TX ring.
pub const PACKET_TX_RING: u32 = 13;
/// RX ring.
pub const PACKET_RX_RING: u32 = 5;
/// Packet fanout.
pub const PACKET_FANOUT: u32 = 18;
/// VLAN tag control.
pub const PACKET_AUXDATA: u32 = 8;

// ---------------------------------------------------------------------------
// Ring buffer versions
// ---------------------------------------------------------------------------

/// TPACKET v1 (original).
pub const TPACKET_V1: u32 = 0;
/// TPACKET v2 (adds VLAN info).
pub const TPACKET_V2: u32 = 1;
/// TPACKET v3 (block-based, variable-length frames).
pub const TPACKET_V3: u32 = 2;

// ---------------------------------------------------------------------------
// Fanout modes
// ---------------------------------------------------------------------------

/// Hash-based fanout.
pub const PACKET_FANOUT_HASH: u16 = 0;
/// Load-balance fanout.
pub const PACKET_FANOUT_LB: u16 = 1;
/// CPU-based fanout.
pub const PACKET_FANOUT_CPU: u16 = 2;
/// Rollover fanout (overflow to next socket).
pub const PACKET_FANOUT_ROLLOVER: u16 = 3;
/// Random fanout.
pub const PACKET_FANOUT_RND: u16 = 4;
/// Queue mapping fanout.
pub const PACKET_FANOUT_QM: u16 = 5;
/// eBPF-based fanout.
pub const PACKET_FANOUT_CBPF: u16 = 6;
/// eBPF fanout.
pub const PACKET_FANOUT_EBPF: u16 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkt_types_distinct() {
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
    fn test_sock_opts_distinct() {
        let opts = [
            PACKET_ADD_MEMBERSHIP,
            PACKET_DROP_MEMBERSHIP,
            PACKET_RECV_OUTPUT,
            PACKET_STATISTICS,
            PACKET_VERSION,
            PACKET_TX_RING,
            PACKET_RX_RING,
            PACKET_FANOUT,
            PACKET_AUXDATA,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_tpacket_versions_distinct() {
        let vers = [TPACKET_V1, TPACKET_V2, TPACKET_V3];
        for i in 0..vers.len() {
            for j in (i + 1)..vers.len() {
                assert_ne!(vers[i], vers[j]);
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
}

//! `<linux/if_packet.h>` — AF_PACKET raw socket ABI.
//!
//! `tcpdump`, Wireshark, `tshark`, libpcap, eBPF probes, every DPDK
//! interface in software-fallback mode, and DHCP clients all use
//! `socket(AF_PACKET, …)` with the constants below. The TPACKET_V3
//! mmap'd ring is the highest-performance way to read raw frames
//! before XDP/AF_XDP.

// ---------------------------------------------------------------------------
// sockaddr_ll.sll_pkttype — packet direction / kind
// ---------------------------------------------------------------------------

pub const PACKET_HOST: u8 = 0;
pub const PACKET_BROADCAST: u8 = 1;
pub const PACKET_MULTICAST: u8 = 2;
pub const PACKET_OTHERHOST: u8 = 3;
pub const PACKET_OUTGOING: u8 = 4;
pub const PACKET_LOOPBACK: u8 = 5;
pub const PACKET_USER: u8 = 6;
pub const PACKET_KERNEL: u8 = 7;

// ---------------------------------------------------------------------------
// Socket options (setsockopt level=SOL_PACKET)
// ---------------------------------------------------------------------------

pub const SOL_PACKET: u32 = 263;
pub const PACKET_ADD_MEMBERSHIP: u32 = 1;
pub const PACKET_DROP_MEMBERSHIP: u32 = 2;
pub const PACKET_RECV_OUTPUT: u32 = 3;
pub const PACKET_RX_RING: u32 = 5;
pub const PACKET_STATISTICS: u32 = 6;
pub const PACKET_COPY_THRESH: u32 = 7;
pub const PACKET_AUXDATA: u32 = 8;
pub const PACKET_ORIGDEV: u32 = 9;
pub const PACKET_VERSION: u32 = 10;
pub const PACKET_HDRLEN: u32 = 11;
pub const PACKET_RESERVE: u32 = 12;
pub const PACKET_TX_RING: u32 = 13;
pub const PACKET_LOSS: u32 = 14;
pub const PACKET_VNET_HDR: u32 = 15;
pub const PACKET_TX_TIMESTAMP: u32 = 16;
pub const PACKET_TIMESTAMP: u32 = 17;
pub const PACKET_FANOUT: u32 = 18;
pub const PACKET_TX_HAS_OFF: u32 = 19;
pub const PACKET_QDISC_BYPASS: u32 = 20;
pub const PACKET_ROLLOVER_STATS: u32 = 21;
pub const PACKET_FANOUT_DATA: u32 = 22;
pub const PACKET_IGNORE_OUTGOING: u32 = 23;

// ---------------------------------------------------------------------------
// tpacket_versions
// ---------------------------------------------------------------------------

pub const TPACKET_V1: u32 = 0;
pub const TPACKET_V2: u32 = 1;
pub const TPACKET_V3: u32 = 2;

// ---------------------------------------------------------------------------
// tpacket_req.tp_status bits
// ---------------------------------------------------------------------------

pub const TP_STATUS_KERNEL: u32 = 0;
pub const TP_STATUS_USER: u32 = 1 << 0;
pub const TP_STATUS_COPY: u32 = 1 << 1;
pub const TP_STATUS_LOSING: u32 = 1 << 2;
pub const TP_STATUS_CSUMNOTREADY: u32 = 1 << 3;
pub const TP_STATUS_VLAN_VALID: u32 = 1 << 4;
pub const TP_STATUS_BLK_TMO: u32 = 1 << 5;
pub const TP_STATUS_VLAN_TPID_VALID: u32 = 1 << 6;
pub const TP_STATUS_CSUM_VALID: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// PACKET_FANOUT modes (low 16 bits of fanout setsockopt argument)
// ---------------------------------------------------------------------------

pub const PACKET_FANOUT_HASH: u32 = 0;
pub const PACKET_FANOUT_LB: u32 = 1;
pub const PACKET_FANOUT_CPU: u32 = 2;
pub const PACKET_FANOUT_ROLLOVER: u32 = 3;
pub const PACKET_FANOUT_RND: u32 = 4;
pub const PACKET_FANOUT_QM: u32 = 5;
pub const PACKET_FANOUT_CBPF: u32 = 6;
pub const PACKET_FANOUT_EBPF: u32 = 7;
/// High-bit flag: defragment before hashing.
pub const PACKET_FANOUT_FLAG_DEFRAG: u32 = 0x8000;
/// High-bit flag: rollover on filter drop.
pub const PACKET_FANOUT_FLAG_ROLLOVER: u32 = 0x1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkttype_dense_0_to_7() {
        let p = [
            PACKET_HOST,
            PACKET_BROADCAST,
            PACKET_MULTICAST,
            PACKET_OTHERHOST,
            PACKET_OUTGOING,
            PACKET_LOOPBACK,
            PACKET_USER,
            PACKET_KERNEL,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_sol_packet_value() {
        // SOL_PACKET is 263 in the Linux ABI.
        assert_eq!(SOL_PACKET, 263);
    }

    #[test]
    fn test_socket_options_distinct() {
        let o = [
            PACKET_ADD_MEMBERSHIP,
            PACKET_DROP_MEMBERSHIP,
            PACKET_RECV_OUTPUT,
            PACKET_RX_RING,
            PACKET_STATISTICS,
            PACKET_COPY_THRESH,
            PACKET_AUXDATA,
            PACKET_ORIGDEV,
            PACKET_VERSION,
            PACKET_HDRLEN,
            PACKET_RESERVE,
            PACKET_TX_RING,
            PACKET_LOSS,
            PACKET_VNET_HDR,
            PACKET_TX_TIMESTAMP,
            PACKET_TIMESTAMP,
            PACKET_FANOUT,
            PACKET_TX_HAS_OFF,
            PACKET_QDISC_BYPASS,
            PACKET_ROLLOVER_STATS,
            PACKET_FANOUT_DATA,
            PACKET_IGNORE_OUTGOING,
        ];
        for i in 0..o.len() {
            for j in (i + 1)..o.len() {
                assert_ne!(o[i], o[j]);
            }
        }
    }

    #[test]
    fn test_tpacket_versions_dense() {
        assert_eq!(TPACKET_V1, 0);
        assert_eq!(TPACKET_V2, 1);
        assert_eq!(TPACKET_V3, 2);
    }

    #[test]
    fn test_tp_status_bits_pow2() {
        for &b in &[
            TP_STATUS_USER,
            TP_STATUS_COPY,
            TP_STATUS_LOSING,
            TP_STATUS_CSUMNOTREADY,
            TP_STATUS_VLAN_VALID,
            TP_STATUS_BLK_TMO,
            TP_STATUS_VLAN_TPID_VALID,
            TP_STATUS_CSUM_VALID,
        ] {
            assert!(b.is_power_of_two());
        }
        assert_eq!(TP_STATUS_KERNEL, 0);
    }

    #[test]
    fn test_fanout_modes_dense() {
        let m = [
            PACKET_FANOUT_HASH,
            PACKET_FANOUT_LB,
            PACKET_FANOUT_CPU,
            PACKET_FANOUT_ROLLOVER,
            PACKET_FANOUT_RND,
            PACKET_FANOUT_QM,
            PACKET_FANOUT_CBPF,
            PACKET_FANOUT_EBPF,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // High-byte flags don't collide with the mode field.
        assert_eq!(PACKET_FANOUT_FLAG_DEFRAG & 0xFF, 0);
        assert_eq!(PACKET_FANOUT_FLAG_ROLLOVER & 0xFF, 0);
    }
}

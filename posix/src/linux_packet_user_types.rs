//! `<linux/if_packet.h>` — `AF_PACKET` raw-socket ABI.
//!
//! `AF_PACKET` is how tcpdump, Wireshark, dhclient, and any DPDK-free
//! L2 packet engine reads frames straight off an interface. The
//! sockopts here switch between the cooked `SOCK_DGRAM` and raw
//! `SOCK_RAW` modes, and configure the `PACKET_MMAP` ring used for
//! zero-copy capture.

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

pub const AF_PACKET: u32 = 17;

// ---------------------------------------------------------------------------
// `sll_pkttype` values — direction of the frame relative to the host
// ---------------------------------------------------------------------------

pub const PACKET_HOST: u32 = 0;
pub const PACKET_BROADCAST: u32 = 1;
pub const PACKET_MULTICAST: u32 = 2;
pub const PACKET_OTHERHOST: u32 = 3;
pub const PACKET_OUTGOING: u32 = 4;
pub const PACKET_LOOPBACK: u32 = 5;
pub const PACKET_USER: u32 = 6;
pub const PACKET_KERNEL: u32 = 7;

// ---------------------------------------------------------------------------
// `SOL_PACKET` socket options
// ---------------------------------------------------------------------------

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
// PACKET_MMAP ring-buffer versions
// ---------------------------------------------------------------------------

pub const TPACKET_V1: u32 = 0;
pub const TPACKET_V2: u32 = 1;
pub const TPACKET_V3: u32 = 2;

// ---------------------------------------------------------------------------
// PACKET_FANOUT modes
// ---------------------------------------------------------------------------

pub const PACKET_FANOUT_HASH: u32 = 0;
pub const PACKET_FANOUT_LB: u32 = 1;
pub const PACKET_FANOUT_CPU: u32 = 2;
pub const PACKET_FANOUT_ROLLOVER: u32 = 3;
pub const PACKET_FANOUT_RND: u32 = 4;
pub const PACKET_FANOUT_QM: u32 = 5;
pub const PACKET_FANOUT_CBPF: u32 = 6;
pub const PACKET_FANOUT_EBPF: u32 = 7;

pub const PACKET_FANOUT_FLAG_ROLLOVER: u32 = 0x1000;
pub const PACKET_FANOUT_FLAG_UNIQUEID: u32 = 0x2000;
pub const PACKET_FANOUT_FLAG_IGNORE_OUTGOING: u32 = 0x4000;
pub const PACKET_FANOUT_FLAG_DEFRAG: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Common `ETH_P_*` types (a few — full list lives elsewhere)
// ---------------------------------------------------------------------------

pub const ETH_P_ALL: u32 = 0x0003;
pub const ETH_P_IP: u32 = 0x0800;
pub const ETH_P_ARP: u32 = 0x0806;
pub const ETH_P_8021Q: u32 = 0x8100;
pub const ETH_P_IPV6: u32 = 0x86DD;
pub const ETH_P_8021AD: u32 = 0x88A8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_packet_is_17() {
        assert_eq!(AF_PACKET, 17);
    }

    #[test]
    fn test_pkttypes_dense_0_to_7() {
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
    fn test_sockopts_distinct() {
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
    fn test_fanout_modes_dense_0_to_7() {
        let f = [
            PACKET_FANOUT_HASH,
            PACKET_FANOUT_LB,
            PACKET_FANOUT_CPU,
            PACKET_FANOUT_ROLLOVER,
            PACKET_FANOUT_RND,
            PACKET_FANOUT_QM,
            PACKET_FANOUT_CBPF,
            PACKET_FANOUT_EBPF,
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_fanout_flags_high_nibble_single_bit() {
        // Flags occupy bits 12..15.
        let fl = [
            PACKET_FANOUT_FLAG_ROLLOVER,
            PACKET_FANOUT_FLAG_UNIQUEID,
            PACKET_FANOUT_FLAG_IGNORE_OUTGOING,
            PACKET_FANOUT_FLAG_DEFRAG,
        ];
        for v in fl {
            assert!(v.is_power_of_two());
            assert!(v >= 0x1000 && v <= 0x8000);
        }
    }

    #[test]
    fn test_ether_types_byte_swapped_form() {
        // ETH_P_ALL is the sentinel "everything"; ETH_P_IP and ETH_P_IPV6
        // match the canonical IANA EtherType values.
        assert_eq!(ETH_P_ALL, 0x0003);
        assert_eq!(ETH_P_IP, 0x0800);
        assert_eq!(ETH_P_ARP, 0x0806);
        assert_eq!(ETH_P_IPV6, 0x86DD);
        assert_eq!(ETH_P_8021Q, 0x8100);
        assert_eq!(ETH_P_8021AD, 0x88A8);
    }
}

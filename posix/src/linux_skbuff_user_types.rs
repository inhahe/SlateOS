//! `<linux/skbuff.h>` — packet-level constants exposed to userspace.
//!
//! Most of `sk_buff` is kernel-internal but several constants leak
//! out via `AF_PACKET`, `tc`, `bpf`, and raw sockets: the packet
//! type the device delivered, the checksum-status word, and the
//! TX/RX queue mapping. Tools like `tcpdump`, `iproute2`, and
//! every eBPF tracer use them.

// ---------------------------------------------------------------------------
// `sll_pkttype` / `skb->pkt_type` — packet directionality
// ---------------------------------------------------------------------------

pub const PACKET_HOST: u8 = 0;
pub const PACKET_BROADCAST: u8 = 1;
pub const PACKET_MULTICAST: u8 = 2;
pub const PACKET_OTHERHOST: u8 = 3;
pub const PACKET_OUTGOING: u8 = 4;
pub const PACKET_LOOPBACK: u8 = 5;
pub const PACKET_USER: u8 = 6;
pub const PACKET_KERNEL: u8 = 7;

pub const PACKET_FASTROUTE: u8 = 6; // legacy alias kept for header compat

// ---------------------------------------------------------------------------
// `skb->ip_summed` — checksum-status enum
// ---------------------------------------------------------------------------

pub const CHECKSUM_NONE: u8 = 0;
pub const CHECKSUM_UNNECESSARY: u8 = 1;
pub const CHECKSUM_COMPLETE: u8 = 2;
pub const CHECKSUM_PARTIAL: u8 = 3;

// ---------------------------------------------------------------------------
// Buffer sizes
// ---------------------------------------------------------------------------

/// Maximum amount of TCP/UDP header data the kernel will linearise
/// up-front for `__pskb_pull_tail`. Picked to comfortably hold any
/// reasonable L2+L3+L4 header.
pub const SKB_MAX_HEADER: u32 = 128;

/// Headroom reserved in front of every packet for `dev_alloc_skb`
/// callers. Drivers fill in the L2 header here.
pub const NET_SKB_PAD: u32 = 64;

/// Maximum number of fragments in a single skb.
pub const MAX_SKB_FRAGS: u32 = 17;

// ---------------------------------------------------------------------------
// GSO type flag bits (subset that's visible to userspace via `SO_*`)
// ---------------------------------------------------------------------------

pub const SKB_GSO_TCPV4: u32 = 1 << 0;
pub const SKB_GSO_DODGY: u32 = 1 << 1;
pub const SKB_GSO_TCP_ECN: u32 = 1 << 2;
pub const SKB_GSO_TCP_FIXEDID: u32 = 1 << 3;
pub const SKB_GSO_TCPV6: u32 = 1 << 4;
pub const SKB_GSO_FCOE: u32 = 1 << 5;
pub const SKB_GSO_GRE: u32 = 1 << 6;
pub const SKB_GSO_GRE_CSUM: u32 = 1 << 7;
pub const SKB_GSO_IPXIP4: u32 = 1 << 8;
pub const SKB_GSO_IPXIP6: u32 = 1 << 9;
pub const SKB_GSO_UDP_TUNNEL: u32 = 1 << 10;
pub const SKB_GSO_UDP_TUNNEL_CSUM: u32 = 1 << 11;

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
    fn test_fastroute_alias_with_user() {
        // Both PACKET_FASTROUTE (legacy) and PACKET_USER live at 6.
        assert_eq!(PACKET_FASTROUTE, PACKET_USER);
    }

    #[test]
    fn test_ip_summed_dense_0_to_3() {
        assert_eq!(CHECKSUM_NONE, 0);
        assert_eq!(CHECKSUM_UNNECESSARY, 1);
        assert_eq!(CHECKSUM_COMPLETE, 2);
        assert_eq!(CHECKSUM_PARTIAL, 3);
    }

    #[test]
    fn test_skb_sizes_power_of_two_ish() {
        assert_eq!(SKB_MAX_HEADER, 128);
        assert!(SKB_MAX_HEADER.is_power_of_two());
        assert_eq!(NET_SKB_PAD, 64);
        assert!(NET_SKB_PAD.is_power_of_two());
        // 17 frags = 16 + 1 (the linear part plus 16 page fragments).
        assert_eq!(MAX_SKB_FRAGS, 17);
    }

    #[test]
    fn test_gso_flag_bits_single_bit_and_dense() {
        let g = [
            SKB_GSO_TCPV4,
            SKB_GSO_DODGY,
            SKB_GSO_TCP_ECN,
            SKB_GSO_TCP_FIXEDID,
            SKB_GSO_TCPV6,
            SKB_GSO_FCOE,
            SKB_GSO_GRE,
            SKB_GSO_GRE_CSUM,
            SKB_GSO_IPXIP4,
            SKB_GSO_IPXIP6,
            SKB_GSO_UDP_TUNNEL,
            SKB_GSO_UDP_TUNNEL_CSUM,
        ];
        let mut or = 0u32;
        for (i, v) in g.iter().enumerate() {
            assert_eq!(*v, 1 << i);
            or |= v;
        }
        assert_eq!(or, 0xFFF);
    }
}

//! `<linux/pkt_cls.h>` (flower part 2) — flower TCP/UDP/SCTP match attrs.
//!
//! "flower" is the most flexible tc classifier — it matches packets
//! against L2–L4 5-tuples. This module covers the TCP/UDP/SCTP port
//! and L4 match attribute IDs that supplement the basic IPv4/IPv6 set.

// ---------------------------------------------------------------------------
// TCA_FLOWER L4 port match attributes
// ---------------------------------------------------------------------------

pub const TCA_FLOWER_KEY_TCP_SRC: u32 = 22;
pub const TCA_FLOWER_KEY_TCP_DST: u32 = 23;
pub const TCA_FLOWER_KEY_UDP_SRC: u32 = 24;
pub const TCA_FLOWER_KEY_UDP_DST: u32 = 25;
pub const TCA_FLOWER_KEY_SCTP_SRC: u32 = 38;
pub const TCA_FLOWER_KEY_SCTP_DST: u32 = 39;
pub const TCA_FLOWER_KEY_ICMPV4_TYPE: u32 = 36;
pub const TCA_FLOWER_KEY_ICMPV4_CODE: u32 = 37;
pub const TCA_FLOWER_KEY_ICMPV6_TYPE: u32 = 40;
pub const TCA_FLOWER_KEY_ICMPV6_CODE: u32 = 41;

// ---------------------------------------------------------------------------
// TCP/UDP source-port mask attributes
// ---------------------------------------------------------------------------

pub const TCA_FLOWER_KEY_TCP_SRC_MASK: u32 = 49;
pub const TCA_FLOWER_KEY_TCP_DST_MASK: u32 = 50;
pub const TCA_FLOWER_KEY_UDP_SRC_MASK: u32 = 51;
pub const TCA_FLOWER_KEY_UDP_DST_MASK: u32 = 52;

// ---------------------------------------------------------------------------
// TCP flags match
// ---------------------------------------------------------------------------

pub const TCA_FLOWER_KEY_TCP_FLAGS: u32 = 71;
pub const TCA_FLOWER_KEY_TCP_FLAGS_MASK: u32 = 72;

// ---------------------------------------------------------------------------
// TCP flag bits (matches struct tcphdr packed flags byte)
// ---------------------------------------------------------------------------

pub const TCP_FLAG_FIN: u16 = 0x001;
pub const TCP_FLAG_SYN: u16 = 0x002;
pub const TCP_FLAG_RST: u16 = 0x004;
pub const TCP_FLAG_PSH: u16 = 0x008;
pub const TCP_FLAG_ACK: u16 = 0x010;
pub const TCP_FLAG_URG: u16 = 0x020;
pub const TCP_FLAG_ECE: u16 = 0x040;
pub const TCP_FLAG_CWR: u16 = 0x080;
pub const TCP_FLAG_NS: u16 = 0x100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_ports_consecutive() {
        assert_eq!(TCA_FLOWER_KEY_TCP_DST, TCA_FLOWER_KEY_TCP_SRC + 1);
        assert_eq!(TCA_FLOWER_KEY_UDP_DST, TCA_FLOWER_KEY_UDP_SRC + 1);
        assert_eq!(TCA_FLOWER_KEY_SCTP_DST, TCA_FLOWER_KEY_SCTP_SRC + 1);
        assert_eq!(TCA_FLOWER_KEY_UDP_SRC, TCA_FLOWER_KEY_TCP_DST + 1);
    }

    #[test]
    fn test_icmp_consecutive() {
        // ICMPv4 type/code, ICMPv6 type/code form two consecutive pairs.
        assert_eq!(TCA_FLOWER_KEY_ICMPV4_CODE, TCA_FLOWER_KEY_ICMPV4_TYPE + 1);
        assert_eq!(TCA_FLOWER_KEY_ICMPV6_CODE, TCA_FLOWER_KEY_ICMPV6_TYPE + 1);
    }

    #[test]
    fn test_port_mask_attrs_consecutive() {
        let m = [
            TCA_FLOWER_KEY_TCP_SRC_MASK,
            TCA_FLOWER_KEY_TCP_DST_MASK,
            TCA_FLOWER_KEY_UDP_SRC_MASK,
            TCA_FLOWER_KEY_UDP_DST_MASK,
        ];
        for w in m.windows(2) {
            assert_eq!(w[1], w[0] + 1);
        }
    }

    #[test]
    fn test_tcp_flags_distinct_single_bit() {
        let f = [
            TCP_FLAG_FIN,
            TCP_FLAG_SYN,
            TCP_FLAG_RST,
            TCP_FLAG_PSH,
            TCP_FLAG_ACK,
            TCP_FLAG_URG,
            TCP_FLAG_ECE,
            TCP_FLAG_CWR,
            TCP_FLAG_NS,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
        // 9 single-bit flags = 0x1FF.
        let or_all = f.iter().fold(0u16, |a, &v| a | v);
        assert_eq!(or_all, 0x1FF);
    }

    #[test]
    fn test_tcp_flags_match_attr_pair_adjacent() {
        // Value + mask attributes for TCP flags are consecutive IDs.
        assert_eq!(TCA_FLOWER_KEY_TCP_FLAGS_MASK, TCA_FLOWER_KEY_TCP_FLAGS + 1);
    }
}

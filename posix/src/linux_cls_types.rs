//! `<linux/pkt_cls.h>` — Traffic control classifier constants.
//!
//! TC classifiers match packets and assign them to classes.
//! These constants define classifier attribute types, BPF
//! classifier flags, and flower match key types.

// ---------------------------------------------------------------------------
// Classifier attribute types (TCA_*)
// ---------------------------------------------------------------------------

/// BPF classifier.
pub const TCA_BPF_UNSPEC: u32 = 0;
/// BPF action.
pub const TCA_BPF_ACT: u32 = 1;
/// BPF police.
pub const TCA_BPF_POLICE: u32 = 2;
/// BPF classid.
pub const TCA_BPF_CLASSID: u32 = 3;
/// BPF ops length.
pub const TCA_BPF_OPS_LEN: u32 = 4;
/// BPF ops (classic BPF program).
pub const TCA_BPF_OPS: u32 = 5;
/// BPF file descriptor (eBPF).
pub const TCA_BPF_FD: u32 = 6;
/// BPF program name.
pub const TCA_BPF_NAME: u32 = 7;
/// BPF flags.
pub const TCA_BPF_FLAGS: u32 = 8;
/// BPF flags (generation 2).
pub const TCA_BPF_FLAGS_GEN: u32 = 9;
/// BPF tag (program tag).
pub const TCA_BPF_TAG: u32 = 10;
/// BPF program ID.
pub const TCA_BPF_ID: u32 = 11;

// ---------------------------------------------------------------------------
// BPF classifier flags
// ---------------------------------------------------------------------------

/// Direct action mode (BPF program returns TC action).
pub const TCA_BPF_FLAG_ACT_DIRECT: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Flower classifier key types
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_FLOWER_UNSPEC: u32 = 0;
/// Classify ID.
pub const TCA_FLOWER_CLASSID: u32 = 1;
/// Ingress interface.
pub const TCA_FLOWER_INDEV: u32 = 2;
/// Action list.
pub const TCA_FLOWER_ACT: u32 = 3;
/// Key Ethernet destination MAC.
pub const TCA_FLOWER_KEY_ETH_DST: u32 = 4;
/// Key Ethernet source MAC.
pub const TCA_FLOWER_KEY_ETH_SRC: u32 = 5;
/// Key EtherType.
pub const TCA_FLOWER_KEY_ETH_TYPE: u32 = 8;
/// Key IPv4 source address.
pub const TCA_FLOWER_KEY_IPV4_SRC: u32 = 9;
/// Key IPv4 destination address.
pub const TCA_FLOWER_KEY_IPV4_DST: u32 = 10;
/// Key IPv6 source address.
pub const TCA_FLOWER_KEY_IPV6_SRC: u32 = 11;
/// Key IPv6 destination address.
pub const TCA_FLOWER_KEY_IPV6_DST: u32 = 12;
/// Key IP protocol.
pub const TCA_FLOWER_KEY_IP_PROTO: u32 = 13;
/// Key TCP source port.
pub const TCA_FLOWER_KEY_TCP_SRC: u32 = 14;
/// Key TCP destination port.
pub const TCA_FLOWER_KEY_TCP_DST: u32 = 15;
/// Key UDP source port.
pub const TCA_FLOWER_KEY_UDP_SRC: u32 = 16;
/// Key UDP destination port.
pub const TCA_FLOWER_KEY_UDP_DST: u32 = 17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bpf_attrs_distinct() {
        let attrs = [
            TCA_BPF_UNSPEC, TCA_BPF_ACT, TCA_BPF_POLICE,
            TCA_BPF_CLASSID, TCA_BPF_OPS_LEN, TCA_BPF_OPS,
            TCA_BPF_FD, TCA_BPF_NAME, TCA_BPF_FLAGS,
            TCA_BPF_FLAGS_GEN, TCA_BPF_TAG, TCA_BPF_ID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_bpf_unspec_is_zero() {
        assert_eq!(TCA_BPF_UNSPEC, 0);
    }

    #[test]
    fn test_act_direct_flag() {
        assert_eq!(TCA_BPF_FLAG_ACT_DIRECT, 1);
    }

    #[test]
    fn test_flower_keys_distinct() {
        let keys = [
            TCA_FLOWER_UNSPEC, TCA_FLOWER_CLASSID,
            TCA_FLOWER_INDEV, TCA_FLOWER_ACT,
            TCA_FLOWER_KEY_ETH_DST, TCA_FLOWER_KEY_ETH_SRC,
            TCA_FLOWER_KEY_ETH_TYPE,
            TCA_FLOWER_KEY_IPV4_SRC, TCA_FLOWER_KEY_IPV4_DST,
            TCA_FLOWER_KEY_IPV6_SRC, TCA_FLOWER_KEY_IPV6_DST,
            TCA_FLOWER_KEY_IP_PROTO,
            TCA_FLOWER_KEY_TCP_SRC, TCA_FLOWER_KEY_TCP_DST,
            TCA_FLOWER_KEY_UDP_SRC, TCA_FLOWER_KEY_UDP_DST,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }

    #[test]
    fn test_flower_unspec_is_zero() {
        assert_eq!(TCA_FLOWER_UNSPEC, 0);
    }
}

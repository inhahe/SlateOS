//! `<linux/neighbour.h>` — netlink neighbour-table ABI (ARP / NDISC).
//!
//! The neighbour cache holds L2 address resolution state for every
//! peer the host has talked to. `iproute2`'s `ip neigh`, NetworkManager,
//! and BGP/OSPF daemons read and modify it via `RTM_*NEIGH` netlink
//! messages.

// ---------------------------------------------------------------------------
// Neighbour states (`ndm_state` bitmask)
// ---------------------------------------------------------------------------

pub const NUD_NONE: u16 = 0x00;
pub const NUD_INCOMPLETE: u16 = 0x01;
pub const NUD_REACHABLE: u16 = 0x02;
pub const NUD_STALE: u16 = 0x04;
pub const NUD_DELAY: u16 = 0x08;
pub const NUD_PROBE: u16 = 0x10;
pub const NUD_FAILED: u16 = 0x20;
pub const NUD_NOARP: u16 = 0x40;
pub const NUD_PERMANENT: u16 = 0x80;

/// Mask of states where the address is "valid" (kernel can transmit).
pub const NUD_VALID: u16 =
    NUD_PERMANENT | NUD_NOARP | NUD_REACHABLE | NUD_PROBE | NUD_STALE | NUD_DELAY;
pub const NUD_CONNECTED: u16 = NUD_PERMANENT | NUD_NOARP | NUD_REACHABLE;

// ---------------------------------------------------------------------------
// Neighbour flags (`ndm_flags`)
// ---------------------------------------------------------------------------

pub const NTF_USE: u8 = 0x01;
pub const NTF_SELF: u8 = 0x02;
pub const NTF_MASTER: u8 = 0x04;
pub const NTF_PROXY: u8 = 0x08;
pub const NTF_EXT_LEARNED: u8 = 0x10;
pub const NTF_OFFLOADED: u8 = 0x20;
pub const NTF_STICKY: u8 = 0x40;
pub const NTF_ROUTER: u8 = 0x80;

// ---------------------------------------------------------------------------
// Netlink attribute types (`NDA_*`)
// ---------------------------------------------------------------------------

pub const NDA_UNSPEC: u32 = 0;
pub const NDA_DST: u32 = 1;
pub const NDA_LLADDR: u32 = 2;
pub const NDA_CACHEINFO: u32 = 3;
pub const NDA_PROBES: u32 = 4;
pub const NDA_VLAN: u32 = 5;
pub const NDA_PORT: u32 = 6;
pub const NDA_VNI: u32 = 7;
pub const NDA_IFINDEX: u32 = 8;
pub const NDA_MASTER: u32 = 9;
pub const NDA_LINK_NETNSID: u32 = 10;
pub const NDA_SRC_VNI: u32 = 11;
pub const NDA_PROTOCOL: u32 = 12;
pub const NDA_NH_ID: u32 = 13;
pub const NDA_FDB_EXT_ATTRS: u32 = 14;
pub const NDA_FLAGS_EXT: u32 = 15;
pub const NDA_NDM_STATE_MASK: u32 = 16;
pub const NDA_NDM_FLAGS_MASK: u32 = 17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_bits_single_bit() {
        let s = [
            NUD_INCOMPLETE,
            NUD_REACHABLE,
            NUD_STALE,
            NUD_DELAY,
            NUD_PROBE,
            NUD_FAILED,
            NUD_NOARP,
            NUD_PERMANENT,
        ];
        for v in s {
            assert!(v.is_power_of_two());
        }
        // NONE is zero.
        assert_eq!(NUD_NONE, 0);
        // Eight dense bits cover the byte.
        assert_eq!(s.iter().fold(0u16, |a, b| a | b), 0xFF);
    }

    #[test]
    fn test_valid_and_connected_subsets() {
        // CONNECTED ⊆ VALID.
        assert_eq!(NUD_CONNECTED & NUD_VALID, NUD_CONNECTED);
        // INCOMPLETE and FAILED are *not* valid.
        assert_eq!(NUD_VALID & NUD_INCOMPLETE, 0);
        assert_eq!(NUD_VALID & NUD_FAILED, 0);
    }

    #[test]
    fn test_ntf_flags_single_bit_and_byte_dense() {
        let f = [
            NTF_USE,
            NTF_SELF,
            NTF_MASTER,
            NTF_PROXY,
            NTF_EXT_LEARNED,
            NTF_OFFLOADED,
            NTF_STICKY,
            NTF_ROUTER,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        // Eight dense bits == 0xFF.
        assert_eq!(f.iter().fold(0u8, |a, b| a | b), 0xFF);
    }

    #[test]
    fn test_nda_attributes_dense_0_to_17() {
        let a = [
            NDA_UNSPEC,
            NDA_DST,
            NDA_LLADDR,
            NDA_CACHEINFO,
            NDA_PROBES,
            NDA_VLAN,
            NDA_PORT,
            NDA_VNI,
            NDA_IFINDEX,
            NDA_MASTER,
            NDA_LINK_NETNSID,
            NDA_SRC_VNI,
            NDA_PROTOCOL,
            NDA_NH_ID,
            NDA_FDB_EXT_ATTRS,
            NDA_FLAGS_EXT,
            NDA_NDM_STATE_MASK,
            NDA_NDM_FLAGS_MASK,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}

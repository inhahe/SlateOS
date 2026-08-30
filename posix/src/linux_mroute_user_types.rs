//! `<linux/mroute.h>` / `<linux/mroute6.h>` — IPv4/IPv6 multicast routing ABI.
//!
//! `pimd`, `mrouted`, and `igmpproxy` use these socket options to
//! turn a Linux box into a multicast router. The user opens a raw
//! IGMP socket and configures the kernel-side multicast forwarding
//! cache (MFC) via `setsockopt(SOL_IP, MRT_*, …)`.

// ---------------------------------------------------------------------------
// IPv4 multicast-router socket options (SOL_IP / IPPROTO_IP level)
// ---------------------------------------------------------------------------

pub const MRT_BASE: u32 = 200;
pub const MRT_INIT: u32 = MRT_BASE; // 200
pub const MRT_DONE: u32 = MRT_BASE + 1;
pub const MRT_ADD_VIF: u32 = MRT_BASE + 2;
pub const MRT_DEL_VIF: u32 = MRT_BASE + 3;
pub const MRT_ADD_MFC: u32 = MRT_BASE + 4;
pub const MRT_DEL_MFC: u32 = MRT_BASE + 5;
pub const MRT_VERSION: u32 = MRT_BASE + 6;
pub const MRT_ASSERT: u32 = MRT_BASE + 7;
pub const MRT_PIM: u32 = MRT_BASE + 8;
pub const MRT_TABLE: u32 = MRT_BASE + 9;
pub const MRT_ADD_MFC_PROXY: u32 = MRT_BASE + 10;
pub const MRT_DEL_MFC_PROXY: u32 = MRT_BASE + 11;
pub const MRT_FLUSH: u32 = MRT_BASE + 12;

// ---------------------------------------------------------------------------
// IPv6 multicast-router socket options (SOL_IPV6 / IPPROTO_IPV6 level)
// ---------------------------------------------------------------------------

pub const MRT6_BASE: u32 = 200;
pub const MRT6_INIT: u32 = MRT6_BASE;
pub const MRT6_DONE: u32 = MRT6_BASE + 1;
pub const MRT6_ADD_MIF: u32 = MRT6_BASE + 2;
pub const MRT6_DEL_MIF: u32 = MRT6_BASE + 3;
pub const MRT6_ADD_MFC: u32 = MRT6_BASE + 4;
pub const MRT6_DEL_MFC: u32 = MRT6_BASE + 5;
pub const MRT6_VERSION: u32 = MRT6_BASE + 6;
pub const MRT6_ASSERT: u32 = MRT6_BASE + 7;
pub const MRT6_PIM: u32 = MRT6_BASE + 8;
pub const MRT6_TABLE: u32 = MRT6_BASE + 9;

// ---------------------------------------------------------------------------
// Per-VIF flags (`struct vifctl.vifc_flags`)
// ---------------------------------------------------------------------------

pub const VIFF_TUNNEL: u32 = 0x01;
pub const VIFF_SRCRT: u32 = 0x02;
pub const VIFF_REGISTER: u32 = 0x04;
pub const VIFF_USE_IFINDEX: u32 = 0x08;

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// Maximum number of virtual interfaces.
pub const MAXVIFS: u32 = 32;
pub const MAXMIFS: u32 = 32;

// ---------------------------------------------------------------------------
// IGMP message types sent up to userspace
// ---------------------------------------------------------------------------

pub const IGMPMSG_NOCACHE: u8 = 1;
pub const IGMPMSG_WRONGVIF: u8 = 2;
pub const IGMPMSG_WHOLEPKT: u8 = 3;
pub const IGMPMSG_WRVIFWHOLE: u8 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v4_sockopts_dense_200_to_212() {
        let o = [
            MRT_INIT,
            MRT_DONE,
            MRT_ADD_VIF,
            MRT_DEL_VIF,
            MRT_ADD_MFC,
            MRT_DEL_MFC,
            MRT_VERSION,
            MRT_ASSERT,
            MRT_PIM,
            MRT_TABLE,
            MRT_ADD_MFC_PROXY,
            MRT_DEL_MFC_PROXY,
            MRT_FLUSH,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v, MRT_BASE + i as u32);
        }
    }

    #[test]
    fn test_v6_sockopts_dense_and_match_v4_layout() {
        let o = [
            MRT6_INIT,
            MRT6_DONE,
            MRT6_ADD_MIF,
            MRT6_DEL_MIF,
            MRT6_ADD_MFC,
            MRT6_DEL_MFC,
            MRT6_VERSION,
            MRT6_ASSERT,
            MRT6_PIM,
            MRT6_TABLE,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v, MRT6_BASE + i as u32);
        }
        // IPv6 and IPv4 base values match — they share the option-number space.
        assert_eq!(MRT_BASE, MRT6_BASE);
    }

    #[test]
    fn test_vif_flags_single_bit_and_dense() {
        let f = [VIFF_TUNNEL, VIFF_SRCRT, VIFF_REGISTER, VIFF_USE_IFINDEX];
        for v in f {
            assert!(v.is_power_of_two());
        }
        assert_eq!(f.iter().fold(0, |a, b| a | b), 0x0F);
    }

    #[test]
    fn test_vif_table_sizes() {
        assert_eq!(MAXVIFS, 32);
        assert_eq!(MAXMIFS, 32);
        // 32 fits in a u32 bitmap or an i8 index.
        assert!(MAXVIFS <= u8::MAX as u32);
    }

    #[test]
    fn test_igmpmsg_codes_dense_1_to_4() {
        let m = [
            IGMPMSG_NOCACHE,
            IGMPMSG_WRONGVIF,
            IGMPMSG_WHOLEPKT,
            IGMPMSG_WRVIFWHOLE,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }
}

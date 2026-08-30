//! `<linux/mroute.h>` — Multicast routing constants.
//!
//! These constants define multicast routing socket options,
//! MFC (Multicast Forwarding Cache) entry flags, VIF
//! (Virtual Interface) attributes, and multicast routing
//! message types.

// ---------------------------------------------------------------------------
// Multicast routing socket options
// ---------------------------------------------------------------------------

/// Add a VIF.
pub const MRT_INIT: u32 = 200;
/// Done with multicast routing.
pub const MRT_DONE: u32 = 201;
/// Add a multicast VIF.
pub const MRT_ADD_VIF: u32 = 202;
/// Delete a VIF.
pub const MRT_DEL_VIF: u32 = 203;
/// Add a multicast forwarding cache entry.
pub const MRT_ADD_MFC: u32 = 204;
/// Delete an MFC entry.
pub const MRT_DEL_MFC: u32 = 205;
/// Version query.
pub const MRT_VERSION: u32 = 206;
/// Assert (upcall on cache miss).
pub const MRT_ASSERT: u32 = 207;
/// PIM (Protocol Independent Multicast).
pub const MRT_PIM: u32 = 208;
/// Table (routing table ID).
pub const MRT_TABLE: u32 = 209;
/// Add MFC entry via proxy.
pub const MRT_ADD_MFC_PROXY: u32 = 210;
/// Delete MFC entry via proxy.
pub const MRT_DEL_MFC_PROXY: u32 = 211;
/// Flush all MFC entries.
pub const MRT_FLUSH: u32 = 212;

// ---------------------------------------------------------------------------
// VIF flags
// ---------------------------------------------------------------------------

/// Tunnel VIF.
pub const VIFF_TUNNEL: u32 = 0x1;
/// Source-route VIF.
pub const VIFF_SRCRT: u32 = 0x2;
/// Register VIF (PIM).
pub const VIFF_REGISTER: u32 = 0x4;
/// Use interface index.
pub const VIFF_USE_IFINDEX: u32 = 0x8;

// ---------------------------------------------------------------------------
// MFC flags
// ---------------------------------------------------------------------------

/// Static MFC entry.
pub const MFC_STATIC: u32 = 1 << 0;
/// Offloaded to hardware.
pub const MFC_OFFLOAD: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Multicast routing message types (for upcalls)
// ---------------------------------------------------------------------------

/// No cache — kernel needs forwarding info.
pub const IGMPMSG_NOCACHE: u32 = 1;
/// Wrong VIF — packet arrived on unexpected interface.
pub const IGMPMSG_WRONGVIF: u32 = 2;
/// Whole-PIM message.
pub const IGMPMSG_WHOLEPKT: u32 = 3;

// ---------------------------------------------------------------------------
// Multicast routing netlink attributes (RTA_MFC_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const RTA_MFC_STATS_UNSPEC: u32 = 0;
/// Packets forwarded.
pub const RTA_MFC_STATS_PACKETS: u32 = 1;
/// Bytes forwarded.
pub const RTA_MFC_STATS_BYTES: u32 = 2;
/// Wrong interface count.
pub const RTA_MFC_STATS_WRONG_IF: u32 = 3;

// ---------------------------------------------------------------------------
// Maximum constants
// ---------------------------------------------------------------------------

/// Maximum number of VIFs.
pub const MAXVIFS: u32 = 32;
/// Maximum number of multicast groups.
pub const MRT_MAX: u32 = MRT_FLUSH;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mrt_options_distinct() {
        let opts = [
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
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_mrt_init_is_200() {
        assert_eq!(MRT_INIT, 200);
    }

    #[test]
    fn test_vif_flags_are_powers_of_two() {
        let flags = [VIFF_TUNNEL, VIFF_SRCRT, VIFF_REGISTER, VIFF_USE_IFINDEX];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_vif_flags_no_overlap() {
        let flags = [VIFF_TUNNEL, VIFF_SRCRT, VIFF_REGISTER, VIFF_USE_IFINDEX];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_mfc_flags_no_overlap() {
        assert_eq!(MFC_STATIC & MFC_OFFLOAD, 0);
    }

    #[test]
    fn test_igmpmsg_types_distinct() {
        let msgs = [IGMPMSG_NOCACHE, IGMPMSG_WRONGVIF, IGMPMSG_WHOLEPKT];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_mfc_stats_distinct() {
        let stats = [
            RTA_MFC_STATS_UNSPEC,
            RTA_MFC_STATS_PACKETS,
            RTA_MFC_STATS_BYTES,
            RTA_MFC_STATS_WRONG_IF,
        ];
        for i in 0..stats.len() {
            for j in (i + 1)..stats.len() {
                assert_ne!(stats[i], stats[j]);
            }
        }
    }

    #[test]
    fn test_maxvifs() {
        assert_eq!(MAXVIFS, 32);
    }

    #[test]
    fn test_mrt_max() {
        assert_eq!(MRT_MAX, MRT_FLUSH);
    }
}

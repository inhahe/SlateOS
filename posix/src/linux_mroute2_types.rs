//! `<linux/mroute.h>` — Additional multicast routing constants.
//!
//! Supplementary multicast routing constants covering
//! ioctl commands, control message types, and cache flags.

// ---------------------------------------------------------------------------
// Multicast routing ioctl commands
// ---------------------------------------------------------------------------

/// Add virtual interface.
pub const MRT_ADD_VIF: u32 = 202;
/// Delete virtual interface.
pub const MRT_DEL_VIF: u32 = 203;
/// Add multicast forwarding cache entry.
pub const MRT_ADD_MFC: u32 = 204;
/// Delete multicast forwarding cache entry.
pub const MRT_DEL_MFC: u32 = 205;
/// Init multicast routing.
pub const MRT_INIT: u32 = 200;
/// Done multicast routing.
pub const MRT_DONE: u32 = 201;
/// Assert.
pub const MRT_ASSERT: u32 = 206;
/// PIM.
pub const MRT_PIM: u32 = 207;
/// Table.
pub const MRT_TABLE: u32 = 209;
/// Add MFC proxy.
pub const MRT_ADD_MFC_PROXY: u32 = 210;
/// Del MFC proxy.
pub const MRT_DEL_MFC_PROXY: u32 = 211;
/// Flush.
pub const MRT_FLUSH: u32 = 212;

// ---------------------------------------------------------------------------
// Multicast routing VIF flags
// ---------------------------------------------------------------------------

/// Tunnel VIF.
pub const VIFF_TUNNEL: u32 = 0x1;
/// Source route VIF.
pub const VIFF_SRCRT: u32 = 0x2;
/// Register VIF.
pub const VIFF_REGISTER: u32 = 0x4;
/// Use interface index.
pub const VIFF_USE_IFINDEX: u32 = 0x8;

// ---------------------------------------------------------------------------
// Multicast routing message types
// ---------------------------------------------------------------------------

/// Cache miss.
pub const IGMPMSG_NOCACHE: u32 = 1;
/// Wrong interface.
pub const IGMPMSG_WRONGVIF: u32 = 2;
/// Whole packet.
pub const IGMPMSG_WHOLEPKT: u32 = 3;
/// Wrvifwhole.
pub const IGMPMSG_WRVIFWHOLE: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            MRT_INIT, MRT_DONE, MRT_ADD_VIF, MRT_DEL_VIF,
            MRT_ADD_MFC, MRT_DEL_MFC, MRT_ASSERT, MRT_PIM,
            MRT_TABLE, MRT_ADD_MFC_PROXY, MRT_DEL_MFC_PROXY,
            MRT_FLUSH,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
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
    fn test_msg_types_distinct() {
        let msgs = [
            IGMPMSG_NOCACHE, IGMPMSG_WRONGVIF,
            IGMPMSG_WHOLEPKT, IGMPMSG_WRVIFWHOLE,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }
}

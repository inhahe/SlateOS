//! `<linux/smc.h>` — Additional SMC (Shared Memory Communications) constants.
//!
//! Supplementary SMC constants covering socket types,
//! CLC message types, and link group modes.

// ---------------------------------------------------------------------------
// SMC socket types
// ---------------------------------------------------------------------------

/// SMC-R (RDMA).
pub const SMC_TYPE_R: u32 = 0;
/// SMC-D (Direct Memory Access, ISM).
pub const SMC_TYPE_D: u32 = 1;
/// Both SMC-R and SMC-D.
pub const SMC_TYPE_BOTH: u32 = 2;

// ---------------------------------------------------------------------------
// SMC CLC (Connection Layer Control) message types
// ---------------------------------------------------------------------------

/// Proposal message.
pub const SMC_CLC_PROPOSAL: u32 = 0x01;
/// Accept message.
pub const SMC_CLC_ACCEPT: u32 = 0x02;
/// Confirm message.
pub const SMC_CLC_CONFIRM: u32 = 0x03;
/// Decline message.
pub const SMC_CLC_DECLINE: u32 = 0x04;

// ---------------------------------------------------------------------------
// SMC decline reasons
// ---------------------------------------------------------------------------

/// No SMC-R device.
pub const SMC_DECLINE_NO_DEVICE: u32 = 0x03010000;
/// No SMC-D device.
pub const SMC_DECLINE_NO_ISM_DEV: u32 = 0x03020000;
/// Incompatible version.
pub const SMC_DECLINE_MISMATCH: u32 = 0x03030000;
/// Resource shortage.
pub const SMC_DECLINE_RESOURCES: u32 = 0x03040000;
/// Synchronization error.
pub const SMC_DECLINE_SYNCERR: u32 = 0x03050000;
/// Peer decline.
pub const SMC_DECLINE_PEERDECL: u32 = 0x03060000;

// ---------------------------------------------------------------------------
// SMC link group roles
// ---------------------------------------------------------------------------

/// Leader role.
pub const SMC_LGR_LEADER: u32 = 0;
/// Follower role.
pub const SMC_LGR_FOLLOWER: u32 = 1;

// ---------------------------------------------------------------------------
// SMC netlink genetlink commands
// ---------------------------------------------------------------------------

/// Unspec.
pub const SMC_NETLINK_CMD_UNSPEC: u32 = 0;
/// Get link groups.
pub const SMC_NETLINK_GET_LGR_SMCR: u32 = 1;
/// Get links.
pub const SMC_NETLINK_GET_LINK_SMCR: u32 = 2;
/// Get link groups (SMC-D).
pub const SMC_NETLINK_GET_LGR_SMCD: u32 = 3;
/// Get devices.
pub const SMC_NETLINK_GET_DEV_SMCD: u32 = 4;
/// Get devices (SMC-R).
pub const SMC_NETLINK_GET_DEV_SMCR: u32 = 5;
/// Get stats.
pub const SMC_NETLINK_GET_STATS: u32 = 6;
/// Get F stats.
pub const SMC_NETLINK_GET_FBACK_STATS: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [SMC_TYPE_R, SMC_TYPE_D, SMC_TYPE_BOTH];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_clc_msgs_distinct() {
        let msgs = [
            SMC_CLC_PROPOSAL, SMC_CLC_ACCEPT,
            SMC_CLC_CONFIRM, SMC_CLC_DECLINE,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_decline_reasons_distinct() {
        let reasons = [
            SMC_DECLINE_NO_DEVICE, SMC_DECLINE_NO_ISM_DEV,
            SMC_DECLINE_MISMATCH, SMC_DECLINE_RESOURCES,
            SMC_DECLINE_SYNCERR, SMC_DECLINE_PEERDECL,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_lgr_roles_distinct() {
        assert_ne!(SMC_LGR_LEADER, SMC_LGR_FOLLOWER);
    }

    #[test]
    fn test_netlink_cmds_distinct() {
        let cmds = [
            SMC_NETLINK_CMD_UNSPEC, SMC_NETLINK_GET_LGR_SMCR,
            SMC_NETLINK_GET_LINK_SMCR, SMC_NETLINK_GET_LGR_SMCD,
            SMC_NETLINK_GET_DEV_SMCD, SMC_NETLINK_GET_DEV_SMCR,
            SMC_NETLINK_GET_STATS, SMC_NETLINK_GET_FBACK_STATS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}

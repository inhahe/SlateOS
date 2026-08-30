//! `<linux/smc.h>` — Shared Memory Communications (SMC) constants.
//!
//! SMC provides RDMA-based communication that's transparent to TCP
//! applications. Used by IBM mainframes (z/OS ↔ Linux) and
//! increasingly on commodity hardware with RoCE NICs.

// ---------------------------------------------------------------------------
// SMC netlink commands
// ---------------------------------------------------------------------------

/// Unspecified.
pub const SMC_CMD_UNSPEC: u8 = 0;
/// Get link info.
pub const SMC_CMD_GET_LGR_SMCR: u8 = 1;
/// Get SMC-D link group.
pub const SMC_CMD_GET_LGR_SMCD: u8 = 2;
/// Get connection info.
pub const SMC_CMD_GET_CONN_SMCR: u8 = 3;
/// Get SMC-D connection info.
pub const SMC_CMD_GET_CONN_SMCD: u8 = 4;
/// Get PNET table.
pub const SMC_CMD_GET_PNET: u8 = 5;
/// Get device info.
pub const SMC_CMD_GET_DEV_SMCR: u8 = 6;
/// Get SMC-D device info.
pub const SMC_CMD_GET_DEV_SMCD: u8 = 7;
/// Get statistics.
pub const SMC_CMD_GET_STATS: u8 = 8;
/// Get fallback statistics.
pub const SMC_CMD_GET_FBACK_STATS: u8 = 9;

// ---------------------------------------------------------------------------
// SMC types
// ---------------------------------------------------------------------------

/// SMC over RDMA.
pub const SMC_TYPE_R: u32 = 0;
/// SMC over ISM (Internal Shared Memory).
pub const SMC_TYPE_D: u32 = 1;
/// Both SMC-R and SMC-D.
pub const SMC_TYPE_B: u32 = 2;

// ---------------------------------------------------------------------------
// CLC (Connection Layer Control) types
// ---------------------------------------------------------------------------

/// CLC proposal.
pub const SMC_CLC_PROPOSAL: u8 = 0x01;
/// CLC accept.
pub const SMC_CLC_ACCEPT: u8 = 0x02;
/// CLC confirm.
pub const SMC_CLC_CONFIRM: u8 = 0x03;
/// CLC decline.
pub const SMC_CLC_DECLINE: u8 = 0x04;

// ---------------------------------------------------------------------------
// SMC fallback reasons
// ---------------------------------------------------------------------------

/// No SMC available.
pub const SMC_FBACK_RSN_NONE: u32 = 0;
/// Peer declined.
pub const SMC_FBACK_RSN_PEER_DECLINE: u32 = 1;
/// No RDMA device.
pub const SMC_FBACK_RSN_NO_RDMA_DEV: u32 = 2;
/// No ISM device.
pub const SMC_FBACK_RSN_NO_ISM_DEV: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            SMC_CMD_UNSPEC,
            SMC_CMD_GET_LGR_SMCR,
            SMC_CMD_GET_LGR_SMCD,
            SMC_CMD_GET_CONN_SMCR,
            SMC_CMD_GET_CONN_SMCD,
            SMC_CMD_GET_PNET,
            SMC_CMD_GET_DEV_SMCR,
            SMC_CMD_GET_DEV_SMCD,
            SMC_CMD_GET_STATS,
            SMC_CMD_GET_FBACK_STATS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_types_distinct() {
        let types = [SMC_TYPE_R, SMC_TYPE_D, SMC_TYPE_B];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_clc_types_distinct() {
        let clcs = [
            SMC_CLC_PROPOSAL,
            SMC_CLC_ACCEPT,
            SMC_CLC_CONFIRM,
            SMC_CLC_DECLINE,
        ];
        for i in 0..clcs.len() {
            for j in (i + 1)..clcs.len() {
                assert_ne!(clcs[i], clcs[j]);
            }
        }
    }

    #[test]
    fn test_fback_reasons_distinct() {
        let reasons = [
            SMC_FBACK_RSN_NONE,
            SMC_FBACK_RSN_PEER_DECLINE,
            SMC_FBACK_RSN_NO_RDMA_DEV,
            SMC_FBACK_RSN_NO_ISM_DEV,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }
}

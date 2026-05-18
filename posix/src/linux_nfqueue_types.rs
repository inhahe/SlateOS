//! `<linux/netfilter/nfnetlink_queue.h>` — NFQUEUE constants.
//!
//! NFQUEUE allows userspace to make accept/drop decisions on
//! packets queued by netfilter.  These constants define message
//! types, verdict codes, and configuration options.

// ---------------------------------------------------------------------------
// NFQUEUE message types
// ---------------------------------------------------------------------------

/// Packet message (from kernel to userspace).
pub const NFQNL_MSG_PACKET: u32 = 0;
/// Verdict message (from userspace to kernel).
pub const NFQNL_MSG_VERDICT: u32 = 1;
/// Configuration message.
pub const NFQNL_MSG_CONFIG: u32 = 2;
/// Batch verdict message.
pub const NFQNL_MSG_VERDICT_BATCH: u32 = 3;

// ---------------------------------------------------------------------------
// NFQUEUE config commands
// ---------------------------------------------------------------------------

/// No command.
pub const NFQNL_CFG_CMD_NONE: u32 = 0;
/// Bind to queue.
pub const NFQNL_CFG_CMD_BIND: u32 = 1;
/// Unbind from queue.
pub const NFQNL_CFG_CMD_UNBIND: u32 = 2;
/// Bind to protocol family.
pub const NFQNL_CFG_CMD_PF_BIND: u32 = 3;
/// Unbind from protocol family.
pub const NFQNL_CFG_CMD_PF_UNBIND: u32 = 4;

// ---------------------------------------------------------------------------
// NFQUEUE copy modes
// ---------------------------------------------------------------------------

/// Don't copy payload.
pub const NFQNL_COPY_NONE: u32 = 0;
/// Copy packet metadata only.
pub const NFQNL_COPY_META: u32 = 1;
/// Copy packet payload.
pub const NFQNL_COPY_PACKET: u32 = 2;

// ---------------------------------------------------------------------------
// NFQUEUE config flags
// ---------------------------------------------------------------------------

/// Fail open (accept if queue is full).
pub const NFQA_CFG_F_FAIL_OPEN: u32 = 1 << 0;
/// Connection tracking (include conntrack info).
pub const NFQA_CFG_F_CONNTRACK: u32 = 1 << 1;
/// GSO (handle Generic Segmentation Offload packets).
pub const NFQA_CFG_F_GSO: u32 = 1 << 2;
/// UID/GID in packet info.
pub const NFQA_CFG_F_UID_GID: u32 = 1 << 3;
/// Security context.
pub const NFQA_CFG_F_SECCTX: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// NFQUEUE packet attributes
// ---------------------------------------------------------------------------

/// Packet header.
pub const NFQA_PACKET_HDR: u32 = 1;
/// Verdict header.
pub const NFQA_VERDICT_HDR: u32 = 2;
/// Packet mark.
pub const NFQA_MARK: u32 = 3;
/// Timestamp.
pub const NFQA_TIMESTAMP: u32 = 4;
/// Input interface index.
pub const NFQA_IFINDEX_INDEV: u32 = 5;
/// Output interface index.
pub const NFQA_IFINDEX_OUTDEV: u32 = 6;
/// Packet payload.
pub const NFQA_PAYLOAD: u32 = 10;
/// Connection tracking.
pub const NFQA_CT: u32 = 11;
/// UID.
pub const NFQA_UID: u32 = 14;
/// GID.
pub const NFQA_GID: u32 = 15;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        let types = [
            NFQNL_MSG_PACKET, NFQNL_MSG_VERDICT,
            NFQNL_MSG_CONFIG, NFQNL_MSG_VERDICT_BATCH,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_packet_is_zero() {
        assert_eq!(NFQNL_MSG_PACKET, 0);
    }

    #[test]
    fn test_config_cmds_distinct() {
        let cmds = [
            NFQNL_CFG_CMD_NONE, NFQNL_CFG_CMD_BIND,
            NFQNL_CFG_CMD_UNBIND, NFQNL_CFG_CMD_PF_BIND,
            NFQNL_CFG_CMD_PF_UNBIND,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_copy_modes_distinct() {
        let modes = [NFQNL_COPY_NONE, NFQNL_COPY_META, NFQNL_COPY_PACKET];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_config_flags_powers_of_two() {
        let flags = [
            NFQA_CFG_F_FAIL_OPEN, NFQA_CFG_F_CONNTRACK,
            NFQA_CFG_F_GSO, NFQA_CFG_F_UID_GID,
            NFQA_CFG_F_SECCTX,
        ];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_config_flags_no_overlap() {
        let flags = [
            NFQA_CFG_F_FAIL_OPEN, NFQA_CFG_F_CONNTRACK,
            NFQA_CFG_F_GSO, NFQA_CFG_F_UID_GID,
            NFQA_CFG_F_SECCTX,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

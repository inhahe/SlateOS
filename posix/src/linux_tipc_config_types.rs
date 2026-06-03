//! `<linux/tipc_config.h>` — TIPC cluster IPC configuration constants.
//!
//! TIPC (Transparent Inter-Process Communication) is the cluster
//! messaging protocol used by Open vSwitch and Ericsson clusters.
//! `tipc-config` userspace tool consumes these command codes and
//! TLV types to manage links, bearers, and the topology server.

// ---------------------------------------------------------------------------
// Configuration command codes (struct tipc_genlmsghdr.cmd)
// ---------------------------------------------------------------------------

/// No-op (reserved).
pub const TIPC_CMD_NOT_NET_ADMIN: u32 = 0;
/// Get bearer names list.
pub const TIPC_CMD_GET_BEARER_NAMES: u32 = 0x0001;
/// Enable a bearer.
pub const TIPC_CMD_ENABLE_BEARER: u32 = 0x4101;
/// Disable a bearer.
pub const TIPC_CMD_DISABLE_BEARER: u32 = 0x4102;
/// Get link statistics.
pub const TIPC_CMD_SHOW_LINK_STATS: u32 = 0x0007;
/// Reset link statistics.
pub const TIPC_CMD_RESET_LINK_STATS: u32 = 0x4109;
/// Set node address.
pub const TIPC_CMD_SET_NODE_ADDR: u32 = 0x4107;
/// Set network ID.
pub const TIPC_CMD_SET_NETID: u32 = 0x4108;

// ---------------------------------------------------------------------------
// TLV (type-length-value) type codes (within tipc_cfg_msg payload)
// ---------------------------------------------------------------------------

/// No payload.
pub const TIPC_TLV_NONE: u16 = 0;
/// Padding.
pub const TIPC_TLV_VOID: u16 = 1;
/// Unsigned 32-bit value.
pub const TIPC_TLV_UNSIGNED: u16 = 2;
/// String value.
pub const TIPC_TLV_STRING: u16 = 3;
/// Large string (>=128 chars).
pub const TIPC_TLV_LARGE_STRING: u16 = 4;
/// Per-bearer config payload.
pub const TIPC_TLV_BEARER_CONFIG: u16 = 8;
/// Link configuration payload.
pub const TIPC_TLV_LINK_CONFIG: u16 = 9;

// ---------------------------------------------------------------------------
// Default cluster / link tuning
// ---------------------------------------------------------------------------

/// Default TIPC bearer priority.
pub const TIPC_DEF_BEARER_PRIORITY: u32 = 10;
/// Maximum bearer priority value.
pub const TIPC_MAX_BEARER_PRIORITY: u32 = 31;
/// Minimum link tolerance (ms).
pub const TIPC_MIN_LINK_TOL: u32 = 50;
/// Maximum link tolerance (ms).
pub const TIPC_MAX_LINK_TOL: u32 = 30000;
/// Maximum text label length used by tipc-config.
pub const TIPC_MAX_LABEL_LEN: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            TIPC_CMD_NOT_NET_ADMIN,
            TIPC_CMD_GET_BEARER_NAMES,
            TIPC_CMD_ENABLE_BEARER,
            TIPC_CMD_DISABLE_BEARER,
            TIPC_CMD_SHOW_LINK_STATS,
            TIPC_CMD_RESET_LINK_STATS,
            TIPC_CMD_SET_NODE_ADDR,
            TIPC_CMD_SET_NETID,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_tlv_types_distinct() {
        let tlvs = [
            TIPC_TLV_NONE,
            TIPC_TLV_VOID,
            TIPC_TLV_UNSIGNED,
            TIPC_TLV_STRING,
            TIPC_TLV_LARGE_STRING,
            TIPC_TLV_BEARER_CONFIG,
            TIPC_TLV_LINK_CONFIG,
        ];
        for i in 0..tlvs.len() {
            for j in (i + 1)..tlvs.len() {
                assert_ne!(tlvs[i], tlvs[j]);
            }
        }
    }

    #[test]
    fn test_bearer_priority_in_range() {
        assert!(TIPC_DEF_BEARER_PRIORITY <= TIPC_MAX_BEARER_PRIORITY);
    }

    #[test]
    fn test_link_tolerance_range_sane() {
        // Maximum tolerance must exceed minimum and both must fit
        // a typical 16-bit jiffies-millisecond range.
        assert!(TIPC_MIN_LINK_TOL < TIPC_MAX_LINK_TOL);
        assert!(TIPC_MAX_LINK_TOL <= 60000);
    }

    #[test]
    fn test_label_len_sane() {
        assert!(TIPC_MAX_LABEL_LEN >= 16);
    }
}

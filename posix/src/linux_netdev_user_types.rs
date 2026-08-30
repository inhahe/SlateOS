//! `<linux/netdev.h>` — genetlink "netdev" family (Linux 6.0+).
//!
//! The "netdev" genetlink family exposes per-interface XDP feature
//! discovery, NAPI configuration, and queue-stats — capabilities that
//! the older `RTM_*` ioctl-style ABI can't express cleanly. `ip link
//! show xdp_features`, `ethtool -i`, and AF_XDP libraries query it.

// ---------------------------------------------------------------------------
// Genetlink family
// ---------------------------------------------------------------------------

pub const NETDEV_FAMILY_NAME: &str = "netdev";
pub const NETDEV_FAMILY_VERSION: u32 = 1;
pub const NETDEV_MCGRP_MGMT: &str = "mgmt";
pub const NETDEV_MCGRP_PAGE_POOL: &str = "page-pool";

// ---------------------------------------------------------------------------
// `enum netdev_cmd`
// ---------------------------------------------------------------------------

pub const NETDEV_CMD_DEV_GET: u32 = 1;
pub const NETDEV_CMD_DEV_ADD_NTF: u32 = 2;
pub const NETDEV_CMD_DEV_DEL_NTF: u32 = 3;
pub const NETDEV_CMD_DEV_CHANGE_NTF: u32 = 4;
pub const NETDEV_CMD_PAGE_POOL_GET: u32 = 5;
pub const NETDEV_CMD_PAGE_POOL_ADD_NTF: u32 = 6;
pub const NETDEV_CMD_PAGE_POOL_DEL_NTF: u32 = 7;
pub const NETDEV_CMD_PAGE_POOL_CHANGE_NTF: u32 = 8;
pub const NETDEV_CMD_PAGE_POOL_STATS_GET: u32 = 9;
pub const NETDEV_CMD_QUEUE_GET: u32 = 10;
pub const NETDEV_CMD_NAPI_GET: u32 = 11;
pub const NETDEV_CMD_QSTATS_GET: u32 = 12;
pub const NETDEV_CMD_BIND_RX: u32 = 13;
pub const NETDEV_CMD_NAPI_SET: u32 = 14;

// ---------------------------------------------------------------------------
// XDP feature bits (`xdp_features`)
// ---------------------------------------------------------------------------

pub const NETDEV_XDP_ACT_BASIC: u64 = 1 << 0;
pub const NETDEV_XDP_ACT_REDIRECT: u64 = 1 << 1;
pub const NETDEV_XDP_ACT_NDO_XMIT: u64 = 1 << 2;
pub const NETDEV_XDP_ACT_XSK_ZEROCOPY: u64 = 1 << 3;
pub const NETDEV_XDP_ACT_HW_OFFLOAD: u64 = 1 << 4;
pub const NETDEV_XDP_ACT_RX_SG: u64 = 1 << 5;
pub const NETDEV_XDP_ACT_NDO_XMIT_SG: u64 = 1 << 6;

pub const NETDEV_XDP_ACT_MASK: u64 = NETDEV_XDP_ACT_BASIC
    | NETDEV_XDP_ACT_REDIRECT
    | NETDEV_XDP_ACT_NDO_XMIT
    | NETDEV_XDP_ACT_XSK_ZEROCOPY
    | NETDEV_XDP_ACT_HW_OFFLOAD
    | NETDEV_XDP_ACT_RX_SG
    | NETDEV_XDP_ACT_NDO_XMIT_SG;

// ---------------------------------------------------------------------------
// XDP-RX metadata features
// ---------------------------------------------------------------------------

pub const NETDEV_XDP_RX_METADATA_TIMESTAMP: u64 = 1 << 0;
pub const NETDEV_XDP_RX_METADATA_HASH: u64 = 1 << 1;
pub const NETDEV_XDP_RX_METADATA_VLAN_TAG: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Queue types (`enum netdev_queue_type`)
// ---------------------------------------------------------------------------

pub const NETDEV_QUEUE_TYPE_RX: u32 = 0;
pub const NETDEV_QUEUE_TYPE_TX: u32 = 1;

// ---------------------------------------------------------------------------
// QSTATS scope (`enum netdev_qstats_scope`)
// ---------------------------------------------------------------------------

pub const NETDEV_QSTATS_SCOPE_QUEUE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_identity() {
        assert_eq!(NETDEV_FAMILY_NAME, "netdev");
        assert_eq!(NETDEV_FAMILY_VERSION, 1);
        assert_eq!(NETDEV_MCGRP_MGMT, "mgmt");
        assert_eq!(NETDEV_MCGRP_PAGE_POOL, "page-pool");
    }

    #[test]
    fn test_commands_dense_1_to_14() {
        let c = [
            NETDEV_CMD_DEV_GET,
            NETDEV_CMD_DEV_ADD_NTF,
            NETDEV_CMD_DEV_DEL_NTF,
            NETDEV_CMD_DEV_CHANGE_NTF,
            NETDEV_CMD_PAGE_POOL_GET,
            NETDEV_CMD_PAGE_POOL_ADD_NTF,
            NETDEV_CMD_PAGE_POOL_DEL_NTF,
            NETDEV_CMD_PAGE_POOL_CHANGE_NTF,
            NETDEV_CMD_PAGE_POOL_STATS_GET,
            NETDEV_CMD_QUEUE_GET,
            NETDEV_CMD_NAPI_GET,
            NETDEV_CMD_QSTATS_GET,
            NETDEV_CMD_BIND_RX,
            NETDEV_CMD_NAPI_SET,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_xdp_act_bits_dense_and_single_bit() {
        let x = [
            NETDEV_XDP_ACT_BASIC,
            NETDEV_XDP_ACT_REDIRECT,
            NETDEV_XDP_ACT_NDO_XMIT,
            NETDEV_XDP_ACT_XSK_ZEROCOPY,
            NETDEV_XDP_ACT_HW_OFFLOAD,
            NETDEV_XDP_ACT_RX_SG,
            NETDEV_XDP_ACT_NDO_XMIT_SG,
        ];
        for v in x {
            assert!(v.is_power_of_two());
        }
        // Seven dense bits.
        assert_eq!(NETDEV_XDP_ACT_MASK, 0x7F);
    }

    #[test]
    fn test_rx_metadata_bits_dense() {
        let m = [
            NETDEV_XDP_RX_METADATA_TIMESTAMP,
            NETDEV_XDP_RX_METADATA_HASH,
            NETDEV_XDP_RX_METADATA_VLAN_TAG,
        ];
        for v in m {
            assert!(v.is_power_of_two());
        }
        // Three dense bits.
        assert_eq!(m.iter().fold(0u64, |a, b| a | b), 0x7);
    }

    #[test]
    fn test_queue_types_dense_0_1() {
        assert_eq!(NETDEV_QUEUE_TYPE_RX, 0);
        assert_eq!(NETDEV_QUEUE_TYPE_TX, 1);
    }

    #[test]
    fn test_qstats_scope_value() {
        assert_eq!(NETDEV_QSTATS_SCOPE_QUEUE, 1);
    }
}

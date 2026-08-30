//! `<linux/connector.h>` — Connector netlink subsystem constants.
//!
//! The connector provides a simple netlink-based pub-sub for kernel
//! subsystems that need to push events to userspace (process exec/exit,
//! filesystem capability change, etc.). Each producer has a {idx,val}
//! identifier and userspace subscribes via NETLINK_CONNECTOR.

// ---------------------------------------------------------------------------
// Connector netlink family
// ---------------------------------------------------------------------------

/// netlink family for connector messages.
pub const NETLINK_CONNECTOR: u32 = 11;

// ---------------------------------------------------------------------------
// Connector subsystem identifiers (cb_id { idx, val })
// ---------------------------------------------------------------------------

pub const CN_IDX_PROC: u32 = 0x1;
pub const CN_VAL_PROC: u32 = 0x1;

pub const CN_IDX_CIFS: u32 = 0x2;
pub const CN_VAL_CIFS: u32 = 0x1;

pub const CN_W1_IDX: u32 = 0x3;
pub const CN_W1_VAL: u32 = 0x1;

pub const CN_IDX_V86D: u32 = 0x4;
pub const CN_VAL_V86D_UVESAFB: u32 = 0x1;

pub const CN_IDX_BB: u32 = 0x5;

pub const CN_DST_IDX: u32 = 0x6;
pub const CN_DST_VAL: u32 = 0x1;

pub const CN_IDX_DM: u32 = 0x7;
pub const CN_VAL_DM_USERSPACE_LOG: u32 = 0x1;

pub const CN_IDX_DRBD: u32 = 0x8;
pub const CN_VAL_DRBD: u32 = 0x1;

pub const CN_KVP_IDX: u32 = 0x9;
pub const CN_KVP_VAL: u32 = 0x1;

pub const CN_VSS_IDX: u32 = 0xA;
pub const CN_VSS_VAL: u32 = 0x1;

// ---------------------------------------------------------------------------
// Maximum payload sizes
// ---------------------------------------------------------------------------

/// Maximum connector message payload (kernel default).
pub const CN_PAYLOAD_MAX: usize = 1024;

/// struct cn_msg fixed header size:
/// `__u32 id_idx + __u32 id_val + __u32 seq + __u32 ack + __u16 len + __u16 flags`.
pub const CN_MSG_HEADER_SIZE: usize = 20;

// ---------------------------------------------------------------------------
// Connector multicast group
// ---------------------------------------------------------------------------

/// netlink multicast group for connector — bind on this to subscribe.
pub const CN_NETLINK_USERS: u32 = 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_netlink_family_is_11() {
        assert_eq!(NETLINK_CONNECTOR, 11);
    }

    #[test]
    fn test_proc_connector_is_first() {
        // CN_IDX_PROC == 0x1 (the proc connector was the original use).
        assert_eq!(CN_IDX_PROC, 1);
        assert_eq!(CN_VAL_PROC, 1);
    }

    #[test]
    fn test_indices_distinct_and_dense() {
        let idx = [
            CN_IDX_PROC,
            CN_IDX_CIFS,
            CN_W1_IDX,
            CN_IDX_V86D,
            CN_IDX_BB,
            CN_DST_IDX,
            CN_IDX_DM,
            CN_IDX_DRBD,
            CN_KVP_IDX,
            CN_VSS_IDX,
        ];
        for (i, &v) in idx.iter().enumerate() {
            // Indices are 1..=10 (1-based).
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_msg_header_size_is_20() {
        // 6 fields totalling 20 bytes: 4*u32 + 2*u16.
        assert_eq!(CN_MSG_HEADER_SIZE, 4 * 4 + 2 * 2);
    }

    #[test]
    fn test_payload_max_is_1k() {
        assert_eq!(CN_PAYLOAD_MAX, 1024);
        assert!(CN_PAYLOAD_MAX.is_power_of_two());
    }

    #[test]
    fn test_users_count_matches_family_id() {
        // Coincidence in the kernel headers — both are 11.
        assert_eq!(CN_NETLINK_USERS, NETLINK_CONNECTOR);
    }
}

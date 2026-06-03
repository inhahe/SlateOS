//! `<scsi/iscsi_if.h>` — iSCSI transport netlink user ABI.
//!
//! `iscsid(8)` and `open-iscsi` drive the kernel's iSCSI transport
//! layer via the `NETLINK_ISCSI` socket family. The constants below
//! match `include/uapi/scsi/iscsi_if.h` and define the netlink
//! message types, session/connection states, and the well-known
//! TCP port for iSCSI targets.

// ---------------------------------------------------------------------------
// Netlink family and well-known endpoints
// ---------------------------------------------------------------------------

/// `NETLINK_ISCSI` family number.
pub const NETLINK_ISCSI: u32 = 8;
/// IANA-assigned iSCSI target port (RFC 3720).
pub const ISCSI_LISTEN_PORT: u16 = 3260;
/// Default `iscsid.conf` path.
pub const ISCSI_CONFIG_PATH: &str = "/etc/iscsi/iscsid.conf";

// ---------------------------------------------------------------------------
// `enum iscsi_uevent_e` — netlink message types
// ---------------------------------------------------------------------------

pub const ISCSI_UEVENT_CREATE_SESSION: u32 = 0x0001;
pub const ISCSI_UEVENT_DESTROY_SESSION: u32 = 0x0002;
pub const ISCSI_UEVENT_CREATE_CONN: u32 = 0x0003;
pub const ISCSI_UEVENT_DESTROY_CONN: u32 = 0x0004;
pub const ISCSI_UEVENT_BIND_CONN: u32 = 0x0005;
pub const ISCSI_UEVENT_SET_PARAM: u32 = 0x0006;
pub const ISCSI_UEVENT_START_CONN: u32 = 0x0007;
pub const ISCSI_UEVENT_STOP_CONN: u32 = 0x0008;
pub const ISCSI_UEVENT_SEND_PDU: u32 = 0x0009;
pub const ISCSI_UEVENT_GET_STATS: u32 = 0x000A;
pub const ISCSI_UEVENT_GET_PARAM: u32 = 0x000B;

// ---------------------------------------------------------------------------
// Stop-connection reason codes
// ---------------------------------------------------------------------------

pub const STOP_CONN_TERM: u32 = 0xFFFF_FFFF;
pub const STOP_CONN_SUSPEND: u32 = 1;
pub const STOP_CONN_RECOVER: u32 = 3;

// ---------------------------------------------------------------------------
// Connection states (`enum iscsi_conn_state`)
// ---------------------------------------------------------------------------

pub const ISCSI_CONN_STATE_FREE: u32 = 0;
pub const ISCSI_CONN_STATE_XPT_WAIT: u32 = 1;
pub const ISCSI_CONN_STATE_IN_LOGIN: u32 = 2;
pub const ISCSI_CONN_STATE_LOGGED_IN: u32 = 3;
pub const ISCSI_CONN_STATE_IN_LOGOUT: u32 = 4;
pub const ISCSI_CONN_STATE_LOGOUT_REQUESTED: u32 = 5;
pub const ISCSI_CONN_STATE_CLEANUP_WAIT: u32 = 6;

// ---------------------------------------------------------------------------
// Buffer sizes
// ---------------------------------------------------------------------------

/// iSCSI Name (iqn.) max length per RFC 3722.
pub const ISCSI_MAX_NAME_LEN: usize = 224;
/// Alias max length per RFC 3720 §12.1.
pub const ISCSI_MAX_ALIAS_LEN: usize = 256;
/// Per-PDU header digest length when CRC32C is selected.
pub const ISCSI_DIGEST_SIZE: usize = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_well_known_endpoints() {
        // IANA-assigned, baked into every iSCSI initiator/target.
        assert_eq!(ISCSI_LISTEN_PORT, 3260);
        assert_eq!(NETLINK_ISCSI, 8);
        assert!(ISCSI_CONFIG_PATH.starts_with('/'));
    }

    #[test]
    fn test_uevents_dense_1_to_0xb() {
        let e = [
            ISCSI_UEVENT_CREATE_SESSION,
            ISCSI_UEVENT_DESTROY_SESSION,
            ISCSI_UEVENT_CREATE_CONN,
            ISCSI_UEVENT_DESTROY_CONN,
            ISCSI_UEVENT_BIND_CONN,
            ISCSI_UEVENT_SET_PARAM,
            ISCSI_UEVENT_START_CONN,
            ISCSI_UEVENT_STOP_CONN,
            ISCSI_UEVENT_SEND_PDU,
            ISCSI_UEVENT_GET_STATS,
            ISCSI_UEVENT_GET_PARAM,
        ];
        for (i, &v) in e.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_stop_codes_distinct() {
        assert_ne!(STOP_CONN_TERM, STOP_CONN_SUSPEND);
        assert_ne!(STOP_CONN_TERM, STOP_CONN_RECOVER);
        assert_ne!(STOP_CONN_SUSPEND, STOP_CONN_RECOVER);
        // TERM is encoded as -1 cast to u32.
        assert_eq!(STOP_CONN_TERM, !0u32);
    }

    #[test]
    fn test_conn_states_dense_0_to_6() {
        let s = [
            ISCSI_CONN_STATE_FREE,
            ISCSI_CONN_STATE_XPT_WAIT,
            ISCSI_CONN_STATE_IN_LOGIN,
            ISCSI_CONN_STATE_LOGGED_IN,
            ISCSI_CONN_STATE_IN_LOGOUT,
            ISCSI_CONN_STATE_LOGOUT_REQUESTED,
            ISCSI_CONN_STATE_CLEANUP_WAIT,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_buffer_sizes_sane() {
        // RFC 3722 caps iSCSI name at 223 chars + NUL → 224.
        assert_eq!(ISCSI_MAX_NAME_LEN, 224);
        assert!(ISCSI_MAX_ALIAS_LEN > ISCSI_MAX_NAME_LEN);
        // CRC32C is a 4-byte digest.
        assert_eq!(ISCSI_DIGEST_SIZE, 4);
    }
}

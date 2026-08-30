//! `<linux/iscsi_if.h>` — Additional iSCSI constants.
//!
//! Supplementary iSCSI constants covering event types,
//! parameter types, session states, and connection states.

// ---------------------------------------------------------------------------
// iSCSI event types (ISCSI_UEVENT_*)
// ---------------------------------------------------------------------------

/// Create session.
pub const ISCSI_UEVENT_CREATE_SESSION: u32 = 0x01;
/// Destroy session.
pub const ISCSI_UEVENT_DESTROY_SESSION: u32 = 0x02;
/// Create connection.
pub const ISCSI_UEVENT_CREATE_CONN: u32 = 0x03;
/// Destroy connection.
pub const ISCSI_UEVENT_DESTROY_CONN: u32 = 0x04;
/// Bind connection.
pub const ISCSI_UEVENT_BIND_CONN: u32 = 0x05;
/// Start connection.
pub const ISCSI_UEVENT_START_CONN: u32 = 0x06;
/// Stop connection.
pub const ISCSI_UEVENT_STOP_CONN: u32 = 0x07;
/// Send PDU.
pub const ISCSI_UEVENT_SEND_PDU: u32 = 0x08;
/// Set host parameter.
pub const ISCSI_UEVENT_SET_HOST_PARAM: u32 = 0x09;
/// Set iface parameter.
pub const ISCSI_UEVENT_SET_IFACE_PARAMS: u32 = 0x0A;
/// Get stats.
pub const ISCSI_UEVENT_GET_STATS: u32 = 0x0B;
/// Set parameter.
pub const ISCSI_UEVENT_SET_PARAM: u32 = 0x0C;
/// Transport register.
pub const ISCSI_UEVENT_TRANSPORT_EP_CONNECT: u32 = 0x0D;
/// Transport poll.
pub const ISCSI_UEVENT_TRANSPORT_EP_POLL: u32 = 0x0E;
/// Transport disconnect.
pub const ISCSI_UEVENT_TRANSPORT_EP_DISCONNECT: u32 = 0x0F;

// ---------------------------------------------------------------------------
// iSCSI session states
// ---------------------------------------------------------------------------

/// Logged in.
pub const ISCSI_SESSION_LOGGED_IN: u32 = 0;
/// Recovery.
pub const ISCSI_SESSION_RECOVERY: u32 = 1;
/// Free.
pub const ISCSI_SESSION_FREE: u32 = 2;
/// Failed.
pub const ISCSI_SESSION_FAILED: u32 = 3;

// ---------------------------------------------------------------------------
// iSCSI connection states
// ---------------------------------------------------------------------------

/// Login (initial).
pub const ISCSI_CONN_LOGIN: u32 = 0;
/// Logged in.
pub const ISCSI_CONN_LOGGED_IN: u32 = 1;
/// Cleanup wait.
pub const ISCSI_CONN_CLEANUP_WAIT: u32 = 2;
/// Started.
pub const ISCSI_CONN_STARTED: u32 = 3;
/// Stopped.
pub const ISCSI_CONN_STOPPED: u32 = 4;
/// Failed.
pub const ISCSI_CONN_FAILED: u32 = 5;

// ---------------------------------------------------------------------------
// iSCSI parameter types (ISCSI_PARAM_*)
// ---------------------------------------------------------------------------

/// Max recv data segment length.
pub const ISCSI_PARAM_MAX_RECV_DLENGTH: u32 = 0;
/// Max xmit data segment length.
pub const ISCSI_PARAM_MAX_XMIT_DLENGTH: u32 = 1;
/// Header digest.
pub const ISCSI_PARAM_HDRDGST_EN: u32 = 2;
/// Data digest.
pub const ISCSI_PARAM_DATADGST_EN: u32 = 3;
/// Initial R2T.
pub const ISCSI_PARAM_INITIAL_R2T_EN: u32 = 4;
/// Max R2T.
pub const ISCSI_PARAM_MAX_R2T: u32 = 5;
/// Immediate data.
pub const ISCSI_PARAM_IMM_DATA_EN: u32 = 6;
/// First burst length.
pub const ISCSI_PARAM_FIRST_BURST: u32 = 7;
/// Max burst length.
pub const ISCSI_PARAM_MAX_BURST: u32 = 8;
/// PDU inorder.
pub const ISCSI_PARAM_PDU_INORDER_EN: u32 = 9;
/// Data sequence inorder.
pub const ISCSI_PARAM_DATASEQ_INORDER_EN: u32 = 10;
/// Default time2wait.
pub const ISCSI_PARAM_DEF_TIME2WAIT: u32 = 11;
/// Default time2retain.
pub const ISCSI_PARAM_DEF_TIME2RETAIN: u32 = 12;

// ---------------------------------------------------------------------------
// iSCSI error codes
// ---------------------------------------------------------------------------

/// Success.
pub const ISCSI_OK: u32 = 0;
/// Error: data digest.
pub const ISCSI_ERR_DATASN: u32 = 1;
/// Error: data offset.
pub const ISCSI_ERR_DATA_OFFSET: u32 = 2;
/// Error: max cmds.
pub const ISCSI_ERR_MAX_CMDS: u32 = 3;
/// Error: connection failed.
pub const ISCSI_ERR_CONN_FAILED: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uevent_types_distinct() {
        let events = [
            ISCSI_UEVENT_CREATE_SESSION,
            ISCSI_UEVENT_DESTROY_SESSION,
            ISCSI_UEVENT_CREATE_CONN,
            ISCSI_UEVENT_DESTROY_CONN,
            ISCSI_UEVENT_BIND_CONN,
            ISCSI_UEVENT_START_CONN,
            ISCSI_UEVENT_STOP_CONN,
            ISCSI_UEVENT_SEND_PDU,
            ISCSI_UEVENT_SET_HOST_PARAM,
            ISCSI_UEVENT_SET_IFACE_PARAMS,
            ISCSI_UEVENT_GET_STATS,
            ISCSI_UEVENT_SET_PARAM,
            ISCSI_UEVENT_TRANSPORT_EP_CONNECT,
            ISCSI_UEVENT_TRANSPORT_EP_POLL,
            ISCSI_UEVENT_TRANSPORT_EP_DISCONNECT,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_session_states_distinct() {
        let states = [
            ISCSI_SESSION_LOGGED_IN,
            ISCSI_SESSION_RECOVERY,
            ISCSI_SESSION_FREE,
            ISCSI_SESSION_FAILED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_conn_states_distinct() {
        let states = [
            ISCSI_CONN_LOGIN,
            ISCSI_CONN_LOGGED_IN,
            ISCSI_CONN_CLEANUP_WAIT,
            ISCSI_CONN_STARTED,
            ISCSI_CONN_STOPPED,
            ISCSI_CONN_FAILED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_params_distinct() {
        let params = [
            ISCSI_PARAM_MAX_RECV_DLENGTH,
            ISCSI_PARAM_MAX_XMIT_DLENGTH,
            ISCSI_PARAM_HDRDGST_EN,
            ISCSI_PARAM_DATADGST_EN,
            ISCSI_PARAM_INITIAL_R2T_EN,
            ISCSI_PARAM_MAX_R2T,
            ISCSI_PARAM_IMM_DATA_EN,
            ISCSI_PARAM_FIRST_BURST,
            ISCSI_PARAM_MAX_BURST,
            ISCSI_PARAM_PDU_INORDER_EN,
            ISCSI_PARAM_DATASEQ_INORDER_EN,
            ISCSI_PARAM_DEF_TIME2WAIT,
            ISCSI_PARAM_DEF_TIME2RETAIN,
        ];
        for i in 0..params.len() {
            for j in (i + 1)..params.len() {
                assert_ne!(params[i], params[j]);
            }
        }
    }

    #[test]
    fn test_error_codes_distinct() {
        let errs = [
            ISCSI_OK,
            ISCSI_ERR_DATASN,
            ISCSI_ERR_DATA_OFFSET,
            ISCSI_ERR_MAX_CMDS,
            ISCSI_ERR_CONN_FAILED,
        ];
        for i in 0..errs.len() {
            for j in (i + 1)..errs.len() {
                assert_ne!(errs[i], errs[j]);
            }
        }
    }
}

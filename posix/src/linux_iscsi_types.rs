//! `<linux/iscsi_if.h>` — iSCSI (Internet SCSI) constants.
//!
//! iSCSI transports SCSI commands over TCP/IP networks, allowing
//! remote block storage access without specialized hardware (unlike
//! Fibre Channel). The initiator (client) connects to targets
//! (storage servers) and maps remote LUNs as local SCSI devices.

// ---------------------------------------------------------------------------
// iSCSI session states
// ---------------------------------------------------------------------------

/// Session logged in.
pub const ISCSI_STATE_LOGGED_IN: u8 = 0;
/// Session failed (transport error).
pub const ISCSI_STATE_FAILED: u8 = 1;
/// Session in recovery.
pub const ISCSI_STATE_RECOVERY: u8 = 2;
/// Session terminated.
pub const ISCSI_STATE_TERMINATE: u8 = 3;

// ---------------------------------------------------------------------------
// iSCSI connection states
// ---------------------------------------------------------------------------

/// Connection up.
pub const ISCSI_CONN_UP: u8 = 0;
/// Connection down.
pub const ISCSI_CONN_DOWN: u8 = 1;
/// Connection failed.
pub const ISCSI_CONN_FAILED: u8 = 2;
/// Connection bound (to session).
pub const ISCSI_CONN_BOUND: u8 = 3;

// ---------------------------------------------------------------------------
// iSCSI PDU opcodes (initiator)
// ---------------------------------------------------------------------------

/// NOP-Out.
pub const ISCSI_OP_NOP_OUT: u8 = 0x00;
/// SCSI command.
pub const ISCSI_OP_SCSI_CMD: u8 = 0x01;
/// Task management request.
pub const ISCSI_OP_TASK_MGT_REQ: u8 = 0x02;
/// Login request.
pub const ISCSI_OP_LOGIN_REQ: u8 = 0x03;
/// Text request.
pub const ISCSI_OP_TEXT_REQ: u8 = 0x04;
/// Data-Out.
pub const ISCSI_OP_DATA_OUT: u8 = 0x05;
/// Logout request.
pub const ISCSI_OP_LOGOUT_REQ: u8 = 0x06;

// ---------------------------------------------------------------------------
// iSCSI PDU opcodes (target)
// ---------------------------------------------------------------------------

/// NOP-In.
pub const ISCSI_OP_NOP_IN: u8 = 0x20;
/// SCSI response.
pub const ISCSI_OP_SCSI_RSP: u8 = 0x21;
/// Task management response.
pub const ISCSI_OP_TASK_MGT_RSP: u8 = 0x22;
/// Login response.
pub const ISCSI_OP_LOGIN_RSP: u8 = 0x23;
/// Text response.
pub const ISCSI_OP_TEXT_RSP: u8 = 0x24;
/// Data-In.
pub const ISCSI_OP_DATA_IN: u8 = 0x25;
/// Logout response.
pub const ISCSI_OP_LOGOUT_RSP: u8 = 0x26;
/// Ready To Transfer.
pub const ISCSI_OP_R2T: u8 = 0x31;
/// Reject.
pub const ISCSI_OP_REJECT: u8 = 0x3F;

// ---------------------------------------------------------------------------
// Default port
// ---------------------------------------------------------------------------

/// iSCSI default TCP port.
pub const ISCSI_DEFAULT_PORT: u16 = 3260;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_states_distinct() {
        let states = [
            ISCSI_STATE_LOGGED_IN,
            ISCSI_STATE_FAILED,
            ISCSI_STATE_RECOVERY,
            ISCSI_STATE_TERMINATE,
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
            ISCSI_CONN_UP,
            ISCSI_CONN_DOWN,
            ISCSI_CONN_FAILED,
            ISCSI_CONN_BOUND,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_initiator_opcodes_distinct() {
        let ops = [
            ISCSI_OP_NOP_OUT,
            ISCSI_OP_SCSI_CMD,
            ISCSI_OP_TASK_MGT_REQ,
            ISCSI_OP_LOGIN_REQ,
            ISCSI_OP_TEXT_REQ,
            ISCSI_OP_DATA_OUT,
            ISCSI_OP_LOGOUT_REQ,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_target_opcodes_distinct() {
        let ops = [
            ISCSI_OP_NOP_IN,
            ISCSI_OP_SCSI_RSP,
            ISCSI_OP_TASK_MGT_RSP,
            ISCSI_OP_LOGIN_RSP,
            ISCSI_OP_TEXT_RSP,
            ISCSI_OP_DATA_IN,
            ISCSI_OP_LOGOUT_RSP,
            ISCSI_OP_R2T,
            ISCSI_OP_REJECT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_default_port() {
        assert_eq!(ISCSI_DEFAULT_PORT, 3260);
    }
}

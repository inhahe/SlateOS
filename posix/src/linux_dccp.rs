//! `<linux/dccp.h>` — Datagram Congestion Control Protocol constants.
//!
//! DCCP (RFC 4340) provides unreliable datagram transport with
//! congestion control. It is designed for streaming media and
//! telephony applications where TCP's reliability is unnecessary
//! but congestion control is still needed.

// ---------------------------------------------------------------------------
// Packet types
// ---------------------------------------------------------------------------

/// Request (connection initiation).
pub const DCCP_PKT_REQUEST: u8 = 0;
/// Response (connection acceptance).
pub const DCCP_PKT_RESPONSE: u8 = 1;
/// Data packet.
pub const DCCP_PKT_DATA: u8 = 2;
/// Acknowledgement.
pub const DCCP_PKT_ACK: u8 = 3;
/// Data + Ack combined.
pub const DCCP_PKT_DATAACK: u8 = 4;
/// Close request.
pub const DCCP_PKT_CLOSEREQ: u8 = 5;
/// Close.
pub const DCCP_PKT_CLOSE: u8 = 6;
/// Reset (abort).
pub const DCCP_PKT_RESET: u8 = 7;
/// Sync.
pub const DCCP_PKT_SYNC: u8 = 8;
/// Sync + Ack.
pub const DCCP_PKT_SYNCACK: u8 = 9;

// ---------------------------------------------------------------------------
// Reset codes
// ---------------------------------------------------------------------------

/// Unspecified reset.
pub const DCCP_RESET_UNSPECIFIED: u8 = 0;
/// Closed (normal termination).
pub const DCCP_RESET_CLOSED: u8 = 1;
/// Aborted.
pub const DCCP_RESET_ABORTED: u8 = 2;
/// No connection.
pub const DCCP_RESET_NO_CONNECTION: u8 = 3;
/// Packet error.
pub const DCCP_RESET_PACKET_ERROR: u8 = 4;
/// Option error.
pub const DCCP_RESET_OPTION_ERROR: u8 = 5;
/// Mandatory error.
pub const DCCP_RESET_MANDATORY_ERROR: u8 = 6;
/// Connection refused.
pub const DCCP_RESET_CONNECTION_REFUSED: u8 = 7;
/// Bad service code.
pub const DCCP_RESET_BAD_SERVICE: u8 = 8;
/// Too busy.
pub const DCCP_RESET_TOO_BUSY: u8 = 9;
/// Bad init cookie.
pub const DCCP_RESET_BAD_INIT_COOKIE: u8 = 10;
/// Aggression penalty.
pub const DCCP_RESET_AGGRESSION: u8 = 11;

// ---------------------------------------------------------------------------
// Congestion Control IDs (CCIDs)
// ---------------------------------------------------------------------------

/// CCID 2: TCP-like congestion control.
pub const DCCP_CCID2: u8 = 2;
/// CCID 3: TCP-Friendly Rate Control (TFRC).
pub const DCCP_CCID3: u8 = 3;

// ---------------------------------------------------------------------------
// Socket options
// ---------------------------------------------------------------------------

/// Set/get service code.
pub const DCCP_SOCKOPT_SERVICE: u32 = 2;
/// Change L4 (CCID).
pub const DCCP_SOCKOPT_CHANGE_L: u32 = 3;
/// Change R4 (CCID).
pub const DCCP_SOCKOPT_CHANGE_R: u32 = 4;
/// Get current TX CCID.
pub const DCCP_SOCKOPT_GET_CUR_MPS: u32 = 5;
/// Set server timewait.
pub const DCCP_SOCKOPT_SERVER_TIMEWAIT: u32 = 6;
/// Set TX CCID.
pub const DCCP_SOCKOPT_CCID_TX_INFO: u32 = 192;
/// Set RX CCID.
pub const DCCP_SOCKOPT_CCID_RX_INFO: u32 = 193;
/// Available CCIDs.
pub const DCCP_SOCKOPT_AVAILABLE_CCIDS: u32 = 12;
/// TX CCID.
pub const DCCP_SOCKOPT_TX_CCID: u32 = 14;
/// RX CCID.
pub const DCCP_SOCKOPT_RX_CCID: u32 = 15;
/// Queue length.
pub const DCCP_SOCKOPT_QPOLICY_TXQLEN: u32 = 16;

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

/// Closed.
pub const DCCP_STATE_CLOSED: u8 = 0;
/// Request sent.
pub const DCCP_STATE_REQUEST: u8 = 1;
/// Respond (server waiting).
pub const DCCP_STATE_RESPOND: u8 = 2;
/// Open (data transfer).
pub const DCCP_STATE_OPEN: u8 = 3;
/// Close request sent.
pub const DCCP_STATE_CLOSEREQ: u8 = 4;
/// Closing.
pub const DCCP_STATE_CLOSING: u8 = 5;
/// Time-wait.
pub const DCCP_STATE_TIMEWAIT: u8 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_types_distinct() {
        let types = [
            DCCP_PKT_REQUEST, DCCP_PKT_RESPONSE, DCCP_PKT_DATA,
            DCCP_PKT_ACK, DCCP_PKT_DATAACK, DCCP_PKT_CLOSEREQ,
            DCCP_PKT_CLOSE, DCCP_PKT_RESET, DCCP_PKT_SYNC,
            DCCP_PKT_SYNCACK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_reset_codes_distinct() {
        let codes = [
            DCCP_RESET_UNSPECIFIED, DCCP_RESET_CLOSED, DCCP_RESET_ABORTED,
            DCCP_RESET_NO_CONNECTION, DCCP_RESET_PACKET_ERROR,
            DCCP_RESET_OPTION_ERROR, DCCP_RESET_MANDATORY_ERROR,
            DCCP_RESET_CONNECTION_REFUSED, DCCP_RESET_BAD_SERVICE,
            DCCP_RESET_TOO_BUSY, DCCP_RESET_BAD_INIT_COOKIE,
            DCCP_RESET_AGGRESSION,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_ccids_distinct() {
        assert_ne!(DCCP_CCID2, DCCP_CCID3);
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            DCCP_STATE_CLOSED, DCCP_STATE_REQUEST, DCCP_STATE_RESPOND,
            DCCP_STATE_OPEN, DCCP_STATE_CLOSEREQ, DCCP_STATE_CLOSING,
            DCCP_STATE_TIMEWAIT,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            DCCP_SOCKOPT_SERVICE, DCCP_SOCKOPT_CHANGE_L,
            DCCP_SOCKOPT_CHANGE_R, DCCP_SOCKOPT_GET_CUR_MPS,
            DCCP_SOCKOPT_SERVER_TIMEWAIT, DCCP_SOCKOPT_CCID_TX_INFO,
            DCCP_SOCKOPT_CCID_RX_INFO, DCCP_SOCKOPT_AVAILABLE_CCIDS,
            DCCP_SOCKOPT_TX_CCID, DCCP_SOCKOPT_RX_CCID,
            DCCP_SOCKOPT_QPOLICY_TXQLEN,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}

//! `<linux/dccp.h>` — Additional DCCP constants.
//!
//! Supplementary DCCP constants covering packet types,
//! feature negotiation options, and reset codes.

// ---------------------------------------------------------------------------
// DCCP packet types
// ---------------------------------------------------------------------------

/// Request packet.
pub const DCCP_PKT_REQUEST: u8 = 0;
/// Response packet.
pub const DCCP_PKT_RESPONSE: u8 = 1;
/// Data packet.
pub const DCCP_PKT_DATA: u8 = 2;
/// Acknowledgment packet.
pub const DCCP_PKT_ACK: u8 = 3;
/// Data + acknowledgment packet.
pub const DCCP_PKT_DATAACK: u8 = 4;
/// Close-request packet.
pub const DCCP_PKT_CLOSEREQ: u8 = 5;
/// Close packet.
pub const DCCP_PKT_CLOSE: u8 = 6;
/// Reset packet.
pub const DCCP_PKT_RESET: u8 = 7;
/// Sync packet.
pub const DCCP_PKT_SYNC: u8 = 8;
/// Sync-acknowledgment packet.
pub const DCCP_PKT_SYNCACK: u8 = 9;
/// Invalid packet type.
pub const DCCP_PKT_INVALID: u8 = 15;

// ---------------------------------------------------------------------------
// DCCP reset codes
// ---------------------------------------------------------------------------

/// Unspecified reset.
pub const DCCP_RESET_CODE_UNSPECIFIED: u8 = 0;
/// Closed.
pub const DCCP_RESET_CODE_CLOSED: u8 = 1;
/// Aborted.
pub const DCCP_RESET_CODE_ABORTED: u8 = 2;
/// No connection.
pub const DCCP_RESET_CODE_NO_CONNECTION: u8 = 3;
/// Packet error.
pub const DCCP_RESET_CODE_PACKET_ERROR: u8 = 4;
/// Option error.
pub const DCCP_RESET_CODE_OPTION_ERROR: u8 = 5;
/// Mandatory error.
pub const DCCP_RESET_CODE_MANDATORY_ERROR: u8 = 6;
/// Connection refused.
pub const DCCP_RESET_CODE_CONNECTION_REFUSED: u8 = 7;
/// Bad service code.
pub const DCCP_RESET_CODE_BAD_SERVICE_CODE: u8 = 8;
/// Too busy.
pub const DCCP_RESET_CODE_TOO_BUSY: u8 = 9;
/// Bad init cookie.
pub const DCCP_RESET_CODE_BAD_INIT_COOKIE: u8 = 10;
/// Aggression penalty.
pub const DCCP_RESET_CODE_AGGRESSION_PENALTY: u8 = 11;

// ---------------------------------------------------------------------------
// DCCP feature negotiation options
// ---------------------------------------------------------------------------

/// Congestion control ID.
pub const DCCPO_CHANGE_L: u8 = 32;
/// Confirm feature (local).
pub const DCCPO_CONFIRM_L: u8 = 33;
/// Change feature (remote).
pub const DCCPO_CHANGE_R: u8 = 34;
/// Confirm feature (remote).
pub const DCCPO_CONFIRM_R: u8 = 35;

// ---------------------------------------------------------------------------
// DCCP congestion control IDs
// ---------------------------------------------------------------------------

/// CCID 2 — TCP-like.
pub const DCCPC_CCID2: u8 = 2;
/// CCID 3 — TFRC (TCP-Friendly Rate Control).
pub const DCCPC_CCID3: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkt_types_distinct() {
        let pkts = [
            DCCP_PKT_REQUEST,
            DCCP_PKT_RESPONSE,
            DCCP_PKT_DATA,
            DCCP_PKT_ACK,
            DCCP_PKT_DATAACK,
            DCCP_PKT_CLOSEREQ,
            DCCP_PKT_CLOSE,
            DCCP_PKT_RESET,
            DCCP_PKT_SYNC,
            DCCP_PKT_SYNCACK,
            DCCP_PKT_INVALID,
        ];
        for i in 0..pkts.len() {
            for j in (i + 1)..pkts.len() {
                assert_ne!(pkts[i], pkts[j]);
            }
        }
    }

    #[test]
    fn test_reset_codes_distinct() {
        let codes = [
            DCCP_RESET_CODE_UNSPECIFIED,
            DCCP_RESET_CODE_CLOSED,
            DCCP_RESET_CODE_ABORTED,
            DCCP_RESET_CODE_NO_CONNECTION,
            DCCP_RESET_CODE_PACKET_ERROR,
            DCCP_RESET_CODE_OPTION_ERROR,
            DCCP_RESET_CODE_MANDATORY_ERROR,
            DCCP_RESET_CODE_CONNECTION_REFUSED,
            DCCP_RESET_CODE_BAD_SERVICE_CODE,
            DCCP_RESET_CODE_TOO_BUSY,
            DCCP_RESET_CODE_BAD_INIT_COOKIE,
            DCCP_RESET_CODE_AGGRESSION_PENALTY,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_feature_options_distinct() {
        let opts = [
            DCCPO_CHANGE_L,
            DCCPO_CONFIRM_L,
            DCCPO_CHANGE_R,
            DCCPO_CONFIRM_R,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_ccids_distinct() {
        assert_ne!(DCCPC_CCID2, DCCPC_CCID3);
    }
}

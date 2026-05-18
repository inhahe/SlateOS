//! `<linux/dccp.h>` — Datagram Congestion Control Protocol constants.
//!
//! DCCP provides unreliable, congestion-controlled datagram transport.
//! These constants define packet types, options, feature negotiation
//! values, and socket options for DCCP connections.

// ---------------------------------------------------------------------------
// DCCP packet types
// ---------------------------------------------------------------------------

/// Request packet (initiates connection).
pub const DCCP_PKT_REQUEST: u8 = 0;
/// Response packet (server accepts).
pub const DCCP_PKT_RESPONSE: u8 = 1;
/// Data packet.
pub const DCCP_PKT_DATA: u8 = 2;
/// Acknowledgement packet.
pub const DCCP_PKT_ACK: u8 = 3;
/// Data + Acknowledgement packet.
pub const DCCP_PKT_DATAACK: u8 = 4;
/// Close-request packet.
pub const DCCP_PKT_CLOSEREQ: u8 = 5;
/// Close packet.
pub const DCCP_PKT_CLOSE: u8 = 6;
/// Reset packet.
pub const DCCP_PKT_RESET: u8 = 7;
/// Sync packet.
pub const DCCP_PKT_SYNC: u8 = 8;
/// Sync-ack packet.
pub const DCCP_PKT_SYNCACK: u8 = 9;

// ---------------------------------------------------------------------------
// DCCP reset codes
// ---------------------------------------------------------------------------

/// Unspecified reset.
pub const DCCP_RESET_UNSPECIFIED: u8 = 0;
/// Closed (normal).
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
pub const DCCP_RESET_BAD_SERVICE_CODE: u8 = 8;
/// Too busy.
pub const DCCP_RESET_TOO_BUSY: u8 = 9;
/// Bad init cookie.
pub const DCCP_RESET_BAD_INIT_COOKIE: u8 = 10;
/// Aggression penalty.
pub const DCCP_RESET_AGGRESSION_PENALTY: u8 = 11;

// ---------------------------------------------------------------------------
// DCCP socket options
// ---------------------------------------------------------------------------

/// Set the congestion control ID.
pub const DCCP_SOCKOPT_CCID: u32 = 13;
/// Get available CCIDs.
pub const DCCP_SOCKOPT_AVAILABLE_CCIDS: u32 = 12;
/// Set the service code.
pub const DCCP_SOCKOPT_SERVICE: u32 = 2;
/// Set TX CCID.
pub const DCCP_SOCKOPT_TX_CCID: u32 = 14;
/// Set RX CCID.
pub const DCCP_SOCKOPT_RX_CCID: u32 = 15;
/// Set the server timewait flag.
pub const DCCP_SOCKOPT_SERVER_TIMEWAIT: u32 = 6;
/// Get the current queue length.
pub const DCCP_SOCKOPT_QPOLICY_ID: u32 = 16;
/// Set maximum TX queue length.
pub const DCCP_SOCKOPT_QPOLICY_TXQLEN: u32 = 17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkt_types_distinct() {
        let pkts = [
            DCCP_PKT_REQUEST, DCCP_PKT_RESPONSE, DCCP_PKT_DATA,
            DCCP_PKT_ACK, DCCP_PKT_DATAACK, DCCP_PKT_CLOSEREQ,
            DCCP_PKT_CLOSE, DCCP_PKT_RESET, DCCP_PKT_SYNC,
            DCCP_PKT_SYNCACK,
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
            DCCP_RESET_UNSPECIFIED, DCCP_RESET_CLOSED,
            DCCP_RESET_ABORTED, DCCP_RESET_NO_CONNECTION,
            DCCP_RESET_PACKET_ERROR, DCCP_RESET_OPTION_ERROR,
            DCCP_RESET_MANDATORY_ERROR, DCCP_RESET_CONNECTION_REFUSED,
            DCCP_RESET_BAD_SERVICE_CODE, DCCP_RESET_TOO_BUSY,
            DCCP_RESET_BAD_INIT_COOKIE, DCCP_RESET_AGGRESSION_PENALTY,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_sockopt_distinct() {
        let opts = [
            DCCP_SOCKOPT_CCID, DCCP_SOCKOPT_AVAILABLE_CCIDS,
            DCCP_SOCKOPT_SERVICE, DCCP_SOCKOPT_TX_CCID,
            DCCP_SOCKOPT_RX_CCID, DCCP_SOCKOPT_SERVER_TIMEWAIT,
            DCCP_SOCKOPT_QPOLICY_ID, DCCP_SOCKOPT_QPOLICY_TXQLEN,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_request_is_zero() {
        assert_eq!(DCCP_PKT_REQUEST, 0);
    }
}

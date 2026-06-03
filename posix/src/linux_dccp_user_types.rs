//! `<linux/dccp.h>` — Datagram Congestion Control Protocol socket ABI.
//!
//! DCCP (RFC 4340) gives UDP-style unreliable delivery with TCP-style
//! congestion control. It's used by media-streaming stacks (RTP/DCCP)
//! and by experimental transports that want congestion-friendly
//! datagrams without TCP's head-of-line blocking.

// ---------------------------------------------------------------------------
// Protocol / socket-option level
// ---------------------------------------------------------------------------

/// `IPPROTO_DCCP` — DCCP IP protocol number (RFC 4340).
pub const IPPROTO_DCCP: u32 = 33;
/// `SOL_DCCP` socket option level.
pub const SOL_DCCP: u32 = 269;

// ---------------------------------------------------------------------------
// Socket options (level=SOL_DCCP)
// ---------------------------------------------------------------------------

/// `DCCP_SOCKOPT_PACKET_SIZE` — deprecated; left for ABI.
pub const DCCP_SOCKOPT_PACKET_SIZE: u32 = 1;
/// `DCCP_SOCKOPT_SERVICE` — DCCP service codes (RFC 4340 §8.1.2).
pub const DCCP_SOCKOPT_SERVICE: u32 = 2;
/// `DCCP_SOCKOPT_CHANGE_L` — request a feature change (local).
pub const DCCP_SOCKOPT_CHANGE_L: u32 = 3;
/// `DCCP_SOCKOPT_CHANGE_R` — request a feature change (remote).
pub const DCCP_SOCKOPT_CHANGE_R: u32 = 4;
/// `DCCP_SOCKOPT_GET_CUR_MPS` — current max packet size.
pub const DCCP_SOCKOPT_GET_CUR_MPS: u32 = 5;
/// `DCCP_SOCKOPT_SERVER_TIMEWAIT` — TIMEWAIT held by server, not client.
pub const DCCP_SOCKOPT_SERVER_TIMEWAIT: u32 = 6;
/// `DCCP_SOCKOPT_SEND_CSCOV` — partial-checksum coverage on send.
pub const DCCP_SOCKOPT_SEND_CSCOV: u32 = 10;
/// `DCCP_SOCKOPT_RECV_CSCOV` — minimum CCV coverage to accept on recv.
pub const DCCP_SOCKOPT_RECV_CSCOV: u32 = 11;
/// `DCCP_SOCKOPT_AVAILABLE_CCIDS` — query CCIDs the kernel supports.
pub const DCCP_SOCKOPT_AVAILABLE_CCIDS: u32 = 12;
/// `DCCP_SOCKOPT_CCID` — set CCID on tx and rx.
pub const DCCP_SOCKOPT_CCID: u32 = 13;
/// `DCCP_SOCKOPT_TX_CCID` — tx-side CCID only.
pub const DCCP_SOCKOPT_TX_CCID: u32 = 14;
/// `DCCP_SOCKOPT_RX_CCID` — rx-side CCID only.
pub const DCCP_SOCKOPT_RX_CCID: u32 = 15;
/// `DCCP_SOCKOPT_QPOLICY_ID` — tx queue policy.
pub const DCCP_SOCKOPT_QPOLICY_ID: u32 = 16;
/// `DCCP_SOCKOPT_QPOLICY_TXQLEN` — tx queue length.
pub const DCCP_SOCKOPT_QPOLICY_TXQLEN: u32 = 17;

// ---------------------------------------------------------------------------
// CCID identifiers (Congestion Control IDs, RFC 4340 §10)
// ---------------------------------------------------------------------------

/// CCID 2 — TCP-like (RFC 4341); default for most use.
pub const DCCPC_CCID2: u32 = 2;
/// CCID 3 — TFRC, smoother bandwidth for streaming (RFC 4342).
pub const DCCPC_CCID3: u32 = 3;

// ---------------------------------------------------------------------------
// Packet types (RFC 4340 §5.1)
// ---------------------------------------------------------------------------

/// DCCP-Request.
pub const DCCP_PKT_REQUEST: u32 = 0;
/// DCCP-Response.
pub const DCCP_PKT_RESPONSE: u32 = 1;
/// DCCP-Data.
pub const DCCP_PKT_DATA: u32 = 2;
/// DCCP-Ack.
pub const DCCP_PKT_ACK: u32 = 3;
/// DCCP-DataAck.
pub const DCCP_PKT_DATAACK: u32 = 4;
/// DCCP-CloseReq.
pub const DCCP_PKT_CLOSEREQ: u32 = 5;
/// DCCP-Close.
pub const DCCP_PKT_CLOSE: u32 = 6;
/// DCCP-Reset.
pub const DCCP_PKT_RESET: u32 = 7;
/// DCCP-Sync.
pub const DCCP_PKT_SYNC: u32 = 8;
/// DCCP-SyncAck.
pub const DCCP_PKT_SYNCACK: u32 = 9;
/// Invalid/reserved packet type.
pub const DCCP_PKT_INVALID: u32 = 11;

// ---------------------------------------------------------------------------
// Reset codes (RFC 4340 §5.6)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const DCCP_RESET_CODE_UNSPECIFIED: u32 = 0;
/// Closed.
pub const DCCP_RESET_CODE_CLOSED: u32 = 1;
/// Aborted.
pub const DCCP_RESET_CODE_ABORTED: u32 = 2;
/// No connection.
pub const DCCP_RESET_CODE_NO_CONNECTION: u32 = 3;
/// Packet error.
pub const DCCP_RESET_CODE_PACKET_ERROR: u32 = 4;
/// Option error.
pub const DCCP_RESET_CODE_OPTION_ERROR: u32 = 5;
/// Mandatory error.
pub const DCCP_RESET_CODE_MANDATORY_ERROR: u32 = 6;
/// Connection refused.
pub const DCCP_RESET_CODE_CONNECTION_REFUSED: u32 = 7;
/// Bad service code.
pub const DCCP_RESET_CODE_BAD_SERVICE_CODE: u32 = 8;
/// Too busy.
pub const DCCP_RESET_CODE_TOO_BUSY: u32 = 9;
/// Bad init cookie.
pub const DCCP_RESET_CODE_BAD_INIT_COOKIE: u32 = 10;
/// Aggression penalty.
pub const DCCP_RESET_CODE_AGGRESSION_PENALTY: u32 = 11;

// ---------------------------------------------------------------------------
// Tx queue policy IDs (set via DCCP_SOCKOPT_QPOLICY_ID)
// ---------------------------------------------------------------------------

/// Simple FIFO tx queue.
pub const DCCPQ_POLICY_SIMPLE: u32 = 0;
/// Priority-based tx queue (cmsg DCCP_SCM_PRIORITY).
pub const DCCPQ_POLICY_PRIO: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_and_level() {
        // IPPROTO_DCCP = 33 is the IANA assignment.
        assert_eq!(IPPROTO_DCCP, 33);
        // SOL_DCCP must not collide with SOL_SOCKET (1) or SOL_TCP (6).
        assert_eq!(SOL_DCCP, 269);
    }

    #[test]
    fn test_sockopts_distinct() {
        let s = [
            DCCP_SOCKOPT_PACKET_SIZE,
            DCCP_SOCKOPT_SERVICE,
            DCCP_SOCKOPT_CHANGE_L,
            DCCP_SOCKOPT_CHANGE_R,
            DCCP_SOCKOPT_GET_CUR_MPS,
            DCCP_SOCKOPT_SERVER_TIMEWAIT,
            DCCP_SOCKOPT_SEND_CSCOV,
            DCCP_SOCKOPT_RECV_CSCOV,
            DCCP_SOCKOPT_AVAILABLE_CCIDS,
            DCCP_SOCKOPT_CCID,
            DCCP_SOCKOPT_TX_CCID,
            DCCP_SOCKOPT_RX_CCID,
            DCCP_SOCKOPT_QPOLICY_ID,
            DCCP_SOCKOPT_QPOLICY_TXQLEN,
        ];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
        // CSCOV options are stable at 10/11; CCID grouping is 13..15.
        assert_eq!(DCCP_SOCKOPT_SEND_CSCOV, 10);
        assert_eq!(DCCP_SOCKOPT_RECV_CSCOV, 11);
        assert_eq!(DCCP_SOCKOPT_CCID, 13);
        assert_eq!(DCCP_SOCKOPT_TX_CCID, 14);
        assert_eq!(DCCP_SOCKOPT_RX_CCID, 15);
    }

    #[test]
    fn test_ccid_values() {
        // CCID 2 (TCP-like) and CCID 3 (TFRC) are the only IETF-blessed
        // CCIDs; anything else is experimental.
        assert_eq!(DCCPC_CCID2, 2);
        assert_eq!(DCCPC_CCID3, 3);
        assert_ne!(DCCPC_CCID2, DCCPC_CCID3);
    }

    #[test]
    fn test_packet_types_dense_0_to_9() {
        let p = [
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
        ];
        // Wire-format packet type is a 4-bit field; values 0..9 used,
        // 10 reserved, 11..15 invalid. The encoding is dense so we can
        // index dispatch tables directly.
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // INVALID must be in the reserved range.
        assert!(DCCP_PKT_INVALID >= 10 && DCCP_PKT_INVALID < 16);
        assert_eq!(DCCP_PKT_INVALID, 11);
    }

    #[test]
    fn test_reset_codes_dense() {
        let r = [
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
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_qpolicy_ids() {
        assert_eq!(DCCPQ_POLICY_SIMPLE, 0);
        assert_eq!(DCCPQ_POLICY_PRIO, 1);
        assert_ne!(DCCPQ_POLICY_SIMPLE, DCCPQ_POLICY_PRIO);
    }
}

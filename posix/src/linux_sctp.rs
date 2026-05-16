//! `<linux/sctp.h>` — Stream Control Transmission Protocol.
//!
//! SCTP is a transport-layer protocol that provides reliable,
//! message-oriented, multi-stream communication between hosts.

// ---------------------------------------------------------------------------
// SCTP socket options (SOL_SCTP)
// ---------------------------------------------------------------------------

/// SCTP socket option level.
pub const SOL_SCTP: i32 = 132;
/// IPPROTO_SCTP.
pub const IPPROTO_SCTP: i32 = 132;

/// SCTP_RTOINFO: RTO parameters.
pub const SCTP_RTOINFO: i32 = 0;
/// Association info.
pub const SCTP_ASSOCINFO: i32 = 1;
/// Init parameters.
pub const SCTP_INITMSG: i32 = 2;
/// Nodelay (like TCP_NODELAY).
pub const SCTP_NODELAY: i32 = 3;
/// Autoclose timeout.
pub const SCTP_AUTOCLOSE: i32 = 4;
/// Set/get default send parameters.
pub const SCTP_DEFAULT_SEND_PARAM: i32 = 10;
/// Event subscription.
pub const SCTP_EVENTS: i32 = 11;
/// Add IP to association.
pub const SCTP_SOCKOPT_BINDX_ADD: i32 = 100;
/// Remove IP from association.
pub const SCTP_SOCKOPT_BINDX_REM: i32 = 101;
/// Peer addresses.
pub const SCTP_SOCKOPT_PEELOFF: i32 = 102;
/// Get peer address params.
pub const SCTP_PEER_ADDR_PARAMS: i32 = 9;
/// Max segment size.
pub const SCTP_MAXSEG: i32 = 13;
/// Status of association.
pub const SCTP_STATUS: i32 = 14;
/// Get/set primary address.
pub const SCTP_PRIMARY_ADDR: i32 = 6;
/// Disable fragments.
pub const SCTP_DISABLE_FRAGMENTS: i32 = 8;
/// Max burst.
pub const SCTP_MAX_BURST: i32 = 20;

// ---------------------------------------------------------------------------
// SCTP chunk types
// ---------------------------------------------------------------------------

/// Data chunk.
pub const SCTP_CID_DATA: u8 = 0;
/// Initiation.
pub const SCTP_CID_INIT: u8 = 1;
/// Initiation acknowledgement.
pub const SCTP_CID_INIT_ACK: u8 = 2;
/// Selective acknowledgement.
pub const SCTP_CID_SACK: u8 = 3;
/// Heartbeat request.
pub const SCTP_CID_HEARTBEAT: u8 = 4;
/// Heartbeat acknowledgement.
pub const SCTP_CID_HEARTBEAT_ACK: u8 = 5;
/// Abort.
pub const SCTP_CID_ABORT: u8 = 6;
/// Shutdown.
pub const SCTP_CID_SHUTDOWN: u8 = 7;
/// Shutdown acknowledgement.
pub const SCTP_CID_SHUTDOWN_ACK: u8 = 8;
/// Error.
pub const SCTP_CID_ERROR: u8 = 9;
/// Cookie echo.
pub const SCTP_CID_COOKIE_ECHO: u8 = 10;
/// Cookie acknowledgement.
pub const SCTP_CID_COOKIE_ACK: u8 = 11;
/// Forward TSN (stream reset).
pub const SCTP_CID_FWD_TSN: u8 = 0xC0;

// ---------------------------------------------------------------------------
// SCTP association states
// ---------------------------------------------------------------------------

/// Closed.
pub const SCTP_STATE_CLOSED: i32 = 0;
/// Cookie wait.
pub const SCTP_STATE_COOKIE_WAIT: i32 = 1;
/// Cookie echoed.
pub const SCTP_STATE_COOKIE_ECHOED: i32 = 2;
/// Established.
pub const SCTP_STATE_ESTABLISHED: i32 = 3;
/// Shutdown pending.
pub const SCTP_STATE_SHUTDOWN_PENDING: i32 = 4;
/// Shutdown sent.
pub const SCTP_STATE_SHUTDOWN_SENT: i32 = 5;
/// Shutdown received.
pub const SCTP_STATE_SHUTDOWN_RECEIVED: i32 = 6;
/// Shutdown ack sent.
pub const SCTP_STATE_SHUTDOWN_ACK_SENT: i32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            SCTP_RTOINFO, SCTP_ASSOCINFO, SCTP_INITMSG,
            SCTP_NODELAY, SCTP_AUTOCLOSE, SCTP_DEFAULT_SEND_PARAM,
            SCTP_EVENTS, SCTP_PRIMARY_ADDR, SCTP_DISABLE_FRAGMENTS,
            SCTP_PEER_ADDR_PARAMS, SCTP_MAXSEG, SCTP_STATUS,
            SCTP_MAX_BURST,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_chunk_types_distinct() {
        let chunks = [
            SCTP_CID_DATA, SCTP_CID_INIT, SCTP_CID_INIT_ACK,
            SCTP_CID_SACK, SCTP_CID_HEARTBEAT, SCTP_CID_HEARTBEAT_ACK,
            SCTP_CID_ABORT, SCTP_CID_SHUTDOWN, SCTP_CID_SHUTDOWN_ACK,
            SCTP_CID_ERROR, SCTP_CID_COOKIE_ECHO, SCTP_CID_COOKIE_ACK,
            SCTP_CID_FWD_TSN,
        ];
        for i in 0..chunks.len() {
            for j in (i + 1)..chunks.len() {
                assert_ne!(chunks[i], chunks[j]);
            }
        }
    }

    #[test]
    fn test_states_sequential() {
        assert_eq!(SCTP_STATE_CLOSED, 0);
        assert_eq!(SCTP_STATE_COOKIE_WAIT, 1);
        assert_eq!(SCTP_STATE_ESTABLISHED, 3);
        assert_eq!(SCTP_STATE_SHUTDOWN_ACK_SENT, 7);
    }

    #[test]
    fn test_sol_sctp() {
        assert_eq!(SOL_SCTP, IPPROTO_SCTP);
        assert_eq!(SOL_SCTP, 132);
    }
}

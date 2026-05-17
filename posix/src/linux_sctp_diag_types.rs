//! `<linux/sctp.h>` — SCTP (Stream Control Transmission Protocol) diagnostics constants.
//!
//! SCTP is a transport protocol providing multi-streaming, multi-
//! homing, and message-oriented delivery. The diagnostics interface
//! allows `ss` to query SCTP association state, endpoint addresses,
//! and per-stream info via netlink. SCTP is used in telecom signaling
//! (SIGTRAN/SS7-over-IP), WebRTC data channels, and high-availability
//! systems requiring multi-homed connections.

// ---------------------------------------------------------------------------
// SCTP socket states
// ---------------------------------------------------------------------------

/// SCTP CLOSED state.
pub const SCTP_STATE_CLOSED: u32 = 0;
/// SCTP COOKIE_WAIT state (after INIT sent).
pub const SCTP_STATE_COOKIE_WAIT: u32 = 1;
/// SCTP COOKIE_ECHOED state.
pub const SCTP_STATE_COOKIE_ECHOED: u32 = 2;
/// SCTP ESTABLISHED state.
pub const SCTP_STATE_ESTABLISHED: u32 = 3;
/// SCTP SHUTDOWN_PENDING state.
pub const SCTP_STATE_SHUTDOWN_PENDING: u32 = 4;
/// SCTP SHUTDOWN_SENT state.
pub const SCTP_STATE_SHUTDOWN_SENT: u32 = 5;
/// SCTP SHUTDOWN_RECEIVED state.
pub const SCTP_STATE_SHUTDOWN_RECEIVED: u32 = 6;
/// SCTP SHUTDOWN_ACK_SENT state.
pub const SCTP_STATE_SHUTDOWN_ACK_SENT: u32 = 7;

// ---------------------------------------------------------------------------
// SCTP diag attributes (SCTP_DIAG_*)
// ---------------------------------------------------------------------------

/// Association info.
pub const SCTP_DIAG_ASSOC_INFO: u32 = 1;
/// VTag (verification tag).
pub const SCTP_DIAG_VTAG: u32 = 2;
/// Primary peer address.
pub const SCTP_DIAG_PEER_ADDR: u32 = 3;
/// Local addresses list.
pub const SCTP_DIAG_LOCAL_ADDRS: u32 = 4;
/// Peer addresses list.
pub const SCTP_DIAG_PEER_ADDRS: u32 = 5;
/// Counters.
pub const SCTP_DIAG_COUNTERS: u32 = 6;
/// Memory info.
pub const SCTP_DIAG_MEMINFO: u32 = 7;

// ---------------------------------------------------------------------------
// SCTP socket options (SOL_SCTP level)
// ---------------------------------------------------------------------------

/// SCTP socket option level.
pub const SOL_SCTP: u32 = 132;
/// Get/set SCTP events.
pub const SCTP_EVENTS: u32 = 11;
/// Disable Nagle algorithm.
pub const SCTP_NODELAY: u32 = 3;
/// Set maximum segment size.
pub const SCTP_MAXSEG: u32 = 13;
/// Get association info.
pub const SCTP_ASSOCINFO: u32 = 1;
/// Get/set init parameters.
pub const SCTP_INITMSG: u32 = 2;
/// Get/set default send parameters.
pub const SCTP_DEFAULT_SEND_PARAM: u32 = 10;
/// Primary address.
pub const SCTP_PRIMARY_ADDR: u32 = 6;
/// Peer address parameters.
pub const SCTP_PEER_ADDR_PARAMS: u32 = 9;
/// Autoclose timeout.
pub const SCTP_AUTOCLOSE: u32 = 4;

// ---------------------------------------------------------------------------
// SCTP init defaults
// ---------------------------------------------------------------------------

/// Default number of outbound streams.
pub const SCTP_DEFAULT_OUTSTREAMS: u32 = 10;
/// Default max inbound streams.
pub const SCTP_DEFAULT_INSTREAMS: u32 = 65535;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            SCTP_STATE_CLOSED, SCTP_STATE_COOKIE_WAIT,
            SCTP_STATE_COOKIE_ECHOED, SCTP_STATE_ESTABLISHED,
            SCTP_STATE_SHUTDOWN_PENDING, SCTP_STATE_SHUTDOWN_SENT,
            SCTP_STATE_SHUTDOWN_RECEIVED, SCTP_STATE_SHUTDOWN_ACK_SENT,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_diag_attrs_distinct() {
        let attrs = [
            SCTP_DIAG_ASSOC_INFO, SCTP_DIAG_VTAG,
            SCTP_DIAG_PEER_ADDR, SCTP_DIAG_LOCAL_ADDRS,
            SCTP_DIAG_PEER_ADDRS, SCTP_DIAG_COUNTERS,
            SCTP_DIAG_MEMINFO,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            SCTP_ASSOCINFO, SCTP_INITMSG, SCTP_NODELAY,
            SCTP_AUTOCLOSE, SCTP_PRIMARY_ADDR,
            SCTP_PEER_ADDR_PARAMS, SCTP_DEFAULT_SEND_PARAM,
            SCTP_EVENTS, SCTP_MAXSEG,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_sol_sctp() {
        assert_eq!(SOL_SCTP, 132);
    }

    #[test]
    fn test_state_ordering() {
        assert!(SCTP_STATE_CLOSED < SCTP_STATE_ESTABLISHED);
        assert!(SCTP_STATE_ESTABLISHED < SCTP_STATE_SHUTDOWN_ACK_SENT);
    }
}

//! `<linux/sctp.h>` — SCTP (Stream Control Transmission Protocol) constants.
//!
//! SCTP is a message-oriented transport protocol that supports
//! multi-homing (multiple IP addresses per endpoint), multi-streaming
//! (independent ordered streams within one association), and message
//! boundaries. Used in telecom signaling (SS7 over IP), WebRTC data
//! channels, and high-availability applications.

// ---------------------------------------------------------------------------
// SCTP socket options (SOL_SCTP level)
// ---------------------------------------------------------------------------

/// Socket option level for SCTP.
pub const SOL_SCTP: u32 = 132;

/// Receive SCTP events as ancillary data.
pub const SCTP_EVENTS: u32 = 11;
/// Get/set association parameters.
pub const SCTP_ASSOCINFO: u32 = 1;
/// Get/set initialization parameters.
pub const SCTP_INITMSG: u32 = 2;
/// Enable/disable Nagle (nodelay).
pub const SCTP_NODELAY: u32 = 3;
/// Set peer address parameters.
pub const SCTP_PEER_ADDR_PARAMS: u32 = 9;
/// Get/set default send parameters.
pub const SCTP_DEFAULT_SEND_PARAM: u32 = 10;
/// Get/set maximum segment size.
pub const SCTP_MAXSEG: u32 = 13;
/// Enable stream reconfiguration.
pub const SCTP_ENABLE_STREAM_RESET: u32 = 118;

// ---------------------------------------------------------------------------
// SCTP notification types
// ---------------------------------------------------------------------------

/// Association change notification.
pub const SCTP_ASSOC_CHANGE: u16 = 1;
/// Peer address change.
pub const SCTP_PEER_ADDR_CHANGE: u16 = 2;
/// Send failed notification.
pub const SCTP_SEND_FAILED: u16 = 4;
/// Remote error notification.
pub const SCTP_REMOTE_ERROR: u16 = 5;
/// Shutdown event.
pub const SCTP_SHUTDOWN_EVENT: u16 = 6;
/// Partial delivery event.
pub const SCTP_PARTIAL_DELIVERY_EVENT: u16 = 7;
/// Adaptation layer event.
pub const SCTP_ADAPTATION_INDICATION: u16 = 8;
/// Stream reset event.
pub const SCTP_STREAM_RESET_EVENT: u16 = 13;

// ---------------------------------------------------------------------------
// SCTP association states
// ---------------------------------------------------------------------------

/// Communication up (association established).
pub const SCTP_COMM_UP: u32 = 0;
/// Communication lost.
pub const SCTP_COMM_LOST: u32 = 1;
/// Association restart detected.
pub const SCTP_RESTART: u32 = 2;
/// Shutdown complete.
pub const SCTP_SHUTDOWN_COMP: u32 = 3;
/// Can't start association.
pub const SCTP_CANT_STR_ASSOC: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            SCTP_ASSOCINFO, SCTP_INITMSG, SCTP_NODELAY,
            SCTP_PEER_ADDR_PARAMS, SCTP_DEFAULT_SEND_PARAM,
            SCTP_EVENTS, SCTP_MAXSEG, SCTP_ENABLE_STREAM_RESET,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_notification_types_distinct() {
        let types = [
            SCTP_ASSOC_CHANGE, SCTP_PEER_ADDR_CHANGE,
            SCTP_SEND_FAILED, SCTP_REMOTE_ERROR,
            SCTP_SHUTDOWN_EVENT, SCTP_PARTIAL_DELIVERY_EVENT,
            SCTP_ADAPTATION_INDICATION, SCTP_STREAM_RESET_EVENT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_assoc_states_distinct() {
        let states = [
            SCTP_COMM_UP, SCTP_COMM_LOST, SCTP_RESTART,
            SCTP_SHUTDOWN_COMP, SCTP_CANT_STR_ASSOC,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_sol_sctp() {
        assert_eq!(SOL_SCTP, 132);
    }
}

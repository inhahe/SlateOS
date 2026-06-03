//! `<linux/sctp.h>` — Additional SCTP constants.
//!
//! Supplementary SCTP constants covering chunk types,
//! parameter types, and error cause codes.

// ---------------------------------------------------------------------------
// SCTP chunk types
// ---------------------------------------------------------------------------

/// Data chunk.
pub const SCTP_CID_DATA: u8 = 0;
/// Initiation chunk.
pub const SCTP_CID_INIT: u8 = 1;
/// Initiation acknowledgment.
pub const SCTP_CID_INIT_ACK: u8 = 2;
/// Selective acknowledgment.
pub const SCTP_CID_SACK: u8 = 3;
/// Heartbeat request.
pub const SCTP_CID_HEARTBEAT: u8 = 4;
/// Heartbeat acknowledgment.
pub const SCTP_CID_HEARTBEAT_ACK: u8 = 5;
/// Abort.
pub const SCTP_CID_ABORT: u8 = 6;
/// Shutdown.
pub const SCTP_CID_SHUTDOWN: u8 = 7;
/// Shutdown acknowledgment.
pub const SCTP_CID_SHUTDOWN_ACK: u8 = 8;
/// Operation error.
pub const SCTP_CID_ERROR: u8 = 9;
/// Cookie echo.
pub const SCTP_CID_COOKIE_ECHO: u8 = 10;
/// Cookie acknowledgment.
pub const SCTP_CID_COOKIE_ACK: u8 = 11;
/// Forward TSN (RFC 3758).
pub const SCTP_CID_FWD_TSN: u8 = 0xC0;
/// Authentication chunk (RFC 4895).
pub const SCTP_CID_AUTH: u8 = 0x0F;
/// Re-configuration chunk (RFC 6525).
pub const SCTP_CID_RECONF: u8 = 130;

// ---------------------------------------------------------------------------
// SCTP error cause codes
// ---------------------------------------------------------------------------

/// Invalid stream identifier.
pub const SCTP_ERROR_INV_STRM: u16 = 1;
/// Missing mandatory parameter.
pub const SCTP_ERROR_MISS_PARAM: u16 = 2;
/// Stale cookie error.
pub const SCTP_ERROR_STALE_COOKIE: u16 = 3;
/// Out of resource.
pub const SCTP_ERROR_NO_RESOURCE: u16 = 4;
/// Unresolvable address.
pub const SCTP_ERROR_DNS_FAILED: u16 = 5;
/// Unrecognized chunk type.
pub const SCTP_ERROR_UNKNOWN_CHUNK: u16 = 6;
/// Invalid mandatory parameter.
pub const SCTP_ERROR_INV_PARAM: u16 = 7;
/// Unrecognized parameters.
pub const SCTP_ERROR_UNKNOWN_PARAM: u16 = 8;
/// No user data.
pub const SCTP_ERROR_NO_DATA: u16 = 9;
/// Cookie received while shutting down.
pub const SCTP_ERROR_COOKIE_IN_SHUTDOWN: u16 = 10;
/// Restart with new addresses.
pub const SCTP_ERROR_RESTART: u16 = 11;
/// User-initiated abort.
pub const SCTP_ERROR_USER_ABORT: u16 = 12;
/// Protocol violation.
pub const SCTP_ERROR_PROTO_VIOLATION: u16 = 13;

// ---------------------------------------------------------------------------
// SCTP socket options
// ---------------------------------------------------------------------------

/// Receive info.
pub const SCTP_RECVRCVINFO: u32 = 32;
/// Receive next info.
pub const SCTP_RECVNXTINFO: u32 = 33;
/// Default send parameters.
pub const SCTP_DEFAULT_SNDINFO: u32 = 34;
/// Interleaving supported.
pub const SCTP_INTERLEAVING_SUPPORTED: u32 = 125;
/// Reconfig supported.
pub const SCTP_RECONFIG_SUPPORTED: u32 = 117;
/// Stream scheduler.
pub const SCTP_STREAM_SCHEDULER: u32 = 123;
/// Stream scheduler value.
pub const SCTP_STREAM_SCHEDULER_VALUE: u32 = 124;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_types_distinct() {
        let chunks = [
            SCTP_CID_DATA,
            SCTP_CID_INIT,
            SCTP_CID_INIT_ACK,
            SCTP_CID_SACK,
            SCTP_CID_HEARTBEAT,
            SCTP_CID_HEARTBEAT_ACK,
            SCTP_CID_ABORT,
            SCTP_CID_SHUTDOWN,
            SCTP_CID_SHUTDOWN_ACK,
            SCTP_CID_ERROR,
            SCTP_CID_COOKIE_ECHO,
            SCTP_CID_COOKIE_ACK,
            SCTP_CID_FWD_TSN,
            SCTP_CID_AUTH,
            SCTP_CID_RECONF,
        ];
        for i in 0..chunks.len() {
            for j in (i + 1)..chunks.len() {
                assert_ne!(chunks[i], chunks[j]);
            }
        }
    }

    #[test]
    fn test_error_causes_distinct() {
        let errors = [
            SCTP_ERROR_INV_STRM,
            SCTP_ERROR_MISS_PARAM,
            SCTP_ERROR_STALE_COOKIE,
            SCTP_ERROR_NO_RESOURCE,
            SCTP_ERROR_DNS_FAILED,
            SCTP_ERROR_UNKNOWN_CHUNK,
            SCTP_ERROR_INV_PARAM,
            SCTP_ERROR_UNKNOWN_PARAM,
            SCTP_ERROR_NO_DATA,
            SCTP_ERROR_COOKIE_IN_SHUTDOWN,
            SCTP_ERROR_RESTART,
            SCTP_ERROR_USER_ABORT,
            SCTP_ERROR_PROTO_VIOLATION,
        ];
        for i in 0..errors.len() {
            for j in (i + 1)..errors.len() {
                assert_ne!(errors[i], errors[j]);
            }
        }
    }

    #[test]
    fn test_socket_opts_distinct() {
        let opts = [
            SCTP_RECVRCVINFO,
            SCTP_RECVNXTINFO,
            SCTP_DEFAULT_SNDINFO,
            SCTP_INTERLEAVING_SUPPORTED,
            SCTP_RECONFIG_SUPPORTED,
            SCTP_STREAM_SCHEDULER,
            SCTP_STREAM_SCHEDULER_VALUE,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}

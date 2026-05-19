//! `<linux/rxrpc.h>` — Additional RxRPC constants.
//!
//! Supplementary RxRPC constants covering packet types,
//! abort codes, and security levels.

// ---------------------------------------------------------------------------
// RxRPC packet types
// ---------------------------------------------------------------------------

/// Data packet.
pub const RXRPC_PACKET_TYPE_DATA: u8 = 1;
/// ACK packet.
pub const RXRPC_PACKET_TYPE_ACK: u8 = 2;
/// Busy packet.
pub const RXRPC_PACKET_TYPE_BUSY: u8 = 3;
/// Abort packet.
pub const RXRPC_PACKET_TYPE_ABORT: u8 = 4;
/// ACKALL packet.
pub const RXRPC_PACKET_TYPE_ACKALL: u8 = 5;
/// Challenge packet.
pub const RXRPC_PACKET_TYPE_CHALLENGE: u8 = 6;
/// Response packet.
pub const RXRPC_PACKET_TYPE_RESPONSE: u8 = 7;
/// Debug packet.
pub const RXRPC_PACKET_TYPE_DEBUG: u8 = 8;
/// Version negotiation.
pub const RXRPC_PACKET_TYPE_VERSION: u8 = 13;

// ---------------------------------------------------------------------------
// RxRPC ACK reasons
// ---------------------------------------------------------------------------

/// Requested ACK.
pub const RXRPC_ACK_REQUESTED: u8 = 1;
/// Duplicate packet.
pub const RXRPC_ACK_DUPLICATE: u8 = 2;
/// Out of sequence.
pub const RXRPC_ACK_OUT_OF_SEQUENCE: u8 = 3;
/// Exceeded window.
pub const RXRPC_ACK_EXCEEDS_WINDOW: u8 = 4;
/// No-operation ACK.
pub const RXRPC_ACK_NOSPACE: u8 = 5;
/// Ping ACK.
pub const RXRPC_ACK_PING: u8 = 6;
/// Ping response.
pub const RXRPC_ACK_PING_RESPONSE: u8 = 7;
/// Delay ACK.
pub const RXRPC_ACK_DELAY: u8 = 8;
/// Idle ACK.
pub const RXRPC_ACK_IDLE: u8 = 9;

// ---------------------------------------------------------------------------
// RxRPC security levels
// ---------------------------------------------------------------------------

/// Plain (no security).
pub const RXRPC_SECURITY_PLAIN: u32 = 0;
/// Authentication only.
pub const RXRPC_SECURITY_AUTH: u32 = 1;
/// Authentication + encryption.
pub const RXRPC_SECURITY_ENCRYPT: u32 = 2;

// ---------------------------------------------------------------------------
// RxRPC security types
// ---------------------------------------------------------------------------

/// No security.
pub const RXRPC_SECURITY_NONE: u32 = 0;
/// RxKAD (Kerberos).
pub const RXRPC_SECURITY_RXKAD: u32 = 2;
/// RxGK (GSS-API).
pub const RXRPC_SECURITY_RXGK: u32 = 4;
/// YFS-RxGK.
pub const RXRPC_SECURITY_YFS_RXGK: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_types_distinct() {
        let types = [
            RXRPC_PACKET_TYPE_DATA, RXRPC_PACKET_TYPE_ACK,
            RXRPC_PACKET_TYPE_BUSY, RXRPC_PACKET_TYPE_ABORT,
            RXRPC_PACKET_TYPE_ACKALL, RXRPC_PACKET_TYPE_CHALLENGE,
            RXRPC_PACKET_TYPE_RESPONSE, RXRPC_PACKET_TYPE_DEBUG,
            RXRPC_PACKET_TYPE_VERSION,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ack_reasons_distinct() {
        let reasons = [
            RXRPC_ACK_REQUESTED, RXRPC_ACK_DUPLICATE,
            RXRPC_ACK_OUT_OF_SEQUENCE, RXRPC_ACK_EXCEEDS_WINDOW,
            RXRPC_ACK_NOSPACE, RXRPC_ACK_PING,
            RXRPC_ACK_PING_RESPONSE, RXRPC_ACK_DELAY,
            RXRPC_ACK_IDLE,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_security_levels_distinct() {
        let levels = [
            RXRPC_SECURITY_PLAIN, RXRPC_SECURITY_AUTH,
            RXRPC_SECURITY_ENCRYPT,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_security_types_distinct() {
        let types = [
            RXRPC_SECURITY_NONE, RXRPC_SECURITY_RXKAD,
            RXRPC_SECURITY_RXGK, RXRPC_SECURITY_YFS_RXGK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}

//! `<linux/phonet.h>` — Phonet protocol constants.
//!
//! Phonet is a packet protocol used by Nokia cellular modems for
//! communication between the application processor and baseband.
//! It uses an 8-bit device/object addressing scheme and provides
//! both datagram and pipe (stream) services.

// ---------------------------------------------------------------------------
// Protocol family
// ---------------------------------------------------------------------------

/// Phonet protocol family number.
pub const PF_PHONET: u16 = 35;
/// Phonet address family (same as PF_PHONET).
pub const AF_PHONET: u16 = 35;

// ---------------------------------------------------------------------------
// Socket types / protocols
// ---------------------------------------------------------------------------

/// Phonet datagram protocol.
pub const PN_PROTO_PHONET: u8 = 0;
/// Phonet pipe protocol (stream).
pub const PN_PROTO_PIPE: u8 = 1;

// ---------------------------------------------------------------------------
// Phonet resource types
// ---------------------------------------------------------------------------

/// Nameservice object.
pub const PN_NAMESERVICE: u8 = 0xDB;
/// Common message handler.
pub const PN_COMMGR: u8 = 0x10;
/// Pipe handler.
pub const PN_PIPE: u8 = 0xD9;

// ---------------------------------------------------------------------------
// Message types (ISI)
// ---------------------------------------------------------------------------

/// Common message.
pub const PN_MSG_COMMON: u8 = 0x00;
/// Indication message.
pub const PN_MSG_IND: u8 = 0x01;
/// Request message.
pub const PN_MSG_REQ: u8 = 0x02;
/// Response message.
pub const PN_MSG_RESP: u8 = 0x03;

// ---------------------------------------------------------------------------
// Pipe messages
// ---------------------------------------------------------------------------

/// Pipe create request.
pub const PNS_PIPE_CREATE_REQ: u8 = 0x00;
/// Pipe create response.
pub const PNS_PIPE_CREATE_RESP: u8 = 0x01;
/// Pipe enable request.
pub const PNS_PIPE_ENABLE_REQ: u8 = 0x04;
/// Pipe enable response.
pub const PNS_PIPE_ENABLE_RESP: u8 = 0x05;
/// Pipe data.
pub const PNS_PIPE_DATA: u8 = 0x20;
/// Pipe remove request.
pub const PNS_PIPE_REMOVE_REQ: u8 = 0x02;
/// Pipe remove response.
pub const PNS_PIPE_REMOVE_RESP: u8 = 0x03;

// ---------------------------------------------------------------------------
// Socket options (SOL_PHONET)
// ---------------------------------------------------------------------------

/// Get/set Phonet resource.
pub const SO_PHONET_RESOURCE: u32 = 1;
/// Get/set pipe handle.
pub const SO_PHONET_PIPE_HANDLE: u32 = 2;

// ---------------------------------------------------------------------------
// Header sizes
// ---------------------------------------------------------------------------

/// Phonet header length.
pub const PHONET_HDR_LEN: u8 = 6;
/// Pipe message header length.
pub const PIPE_HDR_LEN: u8 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_family() {
        assert_eq!(PF_PHONET, AF_PHONET);
        assert_eq!(AF_PHONET, 35);
    }

    #[test]
    fn test_protocols_distinct() {
        assert_ne!(PN_PROTO_PHONET, PN_PROTO_PIPE);
    }

    #[test]
    fn test_resource_types_distinct() {
        let res = [PN_NAMESERVICE, PN_COMMGR, PN_PIPE];
        for i in 0..res.len() {
            for j in (i + 1)..res.len() {
                assert_ne!(res[i], res[j]);
            }
        }
    }

    #[test]
    fn test_msg_types_distinct() {
        let msgs = [PN_MSG_COMMON, PN_MSG_IND, PN_MSG_REQ, PN_MSG_RESP];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_pipe_messages_distinct() {
        let pipes = [
            PNS_PIPE_CREATE_REQ,
            PNS_PIPE_CREATE_RESP,
            PNS_PIPE_ENABLE_REQ,
            PNS_PIPE_ENABLE_RESP,
            PNS_PIPE_DATA,
            PNS_PIPE_REMOVE_REQ,
            PNS_PIPE_REMOVE_RESP,
        ];
        for i in 0..pipes.len() {
            for j in (i + 1)..pipes.len() {
                assert_ne!(pipes[i], pipes[j]);
            }
        }
    }

    #[test]
    fn test_socket_options_distinct() {
        assert_ne!(SO_PHONET_RESOURCE, SO_PHONET_PIPE_HANDLE);
    }
}

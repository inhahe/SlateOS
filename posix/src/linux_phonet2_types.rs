//! `<linux/phonet.h>` — Additional Phonet constants.
//!
//! Supplementary Phonet constants covering resource types,
//! pipe message types, and device indices.

// ---------------------------------------------------------------------------
// Phonet resource types (PN_*)
// ---------------------------------------------------------------------------

/// Common resource.
pub const PN_COMMON: u8 = 0x00;
/// Nameservice resource.
pub const PN_NAMESERVICE: u8 = 0xDB;
/// Pipe resource.
pub const PN_PIPE: u8 = 0xD9;
/// Short data resource.
pub const PN_SHORT_DATA: u8 = 0xDA;

// ---------------------------------------------------------------------------
// Phonet pipe message types
// ---------------------------------------------------------------------------

/// Pipe creation request.
pub const PNS_PIPE_CREATE_REQ: u8 = 0x00;
/// Pipe creation response.
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
// Phonet socket options
// ---------------------------------------------------------------------------

/// Resource ID.
pub const SO_PHONET_RESOURCE: u32 = 1;

// ---------------------------------------------------------------------------
// Phonet header constants
// ---------------------------------------------------------------------------

/// Phonet header length.
pub const PHONET_HLEN: u32 = 8;
/// Maximum Phonet payload.
pub const PHONET_MAX_PAYLOAD: u32 = 65535;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resources_distinct() {
        let res = [PN_COMMON, PN_NAMESERVICE, PN_PIPE, PN_SHORT_DATA];
        for i in 0..res.len() {
            for j in (i + 1)..res.len() {
                assert_ne!(res[i], res[j]);
            }
        }
    }

    #[test]
    fn test_pipe_msgs_distinct() {
        let msgs = [
            PNS_PIPE_CREATE_REQ,
            PNS_PIPE_CREATE_RESP,
            PNS_PIPE_ENABLE_REQ,
            PNS_PIPE_ENABLE_RESP,
            PNS_PIPE_DATA,
            PNS_PIPE_REMOVE_REQ,
            PNS_PIPE_REMOVE_RESP,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_header_len() {
        assert_eq!(PHONET_HLEN, 8);
    }
}

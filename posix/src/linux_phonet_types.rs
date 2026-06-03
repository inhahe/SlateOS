//! `<linux/phonet.h>` — Phonet (Nokia ISI) protocol constants.
//!
//! Phonet is the protocol stack used for communication between the
//! application processor and the cellular modem in Nokia/Intel mobile
//! platforms. It provides a datagram service with source/destination
//! resource addresses. AF_PHONET sockets allow userspace processes to
//! send/receive messages to/from modem resources (SIM, network
//! registration, call control, etc.). The pipe sub-protocol provides
//! reliable, sequenced byte streams.

// ---------------------------------------------------------------------------
// Phonet address family and protocol family
// ---------------------------------------------------------------------------

/// Phonet address family.
pub const AF_PHONET: u32 = 35;
/// Phonet protocol family.
pub const PF_PHONET: u32 = 35;

// ---------------------------------------------------------------------------
// Phonet protocol types
// ---------------------------------------------------------------------------

/// Phonet datagram protocol.
pub const PN_PROTO_PHONET: u32 = 0;
/// Phonet pipe protocol (reliable stream).
pub const PN_PROTO_PIPE: u32 = 1;

// ---------------------------------------------------------------------------
// Phonet resource addresses (well-known)
// ---------------------------------------------------------------------------

/// SIM server resource.
pub const PN_SIM: u32 = 0x09;
/// Network registration resource.
pub const PN_NETWORK: u32 = 0x0A;
/// Call server resource.
pub const PN_CALL: u32 = 0x01;
/// SMS server resource.
pub const PN_SMS: u32 = 0x02;
/// SS (supplementary services) resource.
pub const PN_SS: u32 = 0x06;
/// GPRS resource.
pub const PN_GPRS: u32 = 0x31;
/// Nameservice resource.
pub const PN_NAMESERVICE: u32 = 0xDB;

// ---------------------------------------------------------------------------
// Phonet ioctl commands
// ---------------------------------------------------------------------------

/// Set interface address.
pub const SIOCPNADDRESOURCE: u32 = 0x89F0;
/// Delete interface address.
pub const SIOCPNDELRESOURCE: u32 = 0x89F1;
/// Get interface address.
pub const SIOCPNGETOBJECT: u32 = 0x89F2;

// ---------------------------------------------------------------------------
// Pipe message types
// ---------------------------------------------------------------------------

/// Pipe creation request.
pub const PNS_PIPE_CREATE_REQ: u32 = 0x00;
/// Pipe creation response.
pub const PNS_PIPE_CREATE_RESP: u32 = 0x01;
/// Pipe enable request.
pub const PNS_PIPE_ENABLE_REQ: u32 = 0x04;
/// Pipe enable response.
pub const PNS_PIPE_ENABLE_RESP: u32 = 0x05;
/// Pipe data message.
pub const PNS_PIPE_DATA: u32 = 0x20;
/// Pipe disconnect request.
pub const PNS_PIPE_DISCONNECT_REQ: u32 = 0x06;
/// Pipe disconnect response.
pub const PNS_PIPE_DISCONNECT_RESP: u32 = 0x07;

// ---------------------------------------------------------------------------
// Phonet header flags
// ---------------------------------------------------------------------------

/// Message requires acknowledgement.
pub const PN_MSG_ACK_REQ: u32 = 0x20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_pf_match() {
        assert_eq!(AF_PHONET, PF_PHONET);
        assert_eq!(AF_PHONET, 35);
    }

    #[test]
    fn test_protocols_distinct() {
        assert_ne!(PN_PROTO_PHONET, PN_PROTO_PIPE);
    }

    #[test]
    fn test_resources_distinct() {
        let res = [
            PN_CALL,
            PN_SMS,
            PN_SS,
            PN_SIM,
            PN_NETWORK,
            PN_GPRS,
            PN_NAMESERVICE,
        ];
        for i in 0..res.len() {
            for j in (i + 1)..res.len() {
                assert_ne!(res[i], res[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [SIOCPNADDRESOURCE, SIOCPNDELRESOURCE, SIOCPNGETOBJECT];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
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
            PNS_PIPE_DISCONNECT_REQ,
            PNS_PIPE_DISCONNECT_RESP,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }
}

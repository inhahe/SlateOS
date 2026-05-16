//! `<linux/ppp_defs.h>` + `<linux/if_ppp.h>` — PPP protocol definitions.
//!
//! The Point-to-Point Protocol (PPP) is used for direct serial/modem
//! links, PPPoE (DSL), L2TP VPNs, and PPTP. These constants define
//! protocol numbers, LCP option types, and ioctl commands.

// ---------------------------------------------------------------------------
// PPP protocol numbers
// ---------------------------------------------------------------------------

/// Internet Protocol (IPv4) over PPP.
pub const PPP_IP: u16 = 0x0021;
/// IPv6 over PPP.
pub const PPP_IPV6: u16 = 0x0057;
/// Van Jacobson compressed TCP/IP.
pub const PPP_VJC_COMP: u16 = 0x002D;
/// Van Jacobson uncompressed TCP/IP.
pub const PPP_VJC_UNCOMP: u16 = 0x002F;
/// Compressed Datagram.
pub const PPP_COMP: u16 = 0x00FD;
/// MPLS Unicast.
pub const PPP_MPLS_UC: u16 = 0x0281;
/// MPLS Multicast.
pub const PPP_MPLS_MC: u16 = 0x0283;
/// IPX over PPP.
pub const PPP_IPX: u16 = 0x002B;
/// AppleTalk.
pub const PPP_AT: u16 = 0x0029;

/// Internet Protocol Control Protocol.
pub const PPP_IPCP: u16 = 0x8021;
/// IPv6 Control Protocol.
pub const PPP_IPV6CP: u16 = 0x8057;
/// IPX Control Protocol.
pub const PPP_IPXCP: u16 = 0x802B;
/// Compression Control Protocol.
pub const PPP_CCP: u16 = 0x80FD;
/// Encryption Control Protocol.
pub const PPP_ECP: u16 = 0x8053;

/// Link Control Protocol.
pub const PPP_LCP: u16 = 0xC021;
/// Password Authentication Protocol.
pub const PPP_PAP: u16 = 0xC023;
/// Link Quality Report.
pub const PPP_LQR: u16 = 0xC025;
/// Challenge Handshake Authentication Protocol.
pub const PPP_CHAP: u16 = 0xC223;
/// Extensible Authentication Protocol.
pub const PPP_EAP: u16 = 0xC227;
/// Callback Control Protocol.
pub const PPP_CBCP: u16 = 0xC029;

// ---------------------------------------------------------------------------
// PPP frame bytes
// ---------------------------------------------------------------------------

/// PPP address field (all-stations).
pub const PPP_ALLSTATIONS: u8 = 0xFF;
/// PPP UI (Unnumbered Information) control byte.
pub const PPP_UI: u8 = 0x03;
/// PPP flag byte (HDLC framing).
pub const PPP_FLAG: u8 = 0x7E;
/// PPP escape byte (HDLC framing).
pub const PPP_ESCAPE: u8 = 0x7D;
/// PPP transparency XOR value.
pub const PPP_TRANS: u8 = 0x20;

// ---------------------------------------------------------------------------
// LCP code values
// ---------------------------------------------------------------------------

/// Configure-Request.
pub const PPP_LCP_CONF_REQ: u8 = 1;
/// Configure-Ack.
pub const PPP_LCP_CONF_ACK: u8 = 2;
/// Configure-Nak.
pub const PPP_LCP_CONF_NAK: u8 = 3;
/// Configure-Reject.
pub const PPP_LCP_CONF_REJ: u8 = 4;
/// Terminate-Request.
pub const PPP_LCP_TERM_REQ: u8 = 5;
/// Terminate-Ack.
pub const PPP_LCP_TERM_ACK: u8 = 6;
/// Code-Reject.
pub const PPP_LCP_CODE_REJ: u8 = 7;
/// Protocol-Reject.
pub const PPP_LCP_PROTO_REJ: u8 = 8;
/// Echo-Request.
pub const PPP_LCP_ECHO_REQ: u8 = 9;
/// Echo-Reply.
pub const PPP_LCP_ECHO_REPLY: u8 = 10;
/// Discard-Request.
pub const PPP_LCP_DISC_REQ: u8 = 11;

// ---------------------------------------------------------------------------
// PPP ioctl commands
// ---------------------------------------------------------------------------

/// Set PPP flags.
pub const PPPIOCGFLAGS: u64 = 0x80045A5A;
/// Get PPP flags.
pub const PPPIOCSFLAGS: u64 = 0x40045A5C;
/// Get PPP unit number.
pub const PPPIOCGUNIT: u64 = 0x80045A56;
/// Attach to channel.
pub const PPPIOCATTACH: u64 = 0x40045A5D;
/// Detach from channel.
pub const PPPIOCDETACH: u64 = 0x40045A5C;
/// Connect channel.
pub const PPPIOCCONNECT: u64 = 0x40045A5A;
/// Disconnect channel.
pub const PPPIOCDISCONN: u64 = 0x00005A57;
/// Get channel number.
pub const PPPIOCGCHAN: u64 = 0x80045A57;
/// Create new PPP unit.
pub const PPPIOCNEWUNIT: u64 = 0xC0045A5E;

// ---------------------------------------------------------------------------
// PPP flags
// ---------------------------------------------------------------------------

/// Passive mode.
pub const SC_COMP_PROT: u32 = 1 << 0;
/// Compress protocol field.
pub const SC_COMP_AC: u32 = 1 << 1;
/// Compress address/control.
pub const SC_COMP_TCP: u32 = 1 << 2;
/// VJ TCP header compression.
pub const SC_NO_TCP_CCID: u32 = 1 << 3;
/// BSD-compress.
pub const SC_REJ_COMP_TCP: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// PPP MRU limits
// ---------------------------------------------------------------------------

/// Default MRU.
pub const PPP_MRU: u32 = 1500;
/// Maximum MRU.
pub const PPP_MAXMRU: u32 = 65000;
/// Minimum MRU.
pub const PPP_MINMRU: u32 = 128;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_numbers_distinct() {
        let protos = [
            PPP_IP, PPP_IPV6, PPP_VJC_COMP, PPP_VJC_UNCOMP,
            PPP_COMP, PPP_IPX, PPP_AT,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_control_protocols_distinct() {
        let cps = [
            PPP_IPCP, PPP_IPV6CP, PPP_IPXCP, PPP_CCP, PPP_ECP,
        ];
        for i in 0..cps.len() {
            for j in (i + 1)..cps.len() {
                assert_ne!(cps[i], cps[j]);
            }
        }
    }

    #[test]
    fn test_auth_protocols_distinct() {
        let auth = [PPP_LCP, PPP_PAP, PPP_CHAP, PPP_EAP, PPP_CBCP];
        for i in 0..auth.len() {
            for j in (i + 1)..auth.len() {
                assert_ne!(auth[i], auth[j]);
            }
        }
    }

    #[test]
    fn test_frame_bytes() {
        assert_eq!(PPP_ALLSTATIONS, 0xFF);
        assert_eq!(PPP_UI, 0x03);
        assert_eq!(PPP_FLAG, 0x7E);
        assert_eq!(PPP_ESCAPE, 0x7D);
        assert_eq!(PPP_TRANS, 0x20);
    }

    #[test]
    fn test_lcp_codes_sequential() {
        assert_eq!(PPP_LCP_CONF_REQ, 1);
        assert_eq!(PPP_LCP_CONF_ACK, 2);
        assert_eq!(PPP_LCP_CONF_NAK, 3);
        assert_eq!(PPP_LCP_CONF_REJ, 4);
        assert_eq!(PPP_LCP_TERM_REQ, 5);
        assert_eq!(PPP_LCP_TERM_ACK, 6);
    }

    #[test]
    fn test_mru_limits() {
        assert!(PPP_MINMRU < PPP_MRU);
        assert!(PPP_MRU < PPP_MAXMRU);
        assert_eq!(PPP_MRU, 1500);
    }

    #[test]
    fn test_ip_protocol() {
        assert_eq!(PPP_IP, 0x0021);
        assert_eq!(PPP_IPV6, 0x0057);
    }
}

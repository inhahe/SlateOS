//! `<linux/ppp_defs.h>` — Point-to-Point Protocol constants.
//!
//! PPP (RFC 1661) provides a standard method for transporting
//! multi-protocol datagrams over point-to-point links. It includes
//! link control (LCP), authentication (PAP/CHAP/EAP), and network
//! control protocols (IPCP, IPV6CP). PPP is used in DSL (PPPoE),
//! VPNs (L2TP, PPTP), and serial links.

// ---------------------------------------------------------------------------
// PPP protocol numbers
// ---------------------------------------------------------------------------

/// IP (IPv4).
pub const PPP_PROTO_IP: u16 = 0x0021;
/// IPv6.
pub const PPP_PROTO_IPV6: u16 = 0x0057;
/// IPX.
pub const PPP_PROTO_IPX: u16 = 0x002B;
/// Van Jacobson compressed TCP/IP.
pub const PPP_PROTO_VJC_COMP: u16 = 0x002D;
/// Van Jacobson uncompressed TCP/IP.
pub const PPP_PROTO_VJC_UNCOMP: u16 = 0x002F;
/// Compressed datagram.
pub const PPP_PROTO_COMP: u16 = 0x00FD;
/// IP Control Protocol.
pub const PPP_PROTO_IPCP: u16 = 0x8021;
/// IPv6 Control Protocol.
pub const PPP_PROTO_IPV6CP: u16 = 0x8057;
/// Compression Control Protocol.
pub const PPP_PROTO_CCP: u16 = 0x80FD;
/// Link Control Protocol.
pub const PPP_PROTO_LCP: u16 = 0xC021;
/// Password Authentication Protocol.
pub const PPP_PROTO_PAP: u16 = 0xC023;
/// Challenge Handshake Auth Protocol.
pub const PPP_PROTO_CHAP: u16 = 0xC223;
/// Extensible Authentication Protocol.
pub const PPP_PROTO_EAP: u16 = 0xC227;
/// Multilink PPP.
pub const PPP_PROTO_MP: u16 = 0x003D;

// ---------------------------------------------------------------------------
// LCP codes
// ---------------------------------------------------------------------------

/// Configure-Request.
pub const LCP_CONF_REQ: u8 = 1;
/// Configure-Ack.
pub const LCP_CONF_ACK: u8 = 2;
/// Configure-Nak.
pub const LCP_CONF_NAK: u8 = 3;
/// Configure-Reject.
pub const LCP_CONF_REJ: u8 = 4;
/// Terminate-Request.
pub const LCP_TERM_REQ: u8 = 5;
/// Terminate-Ack.
pub const LCP_TERM_ACK: u8 = 6;
/// Code-Reject.
pub const LCP_CODE_REJ: u8 = 7;
/// Protocol-Reject.
pub const LCP_PROTO_REJ: u8 = 8;
/// Echo-Request.
pub const LCP_ECHO_REQ: u8 = 9;
/// Echo-Reply.
pub const LCP_ECHO_REP: u8 = 10;
/// Discard-Request.
pub const LCP_DISC_REQ: u8 = 11;

// ---------------------------------------------------------------------------
// CHAP algorithm identifiers
// ---------------------------------------------------------------------------

/// MD5-CHAP.
pub const CHAP_MD5: u8 = 5;
/// MS-CHAPv1.
pub const CHAP_MSCHAP_V1: u8 = 0x80;
/// MS-CHAPv2.
pub const CHAP_MSCHAP_V2: u8 = 0x81;

// ---------------------------------------------------------------------------
// PPP ioctl socket options
// ---------------------------------------------------------------------------

/// Get PPP unit number.
pub const PPPIOCGUNIT: u32 = 0xC004_7486;
/// Set PPP MRU.
pub const PPPIOCSMRU: u32 = 0xC004_7452;
/// Set channel.
pub const PPPIOCATTCHAN: u32 = 0x4004_7438;
/// Connect channel.
pub const PPPIOCCONNECT: u32 = 0x4004_743A;
/// Disconnect channel.
pub const PPPIOCDISCONN: u32 = 0x0000_7439;

// ---------------------------------------------------------------------------
// PPP flags
// ---------------------------------------------------------------------------

/// Address/Control field compression.
pub const PPP_FLAG_ACCOMP: u32 = 1 << 0;
/// Protocol field compression.
pub const PPP_FLAG_PCOMP: u32 = 1 << 1;
/// VJ TCP header compression.
pub const PPP_FLAG_VJCCOMP: u32 = 1 << 2;
/// CCP enabled.
pub const PPP_FLAG_CCP: u32 = 1 << 3;
/// Multilink.
pub const PPP_FLAG_MP: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Frame constants
// ---------------------------------------------------------------------------

/// PPP frame start/end flag.
pub const PPP_FLAG_BYTE: u8 = 0x7E;
/// PPP address field (all stations).
pub const PPP_ADDR_BYTE: u8 = 0xFF;
/// PPP control field (unnumbered information).
pub const PPP_CTRL_BYTE: u8 = 0x03;
/// PPP escape byte.
pub const PPP_ESC_BYTE: u8 = 0x7D;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_numbers_distinct() {
        let protos = [
            PPP_PROTO_IP, PPP_PROTO_IPV6, PPP_PROTO_IPX,
            PPP_PROTO_VJC_COMP, PPP_PROTO_VJC_UNCOMP, PPP_PROTO_COMP,
            PPP_PROTO_IPCP, PPP_PROTO_IPV6CP, PPP_PROTO_CCP,
            PPP_PROTO_LCP, PPP_PROTO_PAP, PPP_PROTO_CHAP,
            PPP_PROTO_EAP, PPP_PROTO_MP,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_lcp_codes_distinct() {
        let codes = [
            LCP_CONF_REQ, LCP_CONF_ACK, LCP_CONF_NAK, LCP_CONF_REJ,
            LCP_TERM_REQ, LCP_TERM_ACK, LCP_CODE_REJ, LCP_PROTO_REJ,
            LCP_ECHO_REQ, LCP_ECHO_REP, LCP_DISC_REQ,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_chap_algorithms_distinct() {
        let algs = [CHAP_MD5, CHAP_MSCHAP_V1, CHAP_MSCHAP_V2];
        for i in 0..algs.len() {
            for j in (i + 1)..algs.len() {
                assert_ne!(algs[i], algs[j]);
            }
        }
    }

    #[test]
    fn test_ppp_flags_no_overlap() {
        let flags = [
            PPP_FLAG_ACCOMP, PPP_FLAG_PCOMP,
            PPP_FLAG_VJCCOMP, PPP_FLAG_CCP, PPP_FLAG_MP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_frame_constants() {
        assert_eq!(PPP_FLAG_BYTE, 0x7E);
        assert_eq!(PPP_ADDR_BYTE, 0xFF);
        assert_eq!(PPP_CTRL_BYTE, 0x03);
        assert_eq!(PPP_ESC_BYTE, 0x7D);
    }
}

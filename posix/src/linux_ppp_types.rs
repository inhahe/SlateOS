//! `<linux/ppp_defs.h>` — PPP (Point-to-Point Protocol) constants.
//!
//! PPP is a data link layer protocol for establishing
//! direct connections.  These constants define PPP IOCTL
//! commands, protocol numbers, LCP options, and channel flags.

// ---------------------------------------------------------------------------
// PPP protocol numbers
// ---------------------------------------------------------------------------

/// IP protocol.
pub const PPP_IP: u16 = 0x0021;
/// IPv6 protocol.
pub const PPP_IPV6: u16 = 0x0057;
/// IPX protocol.
pub const PPP_IPX: u16 = 0x002B;
/// VJ compressed TCP.
pub const PPP_VJC_COMP: u16 = 0x002D;
/// VJ uncompressed TCP.
pub const PPP_VJC_UNCOMP: u16 = 0x002F;
/// Compression Control Protocol.
pub const PPP_CCP: u16 = 0x80FD;
/// IP Control Protocol.
pub const PPP_IPCP: u16 = 0x8021;
/// IPv6 Control Protocol.
pub const PPP_IPV6CP: u16 = 0x8057;
/// Link Control Protocol.
pub const PPP_LCP: u16 = 0xC021;
/// Password Authentication Protocol.
pub const PPP_PAP: u16 = 0xC023;
/// Link Quality Report.
pub const PPP_LQR: u16 = 0xC025;
/// Challenge Handshake Authentication.
pub const PPP_CHAP: u16 = 0xC223;
/// Extensible Authentication Protocol.
pub const PPP_EAP: u16 = 0xC227;
/// Multilink Protocol.
pub const PPP_MP: u16 = 0x003D;
/// MPPC/MPPE.
pub const PPP_COMP: u16 = 0x00FD;

// ---------------------------------------------------------------------------
// PPP IOCTL commands
// ---------------------------------------------------------------------------

/// Set PPP flags.
pub const PPPIOCGFLAGS: u32 = 0x5490;
/// Get PPP flags.
pub const PPPIOCSFLAGS: u32 = 0x5491;
/// Get async map.
pub const PPPIOCGASYNCMAP: u32 = 0x5492;
/// Set async map.
pub const PPPIOCSASYNCMAP: u32 = 0x5493;
/// Get unit number.
pub const PPPIOCGUNIT: u32 = 0x5494;
/// Set MRU (Maximum Receive Unit).
pub const PPPIOCSMRU: u32 = 0x5496;
/// Get MRU.
pub const PPPIOCGMRU: u32 = 0x5497;
/// Attach to channel.
pub const PPPIOCATTACH: u32 = 0x549D;
/// Detach from channel.
pub const PPPIOCDETACH: u32 = 0x549C;
/// Connect channel.
pub const PPPIOCCONNECT: u32 = 0x549A;
/// Disconnect channel.
pub const PPPIOCDISCONN: u32 = 0x549B;
/// Create new PPP unit.
pub const PPPIOCNEWUNIT: u32 = 0x549E;
/// Attach to channel by index.
pub const PPPIOCATTCHAN: u32 = 0x5498;

// ---------------------------------------------------------------------------
// PPP flags
// ---------------------------------------------------------------------------

/// Kernel debug flag.
pub const SC_DEBUG: u32 = 0x00000001;
/// Compression protocol.
pub const SC_COMP_PROT: u32 = 0x00000002;
/// Address/control compression.
pub const SC_COMP_AC: u32 = 0x00000004;
/// CCP is open.
pub const SC_CCP_OPEN: u32 = 0x00000008;
/// CCP is up.
pub const SC_CCP_UP: u32 = 0x00000010;
/// Loop back.
pub const SC_LOOP_TRAFFIC: u32 = 0x00000200;
/// Multilink mode.
pub const SC_MULTILINK: u32 = 0x00000400;
/// Receive ACFC.
pub const SC_RCV_B7_0: u32 = 0x01000000;
/// Receive ACFC 1.
pub const SC_RCV_B7_1: u32 = 0x02000000;
/// Receive odd parity.
pub const SC_RCV_EVNP: u32 = 0x04000000;
/// Receive even parity.
pub const SC_RCV_ODDP: u32 = 0x08000000;

// ---------------------------------------------------------------------------
// PPP LCP option types
// ---------------------------------------------------------------------------

/// Maximum Receive Unit.
pub const LCP_OPT_MRU: u8 = 1;
/// Async Control Character Map.
pub const LCP_OPT_ASYNCMAP: u8 = 2;
/// Authentication Protocol.
pub const LCP_OPT_AUTH: u8 = 3;
/// Quality Protocol.
pub const LCP_OPT_QUALITY: u8 = 4;
/// Magic Number.
pub const LCP_OPT_MAGIC: u8 = 5;
/// Protocol Field Compression.
pub const LCP_OPT_PFC: u8 = 7;
/// Address/Control Field Compression.
pub const LCP_OPT_ACFC: u8 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            PPP_IP, PPP_IPV6, PPP_IPX, PPP_VJC_COMP,
            PPP_VJC_UNCOMP, PPP_CCP, PPP_IPCP, PPP_IPV6CP,
            PPP_LCP, PPP_PAP, PPP_LQR, PPP_CHAP, PPP_EAP,
            PPP_MP, PPP_COMP,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_ip_protocol() {
        assert_eq!(PPP_IP, 0x0021);
    }

    #[test]
    fn test_lcp_protocol() {
        assert_eq!(PPP_LCP, 0xC021);
    }

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            PPPIOCGFLAGS, PPPIOCSFLAGS, PPPIOCGASYNCMAP,
            PPPIOCSASYNCMAP, PPPIOCGUNIT, PPPIOCSMRU,
            PPPIOCGMRU, PPPIOCATTACH, PPPIOCDETACH,
            PPPIOCCONNECT, PPPIOCDISCONN, PPPIOCNEWUNIT,
            PPPIOCATTCHAN,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_lcp_opts_distinct() {
        let opts = [
            LCP_OPT_MRU, LCP_OPT_ASYNCMAP, LCP_OPT_AUTH,
            LCP_OPT_QUALITY, LCP_OPT_MAGIC, LCP_OPT_PFC,
            LCP_OPT_ACFC,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_mru_opt_is_one() {
        assert_eq!(LCP_OPT_MRU, 1);
    }
}

//! `<linux/ppp_defs.h>` — Additional PPP constants.
//!
//! Supplementary PPP protocol constants covering protocol numbers,
//! control protocol types, and frame fields.

// ---------------------------------------------------------------------------
// PPP protocol numbers
// ---------------------------------------------------------------------------

/// IP protocol.
pub const PPP_IP: u32 = 0x0021;
/// IPv6 protocol.
pub const PPP_IPV6: u32 = 0x0057;
/// IPX protocol.
pub const PPP_IPX: u32 = 0x002B;
/// VJ compressed TCP.
pub const PPP_VJC_COMP: u32 = 0x002D;
/// VJ uncompressed TCP.
pub const PPP_VJC_UNCOMP: u32 = 0x002F;
/// Compression control protocol.
pub const PPP_COMP: u32 = 0x00FD;
/// MPLS unicast.
pub const PPP_MPLS_UC: u32 = 0x0281;
/// MPLS multicast.
pub const PPP_MPLS_MC: u32 = 0x0283;

// ---------------------------------------------------------------------------
// PPP control protocol types
// ---------------------------------------------------------------------------

/// IP control protocol.
pub const PPP_IPCP: u32 = 0x8021;
/// IPv6 control protocol.
pub const PPP_IPV6CP: u32 = 0x8057;
/// IPX control protocol.
pub const PPP_IPXCP: u32 = 0x802B;
/// Compression control protocol.
pub const PPP_CCP: u32 = 0x80FD;
/// Link control protocol.
pub const PPP_LCP: u32 = 0xC021;
/// Password authentication protocol.
pub const PPP_PAP: u32 = 0xC023;
/// Link quality report.
pub const PPP_LQR: u32 = 0xC025;
/// Challenge handshake authentication protocol.
pub const PPP_CHAP: u32 = 0xC223;

// ---------------------------------------------------------------------------
// PPP frame fields
// ---------------------------------------------------------------------------

/// All-stations address.
pub const PPP_ALLSTATIONS: u32 = 0xFF;
/// Unnumbered information.
pub const PPP_UI: u32 = 0x03;
/// Flag sequence.
pub const PPP_FLAG: u32 = 0x7E;
/// Async escape.
pub const PPP_ESCAPE: u32 = 0x7D;
/// Async transparency modifier.
pub const PPP_TRANS: u32 = 0x20;

// ---------------------------------------------------------------------------
// PPP FCS
// ---------------------------------------------------------------------------

/// Initial FCS value.
pub const PPP_INITFCS: u32 = 0xFFFF;
/// Good final FCS value.
pub const PPP_GOODFCS: u32 = 0xF0B8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            PPP_IP,
            PPP_IPV6,
            PPP_IPX,
            PPP_VJC_COMP,
            PPP_VJC_UNCOMP,
            PPP_COMP,
            PPP_MPLS_UC,
            PPP_MPLS_MC,
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
            PPP_IPCP, PPP_IPV6CP, PPP_IPXCP, PPP_CCP, PPP_LCP, PPP_PAP, PPP_LQR, PPP_CHAP,
        ];
        for i in 0..cps.len() {
            for j in (i + 1)..cps.len() {
                assert_ne!(cps[i], cps[j]);
            }
        }
    }

    #[test]
    fn test_frame_fields_distinct() {
        let fields = [PPP_ALLSTATIONS, PPP_UI, PPP_FLAG, PPP_ESCAPE, PPP_TRANS];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }

    #[test]
    fn test_fcs_values_distinct() {
        assert_ne!(PPP_INITFCS, PPP_GOODFCS);
    }
}

//! `<linux/ppp_defs.h>` — Additional PPP constants (batch 3).
//!
//! Supplementary PPP constants covering LCP option types,
//! IPCP option types, and CCP option types.

// ---------------------------------------------------------------------------
// LCP (Link Control Protocol) option types
// ---------------------------------------------------------------------------

/// Maximum Receive Unit.
pub const LCP_OPT_MRU: u32 = 1;
/// Async Control Character Map.
pub const LCP_OPT_ACCM: u32 = 2;
/// Authentication Protocol.
pub const LCP_OPT_AUTH: u32 = 3;
/// Quality Protocol.
pub const LCP_OPT_QUALITY: u32 = 4;
/// Magic Number.
pub const LCP_OPT_MAGIC: u32 = 5;
/// Protocol Field Compression.
pub const LCP_OPT_PFC: u32 = 7;
/// Address/Control Field Compression.
pub const LCP_OPT_ACFC: u32 = 8;
/// FCS Alternatives.
pub const LCP_OPT_FCS: u32 = 9;
/// Multilink MRRU.
pub const LCP_OPT_MRRU: u32 = 17;
/// Multilink endpoint discriminator.
pub const LCP_OPT_ENDP_DISC: u32 = 19;

// ---------------------------------------------------------------------------
// IPCP (Internet Protocol Control Protocol) option types
// ---------------------------------------------------------------------------

/// IP addresses (deprecated, use IP_ADDRESS).
pub const IPCP_OPT_ADDRESSES: u32 = 1;
/// IP compression protocol.
pub const IPCP_OPT_COMPRESSION: u32 = 2;
/// IP address.
pub const IPCP_OPT_ADDRESS: u32 = 3;
/// Primary DNS server.
pub const IPCP_OPT_DNS1: u32 = 129;
/// Primary NBNS (WINS) server.
pub const IPCP_OPT_NBNS1: u32 = 130;
/// Secondary DNS server.
pub const IPCP_OPT_DNS2: u32 = 131;
/// Secondary NBNS (WINS) server.
pub const IPCP_OPT_NBNS2: u32 = 132;

// ---------------------------------------------------------------------------
// CCP (Compression Control Protocol) option types
// ---------------------------------------------------------------------------

/// Predictor 1.
pub const CCP_OPT_PREDICTOR1: u32 = 1;
/// Predictor 2.
pub const CCP_OPT_PREDICTOR2: u32 = 2;
/// Puddle Jumper.
pub const CCP_OPT_PUDDLE: u32 = 3;
/// BSD Compress.
pub const CCP_OPT_BSD: u32 = 21;
/// Deflate.
pub const CCP_OPT_DEFLATE: u32 = 26;
/// MPPE (Microsoft Point-to-Point Encryption).
pub const CCP_OPT_MPPE: u32 = 18;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lcp_opts_distinct() {
        let opts = [
            LCP_OPT_MRU,
            LCP_OPT_ACCM,
            LCP_OPT_AUTH,
            LCP_OPT_QUALITY,
            LCP_OPT_MAGIC,
            LCP_OPT_PFC,
            LCP_OPT_ACFC,
            LCP_OPT_FCS,
            LCP_OPT_MRRU,
            LCP_OPT_ENDP_DISC,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_ipcp_opts_distinct() {
        let opts = [
            IPCP_OPT_ADDRESSES,
            IPCP_OPT_COMPRESSION,
            IPCP_OPT_ADDRESS,
            IPCP_OPT_DNS1,
            IPCP_OPT_NBNS1,
            IPCP_OPT_DNS2,
            IPCP_OPT_NBNS2,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_ccp_opts_distinct() {
        let opts = [
            CCP_OPT_PREDICTOR1,
            CCP_OPT_PREDICTOR2,
            CCP_OPT_PUDDLE,
            CCP_OPT_BSD,
            CCP_OPT_DEFLATE,
            CCP_OPT_MPPE,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_lcp_mru_is_one() {
        assert_eq!(LCP_OPT_MRU, 1);
    }

    #[test]
    fn test_ipcp_address_is_three() {
        assert_eq!(IPCP_OPT_ADDRESS, 3);
    }
}

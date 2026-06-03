//! `<linux/ip.h>` (options subset) — IP header option constants.
//!
//! IP options are variable-length fields in the IPv4 header (between
//! the fixed 20-byte header and the payload). They provide features
//! like source routing, timestamps, and record-route. In practice,
//! most IP options are rarely used on the modern internet (firewalls
//! often drop packets with options), but they remain important for
//! diagnostic tools and certain protocols.

// ---------------------------------------------------------------------------
// IP option types (single-byte type field)
// ---------------------------------------------------------------------------

/// End of options list.
pub const IPOPT_END: u32 = 0x00;
/// No operation (padding).
pub const IPOPT_NOP: u32 = 0x01;
/// Record route.
pub const IPOPT_RR: u32 = 0x07;
/// Internet timestamp.
pub const IPOPT_TS: u32 = 0x44;
/// Loose source and record route.
pub const IPOPT_LSRR: u32 = 0x83;
/// Strict source and record route.
pub const IPOPT_SSRR: u32 = 0x89;
/// Router alert.
pub const IPOPT_RA: u32 = 0x94;

// ---------------------------------------------------------------------------
// IP option class (bits 6-5 of type byte)
// ---------------------------------------------------------------------------

/// Control option.
pub const IPOPT_CLASS_CONTROL: u32 = 0x00;
/// Measurement option.
pub const IPOPT_CLASS_MEASUREMENT: u32 = 0x40;

// ---------------------------------------------------------------------------
// IP timestamp option flags
// ---------------------------------------------------------------------------

/// Timestamps only (no addresses).
pub const IPOPT_TS_TSONLY: u32 = 0;
/// Timestamps with addresses.
pub const IPOPT_TS_TSANDADDR: u32 = 1;
/// Timestamps for prespecified addresses only.
pub const IPOPT_TS_PRESPEC: u32 = 3;

// ---------------------------------------------------------------------------
// IP TOS (Type of Service) values
// ---------------------------------------------------------------------------

/// Normal service.
pub const IPTOS_NORMAL: u32 = 0x00;
/// Minimize delay.
pub const IPTOS_LOWDELAY: u32 = 0x10;
/// Maximize throughput.
pub const IPTOS_THROUGHPUT: u32 = 0x08;
/// Maximize reliability.
pub const IPTOS_RELIABILITY: u32 = 0x04;
/// Minimize cost.
pub const IPTOS_MINCOST: u32 = 0x02;

// ---------------------------------------------------------------------------
// DSCP (Differentiated Services Code Point) classes
// ---------------------------------------------------------------------------

/// Default forwarding (best effort).
pub const DSCP_DEFAULT: u32 = 0x00;
/// Expedited Forwarding (low loss, low delay, low jitter).
pub const DSCP_EF: u32 = 0xB8;
/// Assured Forwarding class 1, drop precedence 1.
pub const DSCP_AF11: u32 = 0x28;
/// Assured Forwarding class 2, drop precedence 1.
pub const DSCP_AF21: u32 = 0x48;
/// Assured Forwarding class 3, drop precedence 1.
pub const DSCP_AF31: u32 = 0x68;
/// Assured Forwarding class 4, drop precedence 1.
pub const DSCP_AF41: u32 = 0x88;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_option_types_distinct() {
        let opts = [
            IPOPT_END, IPOPT_NOP, IPOPT_RR, IPOPT_TS, IPOPT_LSRR, IPOPT_SSRR, IPOPT_RA,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_ts_flags_distinct() {
        let flags = [IPOPT_TS_TSONLY, IPOPT_TS_TSANDADDR, IPOPT_TS_PRESPEC];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_tos_values() {
        assert_eq!(IPTOS_NORMAL, 0);
        assert_ne!(IPTOS_LOWDELAY, 0);
        assert_ne!(IPTOS_THROUGHPUT, 0);
        assert_ne!(IPTOS_RELIABILITY, 0);
    }

    #[test]
    fn test_dscp_values_distinct() {
        let dscp = [
            DSCP_DEFAULT,
            DSCP_EF,
            DSCP_AF11,
            DSCP_AF21,
            DSCP_AF31,
            DSCP_AF41,
        ];
        for i in 0..dscp.len() {
            for j in (i + 1)..dscp.len() {
                assert_ne!(dscp[i], dscp[j]);
            }
        }
    }
}

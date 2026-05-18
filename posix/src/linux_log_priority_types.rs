//! Syslog priority encoding helpers.
//!
//! A syslog priority is the combination of a facility code
//! and a severity level: `priority = facility | level`.
//! These constants define the encoding masks and helper
//! values for constructing and decomposing priorities.

// ---------------------------------------------------------------------------
// Priority encoding
// ---------------------------------------------------------------------------

/// Number of bits for severity level.
pub const LOG_PRIMASK: u32 = 0x07;
/// Shift to extract/construct facility from priority.
pub const LOG_FACSHIFT: u32 = 3;
/// Mask for facility (after applying to raw priority).
pub const LOG_FACMASK: u32 = 0x03F8;

// ---------------------------------------------------------------------------
// Internal facility indices (facility >> 3)
// ---------------------------------------------------------------------------

/// Kernel facility index.
pub const LOG_FAC_KERN: u32 = 0;
/// User facility index.
pub const LOG_FAC_USER: u32 = 1;
/// Mail facility index.
pub const LOG_FAC_MAIL: u32 = 2;
/// Daemon facility index.
pub const LOG_FAC_DAEMON: u32 = 3;
/// Auth facility index.
pub const LOG_FAC_AUTH: u32 = 4;
/// Syslog internal facility index.
pub const LOG_FAC_SYSLOG: u32 = 5;
/// LPR facility index.
pub const LOG_FAC_LPR: u32 = 6;
/// News facility index.
pub const LOG_FAC_NEWS: u32 = 7;
/// UUCP facility index.
pub const LOG_FAC_UUCP: u32 = 8;
/// Cron facility index.
pub const LOG_FAC_CRON: u32 = 9;
/// Auth private facility index.
pub const LOG_FAC_AUTHPRIV: u32 = 10;
/// FTP facility index.
pub const LOG_FAC_FTP: u32 = 11;
/// Local 0 facility index.
pub const LOG_FAC_LOCAL0: u32 = 16;
/// Local 1 facility index.
pub const LOG_FAC_LOCAL1: u32 = 17;
/// Local 2 facility index.
pub const LOG_FAC_LOCAL2: u32 = 18;
/// Local 3 facility index.
pub const LOG_FAC_LOCAL3: u32 = 19;
/// Local 4 facility index.
pub const LOG_FAC_LOCAL4: u32 = 20;
/// Local 5 facility index.
pub const LOG_FAC_LOCAL5: u32 = 21;
/// Local 6 facility index.
pub const LOG_FAC_LOCAL6: u32 = 22;
/// Local 7 facility index.
pub const LOG_FAC_LOCAL7: u32 = 23;

/// Number of defined facilities.
pub const LOG_NFACILITIES: u32 = 24;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primask() {
        assert_eq!(LOG_PRIMASK, 7);
    }

    #[test]
    fn test_facshift() {
        assert_eq!(LOG_FACSHIFT, 3);
    }

    #[test]
    fn test_facmask() {
        // FACMASK should cover facility bits and not overlap PRIMASK
        assert_eq!(LOG_FACMASK & LOG_PRIMASK, 0);
    }

    #[test]
    fn test_facility_indices_distinct() {
        let facs = [
            LOG_FAC_KERN, LOG_FAC_USER, LOG_FAC_MAIL,
            LOG_FAC_DAEMON, LOG_FAC_AUTH, LOG_FAC_SYSLOG,
            LOG_FAC_LPR, LOG_FAC_NEWS, LOG_FAC_UUCP,
            LOG_FAC_CRON, LOG_FAC_AUTHPRIV, LOG_FAC_FTP,
            LOG_FAC_LOCAL0, LOG_FAC_LOCAL1, LOG_FAC_LOCAL2,
            LOG_FAC_LOCAL3, LOG_FAC_LOCAL4, LOG_FAC_LOCAL5,
            LOG_FAC_LOCAL6, LOG_FAC_LOCAL7,
        ];
        for i in 0..facs.len() {
            for j in (i + 1)..facs.len() {
                assert_ne!(facs[i], facs[j]);
            }
        }
    }

    #[test]
    fn test_kern_is_zero() {
        assert_eq!(LOG_FAC_KERN, 0);
    }

    #[test]
    fn test_local0_is_sixteen() {
        assert_eq!(LOG_FAC_LOCAL0, 16);
    }

    #[test]
    fn test_local7_is_twentythree() {
        assert_eq!(LOG_FAC_LOCAL7, 23);
    }

    #[test]
    fn test_nfacilities() {
        assert_eq!(LOG_NFACILITIES, 24);
    }
}

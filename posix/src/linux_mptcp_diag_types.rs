//! `<linux/mptcp_pm.h>` — MPTCP socket diagnostics constants.
//!
//! The MPTCP diagnostics interface extends inet_diag to report
//! MPTCP-specific connection state: subflow count, address usage,
//! path manager state, and per-subflow TCP_INFO. Allows `ss -M` to
//! show MPTCP connections with all their subflows and path
//! information. Essential for debugging multipath networking setups
//! where connections span WiFi + Ethernet or multiple ISPs.

// ---------------------------------------------------------------------------
// MPTCP diag attributes (MPTCP_ATTR_*)
// ---------------------------------------------------------------------------

/// MPTCP token (connection identifier).
pub const MPTCP_ATTR_TOKEN: u32 = 1;
/// MPTCP connection family (AF_INET/AF_INET6).
pub const MPTCP_ATTR_FAMILY: u32 = 2;
/// Local address.
pub const MPTCP_ATTR_LOC_ID: u32 = 3;
/// Remote address.
pub const MPTCP_ATTR_REM_ID: u32 = 4;
/// Source address.
pub const MPTCP_ATTR_SADDR4: u32 = 5;
/// Source IPv6 address.
pub const MPTCP_ATTR_SADDR6: u32 = 6;
/// Destination address.
pub const MPTCP_ATTR_DADDR4: u32 = 7;
/// Destination IPv6 address.
pub const MPTCP_ATTR_DADDR6: u32 = 8;
/// Source port.
pub const MPTCP_ATTR_SPORT: u32 = 9;
/// Destination port.
pub const MPTCP_ATTR_DPORT: u32 = 10;
/// Backup flag.
pub const MPTCP_ATTR_BACKUP: u32 = 11;
/// Error code.
pub const MPTCP_ATTR_ERROR: u32 = 12;
/// Flags.
pub const MPTCP_ATTR_FLAGS: u32 = 13;
/// Timeout.
pub const MPTCP_ATTR_TIMEOUT: u32 = 14;
/// Subflow interface.
pub const MPTCP_ATTR_IF_IDX: u32 = 15;
/// Connection reset reason.
pub const MPTCP_ATTR_RESET_REASON: u32 = 16;
/// Connection reset transient flag.
pub const MPTCP_ATTR_RESET_TRANSIENT: u32 = 17;
/// Server side flag.
pub const MPTCP_ATTR_SERVER_SIDE: u32 = 18;

// ---------------------------------------------------------------------------
// MPTCP subflow flags
// ---------------------------------------------------------------------------

/// Subflow is in fallback mode (plain TCP).
pub const MPTCP_SUBFLOW_FLAG_MCAP_REM: u32 = 1 << 0;
/// Subflow has local MPTCP capability.
pub const MPTCP_SUBFLOW_FLAG_MCAP_LOC: u32 = 1 << 1;
/// Subflow join (not initial subflow).
pub const MPTCP_SUBFLOW_FLAG_JOIN: u32 = 1 << 2;
/// Subflow is backup path.
pub const MPTCP_SUBFLOW_FLAG_BACKUP: u32 = 1 << 3;
/// Subflow is fully established.
pub const MPTCP_SUBFLOW_FLAG_FULLY_ESTABLISHED: u32 = 1 << 4;
/// Subflow is connected.
pub const MPTCP_SUBFLOW_FLAG_CONNECTED: u32 = 1 << 5;
/// Subflow has HMAC verified.
pub const MPTCP_SUBFLOW_FLAG_MAPVALID: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// MPTCP reset reasons
// ---------------------------------------------------------------------------

/// No specific reason.
pub const MPTCP_RST_EUNSPEC: u32 = 0;
/// MPTCP-specific error.
pub const MPTCP_RST_EMPTCP: u32 = 1;
/// Lack of resources.
pub const MPTCP_RST_ERESOURCE: u32 = 2;
/// Administratively prohibited.
pub const MPTCP_RST_EPROHIBITED: u32 = 3;
/// Too many existing subflows.
pub const MPTCP_RST_EWQ2BIG: u32 = 4;
/// Not reachable via this path.
pub const MPTCP_RST_EBADPERF: u32 = 5;
/// Middlebox interference.
pub const MPTCP_RST_EMIDDLEBOX: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            MPTCP_ATTR_TOKEN, MPTCP_ATTR_FAMILY,
            MPTCP_ATTR_LOC_ID, MPTCP_ATTR_REM_ID,
            MPTCP_ATTR_SADDR4, MPTCP_ATTR_SADDR6,
            MPTCP_ATTR_DADDR4, MPTCP_ATTR_DADDR6,
            MPTCP_ATTR_SPORT, MPTCP_ATTR_DPORT,
            MPTCP_ATTR_BACKUP, MPTCP_ATTR_ERROR,
            MPTCP_ATTR_FLAGS, MPTCP_ATTR_TIMEOUT,
            MPTCP_ATTR_IF_IDX, MPTCP_ATTR_RESET_REASON,
            MPTCP_ATTR_RESET_TRANSIENT, MPTCP_ATTR_SERVER_SIDE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_subflow_flags_no_overlap() {
        let flags = [
            MPTCP_SUBFLOW_FLAG_MCAP_REM, MPTCP_SUBFLOW_FLAG_MCAP_LOC,
            MPTCP_SUBFLOW_FLAG_JOIN, MPTCP_SUBFLOW_FLAG_BACKUP,
            MPTCP_SUBFLOW_FLAG_FULLY_ESTABLISHED,
            MPTCP_SUBFLOW_FLAG_CONNECTED, MPTCP_SUBFLOW_FLAG_MAPVALID,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_reset_reasons_distinct() {
        let reasons = [
            MPTCP_RST_EUNSPEC, MPTCP_RST_EMPTCP,
            MPTCP_RST_ERESOURCE, MPTCP_RST_EPROHIBITED,
            MPTCP_RST_EWQ2BIG, MPTCP_RST_EBADPERF,
            MPTCP_RST_EMIDDLEBOX,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_attrs_sequential() {
        assert_eq!(MPTCP_ATTR_TOKEN, 1);
        assert_eq!(MPTCP_ATTR_SERVER_SIDE, 18);
    }
}

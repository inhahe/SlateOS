//! `<linux/errqueue.h>` — Socket error queue constants.
//!
//! The socket error queue (accessed via `MSG_ERRQUEUE`) delivers
//! ancillary error information for UDP/TCP/raw sockets, including
//! ICMP errors, timestamps, and path MTU updates.

// ---------------------------------------------------------------------------
// Error origins (ee_origin)
// ---------------------------------------------------------------------------

/// No origin.
pub const SO_EE_ORIGIN_NONE: u8 = 0;
/// Local error.
pub const SO_EE_ORIGIN_LOCAL: u8 = 1;
/// ICMP error.
pub const SO_EE_ORIGIN_ICMP: u8 = 2;
/// ICMPv6 error.
pub const SO_EE_ORIGIN_ICMP6: u8 = 3;
/// TX timestamp.
pub const SO_EE_ORIGIN_TXSTATUS: u8 = 4;
/// Zero-window probe error.
pub const SO_EE_ORIGIN_ZEROCOPY: u8 = 5;
/// TX time error.
pub const SO_EE_ORIGIN_TXTIME: u8 = 6;

// ---------------------------------------------------------------------------
// SO_EE_CODE for timestamp events
// ---------------------------------------------------------------------------

/// Software timestamp.
pub const SO_EE_CODE_TXTIME_INVALID_PARAM: u8 = 1;
/// Missed deadline.
pub const SO_EE_CODE_TXTIME_MISSED: u8 = 2;

// ---------------------------------------------------------------------------
// Zero-copy notification codes
// ---------------------------------------------------------------------------

/// Zerocopy success (data sent without copy).
pub const SO_EE_CODE_ZEROCOPY_COPIED: u32 = 1;

// ---------------------------------------------------------------------------
// Socket extended error struct
// ---------------------------------------------------------------------------

/// Socket extended error (16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockExtendedErr {
    /// Error number.
    pub ee_errno: u32,
    /// Origin (SO_EE_ORIGIN_*).
    pub ee_origin: u8,
    /// Type (e.g., ICMP type).
    pub ee_type: u8,
    /// Code (e.g., ICMP code).
    pub ee_code: u8,
    /// Padding.
    pub ee_pad: u8,
    /// Error info (e.g., discovered MTU).
    pub ee_info: u32,
    /// Additional data.
    pub ee_data: u32,
}

impl SockExtendedErr {
    /// Create a zeroed extended error.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_origins_distinct() {
        let origins = [
            SO_EE_ORIGIN_NONE, SO_EE_ORIGIN_LOCAL,
            SO_EE_ORIGIN_ICMP, SO_EE_ORIGIN_ICMP6,
            SO_EE_ORIGIN_TXSTATUS, SO_EE_ORIGIN_ZEROCOPY,
            SO_EE_ORIGIN_TXTIME,
        ];
        for i in 0..origins.len() {
            for j in (i + 1)..origins.len() {
                assert_ne!(origins[i], origins[j]);
            }
        }
    }

    #[test]
    fn test_origin_values() {
        assert_eq!(SO_EE_ORIGIN_NONE, 0);
        assert_eq!(SO_EE_ORIGIN_ICMP, 2);
        assert_eq!(SO_EE_ORIGIN_ICMP6, 3);
    }

    #[test]
    fn test_sock_extended_err_size() {
        assert_eq!(core::mem::size_of::<SockExtendedErr>(), 16);
    }

    #[test]
    fn test_txtime_codes() {
        assert_ne!(SO_EE_CODE_TXTIME_INVALID_PARAM, SO_EE_CODE_TXTIME_MISSED);
    }
}

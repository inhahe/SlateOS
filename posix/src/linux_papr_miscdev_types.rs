//! `<asm/papr-miscdev.h>` (powerpc) — PAPR pSeries misc-device ioctls.
//!
//! On Power Architecture Platform Reference (PAPR) systems running
//! Linux, several `/dev/papr-*` character devices expose RTAS-call
//! and firmware-event interfaces (VPD retrieval, system parameters,
//! platform-dump, indices lookup). These constants cover the ioctl
//! group letters, common request opcodes, and size limits shared
//! across those miscdev nodes.

// ---------------------------------------------------------------------------
// ioctl group letters
// ---------------------------------------------------------------------------

/// Magic byte for `/dev/papr-vpd` ioctl group.
pub const PAPR_VPD_IOC_BASE: u8 = 0xb2;
/// Magic byte for `/dev/papr-sysparm` ioctl group.
pub const PAPR_SYSPARM_IOC_BASE: u8 = 0xb2;
/// Magic byte for `/dev/papr-indices` ioctl group.
pub const PAPR_INDICES_IOC_BASE: u8 = 0xb2;
/// Magic byte for `/dev/papr-physical-attestation` ioctl group.
pub const PAPR_ATTESTATION_IOC_BASE: u8 = 0xb2;

// ---------------------------------------------------------------------------
// VPD ioctl numbers
// ---------------------------------------------------------------------------

/// Begin a VPD retrieval transaction; returns an FD streaming the
/// keyword-encoded buffer.
pub const PAPR_VPD_IOC_CREATE_HANDLE: u32 = 0;

// ---------------------------------------------------------------------------
// sysparm ioctl numbers
// ---------------------------------------------------------------------------

/// Get a system parameter by token.
pub const PAPR_SYSPARM_IOC_GET: u32 = 1;
/// Set a system parameter by token.
pub const PAPR_SYSPARM_IOC_SET: u32 = 2;

// ---------------------------------------------------------------------------
// Sysparm/indices buffer sizes
// ---------------------------------------------------------------------------

/// Maximum bytes returned by an `ibm,get-system-parameter` RTAS call.
pub const PAPR_SYSPARM_MAX_OUTPUT: u32 = 4000;
/// Maximum bytes accepted by an `ibm,set-system-parameter` RTAS call.
pub const PAPR_SYSPARM_MAX_INPUT: u32 = 1024;

/// Maximum bytes accepted by an indices fetch.
pub const PAPR_INDICES_MAX_BUF: u32 = 4096;

// ---------------------------------------------------------------------------
// Indices "indices token" — RTAS function selectors
// ---------------------------------------------------------------------------

/// Token for `ibm,get-indices` "get sensor index range" call.
pub const PAPR_INDICES_GET_SENSORS: u32 = 1;
/// Token for `ibm,get-indices` "get DR connector indices" call.
pub const PAPR_INDICES_GET_DR_CONNECTORS: u32 = 2;
/// Token for `ibm,get-indices` "get firmware-defined VPD locations" call.
pub const PAPR_INDICES_GET_VPD_LOCATIONS: u32 = 3;

// ---------------------------------------------------------------------------
// Common RTAS return-status codes (mirrored across all PAPR miscdevs)
// ---------------------------------------------------------------------------

/// RTAS call succeeded.
pub const PAPR_RTAS_OK: i32 = 0;
/// RTAS hardware error.
pub const PAPR_RTAS_HW_ERROR: i32 = -1;
/// RTAS busy — retry later.
pub const PAPR_RTAS_BUSY: i32 = -2;
/// RTAS parameter error.
pub const PAPR_RTAS_PARAM_ERROR: i32 = -3;
/// RTAS multi-step continuation expected.
pub const PAPR_RTAS_MORE_DATA: i32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_groups_share_papr_magic() {
        // All PAPR miscdevs use the 0xb2 group letter so a single
        // capability check (CAP_SYS_ADMIN on /dev/papr-*) covers them.
        assert_eq!(PAPR_VPD_IOC_BASE, 0xb2);
        assert_eq!(PAPR_SYSPARM_IOC_BASE, 0xb2);
        assert_eq!(PAPR_INDICES_IOC_BASE, 0xb2);
        assert_eq!(PAPR_ATTESTATION_IOC_BASE, 0xb2);
    }

    #[test]
    fn test_sysparm_get_set_distinct() {
        assert_ne!(PAPR_SYSPARM_IOC_GET, PAPR_SYSPARM_IOC_SET);
    }

    #[test]
    fn test_buffer_sizes_sane() {
        assert!(PAPR_SYSPARM_MAX_OUTPUT >= PAPR_SYSPARM_MAX_INPUT);
        assert!(PAPR_INDICES_MAX_BUF >= PAPR_SYSPARM_MAX_OUTPUT);
    }

    #[test]
    fn test_indices_tokens_distinct() {
        let t = [
            PAPR_INDICES_GET_SENSORS,
            PAPR_INDICES_GET_DR_CONNECTORS,
            PAPR_INDICES_GET_VPD_LOCATIONS,
        ];
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
        }
    }

    #[test]
    fn test_rtas_status_codes_distinct_and_signs() {
        // Success is 0, errors are negative, MORE_DATA is positive
        // continuation marker.
        assert_eq!(PAPR_RTAS_OK, 0);
        assert!(PAPR_RTAS_HW_ERROR < 0);
        assert!(PAPR_RTAS_BUSY < 0);
        assert!(PAPR_RTAS_PARAM_ERROR < 0);
        assert!(PAPR_RTAS_MORE_DATA > 0);
        let codes = [
            PAPR_RTAS_OK,
            PAPR_RTAS_HW_ERROR,
            PAPR_RTAS_BUSY,
            PAPR_RTAS_PARAM_ERROR,
            PAPR_RTAS_MORE_DATA,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }
}

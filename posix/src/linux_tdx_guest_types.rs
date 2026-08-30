//! `<linux/tdx-guest.h>` — Intel TDX (Trust Domain Extensions) guest constants.
//!
//! Intel TDX provides hardware-isolated virtual machines (Trust
//! Domains) where the hypervisor is excluded from the TCB. The TDX
//! Module (firmware in a special CPU mode) manages TD lifecycle and
//! memory encryption. Guests interact via the /dev/tdx-guest device
//! for attestation reports and quote generation. TDX protects guest
//! memory confidentiality and integrity against a compromised
//! hypervisor, making it suitable for confidential cloud computing.

// ---------------------------------------------------------------------------
// TDX guest IOCTLs (via /dev/tdx-guest)
// ---------------------------------------------------------------------------

/// Get a TDX attestation report (TDREPORT).
pub const TDX_CMD_GET_REPORT0: u32 = 0xC040_7401;
/// Get a TD quote (remotely verifiable attestation).
pub const TDX_CMD_GET_QUOTE: u32 = 0xC020_7402;
/// Extend a runtime measurement register (RTMR).
pub const TDX_CMD_EXTEND_RTMR: u32 = 0xC040_7403;
/// Get quote size (for buffer allocation).
pub const TDX_CMD_GET_QUOTE_SIZE: u32 = 0x8008_7404;

// ---------------------------------------------------------------------------
// TDX report data sizes
// ---------------------------------------------------------------------------

/// Size of REPORTDATA field (user-provided nonce/data, 64 bytes).
pub const TDX_REPORTDATA_SIZE: u32 = 64;
/// Size of a TDREPORT structure (1024 bytes).
pub const TDX_REPORT_SIZE: u32 = 1024;
/// Size of RTMR (Runtime Measurement Register, 48 bytes SHA-384).
pub const TDX_RTMR_SIZE: u32 = 48;
/// Number of RTMRs available to the guest (0-3).
pub const TDX_NUM_RTMRS: u32 = 4;

// ---------------------------------------------------------------------------
// TDX attributes (from TDREPORT.ATTRIBUTES)
// ---------------------------------------------------------------------------

/// TD is in debug mode (SEAM can read TD memory).
pub const TDX_ATTR_DEBUG: u64 = 1 << 0;
/// TD uses SEPTVEs (Secure EPT Violation Exceptions).
pub const TDX_ATTR_SEPT_VE_DISABLE: u64 = 1 << 28;
/// TD PKS (Protection Keys for Supervisor) is enabled.
pub const TDX_ATTR_PKS: u64 = 1 << 30;
/// TD KL (Key Locker) is enabled.
pub const TDX_ATTR_KL: u64 = 1 << 31;
/// TD perfmon is enabled.
pub const TDX_ATTR_PERFMON: u64 = 1 << 63;

// ---------------------------------------------------------------------------
// TDX TDCALL leaf functions (guest → TDX Module)
// ---------------------------------------------------------------------------

/// Get TD info (CPUID-like info about the TD).
pub const TDCALL_TDINFO: u64 = 1;
/// Extend RTMR.
pub const TDCALL_TDEXTEND: u64 = 2;
/// Get TD report.
pub const TDCALL_TDREPORT: u64 = 4;
/// Accept a pending page (host assigned it, guest must accept).
pub const TDCALL_TDACCEPTPAGE: u64 = 6;
/// Map GPA as shared (remove encryption for I/O).
pub const TDCALL_TDVMCALL: u64 = 0;

// ---------------------------------------------------------------------------
// TDX error codes (from TDCALL return)
// ---------------------------------------------------------------------------

/// Success.
pub const TDX_SUCCESS: u64 = 0;
/// Operand is invalid.
pub const TDX_OPERAND_INVALID: u64 = 0xC000_0100;
/// Operand is busy (try again).
pub const TDX_OPERAND_BUSY: u64 = 0x8000_0200;
/// Page already accepted.
pub const TDX_PAGE_ALREADY_ACCEPTED: u64 = 0x0000_0B0A;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            TDX_CMD_GET_REPORT0,
            TDX_CMD_GET_QUOTE,
            TDX_CMD_EXTEND_RTMR,
            TDX_CMD_GET_QUOTE_SIZE,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_report_sizes() {
        assert_eq!(TDX_REPORTDATA_SIZE, 64);
        assert_eq!(TDX_REPORT_SIZE, 1024);
        assert_eq!(TDX_RTMR_SIZE, 48);
        assert_eq!(TDX_NUM_RTMRS, 4);
    }

    #[test]
    fn test_attributes_no_overlap() {
        let attrs = [
            TDX_ATTR_DEBUG,
            TDX_ATTR_SEPT_VE_DISABLE,
            TDX_ATTR_PKS,
            TDX_ATTR_KL,
            TDX_ATTR_PERFMON,
        ];
        for i in 0..attrs.len() {
            assert!(attrs[i].is_power_of_two());
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }

    #[test]
    fn test_tdcall_leaves_distinct() {
        let leaves = [
            TDCALL_TDINFO,
            TDCALL_TDEXTEND,
            TDCALL_TDREPORT,
            TDCALL_TDACCEPTPAGE,
            TDCALL_TDVMCALL,
        ];
        for i in 0..leaves.len() {
            for j in (i + 1)..leaves.len() {
                assert_ne!(leaves[i], leaves[j]);
            }
        }
    }

    #[test]
    fn test_error_codes_distinct() {
        let errs = [
            TDX_SUCCESS,
            TDX_OPERAND_INVALID,
            TDX_OPERAND_BUSY,
            TDX_PAGE_ALREADY_ACCEPTED,
        ];
        for i in 0..errs.len() {
            for j in (i + 1)..errs.len() {
                assert_ne!(errs[i], errs[j]);
            }
        }
    }
}

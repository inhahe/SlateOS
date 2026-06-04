//! `<linux/arm-smccc.h>` — ARM SMC/HVC calling-convention constants.
//!
//! SMCCC is the calling convention firmware on ARMv8+ uses to expose
//! services (PSCI, TRNG, Spectre mitigations, hypervisor calls) to
//! the OS. The function-ID encoding and well-known service ranges
//! are part of the userspace-visible ABI on `/sys/firmware/`.

// ---------------------------------------------------------------------------
// Function-ID bit layout
// ---------------------------------------------------------------------------

/// Bit 31: fast (1) vs. yielding (0) call.
pub const SMCCC_FUNC_FAST_CALL: u32 = 1 << 31;
/// Bit 30: SMC64 (1) vs. SMC32 (0) calling convention.
pub const SMCCC_FUNC_SMC64: u32 = 1 << 30;
/// Bits 24..29: owning entity (service group).
pub const SMCCC_FUNC_OWNER_SHIFT: u32 = 24;
pub const SMCCC_FUNC_OWNER_MASK: u32 = 0x3F << SMCCC_FUNC_OWNER_SHIFT;
/// Bits 0..15: function number within the owning entity.
pub const SMCCC_FUNC_NUM_MASK: u32 = 0xFFFF;

// ---------------------------------------------------------------------------
// Owning entity (service) IDs
// ---------------------------------------------------------------------------

pub const SMCCC_OWNER_ARCH: u32 = 0;
pub const SMCCC_OWNER_CPU: u32 = 1;
pub const SMCCC_OWNER_SIP: u32 = 2;
pub const SMCCC_OWNER_OEM: u32 = 3;
pub const SMCCC_OWNER_STANDARD: u32 = 4;
pub const SMCCC_OWNER_STANDARD_HYP: u32 = 5;
pub const SMCCC_OWNER_VENDOR_HYP: u32 = 6;
pub const SMCCC_OWNER_TRUSTED_APP: u32 = 48;
pub const SMCCC_OWNER_TRUSTED_OS: u32 = 50;

// ---------------------------------------------------------------------------
// Return codes (all calls signal status through r0/x0)
// ---------------------------------------------------------------------------

pub const SMCCC_RET_SUCCESS: i64 = 0;
pub const SMCCC_RET_NOT_SUPPORTED: i64 = -1;
pub const SMCCC_RET_NOT_REQUIRED: i64 = -2;
pub const SMCCC_RET_INVALID_PARAMETER: i64 = -3;

// ---------------------------------------------------------------------------
// PSCI version (owner=STANDARD, FAST_CALL, num=0)
// ---------------------------------------------------------------------------

pub const PSCI_FN_VERSION: u32 = SMCCC_FUNC_FAST_CALL
    | (SMCCC_OWNER_STANDARD << SMCCC_FUNC_OWNER_SHIFT);
pub const PSCI_VERSION_1_1: u32 = 0x0001_0001;
pub const PSCI_VERSION_1_0: u32 = 0x0001_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_and_smc64_top_bits() {
        // Top two bits encode call-class and width.
        assert_eq!(SMCCC_FUNC_FAST_CALL, 0x8000_0000);
        assert_eq!(SMCCC_FUNC_SMC64, 0x4000_0000);
        assert!(SMCCC_FUNC_FAST_CALL > SMCCC_FUNC_SMC64);
    }

    #[test]
    fn test_owner_mask_shifts_to_six_bits() {
        // Owner field is 6 bits, shifted up by 24.
        assert_eq!(SMCCC_FUNC_OWNER_SHIFT, 24);
        assert_eq!(SMCCC_FUNC_OWNER_MASK, 0x3F00_0000);
        // 6 bits of owner = 64 possible owners.
        assert_eq!((SMCCC_FUNC_OWNER_MASK >> SMCCC_FUNC_OWNER_SHIFT) + 1, 64);
        // Function-number field is the low 16 bits.
        assert_eq!(SMCCC_FUNC_NUM_MASK, 0xFFFF);
        // No overlap between owner and func-num fields.
        assert_eq!(SMCCC_FUNC_OWNER_MASK & SMCCC_FUNC_NUM_MASK, 0);
    }

    #[test]
    fn test_known_owner_ids_distinct() {
        let o = [
            SMCCC_OWNER_ARCH,
            SMCCC_OWNER_CPU,
            SMCCC_OWNER_SIP,
            SMCCC_OWNER_OEM,
            SMCCC_OWNER_STANDARD,
            SMCCC_OWNER_STANDARD_HYP,
            SMCCC_OWNER_VENDOR_HYP,
            SMCCC_OWNER_TRUSTED_APP,
            SMCCC_OWNER_TRUSTED_OS,
        ];
        for (i, &a) in o.iter().enumerate() {
            for &b in &o[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // First 7 are 0..6 dense.
        assert_eq!(SMCCC_OWNER_ARCH, 0);
        assert_eq!(SMCCC_OWNER_VENDOR_HYP, 6);
    }

    #[test]
    fn test_return_codes_negative_except_success() {
        assert_eq!(SMCCC_RET_SUCCESS, 0);
        for v in [
            SMCCC_RET_NOT_SUPPORTED,
            SMCCC_RET_NOT_REQUIRED,
            SMCCC_RET_INVALID_PARAMETER,
        ] {
            assert!(v < 0);
        }
    }

    #[test]
    fn test_psci_version_function_id_layout() {
        // PSCI_FN_VERSION = fast call + standard owner + func 0.
        assert!(PSCI_FN_VERSION & SMCCC_FUNC_FAST_CALL != 0);
        assert_eq!(
            (PSCI_FN_VERSION & SMCCC_FUNC_OWNER_MASK) >> SMCCC_FUNC_OWNER_SHIFT,
            SMCCC_OWNER_STANDARD
        );
        assert_eq!(PSCI_FN_VERSION & SMCCC_FUNC_NUM_MASK, 0);
    }

    #[test]
    fn test_psci_version_ordering() {
        // 1.1 > 1.0 numerically — major in high half, minor in low.
        assert!(PSCI_VERSION_1_1 > PSCI_VERSION_1_0);
        assert_eq!(PSCI_VERSION_1_0 >> 16, 1);
        assert_eq!(PSCI_VERSION_1_1 & 0xFFFF, 1);
    }
}

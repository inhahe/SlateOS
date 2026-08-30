//! `<linux/arm_sdei.h>` / ARM SMCCC — ARM Secure Monitor Call Calling Convention constants.
//!
//! ARM SMCCC defines the interface between Normal World (Linux) and
//! Secure World (TrustZone firmware / hypervisor). SMC (Secure Monitor
//! Call) and HVC (Hypervisor Call) instructions use this convention.
//! Services include PSCI (power management), SDEI (software delegated
//! exception interface), and vendor-specific secure services. The
//! calling convention specifies register usage, function IDs, and
//! return codes.

// ---------------------------------------------------------------------------
// SMCCC calling convention versions
// ---------------------------------------------------------------------------

/// SMCCC v1.0.
pub const SMCCC_VERSION_1_0: u32 = 0x1_0000;
/// SMCCC v1.1.
pub const SMCCC_VERSION_1_1: u32 = 0x1_0001;
/// SMCCC v1.2.
pub const SMCCC_VERSION_1_2: u32 = 0x1_0002;
/// SMCCC v1.3.
pub const SMCCC_VERSION_1_3: u32 = 0x1_0003;

// ---------------------------------------------------------------------------
// SMCCC conduit types
// ---------------------------------------------------------------------------

/// Use SMC instruction (Normal World → Secure World).
pub const SMCCC_CONDUIT_SMC: u32 = 1;
/// Use HVC instruction (guest → hypervisor).
pub const SMCCC_CONDUIT_HVC: u32 = 2;

// ---------------------------------------------------------------------------
// SMCCC function ID bits
// ---------------------------------------------------------------------------

/// Fast call (no preemption, quick return).
pub const SMCCC_FAST_CALL: u32 = 1 << 31;
/// SMC64/HVC64 (64-bit calling convention).
pub const SMCCC_SMC_64: u32 = 1 << 30;
/// Service call owner: ARM Architecture.
pub const SMCCC_OWNER_ARCH: u32 = 0;
/// Service call owner: CPU (implementation defined).
pub const SMCCC_OWNER_CPU: u32 = 1;
/// Service call owner: SiP (Silicon Provider).
pub const SMCCC_OWNER_SIP: u32 = 2;
/// Service call owner: OEM.
pub const SMCCC_OWNER_OEM: u32 = 3;
/// Service call owner: Standard Secure (PSCI, etc.).
pub const SMCCC_OWNER_STANDARD: u32 = 4;
/// Service call owner: Standard Hypervisor.
pub const SMCCC_OWNER_STANDARD_HYP: u32 = 5;
/// Service call owner: Vendor Hypervisor.
pub const SMCCC_OWNER_VENDOR_HYP: u32 = 6;

// ---------------------------------------------------------------------------
// SMCCC return codes
// ---------------------------------------------------------------------------

/// Success.
pub const SMCCC_RET_SUCCESS: i32 = 0;
/// Call not supported.
pub const SMCCC_RET_NOT_SUPPORTED: i32 = -1;
/// Not required (already in desired state).
pub const SMCCC_RET_NOT_REQUIRED: i32 = -2;
/// Invalid parameter.
pub const SMCCC_RET_INVALID_PARAMETER: i32 = -3;

// ---------------------------------------------------------------------------
// PSCI (Power State Coordination Interface) function IDs
// ---------------------------------------------------------------------------

/// Get PSCI version.
pub const PSCI_FN_VERSION: u32 = 0x8400_0000;
/// CPU suspend (enter low-power state).
pub const PSCI_FN_CPU_SUSPEND: u32 = 0x8400_0001;
/// CPU off (shut down calling CPU).
pub const PSCI_FN_CPU_OFF: u32 = 0x8400_0002;
/// CPU on (bring a CPU online).
pub const PSCI_FN_CPU_ON: u32 = 0x8400_0003;
/// System off (shut down the system).
pub const PSCI_FN_SYSTEM_OFF: u32 = 0x8400_0008;
/// System reset (reboot).
pub const PSCI_FN_SYSTEM_RESET: u32 = 0x8400_0009;

// ---------------------------------------------------------------------------
// PSCI return codes
// ---------------------------------------------------------------------------

/// PSCI success.
pub const PSCI_RET_SUCCESS: i32 = 0;
/// PSCI not supported.
pub const PSCI_RET_NOT_SUPPORTED: i32 = -1;
/// PSCI invalid parameters.
pub const PSCI_RET_INVALID_PARAMS: i32 = -2;
/// PSCI denied.
pub const PSCI_RET_DENIED: i32 = -3;
/// PSCI already on.
pub const PSCI_RET_ALREADY_ON: i32 = -4;
/// PSCI on pending.
pub const PSCI_RET_ON_PENDING: i32 = -5;
/// PSCI internal failure.
pub const PSCI_RET_INTERNAL_FAILURE: i32 = -6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_ordered() {
        assert!(SMCCC_VERSION_1_0 < SMCCC_VERSION_1_1);
        assert!(SMCCC_VERSION_1_1 < SMCCC_VERSION_1_2);
        assert!(SMCCC_VERSION_1_2 < SMCCC_VERSION_1_3);
    }

    #[test]
    fn test_conduits_distinct() {
        assert_ne!(SMCCC_CONDUIT_SMC, SMCCC_CONDUIT_HVC);
    }

    #[test]
    fn test_owners_distinct() {
        let owners = [
            SMCCC_OWNER_ARCH,
            SMCCC_OWNER_CPU,
            SMCCC_OWNER_SIP,
            SMCCC_OWNER_OEM,
            SMCCC_OWNER_STANDARD,
            SMCCC_OWNER_STANDARD_HYP,
            SMCCC_OWNER_VENDOR_HYP,
        ];
        for i in 0..owners.len() {
            for j in (i + 1)..owners.len() {
                assert_ne!(owners[i], owners[j]);
            }
        }
    }

    #[test]
    fn test_smccc_return_codes_distinct() {
        let codes = [
            SMCCC_RET_SUCCESS,
            SMCCC_RET_NOT_SUPPORTED,
            SMCCC_RET_NOT_REQUIRED,
            SMCCC_RET_INVALID_PARAMETER,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_psci_functions_distinct() {
        let fns = [
            PSCI_FN_VERSION,
            PSCI_FN_CPU_SUSPEND,
            PSCI_FN_CPU_OFF,
            PSCI_FN_CPU_ON,
            PSCI_FN_SYSTEM_OFF,
            PSCI_FN_SYSTEM_RESET,
        ];
        for i in 0..fns.len() {
            for j in (i + 1)..fns.len() {
                assert_ne!(fns[i], fns[j]);
            }
        }
    }

    #[test]
    fn test_psci_return_codes_distinct() {
        let codes = [
            PSCI_RET_SUCCESS,
            PSCI_RET_NOT_SUPPORTED,
            PSCI_RET_INVALID_PARAMS,
            PSCI_RET_DENIED,
            PSCI_RET_ALREADY_ON,
            PSCI_RET_ON_PENDING,
            PSCI_RET_INTERNAL_FAILURE,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_fast_call_and_smc64_no_overlap() {
        assert_eq!(SMCCC_FAST_CALL & SMCCC_SMC_64, 0);
        assert!(SMCCC_FAST_CALL.is_power_of_two());
        assert!(SMCCC_SMC_64.is_power_of_two());
    }
}

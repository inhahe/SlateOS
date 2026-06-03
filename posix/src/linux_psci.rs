//! `<linux/psci.h>` — ARM PSCI (Power State Coordination Interface) constants.
//!
//! PSCI is the standard ARM firmware interface for CPU power management:
//! hotplug, idle states, system reset, and shutdown. Used by KVM on ARM
//! and the kernel's SMP/cpuidle subsystems.

// ---------------------------------------------------------------------------
// PSCI function IDs (SMC/HVC call numbers)
// ---------------------------------------------------------------------------

/// PSCI version query.
pub const PSCI_0_2_FN_PSCI_VERSION: u32 = 0x8400_0000;
/// CPU suspend (32-bit).
pub const PSCI_0_2_FN_CPU_SUSPEND: u32 = 0x8400_0001;
/// CPU off.
pub const PSCI_0_2_FN_CPU_OFF: u32 = 0x8400_0002;
/// CPU on (32-bit).
pub const PSCI_0_2_FN_CPU_ON: u32 = 0x8400_0003;
/// Affinity info (32-bit).
pub const PSCI_0_2_FN_AFFINITY_INFO: u32 = 0x8400_0004;
/// Migrate (32-bit).
pub const PSCI_0_2_FN_MIGRATE: u32 = 0x8400_0005;
/// Migrate info type.
pub const PSCI_0_2_FN_MIGRATE_INFO_TYPE: u32 = 0x8400_0006;
/// Migrate info up CPU (32-bit).
pub const PSCI_0_2_FN_MIGRATE_INFO_UP_CPU: u32 = 0x8400_0007;
/// System off.
pub const PSCI_0_2_FN_SYSTEM_OFF: u32 = 0x8400_0008;
/// System reset.
pub const PSCI_0_2_FN_SYSTEM_RESET: u32 = 0x8400_0009;

// ---------------------------------------------------------------------------
// PSCI 64-bit function IDs
// ---------------------------------------------------------------------------

/// CPU suspend (64-bit).
pub const PSCI_0_2_FN64_CPU_SUSPEND: u32 = 0xC400_0001;
/// CPU on (64-bit).
pub const PSCI_0_2_FN64_CPU_ON: u32 = 0xC400_0003;
/// Affinity info (64-bit).
pub const PSCI_0_2_FN64_AFFINITY_INFO: u32 = 0xC400_0004;
/// Migrate (64-bit).
pub const PSCI_0_2_FN64_MIGRATE: u32 = 0xC400_0005;
/// Migrate info up CPU (64-bit).
pub const PSCI_0_2_FN64_MIGRATE_INFO_UP_CPU: u32 = 0xC400_0007;

// ---------------------------------------------------------------------------
// PSCI 1.0 function IDs
// ---------------------------------------------------------------------------

/// Query supported PSCI features.
pub const PSCI_1_0_FN_PSCI_FEATURES: u32 = 0x8400_000A;
/// CPU freeze.
pub const PSCI_1_0_FN_CPU_FREEZE: u32 = 0x8400_000B;
/// CPU default suspend (32-bit).
pub const PSCI_1_0_FN_CPU_DEFAULT_SUSPEND: u32 = 0x8400_000C;
/// Node HW state (32-bit).
pub const PSCI_1_0_FN_NODE_HW_STATE: u32 = 0x8400_000D;
/// System suspend (32-bit).
pub const PSCI_1_0_FN_SYSTEM_SUSPEND: u32 = 0x8400_000E;

// ---------------------------------------------------------------------------
// PSCI return codes
// ---------------------------------------------------------------------------

/// Success.
pub const PSCI_RET_SUCCESS: i32 = 0;
/// Not supported.
pub const PSCI_RET_NOT_SUPPORTED: i32 = -1;
/// Invalid parameters.
pub const PSCI_RET_INVALID_PARAMS: i32 = -2;
/// Denied.
pub const PSCI_RET_DENIED: i32 = -3;
/// Already on.
pub const PSCI_RET_ALREADY_ON: i32 = -4;
/// On pending.
pub const PSCI_RET_ON_PENDING: i32 = -5;
/// Internal failure.
pub const PSCI_RET_INTERNAL_FAILURE: i32 = -6;
/// Not present.
pub const PSCI_RET_NOT_PRESENT: i32 = -7;
/// Disabled.
pub const PSCI_RET_DISABLED: i32 = -8;
/// Invalid address.
pub const PSCI_RET_INVALID_ADDRESS: i32 = -9;

// ---------------------------------------------------------------------------
// Affinity states
// ---------------------------------------------------------------------------

/// CPU is on.
pub const PSCI_AFFINITY_LEVEL_ON: u32 = 0;
/// CPU is off.
pub const PSCI_AFFINITY_LEVEL_OFF: u32 = 1;
/// CPU is on pending.
pub const PSCI_AFFINITY_LEVEL_ON_PENDING: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fn_ids_distinct() {
        let fns = [
            PSCI_0_2_FN_PSCI_VERSION,
            PSCI_0_2_FN_CPU_SUSPEND,
            PSCI_0_2_FN_CPU_OFF,
            PSCI_0_2_FN_CPU_ON,
            PSCI_0_2_FN_AFFINITY_INFO,
            PSCI_0_2_FN_MIGRATE,
            PSCI_0_2_FN_MIGRATE_INFO_TYPE,
            PSCI_0_2_FN_MIGRATE_INFO_UP_CPU,
            PSCI_0_2_FN_SYSTEM_OFF,
            PSCI_0_2_FN_SYSTEM_RESET,
        ];
        for i in 0..fns.len() {
            for j in (i + 1)..fns.len() {
                assert_ne!(fns[i], fns[j]);
            }
        }
    }

    #[test]
    fn test_fn64_ids_distinct() {
        let fns = [
            PSCI_0_2_FN64_CPU_SUSPEND,
            PSCI_0_2_FN64_CPU_ON,
            PSCI_0_2_FN64_AFFINITY_INFO,
            PSCI_0_2_FN64_MIGRATE,
            PSCI_0_2_FN64_MIGRATE_INFO_UP_CPU,
        ];
        for i in 0..fns.len() {
            for j in (i + 1)..fns.len() {
                assert_ne!(fns[i], fns[j]);
            }
        }
    }

    #[test]
    fn test_32_64_bit_fn_ids() {
        // 64-bit IDs differ from 32-bit by the top nibble (C vs 8)
        assert_ne!(PSCI_0_2_FN_CPU_SUSPEND, PSCI_0_2_FN64_CPU_SUSPEND);
        assert_ne!(PSCI_0_2_FN_CPU_ON, PSCI_0_2_FN64_CPU_ON);
    }

    #[test]
    fn test_return_codes_distinct() {
        let codes = [
            PSCI_RET_SUCCESS,
            PSCI_RET_NOT_SUPPORTED,
            PSCI_RET_INVALID_PARAMS,
            PSCI_RET_DENIED,
            PSCI_RET_ALREADY_ON,
            PSCI_RET_ON_PENDING,
            PSCI_RET_INTERNAL_FAILURE,
            PSCI_RET_NOT_PRESENT,
            PSCI_RET_DISABLED,
            PSCI_RET_INVALID_ADDRESS,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_affinity_states_distinct() {
        let states = [
            PSCI_AFFINITY_LEVEL_ON,
            PSCI_AFFINITY_LEVEL_OFF,
            PSCI_AFFINITY_LEVEL_ON_PENDING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_success_is_zero() {
        assert_eq!(PSCI_RET_SUCCESS, 0);
    }
}

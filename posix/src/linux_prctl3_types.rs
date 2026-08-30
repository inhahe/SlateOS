//! `<linux/prctl.h>` — Additional prctl constants (batch 3).
//!
//! Supplementary prctl constants covering shadow stack control,
//! memory tagging, and speculation control.

// ---------------------------------------------------------------------------
// Shadow stack control (PR_SET_SHADOW_STACK_STATUS)
// ---------------------------------------------------------------------------

/// Enable shadow stack.
pub const PR_SHADOW_STACK_ENABLE: u64 = 1 << 0;
/// Enable shadow stack write permission.
pub const PR_SHADOW_STACK_WRITE: u64 = 1 << 1;
/// Push shadow stack token.
pub const PR_SHADOW_STACK_PUSH: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Memory tagging (PR_SET_TAGGED_ADDR_CTRL)
// ---------------------------------------------------------------------------

/// Enable tagged address ABI.
pub const PR_TAGGED_ADDR_ENABLE: u64 = 1 << 0;
/// MTE: synchronous tag check fault.
pub const PR_MTE_TCF_SYNC: u64 = 1 << 1;
/// MTE: asynchronous tag check fault.
pub const PR_MTE_TCF_ASYNC: u64 = 1 << 2;
/// MTE: tag check fault mask.
pub const PR_MTE_TCF_MASK: u64 = (1 << 1) | (1 << 2);
/// MTE: tag inclusion mask shift.
pub const PR_MTE_TAG_SHIFT: u32 = 3;
/// MTE: tag inclusion mask.
pub const PR_MTE_TAG_MASK: u64 = 0xFFFF << 3;

// ---------------------------------------------------------------------------
// Speculation control (PR_SET_SPECULATION_CTRL)
// ---------------------------------------------------------------------------

/// Speculation: store bypass.
pub const PR_SPEC_STORE_BYPASS: u32 = 0;
/// Speculation: indirect branch.
pub const PR_SPEC_INDIRECT_BRANCH: u32 = 1;
/// Speculation: L1D flush.
pub const PR_SPEC_L1D_FLUSH: u32 = 2;

/// Spec ctrl: not affected.
pub const PR_SPEC_NOT_AFFECTED: u32 = 0;
/// Spec ctrl: prctl control available.
pub const PR_SPEC_PRCTL: u32 = 1 << 0;
/// Spec ctrl: mitigation enabled.
pub const PR_SPEC_ENABLE: u32 = 1 << 1;
/// Spec ctrl: mitigation disabled.
pub const PR_SPEC_DISABLE: u32 = 1 << 2;
/// Spec ctrl: forced mitigation.
pub const PR_SPEC_FORCE_DISABLE: u32 = 1 << 3;
/// Spec ctrl: disable seccomp override.
pub const PR_SPEC_DISABLE_NOEXEC: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// PAC key control (PR_PAC_*)
// ---------------------------------------------------------------------------

/// Reset PAC APIA key.
pub const PR_PAC_RESET_KEYS_APIA: u64 = 1 << 0;
/// Reset PAC APIB key.
pub const PR_PAC_RESET_KEYS_APIB: u64 = 1 << 1;
/// Reset PAC APDA key.
pub const PR_PAC_RESET_KEYS_APDA: u64 = 1 << 2;
/// Reset PAC APDB key.
pub const PR_PAC_RESET_KEYS_APDB: u64 = 1 << 3;
/// Reset PAC APGA key.
pub const PR_PAC_RESET_KEYS_APGA: u64 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_stack_flags_power_of_two() {
        assert!(PR_SHADOW_STACK_ENABLE.is_power_of_two());
        assert!(PR_SHADOW_STACK_WRITE.is_power_of_two());
        assert!(PR_SHADOW_STACK_PUSH.is_power_of_two());
    }

    #[test]
    fn test_shadow_stack_flags_no_overlap() {
        let flags = [
            PR_SHADOW_STACK_ENABLE,
            PR_SHADOW_STACK_WRITE,
            PR_SHADOW_STACK_PUSH,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_mte_tcf_mask() {
        assert_eq!(PR_MTE_TCF_MASK, PR_MTE_TCF_SYNC | PR_MTE_TCF_ASYNC);
    }

    #[test]
    fn test_spec_types_distinct() {
        let types = [
            PR_SPEC_STORE_BYPASS,
            PR_SPEC_INDIRECT_BRANCH,
            PR_SPEC_L1D_FLUSH,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_spec_ctrl_flags_power_of_two() {
        let flags = [
            PR_SPEC_PRCTL,
            PR_SPEC_ENABLE,
            PR_SPEC_DISABLE,
            PR_SPEC_FORCE_DISABLE,
            PR_SPEC_DISABLE_NOEXEC,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_pac_keys_power_of_two() {
        let keys = [
            PR_PAC_RESET_KEYS_APIA,
            PR_PAC_RESET_KEYS_APIB,
            PR_PAC_RESET_KEYS_APDA,
            PR_PAC_RESET_KEYS_APDB,
            PR_PAC_RESET_KEYS_APGA,
        ];
        for k in &keys {
            assert!(k.is_power_of_two(), "0x{:016x} not power of two", k);
        }
    }

    #[test]
    fn test_pac_keys_no_overlap() {
        let keys = [
            PR_PAC_RESET_KEYS_APIA,
            PR_PAC_RESET_KEYS_APIB,
            PR_PAC_RESET_KEYS_APDA,
            PR_PAC_RESET_KEYS_APDB,
            PR_PAC_RESET_KEYS_APGA,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_eq!(keys[i] & keys[j], 0);
            }
        }
    }
}

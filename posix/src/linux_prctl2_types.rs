//! `<linux/prctl.h>` (additional operations) — prctl() extended constants.
//!
//! prctl() is a multipurpose syscall for per-process/thread settings.
//! This module covers operations beyond basic signal/name handling:
//! memory management hints, speculation control, tagged addresses,
//! shadow stack control, and syscall user dispatch. These are newer
//! additions that control security hardening and architecture-specific
//! features.

// ---------------------------------------------------------------------------
// Speculation control (Spectre mitigations)
// ---------------------------------------------------------------------------

/// Get speculation control state.
pub const PR_GET_SPECULATION_CTRL: u32 = 52;
/// Set speculation control state.
pub const PR_SET_SPECULATION_CTRL: u32 = 53;
/// Speculative store bypass (SSBD) speculation type.
pub const PR_SPEC_STORE_BYPASS: u32 = 0;
/// Indirect branch speculation type.
pub const PR_SPEC_INDIRECT_BRANCH: u32 = 1;
/// Speculative execution is unrestricted.
pub const PR_SPEC_NOT_AFFECTED: u32 = 0;
/// Mitigation can be enabled via prctl.
pub const PR_SPEC_PRCTL: u32 = 1;
/// Mitigation is always enabled.
pub const PR_SPEC_ENABLE: u32 = 2;
/// Mitigation is explicitly disabled.
pub const PR_SPEC_DISABLE: u32 = 4;
/// Mitigation is forcefully disabled (cannot be re-enabled).
pub const PR_SPEC_FORCE_DISABLE: u32 = 8;

// ---------------------------------------------------------------------------
// Tagged address control (ARM64 TBI, x86_64 LAM)
// ---------------------------------------------------------------------------

/// Get tagged address mode.
pub const PR_GET_TAGGED_ADDR_CTRL: u32 = 56;
/// Set tagged address mode.
pub const PR_SET_TAGGED_ADDR_CTRL: u32 = 55;
/// Enable tagged addresses (TBI/LAM).
pub const PR_TAGGED_ADDR_ENABLE: u32 = 0x01;

// ---------------------------------------------------------------------------
// Memory management hints
// ---------------------------------------------------------------------------

/// Get memory merge (KSM) enable state.
pub const PR_GET_MEMORY_MERGE: u32 = 68;
/// Enable/disable KSM merging for this process.
pub const PR_SET_MEMORY_MERGE: u32 = 67;
/// Get VMA name (prctl-set anonymous VMA label).
pub const PR_SET_VMA: u32 = 0x5367_6D61;
/// Sub-command: set VMA name string.
pub const PR_SET_VMA_ANON_NAME: u32 = 0;

// ---------------------------------------------------------------------------
// Syscall user dispatch
// ---------------------------------------------------------------------------

/// Set syscall user dispatch mode.
pub const PR_SET_SYSCALL_USER_DISPATCH: u32 = 59;
/// Syscall user dispatch: off (normal syscall handling).
pub const PR_SYS_DISPATCH_OFF: u32 = 0;
/// Syscall user dispatch: on (signal on syscall from specified region).
pub const PR_SYS_DISPATCH_ON: u32 = 1;

// ---------------------------------------------------------------------------
// Shadow stack (CET) prctl
// ---------------------------------------------------------------------------

/// Lock shadow stack (prevent disabling).
pub const PR_LOCK_SHADOW_STACK_STATUS: u32 = 0x01;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speculation_ops_distinct() {
        assert_ne!(PR_GET_SPECULATION_CTRL, PR_SET_SPECULATION_CTRL);
        assert_ne!(PR_SPEC_STORE_BYPASS, PR_SPEC_INDIRECT_BRANCH);
    }

    #[test]
    fn test_spec_values_distinct() {
        let vals = [
            PR_SPEC_NOT_AFFECTED,
            PR_SPEC_PRCTL,
            PR_SPEC_ENABLE,
            PR_SPEC_DISABLE,
            PR_SPEC_FORCE_DISABLE,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_tagged_addr_ops_distinct() {
        assert_ne!(PR_GET_TAGGED_ADDR_CTRL, PR_SET_TAGGED_ADDR_CTRL);
    }

    #[test]
    fn test_dispatch_values_distinct() {
        assert_ne!(PR_SYS_DISPATCH_OFF, PR_SYS_DISPATCH_ON);
    }

    #[test]
    fn test_memory_merge_ops_distinct() {
        assert_ne!(PR_GET_MEMORY_MERGE, PR_SET_MEMORY_MERGE);
    }
}

//! `<linux/prctl.h>` — extended prctl operations.
//!
//! Additional `prctl()` constants not covered by `<sys/prctl.h>`.
//! These are Linux-specific extensions used by security frameworks,
//! seccomp, and process management.

// Re-export the base prctl function and core constants.
pub use crate::sys_prctl::PR_GET_NAME;
pub use crate::sys_prctl::PR_SET_NAME;
pub use crate::sys_prctl::prctl;

// ---------------------------------------------------------------------------
// Memory management hints
// ---------------------------------------------------------------------------

/// Set/get tagged address ABI mode.
pub const PR_SET_TAGGED_ADDR_CTRL: i32 = 55;
/// Get tagged address ABI mode.
pub const PR_GET_TAGGED_ADDR_CTRL: i32 = 56;

/// Set memory deny-write-execute policy.
pub const PR_SET_MDWE: i32 = 65;
/// Get memory deny-write-execute policy.
pub const PR_GET_MDWE: i32 = 66;

// ---------------------------------------------------------------------------
// Speculation control (extended)
// ---------------------------------------------------------------------------

/// Speculative store bypass disable.
pub const PR_SPEC_STORE_BYPASS: u64 = 0;

/// Indirect branch speculation.
pub const PR_SPEC_INDIRECT_BRANCH: u64 = 1;

/// Disable speculation.
pub const PR_SPEC_DISABLE: u64 = 4;

/// Enable speculation.
pub const PR_SPEC_ENABLE: u64 = 2;

/// Force disable.
pub const PR_SPEC_FORCE_DISABLE: u64 = 8;

/// Not affected by speculation vulnerability.
pub const PR_SPEC_NOT_AFFECTED: u64 = 0;

/// Can be disabled by prctl.
pub const PR_SPEC_PRCTL: u64 = 1;

// ---------------------------------------------------------------------------
// Shadow stack
// ---------------------------------------------------------------------------

/// Enable shadow stack.
pub const PR_SET_SHADOW_STACK_STATUS: i32 = 74;
/// Get shadow stack status.
pub const PR_GET_SHADOW_STACK_STATUS: i32 = 75;
/// Lock shadow stack configuration.
pub const PR_LOCK_SHADOW_STACK_STATUS: i32 = 76;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tagged_addr_ctrl() {
        assert_ne!(PR_SET_TAGGED_ADDR_CTRL, PR_GET_TAGGED_ADDR_CTRL);
    }

    #[test]
    fn test_mdwe() {
        assert_ne!(PR_SET_MDWE, PR_GET_MDWE);
    }

    #[test]
    fn test_shadow_stack() {
        let consts = [
            PR_SET_SHADOW_STACK_STATUS,
            PR_GET_SHADOW_STACK_STATUS,
            PR_LOCK_SHADOW_STACK_STATUS,
        ];
        for i in 0..consts.len() {
            for j in (i + 1)..consts.len() {
                assert_ne!(consts[i], consts[j]);
            }
        }
    }

    #[test]
    fn test_spec_distinct() {
        let specs = [PR_SPEC_STORE_BYPASS, PR_SPEC_INDIRECT_BRANCH];
        assert_ne!(specs[0], specs[1]);
    }

    #[test]
    fn test_spec_actions_distinct() {
        let acts = [
            PR_SPEC_NOT_AFFECTED,
            PR_SPEC_PRCTL,
            PR_SPEC_ENABLE,
            PR_SPEC_DISABLE,
            PR_SPEC_FORCE_DISABLE,
        ];
        for i in 0..acts.len() {
            for j in (i + 1)..acts.len() {
                assert_ne!(acts[i], acts[j]);
            }
        }
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(PR_SET_NAME, crate::sys_prctl::PR_SET_NAME);
    }
}

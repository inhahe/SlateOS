//! `<linux/prctl.h>` — Linux prctl() constants.
//!
//! Re-exports all prctl constants from `sys_prctl`, `sys_prctl_caps`,
//! and `unistd` in a single module matching the kernel header structure.

// ---------------------------------------------------------------------------
// Core prctl constants from sys_prctl
// ---------------------------------------------------------------------------

pub use crate::sys_prctl::PR_SET_PDEATHSIG;
pub use crate::sys_prctl::PR_GET_PDEATHSIG;
pub use crate::sys_prctl::PR_SET_DUMPABLE;
pub use crate::sys_prctl::PR_GET_DUMPABLE;
pub use crate::sys_prctl::PR_SET_CHILD_SUBREAPER;
pub use crate::sys_prctl::PR_GET_CHILD_SUBREAPER;
pub use crate::sys_prctl::PR_SET_TIMERSLACK;
pub use crate::sys_prctl::PR_GET_TIMERSLACK;
pub use crate::sys_prctl::PR_SET_KEEPCAPS;
pub use crate::sys_prctl::PR_GET_KEEPCAPS;
pub use crate::sys_prctl::PR_SET_SPECULATION_CTRL;
pub use crate::sys_prctl::PR_GET_SPECULATION_CTRL;

pub use crate::sys_prctl::SECCOMP_MODE_DISABLED;
pub use crate::sys_prctl::SECCOMP_MODE_STRICT;
pub use crate::sys_prctl::SECCOMP_MODE_FILTER;

// ---------------------------------------------------------------------------
// prctl constants from unistd
// ---------------------------------------------------------------------------

pub use crate::unistd::PR_SET_NAME;
pub use crate::unistd::PR_GET_NAME;
pub use crate::unistd::PR_SET_SECCOMP;
pub use crate::unistd::PR_GET_SECCOMP;
pub use crate::unistd::PR_SET_NO_NEW_PRIVS;
pub use crate::unistd::PR_GET_NO_NEW_PRIVS;

// ---------------------------------------------------------------------------
// Extended prctl constants from sys_prctl_caps
// ---------------------------------------------------------------------------

pub use crate::sys_prctl_caps::PR_SET_TAGGED_ADDR_CTRL;
pub use crate::sys_prctl_caps::PR_GET_TAGGED_ADDR_CTRL;
pub use crate::sys_prctl_caps::PR_SET_MDWE;
pub use crate::sys_prctl_caps::PR_GET_MDWE;
pub use crate::sys_prctl_caps::PR_SPEC_STORE_BYPASS;
pub use crate::sys_prctl_caps::PR_SPEC_INDIRECT_BRANCH;
pub use crate::sys_prctl_caps::PR_SPEC_NOT_AFFECTED;
pub use crate::sys_prctl_caps::PR_SPEC_PRCTL;
pub use crate::sys_prctl_caps::PR_SPEC_ENABLE;
pub use crate::sys_prctl_caps::PR_SPEC_DISABLE;
pub use crate::sys_prctl_caps::PR_SPEC_FORCE_DISABLE;

// ---------------------------------------------------------------------------
// Re-export the prctl function itself
// ---------------------------------------------------------------------------

pub use crate::unistd::prctl;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prctl_constants() {
        assert_ne!(PR_SET_NAME, PR_GET_NAME);
        assert_ne!(PR_SET_SECCOMP, PR_GET_SECCOMP);
        assert_ne!(PR_SET_DUMPABLE, PR_GET_DUMPABLE);
    }

    #[test]
    fn test_seccomp_modes() {
        assert_eq!(SECCOMP_MODE_DISABLED, 0);
        assert_eq!(SECCOMP_MODE_STRICT, 1);
        assert_eq!(SECCOMP_MODE_FILTER, 2);
    }

    #[test]
    fn test_spec_values() {
        assert_ne!(PR_SPEC_ENABLE, PR_SPEC_DISABLE);
        assert_ne!(PR_SPEC_PRCTL, PR_SPEC_NOT_AFFECTED);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(PR_SET_DUMPABLE, crate::sys_prctl::PR_SET_DUMPABLE);
        assert_eq!(PR_SPEC_ENABLE, crate::sys_prctl_caps::PR_SPEC_ENABLE);
        assert_eq!(PR_SET_NAME, crate::unistd::PR_SET_NAME);
    }
}

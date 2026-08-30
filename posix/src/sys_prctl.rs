//! `<sys/prctl.h>` — process control operations.
//!
//! Re-exports `prctl()` and its core constants from the `unistd`
//! module, and adds additional `PR_*` constants that programs
//! including `<sys/prctl.h>` expect.

// ---------------------------------------------------------------------------
// Re-exports from unistd
// ---------------------------------------------------------------------------

pub use crate::unistd::PR_GET_NAME;
pub use crate::unistd::PR_GET_NO_NEW_PRIVS;
pub use crate::unistd::PR_GET_SECCOMP;
pub use crate::unistd::PR_SET_NAME;
pub use crate::unistd::PR_SET_NO_NEW_PRIVS;
pub use crate::unistd::PR_SET_SECCOMP;
pub use crate::unistd::prctl;

// ---------------------------------------------------------------------------
// Additional PR_* constants
// ---------------------------------------------------------------------------

/// Set "dumpable" flag (core dumps allowed).
pub const PR_SET_DUMPABLE: i32 = 4;

/// Get "dumpable" flag.
pub const PR_GET_DUMPABLE: i32 = 3;

/// Set signal sent to child when parent dies.
pub const PR_SET_PDEATHSIG: i32 = 1;

/// Get signal sent to child when parent dies.
pub const PR_GET_PDEATHSIG: i32 = 2;

/// Set the "child subreaper" flag.
pub const PR_SET_CHILD_SUBREAPER: i32 = 36;

/// Get the "child subreaper" flag.
pub const PR_GET_CHILD_SUBREAPER: i32 = 37;

/// Set timer slack value.
pub const PR_SET_TIMERSLACK: i32 = 29;

/// Get timer slack value.
pub const PR_GET_TIMERSLACK: i32 = 30;

/// Set keep-capabilities flag across UID changes.
pub const PR_SET_KEEPCAPS: i32 = 8;

/// Get keep-capabilities flag.
pub const PR_GET_KEEPCAPS: i32 = 7;

/// Set endianness (PowerPC-specific, but defined everywhere).
pub const PR_SET_ENDIAN: i32 = 20;

/// Get endianness.
pub const PR_GET_ENDIAN: i32 = 19;

/// Set the process timing mode.
pub const PR_SET_TIMING: i32 = 14;

/// Get the process timing mode.
pub const PR_GET_TIMING: i32 = 13;

/// Set TSC access mode.
pub const PR_SET_TSC: i32 = 26;

/// Get TSC access mode.
pub const PR_GET_TSC: i32 = 25;

/// Set Speculation Control.
pub const PR_SET_SPECULATION_CTRL: i32 = 53;

/// Get Speculation Control.
pub const PR_GET_SPECULATION_CTRL: i32 = 52;

// ---------------------------------------------------------------------------
// Seccomp modes
// ---------------------------------------------------------------------------

/// Seccomp disabled.
pub const SECCOMP_MODE_DISABLED: i32 = 0;

/// Strict seccomp mode (only read/write/exit/_exit allowed).
pub const SECCOMP_MODE_STRICT: i32 = 1;

/// Filter-based seccomp (BPF program).
pub const SECCOMP_MODE_FILTER: i32 = 2;

// ---------------------------------------------------------------------------
// Endian constants
// ---------------------------------------------------------------------------

/// Big-endian mode.
pub const PR_ENDIAN_BIG: i32 = 0;

/// Little-endian mode.
pub const PR_ENDIAN_LITTLE: i32 = 1;

/// PowerPC pseudo-little-endian.
pub const PR_ENDIAN_PPC_LITTLE: i32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_get_pairs() {
        assert_ne!(PR_SET_NAME, PR_GET_NAME);
        assert_ne!(PR_SET_DUMPABLE, PR_GET_DUMPABLE);
        assert_ne!(PR_SET_PDEATHSIG, PR_GET_PDEATHSIG);
        assert_ne!(PR_SET_SECCOMP, PR_GET_SECCOMP);
        assert_ne!(PR_SET_NO_NEW_PRIVS, PR_GET_NO_NEW_PRIVS);
        assert_ne!(PR_SET_CHILD_SUBREAPER, PR_GET_CHILD_SUBREAPER);
        assert_ne!(PR_SET_TIMERSLACK, PR_GET_TIMERSLACK);
        assert_ne!(PR_SET_KEEPCAPS, PR_GET_KEEPCAPS);
    }

    #[test]
    fn test_constants_positive() {
        let consts = [
            PR_SET_NAME,
            PR_GET_NAME,
            PR_SET_DUMPABLE,
            PR_GET_DUMPABLE,
            PR_SET_PDEATHSIG,
            PR_GET_PDEATHSIG,
            PR_SET_SECCOMP,
            PR_GET_SECCOMP,
            PR_SET_NO_NEW_PRIVS,
            PR_GET_NO_NEW_PRIVS,
        ];
        for &c in &consts {
            assert!(c > 0, "PR_* constant should be positive");
        }
    }

    #[test]
    fn test_all_pr_constants_distinct() {
        let consts = [
            PR_SET_PDEATHSIG,
            PR_GET_PDEATHSIG,
            PR_SET_DUMPABLE,
            PR_GET_DUMPABLE,
            PR_SET_KEEPCAPS,
            PR_GET_KEEPCAPS,
            PR_SET_TIMING,
            PR_GET_TIMING,
            PR_SET_NAME,
            PR_GET_NAME,
            PR_SET_ENDIAN,
            PR_GET_ENDIAN,
            PR_GET_SECCOMP,
            PR_SET_SECCOMP,
            PR_GET_TSC,
            PR_SET_TSC,
            PR_SET_TIMERSLACK,
            PR_GET_TIMERSLACK,
            PR_SET_CHILD_SUBREAPER,
            PR_GET_CHILD_SUBREAPER,
            PR_SET_NO_NEW_PRIVS,
            PR_GET_NO_NEW_PRIVS,
            PR_SET_SPECULATION_CTRL,
            PR_GET_SPECULATION_CTRL,
        ];
        for i in 0..consts.len() {
            for j in (i + 1)..consts.len() {
                assert_ne!(consts[i], consts[j], "PR_* constants must be distinct");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Seccomp constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_seccomp_modes() {
        assert_eq!(SECCOMP_MODE_DISABLED, 0);
        assert_eq!(SECCOMP_MODE_STRICT, 1);
        assert_eq!(SECCOMP_MODE_FILTER, 2);
    }

    // -----------------------------------------------------------------------
    // Endian constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_endian_constants() {
        assert_eq!(PR_ENDIAN_BIG, 0);
        assert_eq!(PR_ENDIAN_LITTLE, 1);
        assert_ne!(PR_ENDIAN_BIG, PR_ENDIAN_LITTLE);
    }

    // -----------------------------------------------------------------------
    // prctl function
    // -----------------------------------------------------------------------

    #[test]
    fn test_prctl_set_name_succeeds() {
        let name = b"test_prctl\0";
        let ret = prctl(PR_SET_NAME, name.as_ptr() as u64, 0, 0, 0);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_prctl_get_name_succeeds() {
        let mut buf = [0xFFu8; 16];
        let ret = prctl(PR_GET_NAME, buf.as_mut_ptr() as u64, 0, 0, 0);
        assert_eq!(ret, 0);
        // Should have written at least a null terminator.
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn test_prctl_invalid_option() {
        let ret = prctl(9999, 0, 0, 0, 0);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // Cross-module
    // -----------------------------------------------------------------------

    #[test]
    fn test_cross_module_consistency() {
        assert_eq!(PR_SET_NAME, crate::unistd::PR_SET_NAME);
        assert_eq!(PR_GET_NAME, crate::unistd::PR_GET_NAME);
        assert_eq!(PR_SET_SECCOMP, crate::unistd::PR_SET_SECCOMP);
    }
}

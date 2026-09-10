//! `<linux/sysinfo.h>` — system information (kernel view).
//!
//! Re-exports the `Sysinfo` struct and `sysinfo()` function from
//! the `unistd` module, plus defines Linux-specific load average
//! scaling constants.

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use crate::unistd::Sysinfo;
pub use crate::unistd::sysinfo;

// ---------------------------------------------------------------------------
// Load average constants
// ---------------------------------------------------------------------------

/// Load average scaling factor (loads[] are fixed-point × this).
pub const SI_LOAD_SHIFT: u32 = 16;

/// Fixed-point 1.0 value for load averages.
pub const FIXED_1: u64 = 1 << SI_LOAD_SHIFT;

/// 1-minute load average index.
pub const LOAD_1MIN: usize = 0;
/// 5-minute load average index.
pub const LOAD_5MIN: usize = 1;
/// 15-minute load average index.
pub const LOAD_15MIN: usize = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 368, which is musl's — and the 112 this used to assert is the
    /// **kernel's**, which is a different structure with the same name.
    ///
    /// The kernel's `struct sysinfo` ends `char _f[20-2*sizeof(long)-sizeof(int)]`
    /// and comes to 112; musl's userspace one ends `char __reserved[256]` and
    /// comes to 368. This module is a re-export for `<linux/sysinfo.h>`
    /// compatibility, not a separate kernel-wire type, so there is one struct
    /// here and it is the one a C caller allocates.
    ///
    /// Which boundary governs was checked rather than assumed:
    /// [`crate::unistd::sysinfo`] fills the caller's structure field by field
    /// from `SYS_CLOCK_MONOTONIC` and `read_process_count`, and never hands it
    /// to the kernel. So the only boundary is with C, and musl's number wins.
    ///
    /// This is the third time in two days that a comment naming the kernel's
    /// structure sat above an assertion about the C library's — see
    /// `design-decisions.md` §1010 (`sigaction`) and §1011.
    #[test]
    fn test_sysinfo_size() {
        assert_eq!(core::mem::size_of::<Sysinfo>(), 368);
    }

    #[test]
    fn test_load_shift() {
        assert_eq!(SI_LOAD_SHIFT, 16);
        assert_eq!(FIXED_1, 65536);
    }

    #[test]
    fn test_load_indices() {
        assert_eq!(LOAD_1MIN, 0);
        assert_eq!(LOAD_5MIN, 1);
        assert_eq!(LOAD_15MIN, 2);
    }

    #[test]
    fn test_sysinfo_call() {
        // SAFETY: All-zero is valid for Sysinfo (all numeric fields).
        let mut info: Sysinfo = unsafe { core::mem::zeroed() };
        let ret = sysinfo(&mut info);
        // Should succeed (returns 0) or fail with stub (-1).
        assert!(ret == 0 || ret == -1);
    }

    #[test]
    fn test_cross_module() {
        let _ = core::mem::size_of::<crate::unistd::Sysinfo>();
    }
}

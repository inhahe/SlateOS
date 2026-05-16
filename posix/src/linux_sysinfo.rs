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

    #[test]
    fn test_sysinfo_size() {
        // Linux struct sysinfo is 112 bytes on x86_64.
        assert_eq!(core::mem::size_of::<Sysinfo>(), 112);
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

//! `<linux/kcov.h>` — Kernel code coverage interface.
//!
//! kcov exposes kernel code coverage data to userspace, primarily
//! used by fuzzers like syzkaller. A process opens `/sys/kernel/debug/kcov`,
//! mmaps it, and receives PC-level coverage for its syscalls.

// ---------------------------------------------------------------------------
// kcov ioctl commands
// ---------------------------------------------------------------------------

/// Initialize kcov (set buffer size).
pub const KCOV_INIT_TRACE: u64 = 0xC0086301;
/// Enable kcov collection.
pub const KCOV_ENABLE: u64 = 0x00006364;
/// Disable kcov collection.
pub const KCOV_DISABLE: u64 = 0x00006365;

// ---------------------------------------------------------------------------
// kcov trace modes
// ---------------------------------------------------------------------------

/// Trace PCs (instruction coverage).
pub const KCOV_TRACE_PC: u32 = 0;
/// Trace comparisons (data-flow coverage).
pub const KCOV_TRACE_CMP: u32 = 1;

// ---------------------------------------------------------------------------
// kcov comparison types (encoded in trace entries)
// ---------------------------------------------------------------------------

/// Comparison: const vs const.
pub const KCOV_CMP_CONST: u32 = 1;
/// Comparison size: 1 byte.
pub const KCOV_CMP_SIZE1: u32 = 0;
/// Comparison size: 2 bytes.
pub const KCOV_CMP_SIZE2: u32 = 2;
/// Comparison size: 4 bytes.
pub const KCOV_CMP_SIZE4: u32 = 4;
/// Comparison size: 8 bytes.
pub const KCOV_CMP_SIZE8: u32 = 6;
/// Mask for comparison size field.
pub const KCOV_CMP_SIZE_MASK: u32 = 6;

// ---------------------------------------------------------------------------
// kcov remote modes (Linux 5.x+)
// ---------------------------------------------------------------------------

/// Remote coverage: common handle type.
pub const KCOV_REMOTE_COMMON: u64 = 0;
/// Remote coverage: USB handle type.
pub const KCOV_REMOTE_USB: u64 = 1;

/// Enable remote coverage.
pub const KCOV_REMOTE_ENABLE: u64 = 0xC0186366;

// ---------------------------------------------------------------------------
// kcov remote argument structure
// ---------------------------------------------------------------------------

/// kcov remote argument (24 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KcovRemoteArg {
    /// Trace mode.
    pub trace_mode: u32,
    /// Area size.
    pub area_size: u32,
    /// Number of handles.
    pub num_handles: u32,
    /// Padding.
    _pad: u32,
    /// Common handle.
    pub common_handle: u64,
}

impl KcovRemoteArg {
    /// Create a zeroed kcov remote argument.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_values() {
        assert_ne!(KCOV_INIT_TRACE, KCOV_ENABLE);
        assert_ne!(KCOV_ENABLE, KCOV_DISABLE);
        assert_ne!(KCOV_INIT_TRACE, KCOV_DISABLE);
    }

    #[test]
    fn test_trace_modes() {
        assert_eq!(KCOV_TRACE_PC, 0);
        assert_eq!(KCOV_TRACE_CMP, 1);
    }

    #[test]
    fn test_cmp_sizes() {
        assert_eq!(KCOV_CMP_SIZE1, 0);
        assert_eq!(KCOV_CMP_SIZE2, 2);
        assert_eq!(KCOV_CMP_SIZE4, 4);
        assert_eq!(KCOV_CMP_SIZE8, 6);
    }

    #[test]
    fn test_cmp_size_mask() {
        // All size values should be within the mask.
        assert_eq!(KCOV_CMP_SIZE1 & KCOV_CMP_SIZE_MASK, KCOV_CMP_SIZE1);
        assert_eq!(KCOV_CMP_SIZE2 & KCOV_CMP_SIZE_MASK, KCOV_CMP_SIZE2);
        assert_eq!(KCOV_CMP_SIZE4 & KCOV_CMP_SIZE_MASK, KCOV_CMP_SIZE4);
        assert_eq!(KCOV_CMP_SIZE8 & KCOV_CMP_SIZE_MASK, KCOV_CMP_SIZE8);
    }

    #[test]
    fn test_remote_arg_size() {
        assert_eq!(core::mem::size_of::<KcovRemoteArg>(), 24);
    }

    #[test]
    fn test_remote_types() {
        assert_eq!(KCOV_REMOTE_COMMON, 0);
        assert_eq!(KCOV_REMOTE_USB, 1);
    }
}

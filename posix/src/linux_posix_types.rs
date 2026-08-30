//! `<linux/posix_types.h>` — Linux POSIX type definitions.
//!
//! Fundamental kernel types mirroring POSIX. These are the base types
//! used by syscall interfaces, filesystem, and process management.

// ---------------------------------------------------------------------------
// Fundamental types (x86_64)
// ---------------------------------------------------------------------------

/// Kernel long (signed pointer-width).
pub type KernelLong = i64;
/// Kernel unsigned long.
pub type KernelUlong = u64;
/// Process ID type.
pub type KernelPidT = i32;
/// User ID type.
pub type KernelUidT = u32;
/// Group ID type.
pub type KernelGidT = u32;
/// Inode number.
pub type KernelInoT = u64;
/// Device number.
pub type KernelDevT = u32;
/// Mode (permissions).
pub type KernelModeT = u32;
/// Link count.
pub type KernelNlinkT = u32;
/// File offset.
pub type KernelOffT = i64;
/// Large file offset.
pub type KernelLoffT = i64;
/// Size (signed).
pub type KernelSsizeT = i64;
/// Timer ID.
pub type KernelTimerT = i32;
/// Clock ID.
pub type KernelClockidT = i32;
/// File descriptor.
pub type KernelFdT = i32;

// ---------------------------------------------------------------------------
// FD set (for select())
// ---------------------------------------------------------------------------

/// Number of bits in fd_set.
pub const FD_SETSIZE: usize = 1024;

/// Kernel fd_set (for select()).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KernelFdSet {
    /// Bit array — each bit represents one file descriptor.
    pub fds_bits: [u64; FD_SETSIZE / 64],
}

impl KernelFdSet {
    /// Create a zeroed fd_set.
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
    fn test_type_sizes() {
        assert_eq!(core::mem::size_of::<KernelLong>(), 8);
        assert_eq!(core::mem::size_of::<KernelUlong>(), 8);
        assert_eq!(core::mem::size_of::<KernelPidT>(), 4);
        assert_eq!(core::mem::size_of::<KernelUidT>(), 4);
        assert_eq!(core::mem::size_of::<KernelGidT>(), 4);
        assert_eq!(core::mem::size_of::<KernelInoT>(), 8);
        assert_eq!(core::mem::size_of::<KernelDevT>(), 4);
        assert_eq!(core::mem::size_of::<KernelModeT>(), 4);
        assert_eq!(core::mem::size_of::<KernelOffT>(), 8);
        assert_eq!(core::mem::size_of::<KernelLoffT>(), 8);
        assert_eq!(core::mem::size_of::<KernelSsizeT>(), 8);
        assert_eq!(core::mem::size_of::<KernelFdT>(), 4);
    }

    #[test]
    fn test_fd_set_size() {
        assert_eq!(FD_SETSIZE, 1024);
        assert_eq!(core::mem::size_of::<KernelFdSet>(), 128);
    }

    #[test]
    fn test_fd_set_zeroed() {
        let fds = KernelFdSet::zeroed();
        for word in &fds.fds_bits {
            assert_eq!(*word, 0);
        }
    }

    #[test]
    fn test_fd_set_bits_count() {
        let fds = KernelFdSet::zeroed();
        assert_eq!(fds.fds_bits.len(), 16); // 1024 / 64
    }
}

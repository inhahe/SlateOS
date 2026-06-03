//! `<sys/select.h>` — synchronous I/O multiplexing.
//!
//! Re-exports `select()`, `pselect()`, `FdSet`, and related
//! definitions from the `poll` module.

pub use crate::poll::FD_SETSIZE;
pub use crate::poll::FdSet;
pub use crate::poll::Timeval;
pub use crate::poll::pselect;
pub use crate::poll::select;

/// Re-export `Timespec` for `pselect()`.
pub use crate::stat::Timespec;

// ---------------------------------------------------------------------------
// FD_SET / FD_CLR / FD_ISSET / FD_ZERO macros as functions
// ---------------------------------------------------------------------------

/// Add a file descriptor to the set.
///
/// # Panics
///
/// Panics if `fd` is negative or >= `FD_SETSIZE`.
pub fn fd_set(fd: i32, set: &mut FdSet) {
    assert!(fd >= 0 && (fd as usize) < FD_SETSIZE);
    let idx = fd as usize / 64;
    let bit = fd as usize % 64;
    // The assert above guarantees `fd < FD_SETSIZE`, so
    // `idx < FD_SETSIZE / 64`, which is exactly `fds_bits.len()`.
    #[allow(clippy::indexing_slicing)]
    {
        set.fds_bits[idx] |= 1u64 << bit;
    }
}

/// Remove a file descriptor from the set.
///
/// # Panics
///
/// Panics if `fd` is negative or >= `FD_SETSIZE`.
pub fn fd_clr(fd: i32, set: &mut FdSet) {
    assert!(fd >= 0 && (fd as usize) < FD_SETSIZE);
    let idx = fd as usize / 64;
    let bit = fd as usize % 64;
    // The assert above guarantees `idx < fds_bits.len()`.
    #[allow(clippy::indexing_slicing)]
    {
        set.fds_bits[idx] &= !(1u64 << bit);
    }
}

/// Test whether a file descriptor is in the set.
///
/// # Panics
///
/// Panics if `fd` is negative or >= `FD_SETSIZE`.
pub fn fd_isset(fd: i32, set: &FdSet) -> bool {
    assert!(fd >= 0 && (fd as usize) < FD_SETSIZE);
    let idx = fd as usize / 64;
    let bit = fd as usize % 64;
    // The assert above guarantees `idx < fds_bits.len()`.
    #[allow(clippy::indexing_slicing)]
    {
        (set.fds_bits[idx] & (1u64 << bit)) != 0
    }
}

/// Clear all file descriptors from the set.
pub fn fd_zero(set: &mut FdSet) {
    for slot in &mut set.fds_bits {
        *slot = 0;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fd_setsize() {
        assert_eq!(FD_SETSIZE, 256);
    }

    #[test]
    fn test_fdset_struct_size() {
        assert!(core::mem::size_of::<FdSet>() > 0);
    }

    #[test]
    fn test_fd_zero_clears() {
        let mut set = FdSet {
            fds_bits: [0xFFFF_FFFF_FFFF_FFFF; 4],
        };
        fd_zero(&mut set);
        for &slot in &set.fds_bits {
            assert_eq!(slot, 0);
        }
    }

    #[test]
    fn test_fd_set_isset() {
        let mut set = FdSet { fds_bits: [0; 4] };
        assert!(!fd_isset(5, &set));
        fd_set(5, &mut set);
        assert!(fd_isset(5, &set));
    }

    #[test]
    fn test_fd_clr() {
        let mut set = FdSet { fds_bits: [0; 4] };
        fd_set(10, &mut set);
        assert!(fd_isset(10, &set));
        fd_clr(10, &mut set);
        assert!(!fd_isset(10, &set));
    }

    #[test]
    fn test_fd_set_multiple() {
        let mut set = FdSet { fds_bits: [0; 4] };
        fd_set(0, &mut set);
        fd_set(63, &mut set);
        fd_set(64, &mut set);
        fd_set(255, &mut set);
        assert!(fd_isset(0, &set));
        assert!(fd_isset(63, &set));
        assert!(fd_isset(64, &set));
        assert!(fd_isset(255, &set));
        assert!(!fd_isset(1, &set));
        assert!(!fd_isset(100, &set));
    }

    #[test]
    fn test_fd_set_does_not_affect_others() {
        let mut set = FdSet { fds_bits: [0; 4] };
        fd_set(42, &mut set);
        for i in 0..FD_SETSIZE as i32 {
            if i == 42 {
                assert!(fd_isset(i, &set));
            } else {
                assert!(!fd_isset(i, &set), "fd {i} should not be set");
            }
        }
    }

    #[test]
    fn test_timeval_size() {
        assert!(core::mem::size_of::<Timeval>() > 0);
    }

    #[test]
    fn test_timespec_size() {
        assert!(core::mem::size_of::<Timespec>() > 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(FD_SETSIZE, crate::poll::FD_SETSIZE);
    }
}

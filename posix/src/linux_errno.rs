//! `<linux/errno.h>` + `<asm-generic/errno.h>` — Error number constants.
//!
//! Re-exports all errno constants and the `set_errno`/`get_errno`
//! functions from the core `errno` module. This provides the
//! standard `<linux/errno.h>` naming convention for code that
//! expects Linux-style includes.

// Re-export everything from the errno module.
pub use crate::errno::*;

// ---------------------------------------------------------------------------
// Additional errno constants that may not be in the base module
// ---------------------------------------------------------------------------

// The base errno module already defines all standard POSIX + Linux
// error codes (EPERM through EHWPOISON). This module exists purely
// as a naming convenience — `linux_errno::ENOENT` is the same as
// `errno::ENOENT`.

// ---------------------------------------------------------------------------
// Errno ranges
// ---------------------------------------------------------------------------

/// Maximum valid errno value (Linux kernel limit).
pub const MAX_ERRNO: i32 = 4095;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_errnos() {
        assert_eq!(EPERM, 1);
        assert_eq!(ENOENT, 2);
        assert_eq!(ESRCH, 3);
        assert_eq!(EINTR, 4);
        assert_eq!(EIO, 5);
    }

    #[test]
    fn test_common_errnos() {
        assert_eq!(ENOMEM, 12);
        assert_eq!(EACCES, 13);
        assert_eq!(EEXIST, 17);
        assert_eq!(EINVAL, 22);
        assert_eq!(ENOSYS, 38);
    }

    #[test]
    fn test_max_errno() {
        assert_eq!(MAX_ERRNO, 4095);
    }

    #[test]
    fn test_errnos_positive() {
        let errnos = [EPERM, ENOENT, EIO, ENOMEM, EACCES, EINVAL, ENOSYS];
        for e in &errnos {
            assert!(*e > 0);
            assert!(*e <= MAX_ERRNO);
        }
    }
}

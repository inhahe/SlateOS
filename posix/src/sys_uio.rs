//! `<sys/uio.h>` — definitions for vector I/O operations.
//!
//! Re-exports `Iovec`, `readv`, `writev`, and related scatter/gather
//! I/O functions from the `file` and `socket` modules.

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

pub use crate::file::Iovec;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

pub use crate::file::preadv;
pub use crate::file::preadv2;
pub use crate::file::pwritev;
pub use crate::file::pwritev2;
pub use crate::file::readv;
pub use crate::file::writev;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of iovec structures (matches `linux_limits::IOV_MAX`).
pub const UIO_MAXIOV: usize = 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iovec_struct_size() {
        // Iovec is { iov_base: *mut u8, iov_len: usize } — 2 pointers.
        assert_eq!(
            core::mem::size_of::<Iovec>(),
            2 * core::mem::size_of::<usize>()
        );
    }

    #[test]
    fn test_iovec_init() {
        let mut buf = [0u8; 64];
        let iov = Iovec {
            iov_base: buf.as_mut_ptr(),
            iov_len: buf.len(),
        };
        assert_eq!(iov.iov_len, 64);
        assert!(!iov.iov_base.is_null());
    }

    #[test]
    fn test_uio_maxiov() {
        assert_eq!(UIO_MAXIOV, 1024);
        assert_eq!(UIO_MAXIOV, crate::linux_limits::IOV_MAX);
    }

    #[test]
    fn test_readv_bad_fd() {
        let mut buf = [0u8; 16];
        let iov = Iovec {
            iov_base: buf.as_mut_ptr(),
            iov_len: buf.len(),
        };
        let ret = readv(-1, &iov, 1);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_writev_bad_fd() {
        let buf = [0u8; 16];
        let iov = Iovec {
            iov_base: buf.as_ptr() as *mut u8,
            iov_len: buf.len(),
        };
        let ret = writev(-1, &iov, 1);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(
            core::mem::size_of::<Iovec>(),
            core::mem::size_of::<crate::file::Iovec>()
        );
    }
}

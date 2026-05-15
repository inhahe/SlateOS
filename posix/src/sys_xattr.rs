//! `<sys/xattr.h>` — extended attribute operations.
//!
//! Re-exports extended attribute functions and flags from the
//! `xattr` module.

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

pub use crate::xattr::getxattr;
pub use crate::xattr::lgetxattr;
pub use crate::xattr::fgetxattr;
pub use crate::xattr::setxattr;
pub use crate::xattr::lsetxattr;
pub use crate::xattr::fsetxattr;
pub use crate::xattr::listxattr;
pub use crate::xattr::removexattr;

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

pub use crate::xattr::XATTR_CREATE;
pub use crate::xattr::XATTR_REPLACE;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xattr_flags() {
        assert_eq!(XATTR_CREATE, 1);
        assert_eq!(XATTR_REPLACE, 2);
    }

    #[test]
    fn test_xattr_flags_distinct() {
        assert_ne!(XATTR_CREATE, XATTR_REPLACE);
    }

    #[test]
    fn test_getxattr_stub() {
        let ret = getxattr(
            b"/tmp/test\0".as_ptr(),
            b"user.test\0".as_ptr(),
            core::ptr::null_mut(),
            0,
        );
        // Returns -1 (ENOTSUP stub).
        assert!(ret <= 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(XATTR_CREATE, crate::xattr::XATTR_CREATE);
        assert_eq!(XATTR_REPLACE, crate::xattr::XATTR_REPLACE);
    }
}

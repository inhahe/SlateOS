//! `<sys/xattr.h>` — extended attribute operations.
//!
//! Re-exports extended attribute functions and flags from the
//! `xattr` module.

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

pub use crate::xattr::fgetxattr;
pub use crate::xattr::flistxattr;
pub use crate::xattr::fremovexattr;
pub use crate::xattr::fsetxattr;
pub use crate::xattr::getxattr;
pub use crate::xattr::lgetxattr;
pub use crate::xattr::listxattr;
pub use crate::xattr::llistxattr;
pub use crate::xattr::lremovexattr;
pub use crate::xattr::lsetxattr;
pub use crate::xattr::removexattr;
pub use crate::xattr::setxattr;

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
        // Host build is validation-only: a well-formed call returns 0.
        // On bare metal this issues SYS_FS_GET_XATTR and returns the
        // attribute length or a negative errno.
        assert!(ret <= 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(XATTR_CREATE, crate::xattr::XATTR_CREATE);
        assert_eq!(XATTR_REPLACE, crate::xattr::XATTR_REPLACE);
    }
}

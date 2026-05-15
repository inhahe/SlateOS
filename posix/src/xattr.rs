//! Extended file attribute stubs.
//!
//! Implements `getxattr`, `lgetxattr`, `fgetxattr`, `setxattr`,
//! `lsetxattr`, `fsetxattr`, `listxattr`, `llistxattr`, `flistxattr`,
//! `removexattr`, `lremovexattr`, `fremovexattr`.
//!
//! Our filesystem does not support extended attributes yet.  All
//! functions return ENOTSUP so programs that probe for xattr support
//! get a clean response instead of a linker error.

use crate::errno;
use crate::types::SsizeT;

// ---------------------------------------------------------------------------
// setxattr flags (match Linux)
// ---------------------------------------------------------------------------

/// Create the attribute; fail if it already exists.
pub const XATTR_CREATE: i32 = 1;
/// Replace the attribute; fail if it doesn't exist.
pub const XATTR_REPLACE: i32 = 2;

// ---------------------------------------------------------------------------
// getxattr / lgetxattr / fgetxattr
// ---------------------------------------------------------------------------

/// Get an extended attribute value.
///
/// Stub: returns -1 with ENOTSUP.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getxattr(
    _path: *const u8,
    _name: *const u8,
    _value: *mut u8,
    _size: usize,
) -> SsizeT {
    errno::set_errno(errno::ENOTSUP);
    -1
}

/// Get an extended attribute value (don't follow symlinks).
///
/// Stub: returns -1 with ENOTSUP.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lgetxattr(
    _path: *const u8,
    _name: *const u8,
    _value: *mut u8,
    _size: usize,
) -> SsizeT {
    errno::set_errno(errno::ENOTSUP);
    -1
}

/// Get an extended attribute value by file descriptor.
///
/// Stub: returns -1 with ENOTSUP.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fgetxattr(
    _fd: i32,
    _name: *const u8,
    _value: *mut u8,
    _size: usize,
) -> SsizeT {
    errno::set_errno(errno::ENOTSUP);
    -1
}

// ---------------------------------------------------------------------------
// setxattr / lsetxattr / fsetxattr
// ---------------------------------------------------------------------------

/// Set an extended attribute value.
///
/// Stub: returns -1 with ENOTSUP.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setxattr(
    _path: *const u8,
    _name: *const u8,
    _value: *const u8,
    _size: usize,
    _flags: i32,
) -> i32 {
    errno::set_errno(errno::ENOTSUP);
    -1
}

/// Set an extended attribute value (don't follow symlinks).
///
/// Stub: returns -1 with ENOTSUP.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lsetxattr(
    _path: *const u8,
    _name: *const u8,
    _value: *const u8,
    _size: usize,
    _flags: i32,
) -> i32 {
    errno::set_errno(errno::ENOTSUP);
    -1
}

/// Set an extended attribute value by file descriptor.
///
/// Stub: returns -1 with ENOTSUP.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fsetxattr(
    _fd: i32,
    _name: *const u8,
    _value: *const u8,
    _size: usize,
    _flags: i32,
) -> i32 {
    errno::set_errno(errno::ENOTSUP);
    -1
}

// ---------------------------------------------------------------------------
// listxattr / llistxattr / flistxattr
// ---------------------------------------------------------------------------

/// List extended attribute names.
///
/// Stub: returns -1 with ENOTSUP.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn listxattr(
    _path: *const u8,
    _list: *mut u8,
    _size: usize,
) -> SsizeT {
    errno::set_errno(errno::ENOTSUP);
    -1
}

/// List extended attribute names (don't follow symlinks).
///
/// Stub: returns -1 with ENOTSUP.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn llistxattr(
    _path: *const u8,
    _list: *mut u8,
    _size: usize,
) -> SsizeT {
    errno::set_errno(errno::ENOTSUP);
    -1
}

/// List extended attribute names by file descriptor.
///
/// Stub: returns -1 with ENOTSUP.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn flistxattr(
    _fd: i32,
    _list: *mut u8,
    _size: usize,
) -> SsizeT {
    errno::set_errno(errno::ENOTSUP);
    -1
}

// ---------------------------------------------------------------------------
// removexattr / lremovexattr / fremovexattr
// ---------------------------------------------------------------------------

/// Remove an extended attribute.
///
/// Stub: returns -1 with ENOTSUP.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn removexattr(
    _path: *const u8,
    _name: *const u8,
) -> i32 {
    errno::set_errno(errno::ENOTSUP);
    -1
}

/// Remove an extended attribute (don't follow symlinks).
///
/// Stub: returns -1 with ENOTSUP.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lremovexattr(
    _path: *const u8,
    _name: *const u8,
) -> i32 {
    errno::set_errno(errno::ENOTSUP);
    -1
}

/// Remove an extended attribute by file descriptor.
///
/// Stub: returns -1 with ENOTSUP.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fremovexattr(
    _fd: i32,
    _name: *const u8,
) -> i32 {
    errno::set_errno(errno::ENOTSUP);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Constants --

    #[test]
    fn test_xattr_flag_values() {
        assert_eq!(XATTR_CREATE, 1);
        assert_eq!(XATTR_REPLACE, 2);
    }

    #[test]
    fn test_xattr_flags_distinct() {
        assert_ne!(XATTR_CREATE, XATTR_REPLACE);
        assert_eq!(XATTR_CREATE & XATTR_REPLACE, 0);
    }

    // -- getxattr family returns ENOTSUP --

    #[test]
    fn test_getxattr_enotsup() {
        errno::set_errno(0);
        let result = getxattr(
            b"/tmp/test\0".as_ptr(),
            b"user.test\0".as_ptr(),
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_lgetxattr_enotsup() {
        errno::set_errno(0);
        let result = lgetxattr(
            b"/tmp/test\0".as_ptr(),
            b"user.test\0".as_ptr(),
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_fgetxattr_enotsup() {
        errno::set_errno(0);
        let result = fgetxattr(3, b"user.test\0".as_ptr(), core::ptr::null_mut(), 0);
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    // -- setxattr family returns ENOTSUP --

    #[test]
    fn test_setxattr_enotsup() {
        errno::set_errno(0);
        let result = setxattr(
            b"/tmp/test\0".as_ptr(),
            b"user.test\0".as_ptr(),
            b"value\0".as_ptr(),
            5,
            0,
        );
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_setxattr_create_flag_enotsup() {
        errno::set_errno(0);
        let result = setxattr(
            b"/tmp/test\0".as_ptr(),
            b"user.test\0".as_ptr(),
            b"value\0".as_ptr(),
            5,
            XATTR_CREATE,
        );
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_lsetxattr_enotsup() {
        errno::set_errno(0);
        let result = lsetxattr(
            b"/tmp/test\0".as_ptr(),
            b"user.test\0".as_ptr(),
            b"value\0".as_ptr(),
            5,
            0,
        );
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_fsetxattr_enotsup() {
        errno::set_errno(0);
        let result = fsetxattr(3, b"user.test\0".as_ptr(), b"value\0".as_ptr(), 5, 0);
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    // -- listxattr family returns ENOTSUP --

    #[test]
    fn test_listxattr_enotsup() {
        errno::set_errno(0);
        let result = listxattr(b"/tmp/test\0".as_ptr(), core::ptr::null_mut(), 0);
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_llistxattr_enotsup() {
        errno::set_errno(0);
        let result = llistxattr(b"/tmp/test\0".as_ptr(), core::ptr::null_mut(), 0);
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_flistxattr_enotsup() {
        errno::set_errno(0);
        let result = flistxattr(3, core::ptr::null_mut(), 0);
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    // -- removexattr family returns ENOTSUP --

    #[test]
    fn test_removexattr_enotsup() {
        errno::set_errno(0);
        let result = removexattr(b"/tmp/test\0".as_ptr(), b"user.test\0".as_ptr());
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_lremovexattr_enotsup() {
        errno::set_errno(0);
        let result = lremovexattr(b"/tmp/test\0".as_ptr(), b"user.test\0".as_ptr());
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_fremovexattr_enotsup() {
        errno::set_errno(0);
        let result = fremovexattr(3, b"user.test\0".as_ptr());
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    // -- Null pointer handling (should still return ENOTSUP, not crash) --

    #[test]
    fn test_getxattr_null_path() {
        errno::set_errno(0);
        let result = getxattr(
            core::ptr::null(),
            b"user.test\0".as_ptr(),
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_setxattr_null_path() {
        errno::set_errno(0);
        let result = setxattr(
            core::ptr::null(),
            b"user.test\0".as_ptr(),
            core::ptr::null(),
            0,
            0,
        );
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_listxattr_null_path() {
        errno::set_errno(0);
        let result = listxattr(core::ptr::null(), core::ptr::null_mut(), 0);
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_removexattr_null_path() {
        errno::set_errno(0);
        let result = removexattr(core::ptr::null(), core::ptr::null());
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    // -- getxattr with buffer --

    #[test]
    fn test_getxattr_with_buffer_enotsup() {
        errno::set_errno(0);
        let mut buf = [0u8; 256];
        let result = getxattr(
            b"/tmp/test\0".as_ptr(),
            b"user.test\0".as_ptr(),
            buf.as_mut_ptr(),
            buf.len(),
        );
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    #[test]
    fn test_listxattr_with_buffer_enotsup() {
        errno::set_errno(0);
        let mut buf = [0u8; 256];
        let result = listxattr(
            b"/tmp/test\0".as_ptr(),
            buf.as_mut_ptr(),
            buf.len(),
        );
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    // -- setxattr with XATTR_REPLACE flag --

    #[test]
    fn test_setxattr_replace_flag_enotsup() {
        errno::set_errno(0);
        let result = setxattr(
            b"/tmp/test\0".as_ptr(),
            b"user.test\0".as_ptr(),
            b"value\0".as_ptr(),
            5,
            XATTR_REPLACE,
        );
        assert_eq!(result, -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }

    // -- fd-based variants with negative/zero fds --

    #[test]
    fn test_fgetxattr_fd_zero() {
        assert_eq!(fgetxattr(0, b"user.test\0".as_ptr(), core::ptr::null_mut(), 0), -1);
    }

    #[test]
    fn test_fsetxattr_fd_zero() {
        assert_eq!(fsetxattr(0, b"user.test\0".as_ptr(), b"v\0".as_ptr(), 1, 0), -1);
    }

    #[test]
    fn test_flistxattr_fd_zero() {
        assert_eq!(flistxattr(0, core::ptr::null_mut(), 0), -1);
    }

    #[test]
    fn test_fremovexattr_fd_zero() {
        assert_eq!(fremovexattr(0, b"user.test\0".as_ptr()), -1);
    }

    #[test]
    fn test_fgetxattr_negative_fd() {
        assert_eq!(fgetxattr(-1, b"user.test\0".as_ptr(), core::ptr::null_mut(), 0), -1);
    }

    #[test]
    fn test_fsetxattr_negative_fd() {
        assert_eq!(fsetxattr(-1, b"user.test\0".as_ptr(), b"v\0".as_ptr(), 1, 0), -1);
    }

    #[test]
    fn test_flistxattr_negative_fd() {
        assert_eq!(flistxattr(-1, core::ptr::null_mut(), 0), -1);
    }

    #[test]
    fn test_fremovexattr_negative_fd() {
        assert_eq!(fremovexattr(-1, b"user.test\0".as_ptr()), -1);
    }

    // -- lgetxattr / lsetxattr / llistxattr / lremovexattr null args --

    #[test]
    fn test_lgetxattr_null_name() {
        assert_eq!(lgetxattr(b"/tmp\0".as_ptr(), core::ptr::null(), core::ptr::null_mut(), 0), -1);
    }

    #[test]
    fn test_lsetxattr_null_name() {
        assert_eq!(lsetxattr(b"/tmp\0".as_ptr(), core::ptr::null(), core::ptr::null(), 0, 0), -1);
    }

    #[test]
    fn test_lremovexattr_null_name() {
        assert_eq!(lremovexattr(b"/tmp\0".as_ptr(), core::ptr::null()), -1);
    }

    #[test]
    fn test_llistxattr_null_path_enotsup() {
        errno::set_errno(0);
        assert_eq!(llistxattr(core::ptr::null(), core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOTSUP);
    }
}

//! Extended file attributes (`getxattr`/`setxattr`/`listxattr`/`removexattr`
//! and their `l*` / `f*` variants).
//!
//! These wrap the kernel xattr syscalls (`SYS_FS_GET_XATTR`,
//! `SYS_FS_SET_XATTR`, `SYS_FS_REMOVE_XATTR`, `SYS_FS_LIST_XATTRS`), which
//! ext4 implements via inline + external xattr blocks.  Each entry point
//! validates its arguments (NULL path/name → EFAULT, bad/closed fd → EBADF,
//! conflicting setxattr flags → EINVAL) and then, on bare metal, issues the
//! corresponding syscall.  On the host build (no kernel) the syscall is
//! skipped and the call returns a validation-only result.
//!
//! The `l*` variants correctly operate on the symlink inode itself (they set
//! the NO_FOLLOW flag bit on the kernel xattr syscall, which resolves the
//! final path component without following it — memfs/ext4 back this via their
//! `*_no_follow` VFS methods).
//!
//! LIMITATIONS (tracked in todo.txt):
//!   * The kernel collapses "file not found" and "attribute not found" into
//!     one error, so a missing attribute reports ENOENT rather than the
//!     Linux-conventional ENODATA on `getxattr`/`removexattr`.

use crate::errno;
use crate::types::SsizeT;

// ---------------------------------------------------------------------------
// setxattr flags (match Linux)
// ---------------------------------------------------------------------------

/// Create the attribute; fail if it already exists.
pub const XATTR_CREATE: i32 = 1;
/// Replace the attribute; fail if it doesn't exist.
pub const XATTR_REPLACE: i32 = 2;

/// Validate the `flags` argument to the `set*xattr` family.
///
/// Returns `false` (and sets `errno = EINVAL`) when the flags are invalid:
/// any bit outside `XATTR_CREATE | XATTR_REPLACE`, or both flags set at
/// once (Linux rejects the contradictory "create and replace" request).
fn setxattr_flags_valid(flags: i32) -> bool {
    if flags & !(XATTR_CREATE | XATTR_REPLACE) != 0 {
        errno::set_errno(errno::EINVAL);
        return false;
    }
    if (flags & XATTR_CREATE != 0) && (flags & XATTR_REPLACE != 0) {
        errno::set_errno(errno::EINVAL);
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Bare-metal syscall workers
// ---------------------------------------------------------------------------

/// Issue `SYS_FS_GET_XATTR` for an already-resolved path.
///
/// Returns the attribute length on success (after an ERANGE check when the
/// caller provided a non-zero, too-small buffer) or -1 with `errno` set.
#[cfg(target_os = "none")]
fn do_getxattr(
    path_ptr: *const u8,
    path_len: usize,
    name: *const u8,
    value: *mut u8,
    size: usize,
    no_follow: bool,
) -> SsizeT {
    // arg5 bit 0 = NO_FOLLOW (lgetxattr → read the link inode's own xattrs).
    let ret = crate::syscall::syscall6(
        crate::syscall::SYS_FS_GET_XATTR,
        path_ptr as u64,
        path_len as u64,
        name as u64,
        value as u64,
        size as u64,
        u64::from(no_follow),
    );
    if ret < 0 {
        return errno::translate(ret) as SsizeT;
    }
    // ret is the TRUE attribute length.  A non-zero buffer that is too small
    // is ERANGE (the kernel copied only what fit).
    let true_len = ret as usize;
    if size != 0 && true_len > size {
        errno::set_errno(errno::ERANGE);
        return -1;
    }
    ret as SsizeT
}

/// Issue `SYS_FS_LIST_XATTRS` for an already-resolved path.
#[cfg(target_os = "none")]
fn do_listxattr(
    path_ptr: *const u8,
    path_len: usize,
    list: *mut u8,
    size: usize,
    no_follow: bool,
) -> SsizeT {
    // arg4 bit 0 = NO_FOLLOW (llistxattr → list the link inode's own xattrs).
    let ret = crate::syscall::syscall5(
        crate::syscall::SYS_FS_LIST_XATTRS,
        path_ptr as u64,
        path_len as u64,
        list as u64,
        size as u64,
        u64::from(no_follow),
    );
    if ret < 0 {
        return errno::translate(ret) as SsizeT;
    }
    let total = ret as usize;
    if size != 0 && total > size {
        errno::set_errno(errno::ERANGE);
        return -1;
    }
    ret as SsizeT
}

/// Issue `SYS_FS_SET_XATTR` for an already-resolved path, honouring the
/// `XATTR_CREATE` / `XATTR_REPLACE` flags via a pre-existence check (the
/// kernel syscall carries no flags).
#[cfg(target_os = "none")]
fn do_setxattr(
    path_ptr: *const u8,
    path_len: usize,
    name: *const u8,
    value: *const u8,
    size: usize,
    flags: i32,
    no_follow: bool,
) -> i32 {
    if flags & (XATTR_CREATE | XATTR_REPLACE) != 0 {
        // Probe for existence with a size query (val_cap = 0).  The probe must
        // use the same follow mode as the set so CREATE/REPLACE reason about
        // the same inode (the link itself for lsetxattr).
        let exists = crate::syscall::syscall6(
            crate::syscall::SYS_FS_GET_XATTR,
            path_ptr as u64,
            path_len as u64,
            name as u64,
            0,
            0,
            u64::from(no_follow),
        ) >= 0;
        if (flags & XATTR_CREATE != 0) && exists {
            errno::set_errno(errno::EEXIST);
            return -1;
        }
        if (flags & XATTR_REPLACE != 0) && !exists {
            errno::set_errno(errno::ENODATA);
            return -1;
        }
    }
    // arg5 bit 0 = NO_FOLLOW (lsetxattr → write the link inode's own xattrs).
    let ret = crate::syscall::syscall6(
        crate::syscall::SYS_FS_SET_XATTR,
        path_ptr as u64,
        path_len as u64,
        name as u64,
        value as u64,
        size as u64,
        u64::from(no_follow),
    );
    if ret < 0 {
        return errno::translate(ret) as i32;
    }
    0
}

/// Issue `SYS_FS_REMOVE_XATTR` for an already-resolved path.
#[cfg(target_os = "none")]
fn do_removexattr(path_ptr: *const u8, path_len: usize, name: *const u8, no_follow: bool) -> i32 {
    // arg3 bit 0 = NO_FOLLOW (lremovexattr → remove from the link inode).
    let ret = crate::syscall::syscall4(
        crate::syscall::SYS_FS_REMOVE_XATTR,
        path_ptr as u64,
        path_len as u64,
        name as u64,
        u64::from(no_follow),
    );
    if ret < 0 {
        return errno::translate(ret) as i32;
    }
    0
}

/// Resolve an open fd to its stored path, or set `errno` and return `None`.
///
/// A path-less descriptor (pipe, socket, …) has no backing file and thus no
/// extended attributes, so we report `ENOTSUP` — matching how Linux reports
/// xattr operations on filesystems/objects without xattr support.
#[cfg(target_os = "none")]
fn fd_to_path(fd: i32, buf: &mut [u8; crate::unistd::PATH_MAX]) -> Option<usize> {
    let len = crate::fdtable::get_fd_path(fd, buf);
    if len == 0 {
        errno::set_errno(errno::ENOTSUP);
        return None;
    }
    Some(len)
}

// ---------------------------------------------------------------------------
// getxattr / lgetxattr / fgetxattr
// ---------------------------------------------------------------------------

/// Get an extended attribute value.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getxattr(
    path: *const u8,
    name: *const u8,
    value: *mut u8,
    size: usize,
) -> SsizeT {
    if path.is_null() || name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        let Some(len) = crate::file::resolve_or_err(path, &mut buf) else {
            return -1;
        };
        do_getxattr(buf.as_ptr(), len, name, value, size, false)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (value, size);
        0
    }
}

/// Get an extended attribute value WITHOUT following a trailing symlink:
/// reads the link inode's own xattrs (`lgetxattr`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lgetxattr(
    path: *const u8,
    name: *const u8,
    value: *mut u8,
    size: usize,
) -> SsizeT {
    if path.is_null() || name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        let Some(len) = crate::file::resolve_or_err(path, &mut buf) else {
            return -1;
        };
        do_getxattr(buf.as_ptr(), len, name, value, size, true)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (value, size);
        0
    }
}

/// Get an extended attribute value by file descriptor.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fgetxattr(fd: i32, name: *const u8, value: *mut u8, size: usize) -> SsizeT {
    if fd < 0 || crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        let Some(len) = fd_to_path(fd, &mut buf) else {
            return -1;
        };
        do_getxattr(buf.as_ptr(), len, name, value, size, false)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (value, size);
        0
    }
}

// ---------------------------------------------------------------------------
// setxattr / lsetxattr / fsetxattr
// ---------------------------------------------------------------------------

/// Set an extended attribute value.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setxattr(
    path: *const u8,
    name: *const u8,
    value: *const u8,
    size: usize,
    flags: i32,
) -> i32 {
    if path.is_null() || name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if !setxattr_flags_valid(flags) {
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        let Some(len) = crate::file::resolve_or_err(path, &mut buf) else {
            return -1;
        };
        do_setxattr(buf.as_ptr(), len, name, value, size, flags, false)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (value, size);
        0
    }
}

/// Set an extended attribute value WITHOUT following a trailing symlink:
/// writes the link inode's own xattrs (`lsetxattr`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lsetxattr(
    path: *const u8,
    name: *const u8,
    value: *const u8,
    size: usize,
    flags: i32,
) -> i32 {
    if path.is_null() || name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if !setxattr_flags_valid(flags) {
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        let Some(len) = crate::file::resolve_or_err(path, &mut buf) else {
            return -1;
        };
        do_setxattr(buf.as_ptr(), len, name, value, size, flags, true)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (value, size);
        0
    }
}

/// Set an extended attribute value by file descriptor.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fsetxattr(
    fd: i32,
    name: *const u8,
    value: *const u8,
    size: usize,
    flags: i32,
) -> i32 {
    if fd < 0 || crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if !setxattr_flags_valid(flags) {
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        let Some(len) = fd_to_path(fd, &mut buf) else {
            return -1;
        };
        do_setxattr(buf.as_ptr(), len, name, value, size, flags, false)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (value, size);
        0
    }
}

// ---------------------------------------------------------------------------
// listxattr / llistxattr / flistxattr
// ---------------------------------------------------------------------------

/// List extended attribute names.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn listxattr(path: *const u8, list: *mut u8, size: usize) -> SsizeT {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        let Some(len) = crate::file::resolve_or_err(path, &mut buf) else {
            return -1;
        };
        do_listxattr(buf.as_ptr(), len, list, size, false)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (list, size);
        0
    }
}

/// List extended attribute names WITHOUT following a trailing symlink:
/// lists the link inode's own xattrs (`llistxattr`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn llistxattr(path: *const u8, list: *mut u8, size: usize) -> SsizeT {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        let Some(len) = crate::file::resolve_or_err(path, &mut buf) else {
            return -1;
        };
        do_listxattr(buf.as_ptr(), len, list, size, true)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (list, size);
        0
    }
}

/// List extended attribute names by file descriptor.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn flistxattr(fd: i32, list: *mut u8, size: usize) -> SsizeT {
    if fd < 0 || crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        let Some(len) = fd_to_path(fd, &mut buf) else {
            return -1;
        };
        do_listxattr(buf.as_ptr(), len, list, size, false)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (list, size);
        0
    }
}

// ---------------------------------------------------------------------------
// removexattr / lremovexattr / fremovexattr
// ---------------------------------------------------------------------------

/// Remove an extended attribute.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn removexattr(path: *const u8, name: *const u8) -> i32 {
    if path.is_null() || name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        let Some(len) = crate::file::resolve_or_err(path, &mut buf) else {
            return -1;
        };
        do_removexattr(buf.as_ptr(), len, name, false)
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
}

/// Remove an extended attribute WITHOUT following a trailing symlink:
/// removes from the link inode's own xattrs (`lremovexattr`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lremovexattr(path: *const u8, name: *const u8) -> i32 {
    if path.is_null() || name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        let Some(len) = crate::file::resolve_or_err(path, &mut buf) else {
            return -1;
        };
        do_removexattr(buf.as_ptr(), len, name, true)
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
}

/// Remove an extended attribute by file descriptor.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fremovexattr(fd: i32, name: *const u8) -> i32 {
    if fd < 0 || crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        let Some(len) = fd_to_path(fd, &mut buf) else {
            return -1;
        };
        do_removexattr(buf.as_ptr(), len, name, false)
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// These run on the host build, where the kernel syscalls are not issued.
// They exercise the argument-validation surface (NULL path/name → EFAULT,
// bad/closed fd → EBADF, conflicting setxattr flags → EINVAL) and confirm
// that well-formed calls return the validation-only success value.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fdtable::{self, HandleKind};

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

    #[test]
    fn test_setxattr_flags_valid() {
        assert!(setxattr_flags_valid(0));
        assert!(setxattr_flags_valid(XATTR_CREATE));
        assert!(setxattr_flags_valid(XATTR_REPLACE));
        // Both at once is contradictory → EINVAL.
        assert!(!setxattr_flags_valid(XATTR_CREATE | XATTR_REPLACE));
        // Unknown bit → EINVAL.
        assert!(!setxattr_flags_valid(0x100));
    }

    // -- NULL path/name → EFAULT --

    #[test]
    fn test_getxattr_null_path_efault() {
        errno::set_errno(0);
        assert_eq!(
            getxattr(
                core::ptr::null(),
                b"user.test\0".as_ptr(),
                core::ptr::null_mut(),
                0
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_getxattr_null_name_efault() {
        errno::set_errno(0);
        assert_eq!(
            getxattr(
                b"/tmp/test\0".as_ptr(),
                core::ptr::null(),
                core::ptr::null_mut(),
                0
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_setxattr_null_path_efault() {
        errno::set_errno(0);
        assert_eq!(
            setxattr(
                core::ptr::null(),
                b"user.test\0".as_ptr(),
                core::ptr::null(),
                0,
                0
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_listxattr_null_path_efault() {
        errno::set_errno(0);
        assert_eq!(listxattr(core::ptr::null(), core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_removexattr_null_path_efault() {
        errno::set_errno(0);
        assert_eq!(removexattr(core::ptr::null(), b"user.test\0".as_ptr()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_removexattr_null_name_efault() {
        errno::set_errno(0);
        assert_eq!(removexattr(b"/tmp/test\0".as_ptr(), core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_lgetxattr_null_name_efault() {
        errno::set_errno(0);
        assert_eq!(
            lgetxattr(
                b"/tmp\0".as_ptr(),
                core::ptr::null(),
                core::ptr::null_mut(),
                0
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_lsetxattr_null_name_efault() {
        errno::set_errno(0);
        assert_eq!(
            lsetxattr(
                b"/tmp\0".as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                0,
                0
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_lremovexattr_null_name_efault() {
        errno::set_errno(0);
        assert_eq!(lremovexattr(b"/tmp\0".as_ptr(), core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- Conflicting / invalid setxattr flags → EINVAL --

    #[test]
    fn test_setxattr_both_flags_einval() {
        errno::set_errno(0);
        assert_eq!(
            setxattr(
                b"/tmp/test\0".as_ptr(),
                b"user.test\0".as_ptr(),
                b"value\0".as_ptr(),
                5,
                XATTR_CREATE | XATTR_REPLACE,
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setxattr_unknown_flag_einval() {
        errno::set_errno(0);
        assert_eq!(
            setxattr(
                b"/tmp/test\0".as_ptr(),
                b"user.test\0".as_ptr(),
                b"v\0".as_ptr(),
                1,
                0x40
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- fd variants: bad fd → EBADF --

    #[test]
    fn test_fgetxattr_negative_fd_ebadf() {
        errno::set_errno(0);
        assert_eq!(
            fgetxattr(-1, b"user.test\0".as_ptr(), core::ptr::null_mut(), 0),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fsetxattr_negative_fd_ebadf() {
        errno::set_errno(0);
        assert_eq!(
            fsetxattr(-1, b"user.test\0".as_ptr(), b"v\0".as_ptr(), 1, 0),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_flistxattr_negative_fd_ebadf() {
        errno::set_errno(0);
        assert_eq!(flistxattr(-1, core::ptr::null_mut(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fremovexattr_negative_fd_ebadf() {
        errno::set_errno(0);
        assert_eq!(fremovexattr(-1, b"user.test\0".as_ptr()), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fgetxattr_unopen_fd_ebadf() {
        let probe: i32 = 0x4000_0010;
        if fdtable::get_fd(probe).is_some() {
            let _ = fdtable::close_fd(probe);
        }
        errno::set_errno(0);
        assert_eq!(
            fgetxattr(probe, b"user.test\0".as_ptr(), core::ptr::null_mut(), 0),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fsetxattr_bad_fd_beats_flag_check() {
        // EBADF is reported before the flag validation, matching the order
        // Linux uses (the fd is checked first).
        errno::set_errno(0);
        assert_eq!(
            fsetxattr(
                -1,
                b"user.test\0".as_ptr(),
                b"v\0".as_ptr(),
                1,
                XATTR_CREATE | XATTR_REPLACE
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- fd variants: NULL name on a valid fd → EFAULT --

    #[test]
    fn test_fgetxattr_open_fd_null_name_efault() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        errno::set_errno(0);
        assert_eq!(
            fgetxattr(fd, core::ptr::null(), core::ptr::null_mut(), 0),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
        let _ = fdtable::close_fd(fd);
    }

    // -- Host validation-only success path --

    #[test]
    fn test_getxattr_valid_returns_zero_on_host() {
        // On the host build the syscall is skipped; a well-formed call
        // returns 0 (zero-length result) after validation.
        let mut buf = [0u8; 64];
        assert_eq!(
            getxattr(
                b"/etc/passwd\0".as_ptr(),
                b"user.test\0".as_ptr(),
                buf.as_mut_ptr(),
                buf.len()
            ),
            0
        );
    }

    #[test]
    fn test_setxattr_valid_returns_zero_on_host() {
        assert_eq!(
            setxattr(
                b"/etc/passwd\0".as_ptr(),
                b"user.test\0".as_ptr(),
                b"value\0".as_ptr(),
                5,
                0
            ),
            0
        );
    }

    #[test]
    fn test_setxattr_create_flag_valid_on_host() {
        assert_eq!(
            setxattr(
                b"/etc/passwd\0".as_ptr(),
                b"user.test\0".as_ptr(),
                b"v\0".as_ptr(),
                1,
                XATTR_CREATE
            ),
            0
        );
    }

    #[test]
    fn test_setxattr_replace_flag_valid_on_host() {
        assert_eq!(
            setxattr(
                b"/etc/passwd\0".as_ptr(),
                b"user.test\0".as_ptr(),
                b"v\0".as_ptr(),
                1,
                XATTR_REPLACE
            ),
            0
        );
    }

    #[test]
    fn test_listxattr_valid_returns_zero_on_host() {
        let mut buf = [0u8; 64];
        assert_eq!(
            listxattr(b"/etc/passwd\0".as_ptr(), buf.as_mut_ptr(), buf.len()),
            0
        );
    }

    #[test]
    fn test_removexattr_valid_returns_zero_on_host() {
        assert_eq!(
            removexattr(b"/etc/passwd\0".as_ptr(), b"user.test\0".as_ptr()),
            0
        );
    }

    #[test]
    fn test_fgetxattr_open_fd_returns_zero_on_host() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(
            fgetxattr(fd, b"user.test\0".as_ptr(), core::ptr::null_mut(), 0),
            0
        );
        let _ = fdtable::close_fd(fd);
    }
}

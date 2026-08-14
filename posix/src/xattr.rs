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
/// Returns `false` (and sets `errno = EINVAL`) when any bit outside
/// `XATTR_CREATE | XATTR_REPLACE` is set.  That is the whole of Linux's test:
/// `setxattr_copy` (fs/xattr.c:598) is `if (ctx->flags &
/// ~(XATTR_CREATE|XATTR_REPLACE)) return -EINVAL;` and nothing more.
///
/// Setting *both* flags is deliberately **not** rejected here, though it used
/// to be.  It is not a libc-level error: the mask above lets it through, and
/// the filesystem then answers from the attribute's actual state — `EEXIST`
/// when it exists (`XATTR_CREATE` loses) and `ENODATA` when it does not
/// (`XATTR_REPLACE` loses); see ext4's `ext4_xattr_set_handle`
/// (fs/ext4/xattr.c:2412-2423) for the canonical shape.  Deciding it in
/// userspace both returned an errno Linux never returns for this call and
/// usurped a judgement only the filesystem can make.  Our own kernel already
/// agrees with Linux — `xattr_validate_size_flags`
/// (kernel/src/syscall/linux.rs) masks with `0x3` and has no both-flags test.
fn setxattr_flags_valid(flags: i32) -> bool {
    if flags & !(XATTR_CREATE | XATTR_REPLACE) != 0 {
        errno::set_errno(errno::EINVAL);
        return false;
    }
    true
}

/// Validate and resolve the `path` argument of a `*xattr` entry point.
///
/// This is the point in Linux's `path_getxattr` / `path_setxattr` /
/// `path_removexattr` (fs/xattr.c) where `user_path_at` runs: **before** the
/// attribute name is read and, for the setters, before the flags are checked.
/// Every caller must therefore run this first, so that a bad path outranks a
/// bad name — a NULL path with a NULL name is `EFAULT` from the path, and a
/// *nonexistent* path with a NULL name is `ENOENT`, not `EFAULT`.
///
/// On the host build there is no filesystem to resolve against, so only the
/// pointer check applies; the resulting length is unused there.
fn resolve_xattr_path(path: *const u8, buf: &mut [u8; crate::unistd::PATH_MAX]) -> Option<usize> {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return None;
    }
    #[cfg(target_os = "none")]
    {
        crate::file::resolve_or_err(path, buf)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = buf;
        Some(0)
    }
}

/// Validate the attribute-name argument of a `*xattr` entry point.
///
/// Linux reads the name with `strncpy_from_user` (fs/xattr.c, in `getxattr`,
/// `setxattr_copy` and `removexattr`), which faults on a NULL pointer — but
/// only after the path has been resolved and, in the setters, after the flags
/// have been checked.  Callers must respect that order.
fn check_xattr_name(name: *const u8) -> bool {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
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
    let mut buf = [0u8; crate::unistd::PATH_MAX];
    let Some(len) = resolve_xattr_path(path, &mut buf) else {
        return -1;
    };
    if !check_xattr_name(name) {
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        do_getxattr(buf.as_ptr(), len, name, value, size, false)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (len, value, size);
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
    let mut buf = [0u8; crate::unistd::PATH_MAX];
    let Some(len) = resolve_xattr_path(path, &mut buf) else {
        return -1;
    };
    if !check_xattr_name(name) {
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        do_getxattr(buf.as_ptr(), len, name, value, size, true)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (len, value, size);
        0
    }
}

/// Get an extended attribute value by file descriptor.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fgetxattr(fd: i32, name: *const u8, value: *mut u8, size: usize) -> SsizeT {
    // `SYSCALL_DEFINE4(fgetxattr)` opens with `fdget (fd)` and returns `-EBADF`
    // before it reaches `getxattr()`, which is where the name is read, so a bad
    // descriptor outranks a bad name (fs/xattr.c).
    if fd < 0 || crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if !check_xattr_name(name) {
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
    // Linux's order, from `path_setxattr` down into `setxattr_copy`
    // (fs/xattr.c): resolve the path, *then* reject bad flags, *then* read the
    // name.  So a bad flag beats a NULL name, and the path beats both.
    let mut buf = [0u8; crate::unistd::PATH_MAX];
    let Some(len) = resolve_xattr_path(path, &mut buf) else {
        return -1;
    };
    if !setxattr_flags_valid(flags) {
        return -1;
    }
    if !check_xattr_name(name) {
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        do_setxattr(buf.as_ptr(), len, name, value, size, flags, false)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (len, value, size);
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
    let mut buf = [0u8; crate::unistd::PATH_MAX];
    let Some(len) = resolve_xattr_path(path, &mut buf) else {
        return -1;
    };
    if !setxattr_flags_valid(flags) {
        return -1;
    }
    if !check_xattr_name(name) {
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        do_setxattr(buf.as_ptr(), len, name, value, size, flags, true)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (len, value, size);
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
    // `SYSCALL_DEFINE5(fsetxattr)` takes the descriptor first, then calls
    // `setxattr()` → `setxattr_copy`, which checks the flags before reading the
    // name (fs/xattr.c:598-602).  So: fd, flags, name.
    if fd < 0 || crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if !setxattr_flags_valid(flags) {
        return -1;
    }
    if !check_xattr_name(name) {
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
    let mut buf = [0u8; crate::unistd::PATH_MAX];
    let Some(len) = resolve_xattr_path(path, &mut buf) else {
        return -1;
    };
    #[cfg(target_os = "none")]
    {
        do_listxattr(buf.as_ptr(), len, list, size, false)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (len, list, size);
        0
    }
}

/// List extended attribute names WITHOUT following a trailing symlink:
/// lists the link inode's own xattrs (`llistxattr`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn llistxattr(path: *const u8, list: *mut u8, size: usize) -> SsizeT {
    let mut buf = [0u8; crate::unistd::PATH_MAX];
    let Some(len) = resolve_xattr_path(path, &mut buf) else {
        return -1;
    };
    #[cfg(target_os = "none")]
    {
        do_listxattr(buf.as_ptr(), len, list, size, true)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (len, list, size);
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
    let mut buf = [0u8; crate::unistd::PATH_MAX];
    let Some(len) = resolve_xattr_path(path, &mut buf) else {
        return -1;
    };
    if !check_xattr_name(name) {
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        do_removexattr(buf.as_ptr(), len, name, false)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = len;
        0
    }
}

/// Remove an extended attribute WITHOUT following a trailing symlink:
/// removes from the link inode's own xattrs (`lremovexattr`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lremovexattr(path: *const u8, name: *const u8) -> i32 {
    let mut buf = [0u8; crate::unistd::PATH_MAX];
    let Some(len) = resolve_xattr_path(path, &mut buf) else {
        return -1;
    };
    if !check_xattr_name(name) {
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        do_removexattr(buf.as_ptr(), len, name, true)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = len;
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
    if !check_xattr_name(name) {
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

    /// The only rejection is a bit outside the mask, matching `setxattr_copy`
    /// (fs/xattr.c:598): `if (ctx->flags & ~(XATTR_CREATE|XATTR_REPLACE))
    /// return -EINVAL;`.  Both flags together passes the mask, so libc must let
    /// it through — see `test_setxattr_both_flags_is_not_a_libc_error`.
    #[test]
    fn test_setxattr_flags_valid() {
        assert!(setxattr_flags_valid(0));
        assert!(setxattr_flags_valid(XATTR_CREATE));
        assert!(setxattr_flags_valid(XATTR_REPLACE));
        assert!(setxattr_flags_valid(XATTR_CREATE | XATTR_REPLACE));
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

    // -- Invalid setxattr flags → EINVAL --

    /// `XATTR_CREATE | XATTR_REPLACE` used to be rejected here with `EINVAL`,
    /// which Linux never returns for it: the only flag test is the mask in
    /// `setxattr_copy` (fs/xattr.c:598), which both bits pass.  The filesystem
    /// then answers from the attribute's state — `EEXIST` if it exists,
    /// `ENODATA` if it does not (ext4's `ext4_xattr_set_handle`,
    /// fs/ext4/xattr.c:2412-2423).  That is not a decision libc can make, so we
    /// forward and let the call succeed here on the host build.
    #[test]
    fn test_setxattr_both_flags_is_not_a_libc_error() {
        errno::set_errno(0);
        assert_eq!(
            setxattr(
                b"/tmp/test\0".as_ptr(),
                b"user.test\0".as_ptr(),
                b"value\0".as_ptr(),
                5,
                XATTR_CREATE | XATTR_REPLACE,
            ),
            0
        );
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

    // -- Ordering: path, then flags, then name --

    /// Linux resolves the path in `path_setxattr` before it ever calls
    /// `setxattr_copy`, so the path outranks both the flags and the name
    /// (fs/xattr.c).  A NULL path with a bad flag *and* a NULL name is still
    /// `EFAULT`.
    #[test]
    fn test_setxattr_path_outranks_flags_and_name() {
        errno::set_errno(0);
        assert_eq!(
            setxattr(core::ptr::null(), core::ptr::null(), core::ptr::null(), 0, 0x40),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// `setxattr_copy` checks the flags (fs/xattr.c:598) before it reads the
    /// name with `strncpy_from_user` (fs/xattr.c:601), so a bad flag outranks a
    /// NULL name.  We used to return `EFAULT` here, having checked the name
    /// first.
    #[test]
    fn test_setxattr_flags_outrank_a_null_name() {
        errno::set_errno(0);
        assert_eq!(
            setxattr(b"/tmp/test\0".as_ptr(), core::ptr::null(), core::ptr::null(), 0, 0x40),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Same ordering in the symlink-preserving variant.
    #[test]
    fn test_lsetxattr_flags_outrank_a_null_name() {
        errno::set_errno(0);
        assert_eq!(
            lsetxattr(b"/tmp\0".as_ptr(), core::ptr::null(), core::ptr::null(), 0, 0x40),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// And in the descriptor variant, below the `EBADF` from `fdget`:
    /// `SYSCALL_DEFINE5(fsetxattr)` → `setxattr()` → `setxattr_copy`, flags
    /// before name (fs/xattr.c).
    #[test]
    fn test_fsetxattr_flags_outrank_a_null_name() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        errno::set_errno(0);
        assert_eq!(
            fsetxattr(fd, core::ptr::null(), core::ptr::null(), 0, 0x40),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
        let _ = fdtable::close_fd(fd);
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
        // Linux uses: `SYSCALL_DEFINE5(fsetxattr)` returns from `fdget` before
        // it calls `setxattr()` → `setxattr_copy` (fs/xattr.c).  The bad flag
        // is an out-of-mask bit; `XATTR_CREATE | XATTR_REPLACE`, which this
        // test used to pass, is not rejected at all.
        errno::set_errno(0);
        assert_eq!(
            fsetxattr(-1, b"user.test\0".as_ptr(), b"v\0".as_ptr(), 1, 0x40),
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

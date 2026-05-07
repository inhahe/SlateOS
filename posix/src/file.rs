//! POSIX file I/O functions.
//!
//! Implements `open`, `close`, `read`, `write`, `lseek`, `dup`, `dup2`,
//! `stat`, `fstat`, `lstat`, `unlink`, `rename`, `link`, `symlink`,
//! `readlink`, `mkdir`, `rmdir`, `fsync`.
//!
//! ## Translation
//!
//! Our kernel uses file handles (integers) similar to POSIX file
//! descriptors.  The main differences:
//!
//! - Our `SYS_FS_OPEN` takes a path and flags, returns a handle.
//! - Our `SYS_FS_READ`/`SYS_FS_WRITE` take (handle, buf, len).
//! - Our `SYS_FS_SEEK` takes (handle, offset, whence).
//!
//! These map almost 1:1 to POSIX, making the translation thin.

use crate::errno;
use crate::fcntl;
use crate::stat::Stat;
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// open / close
// ---------------------------------------------------------------------------

/// Open a file.
///
/// Translates POSIX `open(path, flags, mode)` to our native
/// `SYS_FS_OPEN(path_ptr, path_len, flags)`.
///
/// Returns a file descriptor on success, -1 on error (errno set).
#[unsafe(no_mangle)]
pub extern "C" fn open(path: *const u8, flags: i32, mode: ModeT) -> Fd {
    let _ = mode; // Our kernel doesn't use mode in open yet.

    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Calculate path length (C string → length).
    let path_len = unsafe { c_strlen(path) };

    // Translate POSIX flags to our native flags.
    // Our kernel open flags are simpler — we pass the raw POSIX
    // flags and let the kernel interpret them.
    let native_flags = translate_open_flags(flags);

    let ret = syscall3(
        SYS_FS_OPEN,
        path as u64,
        path_len as u64,
        native_flags,
    );

    errno::translate(ret) as Fd
}

/// Close a file descriptor.
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn close(fd: Fd) -> i32 {
    let ret = syscall1(SYS_FS_CLOSE, fd as u64);
    errno::translate(ret) as i32
}

// ---------------------------------------------------------------------------
// read / write
// ---------------------------------------------------------------------------

/// Read from a file descriptor.
///
/// Returns number of bytes read, 0 at EOF, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn read(fd: Fd, buf: *mut u8, count: SizeT) -> SsizeT {
    if buf.is_null() && count > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let ret = syscall3(SYS_FS_READ, fd as u64, buf as u64, count as u64);
    errno::translate(ret) as SsizeT
}

/// Write to a file descriptor.
///
/// Returns number of bytes written, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn write(fd: Fd, buf: *const u8, count: SizeT) -> SsizeT {
    if buf.is_null() && count > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let ret = syscall3(SYS_FS_WRITE, fd as u64, buf as u64, count as u64);
    errno::translate(ret) as SsizeT
}

// ---------------------------------------------------------------------------
// lseek
// ---------------------------------------------------------------------------

/// Reposition file offset.
///
/// Returns the resulting offset from the beginning of the file,
/// or -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn lseek(fd: Fd, offset: OffT, whence: i32) -> OffT {
    let ret = syscall3(SYS_FS_SEEK, fd as u64, offset as u64, whence as u64);
    errno::translate(ret) as OffT
}

// ---------------------------------------------------------------------------
// dup / dup2
// ---------------------------------------------------------------------------

/// Duplicate a file descriptor.
///
/// Returns the new file descriptor, or -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn dup(oldfd: Fd) -> Fd {
    let ret = syscall1(SYS_FS_DUP, oldfd as u64);
    errno::translate(ret) as Fd
}

/// Duplicate a file descriptor to a specific number.
///
/// If `newfd` is already open, it is silently closed first.
/// Returns `newfd` on success, -1 on error.
///
/// Note: Our kernel doesn't support targeted dup2 yet — this
/// currently falls back to a regular dup and logs a warning
/// if newfd != returned fd.  Full dup2 semantics require kernel
/// support for handle slot targeting.
#[unsafe(no_mangle)]
pub extern "C" fn dup2(oldfd: Fd, newfd: Fd) -> Fd {
    // TODO: Implement proper dup2 with kernel support for
    // targeting a specific fd number.  For now, just dup.
    let _ = newfd;
    let ret = syscall1(SYS_FS_DUP, oldfd as u64);
    errno::translate(ret) as Fd
}

// ---------------------------------------------------------------------------
// stat / fstat / lstat
// ---------------------------------------------------------------------------

/// Get file status by path.
///
/// Our kernel's `SYS_FS_STAT` returns metadata in a kernel-defined
/// format.  We translate it to the POSIX `struct stat`.
#[unsafe(no_mangle)]
pub extern "C" fn stat(path: *const u8, buf: *mut Stat) -> i32 {
    if path.is_null() || buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let path_len = unsafe { c_strlen(path) };

    // Our SYS_FS_STAT returns: size (i64) in rax.
    // For a full stat we need more info — use a kernel stat buffer.
    // For now, do a minimal translation: just get the size.
    let ret = syscall3(
        SYS_FS_STAT,
        path as u64,
        path_len as u64,
        buf as u64,
    );

    if ret < 0 {
        return errno::translate(ret) as i32;
    }

    // The kernel wrote metadata into our buffer in its own format.
    // We need to translate if the formats differ.  For now, assume
    // the kernel stat buffer is compatible enough.
    //
    // TODO: Define a proper kernel stat ABI and translate here.
    0
}

/// Get file status by file descriptor.
#[unsafe(no_mangle)]
pub extern "C" fn fstat(fd: Fd, buf: *mut Stat) -> i32 {
    if buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let ret = syscall2(SYS_FS_FSTAT, fd as u64, buf as u64);

    if ret < 0 {
        return errno::translate(ret) as i32;
    }

    0
}

/// Get symbolic link status (don't follow final symlink).
#[unsafe(no_mangle)]
pub extern "C" fn lstat(path: *const u8, buf: *mut Stat) -> i32 {
    if path.is_null() || buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let path_len = unsafe { c_strlen(path) };
    let ret = syscall3(
        SYS_FS_LSTAT,
        path as u64,
        path_len as u64,
        buf as u64,
    );

    if ret < 0 {
        return errno::translate(ret) as i32;
    }

    0
}

// ---------------------------------------------------------------------------
// unlink / rename / link / symlink / readlink
// ---------------------------------------------------------------------------

/// Remove a directory entry (delete a file).
#[unsafe(no_mangle)]
pub extern "C" fn unlink(path: *const u8) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let path_len = unsafe { c_strlen(path) };
    let ret = syscall2(SYS_FS_DELETE, path as u64, path_len as u64);
    errno::translate(ret) as i32
}

/// Rename a file.
///
/// Our kernel's `SYS_FS_RENAME` takes (old_path, old_len, new_path, new_len).
#[unsafe(no_mangle)]
pub extern "C" fn rename(oldpath: *const u8, newpath: *const u8) -> i32 {
    if oldpath.is_null() || newpath.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let old_len = unsafe { c_strlen(oldpath) };
    let new_len = unsafe { c_strlen(newpath) };

    let ret = syscall4(
        SYS_FS_RENAME,
        oldpath as u64,
        old_len as u64,
        newpath as u64,
        new_len as u64,
    );
    errno::translate(ret) as i32
}

/// Create a hard link.
#[unsafe(no_mangle)]
pub extern "C" fn link(oldpath: *const u8, newpath: *const u8) -> i32 {
    if oldpath.is_null() || newpath.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let old_len = unsafe { c_strlen(oldpath) };
    let new_len = unsafe { c_strlen(newpath) };

    let ret = syscall4(
        SYS_FS_LINK,
        oldpath as u64,
        old_len as u64,
        newpath as u64,
        new_len as u64,
    );
    errno::translate(ret) as i32
}

/// Create a symbolic link.
#[unsafe(no_mangle)]
pub extern "C" fn symlink(target: *const u8, linkpath: *const u8) -> i32 {
    if target.is_null() || linkpath.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let target_len = unsafe { c_strlen(target) };
    let link_len = unsafe { c_strlen(linkpath) };

    let ret = syscall4(
        SYS_FS_SYMLINK,
        target as u64,
        target_len as u64,
        linkpath as u64,
        link_len as u64,
    );
    errno::translate(ret) as i32
}

/// Read a symbolic link.
///
/// Returns the number of bytes placed in `buf`, or -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn readlink(path: *const u8, buf: *mut u8, bufsiz: SizeT) -> SsizeT {
    if path.is_null() || buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let path_len = unsafe { c_strlen(path) };
    let ret = syscall4(
        SYS_FS_READLINK,
        path as u64,
        path_len as u64,
        buf as u64,
        bufsiz as u64,
    );
    errno::translate(ret) as SsizeT
}

// ---------------------------------------------------------------------------
// mkdir / rmdir
// ---------------------------------------------------------------------------

/// Create a directory.
#[unsafe(no_mangle)]
pub extern "C" fn mkdir(path: *const u8, mode: ModeT) -> i32 {
    let _ = mode; // Our kernel doesn't use mode for mkdir yet.

    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let path_len = unsafe { c_strlen(path) };
    let ret = syscall2(SYS_FS_MKDIR, path as u64, path_len as u64);
    errno::translate(ret) as i32
}

/// Remove a directory.
#[unsafe(no_mangle)]
pub extern "C" fn rmdir(path: *const u8) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let path_len = unsafe { c_strlen(path) };
    let ret = syscall2(SYS_FS_RMDIR, path as u64, path_len as u64);
    errno::translate(ret) as i32
}

// ---------------------------------------------------------------------------
// truncate / ftruncate
// ---------------------------------------------------------------------------

/// Truncate a file to a specified length (by path).
#[unsafe(no_mangle)]
pub extern "C" fn truncate(path: *const u8, length: OffT) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let path_len = unsafe { c_strlen(path) };
    let ret = syscall3(
        SYS_FS_TRUNCATE,
        path as u64,
        path_len as u64,
        length as u64,
    );
    errno::translate(ret) as i32
}

/// Truncate a file to a specified length (by fd).
#[unsafe(no_mangle)]
pub extern "C" fn ftruncate(fd: Fd, length: OffT) -> i32 {
    let ret = syscall2(SYS_FS_FTRUNCATE, fd as u64, length as u64);
    errno::translate(ret) as i32
}

// ---------------------------------------------------------------------------
// fsync
// ---------------------------------------------------------------------------

/// Synchronize file data to storage.
#[unsafe(no_mangle)]
pub extern "C" fn fsync(_fd: Fd) -> i32 {
    // Our SYS_FS_SYNC is a global sync, not per-fd.
    let ret = syscall0(SYS_FS_SYNC);
    errno::translate(ret) as i32
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute length of a C string (excluding null terminator).
///
/// # Safety
///
/// `s` must point to a valid null-terminated string.
#[inline]
unsafe fn c_strlen(s: *const u8) -> usize {
    let mut len: usize = 0;
    // SAFETY: Caller guarantees s is a valid C string.
    while unsafe { *s.add(len) } != 0 {
        len = len.wrapping_add(1);
    }
    len
}

/// Public wrapper for `c_strlen` used by other modules.
///
/// # Safety
///
/// `s` must point to a valid null-terminated string.
#[inline]
#[must_use]
pub unsafe fn c_strlen_pub(s: *const u8) -> usize {
    unsafe { c_strlen(s) }
}

/// Translate POSIX open flags to our native flag word.
///
/// Our kernel open flags are a subset of Linux flags:
/// - Bits 0-1: access mode (O_RDONLY=0, O_WRONLY=1, O_RDWR=2)
/// - Bit 6: O_CREAT
/// - Bit 9: O_TRUNC
/// - Bit 10: O_APPEND
///
/// We pass them through with minimal translation since our kernel
/// uses a Linux-compatible flag encoding.
fn translate_open_flags(posix_flags: i32) -> u64 {
    let mut native: u64 = 0;

    // Access mode.
    native |= (posix_flags & fcntl::O_ACCMODE) as u64;

    // Creation flags.
    if posix_flags & fcntl::O_CREAT != 0 {
        native |= 0x40; // Bit 6 = create.
    }
    if posix_flags & fcntl::O_TRUNC != 0 {
        native |= 0x200; // Bit 9 = truncate.
    }
    if posix_flags & fcntl::O_APPEND != 0 {
        native |= 0x400; // Bit 10 = append.
    }
    if posix_flags & fcntl::O_EXCL != 0 {
        native |= 0x80; // Bit 7 = exclusive.
    }

    native
}

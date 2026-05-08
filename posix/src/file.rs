//! POSIX file I/O functions.
//!
//! Implements `open`, `close`, `read`, `write`, `lseek`, `dup`, `dup2`,
//! `stat`, `fstat`, `lstat`, `unlink`, `rename`, `link`, `symlink`,
//! `readlink`, `mkdir`, `rmdir`, `fsync`.
//!
//! ## Translation
//!
//! Our kernel uses separate handle namespaces for files, pipes, and
//! channels.  POSIX unifies everything as integer file descriptors.
//! The fd table (`fdtable`) bridges this gap.
//!
//! `read`, `write`, `close` dispatch to the correct kernel syscall
//! based on the handle type stored in the fd table entry.

use crate::errno;
use crate::fcntl;
use crate::fdtable::{self, HandleKind};
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

    if ret < 0 {
        return errno::translate(ret) as Fd;
    }

    // Register the kernel file handle in the fd table.
    let kernel_handle = ret as u64;
    if let Some(fd) = fdtable::alloc_fd(HandleKind::File, kernel_handle) {
        fd
    } else {
        // Fd table full — close the kernel handle.
        let _ = syscall1(SYS_FS_CLOSE, kernel_handle);
        errno::set_errno(errno::EMFILE);
        -1
    }
}

/// Close a file descriptor.
///
/// Dispatches to the appropriate kernel close syscall based on
/// the handle type stored in the fd table.
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn close(fd: Fd) -> i32 {
    let Some(entry) = fdtable::close_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    let ret = match entry.kind {
        HandleKind::File => syscall1(SYS_FS_CLOSE, entry.handle),
        HandleKind::Pipe => syscall1(SYS_PIPE_CLOSE, entry.handle),
        HandleKind::Console => return 0, // Console fds don't need kernel close.
        HandleKind::TcpStream => {
            crate::socket::clear_meta(fd);
            if entry.handle == 0 { return 0; } // Unconnected socket, nothing to close.
            syscall1(SYS_TCP_CLOSE, entry.handle)
        }
        HandleKind::TcpListener => {
            crate::socket::clear_meta(fd);
            syscall1(SYS_TCP_CLOSE_LISTENER, entry.handle)
        }
        HandleKind::UdpSocket => {
            crate::socket::clear_meta(fd);
            if entry.handle == 0 { return 0; } // Unbound socket, nothing to close.
            syscall1(SYS_UDP_CLOSE, entry.handle)
        }
    };

    errno::translate(ret) as i32
}

// ---------------------------------------------------------------------------
// read / write
// ---------------------------------------------------------------------------

/// Read from a file descriptor.
///
/// Dispatches to the correct kernel read syscall based on handle type:
/// - File → `SYS_FS_READ`
/// - Pipe → `SYS_PIPE_READ`
/// - Console → `SYS_CONSOLE_READ_CHAR` (one byte at a time)
///
/// Returns number of bytes read, 0 at EOF, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn read(fd: Fd, buf: *mut u8, count: SizeT) -> SsizeT {
    if buf.is_null() && count > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else { return -1; };

    let ret = match entry.kind {
        HandleKind::File => {
            syscall3(SYS_FS_READ, entry.handle, buf as u64, count as u64)
        }
        HandleKind::Pipe => {
            syscall3(SYS_PIPE_READ, entry.handle, buf as u64, count as u64)
        }
        HandleKind::Console => {
            // Console read: one character at a time via SYS_CONSOLE_READ_CHAR.
            if count == 0 {
                return 0;
            }
            let ch = syscall0(SYS_CONSOLE_READ_CHAR);
            if ch < 0 {
                return errno::translate(ch) as SsizeT;
            }
            // SAFETY: buf is valid for at least `count` bytes (checked above).
            unsafe { *buf = ch as u8; }
            1
        }
        HandleKind::TcpStream => {
            syscall3(SYS_TCP_RECV, entry.handle, buf as u64, count as u64)
        }
        HandleKind::TcpListener | HandleKind::UdpSocket => {
            // Listeners are not readable via read(); use accept().
            // UDP is not readable via read(); use recvfrom().
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    };

    errno::translate(ret) as SsizeT
}

/// Write to a file descriptor.
///
/// Dispatches to the correct kernel write syscall based on handle type:
/// - File → `SYS_FS_WRITE`
/// - Pipe → `SYS_PIPE_WRITE`
/// - Console → `SYS_CONSOLE_WRITE`
///
/// Returns number of bytes written, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn write(fd: Fd, buf: *const u8, count: SizeT) -> SsizeT {
    if buf.is_null() && count > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else { return -1; };

    let ret = match entry.kind {
        HandleKind::File => {
            syscall3(SYS_FS_WRITE, entry.handle, buf as u64, count as u64)
        }
        HandleKind::Pipe => {
            syscall3(SYS_PIPE_WRITE, entry.handle, buf as u64, count as u64)
        }
        HandleKind::Console => {
            syscall2(SYS_CONSOLE_WRITE, buf as u64, count as u64)
        }
        HandleKind::TcpStream => {
            syscall3(SYS_TCP_SEND, entry.handle, buf as u64, count as u64)
        }
        HandleKind::TcpListener | HandleKind::UdpSocket => {
            // Listeners are not writable via write(); use accept().
            // UDP is not writable via write(); use sendto().
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    };

    errno::translate(ret) as SsizeT
}

// ---------------------------------------------------------------------------
// lseek
// ---------------------------------------------------------------------------

/// Reposition file offset.
///
/// Only valid for File handles.  Pipes and consoles are not seekable
/// and return ESPIPE.
///
/// Returns the resulting offset from the beginning of the file,
/// or -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn lseek(fd: Fd, offset: OffT, whence: i32) -> OffT {
    let Some(entry) = lookup_fd(fd) else { return -1; };

    match entry.kind {
        HandleKind::File => {
            let ret = syscall3(SYS_FS_SEEK, entry.handle, offset as u64, whence as u64);
            errno::translate(ret) as OffT
        }
        HandleKind::Pipe | HandleKind::Console
        | HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {
            errno::set_errno(errno::ESPIPE);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// dup / dup2
// ---------------------------------------------------------------------------

/// Duplicate a file descriptor.
///
/// Returns the lowest available fd pointing to the same resource,
/// or -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn dup(oldfd: Fd) -> Fd {
    let Some(entry) = lookup_fd(oldfd) else { return -1; };

    match entry.kind {
        HandleKind::File => {
            // Kernel-level dup creates a new independent handle.
            let ret = syscall1(SYS_FS_DUP, entry.handle);
            if ret < 0 {
                return errno::translate(ret) as Fd;
            }
            if let Some(fd) = fdtable::alloc_fd(HandleKind::File, ret as u64) {
                fd
            } else {
                let _ = syscall1(SYS_FS_CLOSE, ret as u64);
                errno::set_errno(errno::EMFILE);
                -1
            }
        }
        HandleKind::Console => {
            // Console handles are shared — just allocate a new fd entry.
            if let Some(fd) = fdtable::alloc_fd(HandleKind::Console, entry.handle) {
                fd
            } else {
                errno::set_errno(errno::EMFILE);
                -1
            }
        }
        HandleKind::Pipe
        | HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {
            // No kernel-level dup available for pipes or sockets.
            // TODO: Add refcounting to fd table or per-kind kernel dup.
            errno::set_errno(errno::ENOSYS);
            -1
        }
    }
}

/// Duplicate a file descriptor to a specific number.
///
/// If `newfd` is already open, it is silently closed first.
/// Returns `newfd` on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn dup2(oldfd: Fd, newfd: Fd) -> Fd {
    if oldfd == newfd {
        // POSIX: if oldfd == newfd and oldfd is valid, return newfd.
        if fdtable::get_fd(oldfd).is_some() {
            return newfd;
        }
        errno::set_errno(errno::EBADF);
        return -1;
    }

    let Some(entry) = lookup_fd(oldfd) else { return -1; };

    if newfd < 0 || newfd as usize >= fdtable::MAX_FDS {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // For File handles, create a kernel-level duplicate.
    // For Console, share the handle.
    // For Pipe, not yet supported.
    let new_handle = match entry.kind {
        HandleKind::File => {
            let ret = syscall1(SYS_FS_DUP, entry.handle);
            if ret < 0 {
                return errno::translate(ret) as Fd;
            }
            ret as u64
        }
        HandleKind::Console => entry.handle,
        HandleKind::Pipe
        | HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {
            errno::set_errno(errno::ENOSYS);
            return -1;
        }
    };

    // Install at newfd, closing whatever was there.
    if let Some(old) = fdtable::install_fd(newfd, entry.kind, new_handle) {
        // Close the previously open handle.
        let _ = close_kernel_handle(old.kind, old.handle);
    }

    newfd
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
///
/// Only meaningful for File handles.  Pipe fds return a
/// minimal stat with `st_mode = S_IFIFO`.
#[unsafe(no_mangle)]
pub extern "C" fn fstat(fd: Fd, buf: *mut Stat) -> i32 {
    if buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else { return -1; };

    match entry.kind {
        HandleKind::File => {
            let ret = syscall2(SYS_FS_FSTAT, entry.handle, buf as u64);
            if ret < 0 {
                return errno::translate(ret) as i32;
            }
            0
        }
        HandleKind::Pipe => {
            // Return minimal stat for a pipe.
            // SAFETY: buf validity checked above.
            unsafe {
                core::ptr::write_bytes(buf, 0, 1);
                (*buf).st_mode = crate::fcntl::S_IFIFO;
            }
            0
        }
        HandleKind::Console => {
            // Return minimal stat for a character device.
            unsafe {
                core::ptr::write_bytes(buf, 0, 1);
                (*buf).st_mode = crate::fcntl::S_IFCHR;
            }
            0
        }
        HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {
            // Return minimal stat for a socket.
            unsafe {
                core::ptr::write_bytes(buf, 0, 1);
                (*buf).st_mode = crate::fcntl::S_IFSOCK;
            }
            0
        }
    }
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
    let Some(entry) = lookup_fd(fd) else { return -1; };

    match entry.kind {
        HandleKind::File => {
            let ret = syscall2(SYS_FS_FTRUNCATE, entry.handle, length as u64);
            errno::translate(ret) as i32
        }
        HandleKind::Pipe | HandleKind::Console
        | HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// fsync
// ---------------------------------------------------------------------------

/// Synchronize file data to storage.
///
/// Only meaningful for File handles.  Returns 0 for pipes/console.
#[unsafe(no_mangle)]
pub extern "C" fn fsync(fd: Fd) -> i32 {
    let Some(entry) = lookup_fd(fd) else { return -1; };

    match entry.kind {
        HandleKind::File => {
            // Our SYS_FS_SYNC is a global sync, not per-fd.
            let ret = syscall0(SYS_FS_SYNC);
            errno::translate(ret) as i32
        }
        HandleKind::Pipe | HandleKind::Console
        | HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => 0,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Look up an fd in the table, setting EBADF errno if not found.
///
/// Reduces repetitive `match fdtable::get_fd + errno::set_errno(EBADF)` patterns.
#[must_use]
fn lookup_fd(fd: Fd) -> Option<fdtable::FdEntry> {
    let entry = fdtable::get_fd(fd);
    if entry.is_none() {
        errno::set_errno(errno::EBADF);
    }
    entry
}

/// Close an underlying kernel handle by type.
///
/// Used when tearing down an fd entry (e.g., during dup2 when the
/// target fd was previously open).
fn close_kernel_handle(kind: HandleKind, handle: u64) -> i64 {
    match kind {
        HandleKind::File => syscall1(SYS_FS_CLOSE, handle),
        HandleKind::Pipe => syscall1(SYS_PIPE_CLOSE, handle),
        HandleKind::Console => 0, // Console handles are not closeable.
        HandleKind::TcpStream => syscall1(SYS_TCP_CLOSE, handle),
        HandleKind::TcpListener => syscall1(SYS_TCP_CLOSE_LISTENER, handle),
        HandleKind::UdpSocket => syscall1(SYS_UDP_CLOSE, handle),
    }
}

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

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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn open(path: *const u8, flags: i32, mode: ModeT) -> Fd {
    let _ = mode; // Our kernel doesn't use mode in open yet.

    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Resolve relative paths against CWD and normalize.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };

    let native_flags = translate_open_flags(flags);

    let ret = syscall3(
        SYS_FS_OPEN,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        native_flags,
    );

    if ret < 0 {
        return errno::translate(ret) as Fd;
    }

    // Register the kernel file handle in the fd table.
    // Store the original POSIX flags (access mode + status flags) so
    // fcntl(F_GETFL) can return them.  Strip creation-only flags
    // (O_CREAT, O_EXCL, O_TRUNC, O_NOCTTY, O_DIRECTORY) that don't
    // survive past open().
    let stored_flags = flags & (fcntl::O_ACCMODE | fcntl::O_APPEND
        | fcntl::O_NONBLOCK | fcntl::O_SYNC | fcntl::O_NOFOLLOW);
    let kernel_handle = ret as u64;
    if let Some(fd_num) = fdtable::alloc_fd_with_flags(
        HandleKind::File,
        kernel_handle,
        stored_flags,
    ) {
        // Set FD_CLOEXEC if O_CLOEXEC was requested.
        if flags & fcntl::O_CLOEXEC != 0 {
            let _ = fdtable::set_fd_flags(fd_num, fdtable::FD_CLOEXEC);
        }
        // Store the resolved absolute path for fchdir() / *at() dirfd.
        fdtable::store_fd_path(fd_num, resolved.as_ptr(), resolved_len);
        fd_num
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn close(fd: Fd) -> i32 {
    // Clear stored path before closing the fd entry.
    fdtable::clear_fd_path(fd);

    let Some(entry) = fdtable::close_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    // Read socket metadata BEFORE clearing (need it for SO_LINGER check).
    let socket_meta = match entry.kind {
        HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {
            let m = crate::socket::get_meta(fd);
            crate::socket::clear_meta(fd);
            m
        }
        _ => None,
    };

    // If another fd still references the same kernel handle (from
    // dup on handle types without kernel-level duplication), skip
    // the kernel close — the handle is still in use.
    if fdtable::is_handle_referenced(entry.kind, entry.handle) {
        return 0;
    }

    let ret = match entry.kind {
        HandleKind::File => syscall1(SYS_FS_CLOSE, entry.handle),
        HandleKind::Pipe => syscall1(SYS_PIPE_CLOSE, entry.handle),
        HandleKind::Console => return 0, // Console fds don't need kernel close.
        HandleKind::TcpStream => {
            if entry.handle == 0 { return 0; } // Unconnected socket, nothing to close.
            let (linger_on, linger_secs) = socket_meta
                .map_or((false, 0i32), |m| (m.linger_onoff, m.linger_secs));
            if linger_on && linger_secs == 0 {
                // SO_LINGER with timeout 0: send RST (abortive close).
                syscall1(SYS_TCP_ABORT, entry.handle)
            } else if linger_on && linger_secs > 0 {
                // SO_LINGER with positive timeout: initiate graceful close,
                // then block until close completes or timeout expires.
                const POLL_NS: u64 = 10_000_000; // 10ms
                let ret = syscall1(SYS_TCP_CLOSE, entry.handle);
                if ret < 0 {
                    return errno::translate(ret) as i32;
                }
                // Wait for connection to reach CLOSED/TIME_WAIT.
                let deadline_ns = (syscall0(SYS_CLOCK_MONOTONIC) as u64)
                    .saturating_add((linger_secs as u64).saturating_mul(1_000_000_000));
                loop {
                    let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
                    if now >= deadline_ns {
                        // Timeout expired — abort any remaining state.
                        let _ = syscall1(SYS_TCP_ABORT, entry.handle);
                        break;
                    }
                    // Check if connection is fully closed (POLL_HANGUP set).
                    let status = syscall1(SYS_TCP_POLL_STATUS, entry.handle) as u16;
                    if (status & 0x0010) != 0 {
                        break; // POLL_HANGUP: close handshake completed.
                    }
                    let _ = syscall1(SYS_SLEEP, POLL_NS);
                }
                ret
            } else {
                // No linger (default): non-blocking graceful close.
                syscall1(SYS_TCP_CLOSE, entry.handle)
            }
        }
        HandleKind::TcpListener => {
            syscall1(SYS_TCP_CLOSE_LISTENER, entry.handle)
        }
        HandleKind::UdpSocket => {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn read(fd: Fd, buf: *mut u8, count: SizeT) -> SsizeT {
    if buf.is_null() && count > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // POSIX: "If nbyte is 0, read() will return 0 and have no other
    // results."  Short-circuit before touching the kernel so a 0-length
    // read on a reset TCP connection doesn't spuriously return an error.
    if count == 0 {
        return 0;
    }

    let Some(entry) = lookup_fd(fd) else { return -1; };

    let ret = match entry.kind {
        HandleKind::File => {
            syscall3(SYS_FS_READ, entry.handle, buf as u64, count as u64)
        }
        HandleKind::Pipe => {
            // Use non-blocking read when O_NONBLOCK is set on the fd.
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0)
                & crate::fcntl::O_NONBLOCK != 0;
            if is_nb {
                syscall3(SYS_PIPE_TRY_READ, entry.handle, buf as u64, count as u64)
            } else {
                syscall3(SYS_PIPE_READ, entry.handle, buf as u64, count as u64)
            }
        }
        HandleKind::Console => {
            // Console read: one character at a time via SYS_CONSOLE_READ_CHAR.
            let ch = syscall0(SYS_CONSOLE_READ_CHAR);
            if ch < 0 {
                return errno::translate(ch) as SsizeT;
            }
            // SAFETY: buf is valid for at least `count` bytes (checked above).
            unsafe { *buf = ch as u8; }
            1
        }
        HandleKind::TcpStream => {
            if entry.handle == 0 {
                errno::set_errno(errno::ENOTCONN);
                return -1;
            }
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0)
                & crate::fcntl::O_NONBLOCK != 0;
            let timeout_ms = crate::socket::get_meta(fd).map_or(0u64, |m| m.rcvtimeo_ms);

            // Always try non-blocking first — we implement blocking
            // and SO_RCVTIMEO in the POSIX layer via tcp_recv_wait.
            let ret = syscall4(
                SYS_TCP_RECV, entry.handle, buf as u64, count as u64,
                0x40, // MSG_DONTWAIT
            );
            if ret >= 0 {
                return ret as SsizeT;
            }
            let posix_err = crate::socket::translate_net_error(ret);
            if (posix_err == errno::EAGAIN || posix_err == errno::EWOULDBLOCK) && !is_nb {
                // Blocking socket — poll-wait with SO_RCVTIMEO.
                // timeout_ms == 0 means wait indefinitely.
                return crate::socket::tcp_recv_wait(
                    entry.handle, buf, count, 0, timeout_ms,
                );
            }
            errno::set_errno(posix_err);
            return -1;
        }
        HandleKind::UdpSocket => {
            // read() on UDP behaves like recv(flags=0).  If the socket
            // is bound (has a handle), it receives the next datagram.
            // If unbound (handle==0), recv() will return EINVAL.
            // Unlike write(), read() does NOT require connect() — the
            // source address is simply discarded (use recvfrom() to get it).
            return unsafe {
                crate::socket::recv(fd, buf, count, 0)
            } as SsizeT;
        }
        HandleKind::TcpListener => {
            // Listeners are not readable via read(); use accept().
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn write(fd: Fd, buf: *const u8, count: SizeT) -> SsizeT {
    if buf.is_null() && count > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // POSIX: "If nbyte is zero and the file is a regular file, write()
    // will return zero and have no other results."  For non-regular files
    // (pipes, sockets) the spec says "unspecified", but Linux returns 0
    // without error, and programs depend on this behavior.
    if count == 0 {
        return 0;
    }

    let Some(entry) = lookup_fd(fd) else { return -1; };

    let ret = match entry.kind {
        HandleKind::File => {
            // O_APPEND: seek to EOF before each write so the data is
            // appended atomically (w.r.t. single-process).  This handles
            // the case where O_APPEND was added via fcntl(F_SETFL) after
            // open() — the kernel handle doesn't know about the flag
            // change, so we must seek explicitly.  When O_APPEND was in
            // the original open() flags the kernel already appends, but
            // the redundant seek is harmless (it targets the same offset
            // the kernel would use).
            let status = fdtable::get_status_flags(fd).unwrap_or(0);
            if status & crate::fcntl::O_APPEND != 0 {
                // SEEK_END(2), offset 0 → position at EOF.
                let _ = syscall3(SYS_FS_SEEK, entry.handle, 0, 2);
            }
            syscall3(SYS_FS_WRITE, entry.handle, buf as u64, count as u64)
        }
        HandleKind::Pipe => {
            // Use non-blocking write when O_NONBLOCK is set on the fd.
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0)
                & crate::fcntl::O_NONBLOCK != 0;
            let ret = if is_nb {
                syscall3(SYS_PIPE_TRY_WRITE, entry.handle, buf as u64, count as u64)
            } else {
                syscall3(SYS_PIPE_WRITE, entry.handle, buf as u64, count as u64)
            };
            if ret == errno::native::CHANNEL_CLOSED {
                // Reader has closed — POSIX mandates EPIPE (not ECONNRESET).
                errno::set_errno(errno::EPIPE);
                return -1;
            }
            ret
        }
        HandleKind::Console => {
            syscall2(SYS_CONSOLE_WRITE, buf as u64, count as u64)
        }
        HandleKind::TcpStream => {
            if entry.handle == 0 {
                errno::set_errno(errno::ENOTCONN);
                return -1;
            }
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0)
                & crate::fcntl::O_NONBLOCK != 0;
            if !is_nb {
                // Blocking socket: use tcp_send_wait for full-write
                // semantics.  Linux's blocking write() loops until ALL
                // bytes are accepted; programs depend on this (same
                // behavior as send() on a blocking socket).
                let timeout_ms = crate::socket::get_meta(fd)
                    .map_or(0u64, |m| m.sndtimeo_ms);
                return crate::socket::tcp_send_wait(
                    entry.handle, buf, count, timeout_ms,
                );
            }
            // Non-blocking: try once.
            let ret = syscall3(SYS_TCP_SEND, entry.handle, buf as u64, count as u64);
            if ret >= 0 {
                return ret as SsizeT;
            }
            // ChannelClosed (-300) needs EPIPE/ECONNRESET distinction:
            // RST from peer → ECONNRESET; local shutdown/graceful close → EPIPE.
            if ret == errno::native::CHANNEL_CLOSED {
                let last = syscall1(
                    crate::syscall::SYS_TCP_LAST_ERROR, entry.handle,
                ) as u8;
                errno::set_errno(if last == 2 { errno::ECONNRESET } else { errno::EPIPE });
                return -1;
            }
            return errno::translate(ret) as SsizeT;
        }
        HandleKind::UdpSocket => {
            // POSIX: write() on a connected UDP socket behaves like send().
            let meta = crate::socket::get_meta(fd);
            let is_connected = meta.is_some_and(|m| m.peer_addr != 0 || m.peer_port != 0);
            if !is_connected {
                errno::set_errno(errno::EDESTADDRREQ);
                return -1;
            }
            return unsafe {
                crate::socket::send(fd, buf, count, 0)
            } as SsizeT;
        }
        HandleKind::TcpListener => {
            // Listeners are not writable via write(); use accept().
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lseek(fd: Fd, offset: OffT, whence: i32) -> OffT {
    // POSIX: EINVAL if whence is not a valid value.
    if whence != crate::fcntl::SEEK_SET
        && whence != crate::fcntl::SEEK_CUR
        && whence != crate::fcntl::SEEK_END
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

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
// pread / pwrite
// ---------------------------------------------------------------------------

/// Read from a file at a given offset without changing the file position.
///
/// This is implemented as seek→read→seek-back.  This is not atomic
/// with respect to other threads, but sufficient for single-threaded
/// programs.  Pipes and consoles return `ESPIPE`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pread(fd: Fd, buf: *mut u8, count: SizeT, offset: OffT) -> SsizeT {
    if buf.is_null() && count > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // POSIX: "If nbyte is 0, read() will return 0 and have no other results."
    if count == 0 {
        return 0;
    }
    // POSIX: pread with negative offset shall fail with EINVAL.
    // Without this check, a negative OffT cast to u64 becomes a huge
    // positive seek position, causing spurious errors or wrong data.
    if offset < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else { return -1; };

    if entry.kind != HandleKind::File {
        errno::set_errno(errno::ESPIPE);
        return -1;
    }

    // Save current position.
    let saved = syscall3(SYS_FS_SEEK, entry.handle, 0, crate::fcntl::SEEK_CUR as u64);
    if saved < 0 {
        return errno::translate(saved) as SsizeT;
    }

    // Seek to the requested offset.
    let seek_ret = syscall3(SYS_FS_SEEK, entry.handle, offset as u64, crate::fcntl::SEEK_SET as u64);
    if seek_ret < 0 {
        return errno::translate(seek_ret) as SsizeT;
    }

    // Read.
    let read_ret = syscall3(SYS_FS_READ, entry.handle, buf as u64, count as u64);

    // Restore original position (best effort — if this fails, the file
    // position is lost, but the alternative is leaking the error).
    let _ = syscall3(SYS_FS_SEEK, entry.handle, saved as u64, crate::fcntl::SEEK_SET as u64);

    if read_ret < 0 {
        return errno::translate(read_ret) as SsizeT;
    }
    read_ret as SsizeT
}

/// Write to a file at a given offset without changing the file position.
///
/// Same seek→write→seek-back strategy as `pread`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pwrite(fd: Fd, buf: *const u8, count: SizeT, offset: OffT) -> SsizeT {
    if buf.is_null() && count > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // POSIX: "If nbyte is 0 and the file is a regular file, write() will
    // return zero and have no other results."
    if count == 0 {
        return 0;
    }
    // POSIX: pwrite with negative offset shall fail with EINVAL.
    if offset < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else { return -1; };

    if entry.kind != HandleKind::File {
        errno::set_errno(errno::ESPIPE);
        return -1;
    }

    // Save current position.
    let saved = syscall3(SYS_FS_SEEK, entry.handle, 0, crate::fcntl::SEEK_CUR as u64);
    if saved < 0 {
        return errno::translate(saved) as SsizeT;
    }

    // Seek to the requested offset.
    let seek_ret = syscall3(SYS_FS_SEEK, entry.handle, offset as u64, crate::fcntl::SEEK_SET as u64);
    if seek_ret < 0 {
        return errno::translate(seek_ret) as SsizeT;
    }

    // Write.
    let write_ret = syscall3(SYS_FS_WRITE, entry.handle, buf as u64, count as u64);

    // Restore original position.
    let _ = syscall3(SYS_FS_SEEK, entry.handle, saved as u64, crate::fcntl::SEEK_SET as u64);

    if write_ret < 0 {
        return errno::translate(write_ret) as SsizeT;
    }
    write_ret as SsizeT
}

// ---------------------------------------------------------------------------
// readv / writev
// ---------------------------------------------------------------------------

/// I/O vector for scatter/gather I/O.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Iovec {
    /// Base address of buffer.
    pub iov_base: *mut u8,
    /// Length of buffer.
    pub iov_len: SizeT,
}

/// Read data into multiple buffers (scatter read).
///
/// Reads sequentially into each iovec buffer.  Returns the total
/// number of bytes read, or -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn readv(fd: Fd, iov: *const Iovec, iovcnt: i32) -> SsizeT {
    if iov.is_null() || iovcnt <= 0 || iovcnt > 1024 {
        // POSIX: EINVAL if iovcnt ≤ 0 or > IOV_MAX (1024).
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let mut total: SsizeT = 0;
    let mut i: i32 = 0;
    while i < iovcnt {
        // SAFETY: Caller guarantees iov is valid for iovcnt entries.
        let vec = unsafe { &*iov.add(i as usize) };
        if vec.iov_len > 0 {
            let n = read(fd, vec.iov_base, vec.iov_len);
            if n < 0 {
                // If we already read some data, return that.
                if total > 0 {
                    return total;
                }
                return n;
            }
            total = total.wrapping_add(n);
            // Short read — don't continue to next buffer.
            if (n as SizeT) < vec.iov_len {
                break;
            }
        }
        i = i.wrapping_add(1);
    }

    total
}

/// Write data from multiple buffers (gather write).
///
/// Writes sequentially from each iovec buffer.  Returns the total
/// number of bytes written, or -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn writev(fd: Fd, iov: *const Iovec, iovcnt: i32) -> SsizeT {
    if iov.is_null() || iovcnt <= 0 || iovcnt > 1024 {
        // POSIX: EINVAL if iovcnt ≤ 0 or > IOV_MAX (1024).
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let mut total: SsizeT = 0;
    let mut i: i32 = 0;
    while i < iovcnt {
        // SAFETY: Caller guarantees iov is valid for iovcnt entries.
        let vec = unsafe { &*iov.add(i as usize) };
        if vec.iov_len > 0 {
            let n = write(fd, vec.iov_base.cast_const(), vec.iov_len);
            if n < 0 {
                if total > 0 {
                    return total;
                }
                return n;
            }
            total = total.wrapping_add(n);
            if (n as SizeT) < vec.iov_len {
                break;
            }
        }
        i = i.wrapping_add(1);
    }

    total
}

// ---------------------------------------------------------------------------
// dup / dup2
// ---------------------------------------------------------------------------

/// Duplicate a file descriptor.
///
/// Returns the lowest available fd pointing to the same resource,
/// or -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dup(oldfd: Fd) -> Fd {
    let Some(entry) = lookup_fd(oldfd) else { return -1; };

    // dup'd fds inherit the source fd's status flags (O_APPEND, etc.)
    // but NOT the fd-level flags (FD_CLOEXEC is cleared on the new fd).
    let src_status = entry.status_flags;

    match entry.kind {
        HandleKind::File => {
            // Kernel-level dup creates a new independent handle.
            let ret = syscall1(SYS_FS_DUP, entry.handle);
            if ret < 0 {
                return errno::translate(ret) as Fd;
            }
            if let Some(fd) = fdtable::alloc_fd_with_flags(
                HandleKind::File, ret as u64, src_status,
            ) {
                fdtable::copy_fd_path(oldfd, fd);
                fd
            } else {
                let _ = syscall1(SYS_FS_CLOSE, ret as u64);
                errno::set_errno(errno::EMFILE);
                -1
            }
        }
        HandleKind::Console => {
            // Console handles are shared — just allocate a new fd entry.
            if let Some(fd) = fdtable::alloc_fd_with_flags(
                HandleKind::Console, entry.handle, src_status,
            ) {
                fdtable::copy_fd_path(oldfd, fd);
                fd
            } else {
                errno::set_errno(errno::EMFILE);
                -1
            }
        }
        HandleKind::Pipe => {
            // No kernel-level dup for pipes.  Share the handle;
            // close() uses is_handle_referenced() to only close the
            // kernel handle when the last fd referencing it is closed.
            if let Some(fd) = fdtable::alloc_fd_with_flags(
                HandleKind::Pipe, entry.handle, src_status,
            ) {
                fdtable::copy_fd_path(oldfd, fd);
                fd
            } else {
                errno::set_errno(errno::EMFILE);
                -1
            }
        }
        HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {
            // Share the handle (same refcounting as pipes).
            if let Some(new_fd) = fdtable::alloc_fd_with_flags(
                entry.kind, entry.handle, src_status,
            ) {
                // Copy socket metadata so getpeername/getsockname
                // works on the dup'd fd too.
                crate::socket::copy_meta(oldfd, new_fd);
                fdtable::copy_fd_path(oldfd, new_fd);
                new_fd
            } else {
                errno::set_errno(errno::EMFILE);
                -1
            }
        }
    }
}

/// Duplicate a file descriptor to a specific number.
///
/// If `newfd` is already open, it is silently closed first.
/// Returns `newfd` on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
    // For Console/Pipe/Socket, share the same handle (refcounted
    // via is_handle_referenced() in close()).
    let new_handle = match entry.kind {
        HandleKind::File => {
            let ret = syscall1(SYS_FS_DUP, entry.handle);
            if ret < 0 {
                return errno::translate(ret) as Fd;
            }
            ret as u64
        }
        HandleKind::Console
        | HandleKind::Pipe
        | HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {
            entry.handle
        }
    };

    // Install at newfd, closing whatever was there.
    // dup2 inherits the source's status flags (O_APPEND, O_NONBLOCK, etc.).
    if let Some(old) = fdtable::install_fd_with_flags(
        newfd, entry.kind, new_handle, entry.status_flags,
    ) {
        // Read socket metadata BEFORE clearing — SO_LINGER settings
        // must be respected when closing the evicted handle, just like
        // close() does.
        let evicted_meta = match old.kind {
            HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {
                let m = crate::socket::get_meta(newfd);
                crate::socket::clear_meta(newfd);
                m
            }
            _ => None,
        };
        // Only close the old kernel handle if no other fd still uses it.
        if !fdtable::is_handle_referenced(old.kind, old.handle) {
            // For TCP streams: respect SO_LINGER on the evicted socket,
            // matching close() behavior per POSIX dup2 spec ("closed first").
            if old.kind == HandleKind::TcpStream && old.handle != 0 {
                let (linger_on, linger_secs) = evicted_meta
                    .map_or((false, 0i32), |m| (m.linger_onoff, m.linger_secs));
                if linger_on && linger_secs == 0 {
                    // Abortive close: send RST.
                    let _ = syscall1(SYS_TCP_ABORT, old.handle);
                } else {
                    // Graceful close (default or linger with timeout).
                    // Blocking linger wait is skipped for dup2 — programs
                    // rarely set SO_LINGER(>0) on fds they then dup2 over,
                    // and blocking in dup2 would be surprising.
                    let _ = syscall1(SYS_TCP_CLOSE, old.handle);
                }
            } else {
                let _ = close_kernel_handle(old.kind, old.handle);
            }
        }
    }

    // Copy socket metadata for dup'd socket fds.
    match entry.kind {
        HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {
            crate::socket::copy_meta(oldfd, newfd);
        }
        _ => {}
    }

    // Copy the stored path so fchdir/dirfd works on the dup'd fd.
    fdtable::copy_fd_path(oldfd, newfd);

    newfd
}

// ---------------------------------------------------------------------------
// dup3
// ---------------------------------------------------------------------------

/// Duplicate a file descriptor, with flags.
///
/// Like `dup2`, but the `flags` parameter can include `O_CLOEXEC`.
///
/// Returns `newfd` on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dup3(oldfd: Fd, newfd: Fd, flags: i32) -> Fd {
    if oldfd == newfd {
        // POSIX / Linux: dup3 returns EINVAL when oldfd == newfd
        // (unlike dup2 which succeeds).
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let result = dup2(oldfd, newfd);
    if result >= 0 && flags & fcntl::O_CLOEXEC != 0 {
        let _ = fdtable::set_fd_flags(result, fdtable::FD_CLOEXEC);
    }
    result
}

// ---------------------------------------------------------------------------
// close_range / closefrom — bulk close
// ---------------------------------------------------------------------------

/// Close all file descriptors in the range `[first, last]`.
///
/// Linux-compatible `close_range` syscall wrapper.  On success returns 0;
/// on error returns -1 and sets errno.
///
/// `flags` is currently ignored (Linux supports `CLOSE_RANGE_UNSHARE`
/// and `CLOSE_RANGE_CLOEXEC`, neither of which is meaningful for us).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn close_range(first: u32, last: u32, _flags: u32) -> i32 {
    // Cap at MAX_FDS-1: no fd beyond the table limit can be open,
    // and iterating up to u32::MAX would take ~4 billion iterations
    // due to wrapping.  Programs commonly pass UINT_MAX as `last`
    // to close "everything from first upward."
    let max = fdtable::MAX_FDS as u32;
    let effective_last = if last >= max { max.wrapping_sub(1) } else { last };
    let mut fd = first;
    while fd <= effective_last {
        // close() is best-effort here — ignore errors on individual fds.
        let _ = close(fd as i32);
        fd = fd.wrapping_add(1);
    }
    0
}

/// Close all file descriptors >= `lowfd`.
///
/// BSD/Solaris extension.  Closes all fds from `lowfd` to the table
/// size limit.  Returns nothing (void in C).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn closefrom(lowfd: i32) {
    let max_fd = fdtable::MAX_FDS as i32;
    let mut fd = lowfd.max(0);
    while fd < max_fd {
        let _ = close(fd);
        fd = fd.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// stat / fstat / lstat
// ---------------------------------------------------------------------------

/// Get file status by path.
///
/// Our kernel's `SYS_FS_STAT` returns metadata in a kernel-defined
/// format.  We translate it to the POSIX `struct stat`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn stat(path: *const u8, buf: *mut Stat) -> i32 {
    if path.is_null() || buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };

    let ret = syscall3(
        SYS_FS_STAT,
        resolved.as_ptr() as u64,
        resolved_len as u64,
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lstat(path: *const u8, buf: *mut Stat) -> i32 {
    if path.is_null() || buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };

    let ret = syscall3(
        SYS_FS_LSTAT,
        resolved.as_ptr() as u64,
        resolved_len as u64,
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn unlink(path: *const u8) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };

    let ret = syscall2(SYS_FS_DELETE, resolved.as_ptr() as u64, resolved_len as u64);
    errno::translate(ret) as i32
}

/// Rename a file.
///
/// Our kernel's `SYS_FS_RENAME` takes (old_path, old_len, new_path, new_len).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn rename(oldpath: *const u8, newpath: *const u8) -> i32 {
    if oldpath.is_null() || newpath.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let mut old_resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(old_len) = resolve_or_err(oldpath, &mut old_resolved) else {
        return -1;
    };
    let mut new_resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(new_len) = resolve_or_err(newpath, &mut new_resolved) else {
        return -1;
    };

    let ret = syscall4(
        SYS_FS_RENAME,
        old_resolved.as_ptr() as u64,
        old_len as u64,
        new_resolved.as_ptr() as u64,
        new_len as u64,
    );
    errno::translate(ret) as i32
}

/// Create a hard link.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn link(oldpath: *const u8, newpath: *const u8) -> i32 {
    if oldpath.is_null() || newpath.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let mut old_resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(old_len) = resolve_or_err(oldpath, &mut old_resolved) else {
        return -1;
    };
    let mut new_resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(new_len) = resolve_or_err(newpath, &mut new_resolved) else {
        return -1;
    };

    let ret = syscall4(
        SYS_FS_LINK,
        old_resolved.as_ptr() as u64,
        old_len as u64,
        new_resolved.as_ptr() as u64,
        new_len as u64,
    );
    errno::translate(ret) as i32
}

/// Create a symbolic link.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn symlink(target: *const u8, linkpath: *const u8) -> i32 {
    if target.is_null() || linkpath.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Target is stored verbatim — do NOT resolve it.  The filesystem
    // records the exact string and resolves it at follow time.
    let target_len = unsafe { c_strlen(target) };

    // Linkpath is the filesystem location where the symlink is created.
    let mut link_resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(link_len) = resolve_or_err(linkpath, &mut link_resolved) else {
        return -1;
    };

    let ret = syscall4(
        SYS_FS_SYMLINK,
        target as u64,
        target_len as u64,
        link_resolved.as_ptr() as u64,
        link_len as u64,
    );
    errno::translate(ret) as i32
}

/// Read a symbolic link.
///
/// Returns the number of bytes placed in `buf`, or -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn readlink(path: *const u8, buf: *mut u8, bufsiz: SizeT) -> SsizeT {
    if path.is_null() || buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };

    let ret = syscall4(
        SYS_FS_READLINK,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        buf as u64,
        bufsiz as u64,
    );
    errno::translate(ret) as SsizeT
}

// ---------------------------------------------------------------------------
// mkdir / rmdir
// ---------------------------------------------------------------------------

/// Create a directory.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mkdir(path: *const u8, mode: ModeT) -> i32 {
    let _ = mode; // Our kernel doesn't use mode for mkdir yet.

    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };

    let ret = syscall2(SYS_FS_MKDIR, resolved.as_ptr() as u64, resolved_len as u64);
    errno::translate(ret) as i32
}

/// Remove a directory.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn rmdir(path: *const u8) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };

    let ret = syscall2(SYS_FS_RMDIR, resolved.as_ptr() as u64, resolved_len as u64);
    errno::translate(ret) as i32
}

// ---------------------------------------------------------------------------
// truncate / ftruncate
// ---------------------------------------------------------------------------

/// Truncate a file to a specified length (by path).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn truncate(path: *const u8, length: OffT) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // POSIX: "If length is negative, the function shall fail and the
    // file size shall remain unchanged.  [EINVAL]."
    if length < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };

    let ret = syscall3(
        SYS_FS_TRUNCATE,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        length as u64,
    );
    errno::translate(ret) as i32
}

/// Truncate a file to a specified length (by fd).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftruncate(fd: Fd, length: OffT) -> i32 {
    // POSIX: "If length is negative, the function shall fail and the
    // file size shall remain unchanged.  [EINVAL]."
    if length < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

/// Sync file data to disk (without metadata).
///
/// POSIX: like `fsync` but only syncs data, not metadata (atime,
/// mtime, etc.).  Our kernel doesn't distinguish, so this delegates
/// to `fsync`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fdatasync(fd: Fd) -> i32 {
    // Our kernel has no separate data-only sync — delegate to fsync.
    fsync(fd)
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

/// Resolve a C-string path relative to the current working directory.
///
/// On success writes the normalized absolute path into `resolved` and
/// returns its byte length.  On failure sets errno to `ENAMETOOLONG`
/// and returns `None`.
#[must_use]
fn resolve_or_err(
    path: *const u8,
    resolved: &mut [u8; crate::unistd::PATH_MAX],
) -> Option<usize> {
    // SAFETY: All callers already null-checked `path`.
    if let Some(len) = unsafe { crate::unistd::resolve_path(path, resolved) } {
        Some(len)
    } else {
        // Distinguish empty path (ENOENT) from path-too-long (ENAMETOOLONG).
        // POSIX: "If the value of path is an empty string, the function
        // shall fail and report [ENOENT]."
        // SAFETY: Callers guarantee path is non-null and a valid C string.
        if unsafe { *path } == 0 {
            errno::set_errno(errno::ENOENT);
        } else {
            errno::set_errno(errno::ENAMETOOLONG);
        }
        None
    }
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

// ---------------------------------------------------------------------------
// access
// ---------------------------------------------------------------------------

/// Check file accessibility.
///
/// Tests whether the calling process can access the file at `path`
/// using the mode flags:
/// - `F_OK` (0): check existence only.
/// - `R_OK` (4): check read permission.
/// - `W_OK` (2): check write permission.
/// - `X_OK` (1): check execute permission.
///
/// Since our OS doesn't have a permission system yet, we check only
/// existence (via `SYS_FS_STAT`) and report all modes as accessible
/// if the file exists.
///
/// Returns 0 on success, -1 on error (errno set).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn access(path: *const u8, _mode: i32) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Resolve relative paths against CWD.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };

    // Use stat to check if the file exists.
    let mut stat_buf = core::mem::MaybeUninit::<Stat>::zeroed();
    let ret = syscall3(
        SYS_FS_STAT,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        stat_buf.as_mut_ptr() as u64,
    );

    if ret < 0 {
        return errno::translate(ret) as i32;
    }

    // File exists.  Since we don't have permissions, all modes succeed.
    0
}

/// Check file accessibility relative to a directory fd.
///
/// `faccessat(AT_FDCWD, path, mode, 0)` is equivalent to `access(path, mode)`.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn faccessat(dirfd: i32, path: *const u8, mode: i32, _flags: i32) -> i32 {
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return access(path, mode);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 { return -1; }
    access(full.as_ptr(), mode)
}

// ---------------------------------------------------------------------------
// *at() functions
// ---------------------------------------------------------------------------
//
// These delegate to the non-*at version when dirfd == AT_FDCWD (-100) or
// when the path is absolute (POSIX: dirfd is ignored for absolute paths).
//
// When dirfd is a real fd and path is relative, we resolve the absolute
// path by looking up the stored path for dirfd (set at open time) and
// concatenating: dir_path + "/" + relative_path.  The result is passed
// to the non-*at function which does its own resolve_path / normalization.
//
// **Limitation:** the stored dirfd path may be stale if the directory was
// renamed after opening.  Real kernels use dentry-based resolution that
// follows renames; our path-string approach doesn't.

/// Returns `true` if the C-string `path` starts with `b'/'` (absolute).
///
/// Returns `false` for null or empty paths.
#[inline]
fn is_absolute_path(path: *const u8) -> bool {
    // SAFETY: Callers guarantee `path` is either null or a valid C-string.
    // We only read the first byte (if non-null), which is always safe for
    // a valid C-string (it's either the first character or the null terminator).
    !path.is_null() && unsafe { *path } == b'/'
}

/// Build an absolute path from a dirfd's stored path and a relative path.
///
/// Concatenates `dir_path[..dir_len] + "/" + rel_path` (C-string) into
/// `out`, null-terminated.  Returns the total length (excluding null),
/// or 0 if the result would exceed `PATH_MAX`.
///
/// Callers pass a dirfd path obtained from [`fdtable::get_fd_path()`]
/// and the user-supplied relative path from the `*at()` call.
fn build_at_path(
    dir_path: &[u8],
    dir_len: usize,
    rel_path: *const u8,
    out: &mut [u8; crate::unistd::PATH_MAX],
) -> usize {
    if rel_path.is_null() {
        return 0;
    }
    // SAFETY: rel_path is a valid C string (caller contract from POSIX).
    let rel_len = unsafe { crate::string::strlen(rel_path) };

    // Need: dir_len + 1 (slash) + rel_len + 1 (null) <= PATH_MAX.
    let total = dir_len.wrapping_add(1).wrapping_add(rel_len);
    if total >= crate::unistd::PATH_MAX {
        return 0;
    }

    // Copy dir_path.
    let mut pos = 0;
    while pos < dir_len {
        if let (Some(&src), Some(dst)) = (dir_path.get(pos), out.get_mut(pos)) {
            *dst = src;
        }
        pos = pos.wrapping_add(1);
    }

    // Append separator (skip if dir_path already ends with '/').
    let needs_slash = dir_len > 0
        && dir_path.get(dir_len.wrapping_sub(1)).copied() != Some(b'/');
    if needs_slash {
        if let Some(dst) = out.get_mut(pos) {
            *dst = b'/';
        }
        pos = pos.wrapping_add(1);
    }

    // Copy relative path.
    // SAFETY: rel_path is valid for rel_len bytes (strlen just measured it).
    let mut i = 0;
    while i < rel_len {
        if let Some(dst) = out.get_mut(pos) {
            *dst = unsafe { *rel_path.add(i) };
        }
        pos = pos.wrapping_add(1);
        i = i.wrapping_add(1);
    }

    // Null-terminate.
    if let Some(dst) = out.get_mut(pos) {
        *dst = 0;
    }

    pos
}

/// Resolve a dirfd + relative path into an absolute path.
///
/// Only called when `dirfd != AT_FDCWD` and `path` is relative.
/// Looks up the stored path for `dirfd` and builds
/// `dir_path + "/" + rel_path` in `out`.
///
/// Returns the total length (excluding null), or 0 on error with
/// errno set (`EBADF`, `ENOTDIR`, or `ENAMETOOLONG`).
fn resolve_dirfd_path(
    dirfd: i32,
    path: *const u8,
    out: &mut [u8; crate::unistd::PATH_MAX],
) -> usize {
    // Verify the dirfd is valid.
    if crate::fdtable::get_fd(dirfd).is_none() {
        errno::set_errno(errno::EBADF);
        return 0;
    }

    // Look up the stored path for dirfd.
    let mut dir_path = [0u8; crate::unistd::PATH_MAX];
    let dir_len = crate::fdtable::get_fd_path(dirfd, &mut dir_path);
    if dir_len == 0 {
        // dirfd has no stored path — not a directory fd, or opened
        // outside our open() (e.g., a pipe or socket).
        errno::set_errno(errno::ENOTDIR);
        return 0;
    }

    let total = build_at_path(&dir_path, dir_len, path, out);
    if total == 0 {
        errno::set_errno(errno::ENAMETOOLONG);
        return 0;
    }
    total
}

/// AT_FDCWD: use the current working directory.
pub const AT_FDCWD: i32 = -100;
/// AT_SYMLINK_NOFOLLOW: do not follow symlinks.
pub const AT_SYMLINK_NOFOLLOW: i32 = 0x100;
/// AT_REMOVEDIR: unlinkat should remove a directory.
pub const AT_REMOVEDIR: i32 = 0x200;
/// AT_SYMLINK_FOLLOW: follow symlinks (e.g., in `linkat`).
pub const AT_SYMLINK_FOLLOW: i32 = 0x400;
/// AT_EMPTY_PATH: operate on the fd itself (Linux 2.6.39+).
pub const AT_EMPTY_PATH: i32 = 0x1000;
/// AT_EACCESS: check using effective IDs in faccessat.
pub const AT_EACCESS: i32 = 0x200;

/// Open a file relative to a directory fd.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn openat(dirfd: i32, path: *const u8, flags: i32, mode: ModeT) -> Fd {
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return open(path, flags, mode);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 { return -1; }
    open(full.as_ptr(), flags, mode)
}

/// Get file status relative to a directory fd.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
/// When `flags` includes `AT_SYMLINK_NOFOLLOW`, uses `lstat` (does
/// not follow symlinks).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fstatat(dirfd: i32, path: *const u8, buf: *mut Stat, flags: i32) -> i32 {
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return if flags & AT_SYMLINK_NOFOLLOW != 0 { lstat(path, buf) } else { stat(path, buf) };
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 { return -1; }
    if flags & AT_SYMLINK_NOFOLLOW != 0 { lstat(full.as_ptr(), buf) } else { stat(full.as_ptr(), buf) }
}

/// Remove a file or directory relative to a directory fd.
///
/// When `flags` includes `AT_REMOVEDIR`, acts like rmdir.
/// Otherwise acts like unlink.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn unlinkat(dirfd: i32, path: *const u8, flags: i32) -> i32 {
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return if flags & AT_REMOVEDIR != 0 { rmdir(path) } else { unlink(path) };
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 { return -1; }
    if flags & AT_REMOVEDIR != 0 { rmdir(full.as_ptr()) } else { unlink(full.as_ptr()) }
}

/// Rename a file relative to directory fds.
///
/// POSIX: each `dirfd` is ignored when its corresponding path is absolute.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn renameat(
    olddirfd: i32,
    oldpath: *const u8,
    newdirfd: i32,
    newpath: *const u8,
) -> i32 {
    // Resolve each path independently — each dirfd is ignored for
    // absolute paths (POSIX).
    let old_needs_resolve = olddirfd != AT_FDCWD && !is_absolute_path(oldpath);
    let new_needs_resolve = newdirfd != AT_FDCWD && !is_absolute_path(newpath);

    let old_ptr;
    let mut old_full = [0u8; crate::unistd::PATH_MAX];
    if old_needs_resolve {
        let len = resolve_dirfd_path(olddirfd, oldpath, &mut old_full);
        if len == 0 { return -1; }
        old_ptr = old_full.as_ptr();
    } else {
        old_ptr = oldpath;
    }

    let new_ptr;
    let mut new_full = [0u8; crate::unistd::PATH_MAX];
    if new_needs_resolve {
        let len = resolve_dirfd_path(newdirfd, newpath, &mut new_full);
        if len == 0 { return -1; }
        new_ptr = new_full.as_ptr();
    } else {
        new_ptr = newpath;
    }

    rename(old_ptr, new_ptr)
}

/// Rename a file with flags (Linux extension).
///
/// `flags` can include `RENAME_NOREPLACE` (1), `RENAME_EXCHANGE` (2).
/// Our kernel doesn't support these flags yet, so non-zero flags
/// return EINVAL.  Zero flags delegates to `renameat`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn renameat2(
    olddirfd: i32,
    oldpath: *const u8,
    newdirfd: i32,
    newpath: *const u8,
    flags: u32,
) -> i32 {
    if flags != 0 {
        // RENAME_NOREPLACE and RENAME_EXCHANGE require kernel support.
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    renameat(olddirfd, oldpath, newdirfd, newpath)
}

/// Create a directory relative to a directory fd.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mkdirat(dirfd: i32, path: *const u8, mode: ModeT) -> i32 {
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return mkdir(path, mode);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 { return -1; }
    mkdir(full.as_ptr(), mode)
}

/// Read a symbolic link relative to a directory fd.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn readlinkat(
    dirfd: i32,
    path: *const u8,
    buf: *mut u8,
    bufsiz: SizeT,
) -> SsizeT {
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return readlink(path, buf, bufsiz);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 { return -1; }
    readlink(full.as_ptr(), buf, bufsiz)
}

/// Create a symbolic link relative to a directory fd.
///
/// POSIX: if `linkpath` is absolute, `newdirfd` is ignored.
/// Note: `target` is stored as-is (not resolved), so its absoluteness
/// doesn't affect whether we need `newdirfd`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn symlinkat(target: *const u8, newdirfd: i32, linkpath: *const u8) -> i32 {
    if newdirfd == AT_FDCWD || is_absolute_path(linkpath) {
        return symlink(target, linkpath);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(newdirfd, linkpath, &mut full);
    if len == 0 { return -1; }
    symlink(target, full.as_ptr())
}

/// Create a hard link relative to directory fds.
///
/// POSIX: each `dirfd` is ignored when its corresponding path is absolute.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn linkat(
    olddirfd: i32,
    oldpath: *const u8,
    newdirfd: i32,
    newpath: *const u8,
    _flags: i32,
) -> i32 {
    let old_needs_resolve = olddirfd != AT_FDCWD && !is_absolute_path(oldpath);
    let new_needs_resolve = newdirfd != AT_FDCWD && !is_absolute_path(newpath);

    let old_ptr;
    let mut old_full = [0u8; crate::unistd::PATH_MAX];
    if old_needs_resolve {
        let len = resolve_dirfd_path(olddirfd, oldpath, &mut old_full);
        if len == 0 { return -1; }
        old_ptr = old_full.as_ptr();
    } else {
        old_ptr = oldpath;
    }

    let new_ptr;
    let mut new_full = [0u8; crate::unistd::PATH_MAX];
    if new_needs_resolve {
        let len = resolve_dirfd_path(newdirfd, newpath, &mut new_full);
        if len == 0 { return -1; }
        new_ptr = new_full.as_ptr();
    } else {
        new_ptr = newpath;
    }

    link(old_ptr, new_ptr)
}

/// Change file mode bits relative to a directory fd.
///
/// Stub: accepts silently.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fchmodat(dirfd: i32, path: *const u8, mode: ModeT, _flags: i32) -> i32 {
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return chmod(path, mode);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 { return -1; }
    chmod(full.as_ptr(), mode)
}

/// Change file owner/group relative to a directory fd.
///
/// Stub: accepts silently.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fchownat(
    dirfd: i32,
    path: *const u8,
    owner: UidT,
    group: GidT,
    _flags: i32,
) -> i32 {
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return chown(path, owner, group);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 { return -1; }
    chown(full.as_ptr(), owner, group)
}

// ---------------------------------------------------------------------------
// chmod / fchmod / chown / fchown (stubs)
// ---------------------------------------------------------------------------

/// Change file mode bits.
///
/// Stub: our OS doesn't have file permissions yet.  Accepts silently.
///
/// Returns 0 (always succeeds).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn chmod(_path: *const u8, _mode: ModeT) -> i32 {
    // No permission system yet — accept silently.
    0
}

/// Change file mode bits (by fd).
///
/// Stub: accepts silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fchmod(_fd: Fd, _mode: ModeT) -> i32 {
    0
}

/// Change file owner and group.
///
/// Stub: our OS doesn't have multi-user support.  Accepts silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn chown(_path: *const u8, _owner: UidT, _group: GidT) -> i32 {
    0
}

/// Change file owner and group (by fd).
///
/// Stub: accepts silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fchown(_fd: Fd, _owner: UidT, _group: GidT) -> i32 {
    0
}

/// Change file owner and group (don't follow symlinks).
///
/// Like `chown`, but does not follow symbolic links — changes ownership
/// of the link itself rather than its target.  Stub: accepts silently.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lchown(_path: *const u8, _owner: UidT, _group: GidT) -> i32 {
    0
}

/// Process-local file mode creation mask.
///
/// Initialized to 0o022 (typical POSIX default: owner rw, group/other r).
/// umask() reads and writes this value atomically (single-threaded).
static mut UMASK_VALUE: ModeT = 0o022;

/// Set file mode creation mask.
///
/// Stores the new mask and returns the previous one.  While the kernel
/// doesn't enforce permissions yet, this gives correct POSIX semantics
/// for programs that query or chain umask values.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn umask(cmask: ModeT) -> ModeT {
    // SAFETY: Single-threaded access to UMASK_VALUE.
    let previous = unsafe { core::ptr::addr_of!(UMASK_VALUE).read() };
    // Only the low 9 bits (rwxrwxrwx) are meaningful for the mask.
    unsafe { core::ptr::addr_of_mut!(UMASK_VALUE).write(cmask & 0o777); }
    previous
}

/// Get the current umask value without modifying it.
///
/// Not a POSIX function, but useful for internal callers that need
/// to apply the mask (e.g., open, mkdir) without side effects.
pub(crate) fn get_umask() -> ModeT {
    // SAFETY: Single-threaded access.
    unsafe { core::ptr::addr_of!(UMASK_VALUE).read() }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// posix_fadvise — file access advice
// ---------------------------------------------------------------------------

/// Normal access pattern (no special advice).
pub const POSIX_FADV_NORMAL: i32 = 0;
/// Sequential access pattern.
pub const POSIX_FADV_SEQUENTIAL: i32 = 2;
/// Random access pattern.
pub const POSIX_FADV_RANDOM: i32 = 1;
/// Data will be accessed once.
pub const POSIX_FADV_NOREUSE: i32 = 5;
/// Data will be accessed soon.
pub const POSIX_FADV_WILLNEED: i32 = 3;
/// Data will not be accessed soon.
pub const POSIX_FADV_DONTNEED: i32 = 4;

/// Advise the kernel about file access patterns.
///
/// Stub: always returns 0 (success).  The kernel doesn't use file
/// access hints yet, but programs that call `posix_fadvise` should
/// not fail.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_fadvise(_fd: Fd, _offset: OffT, _len: OffT, _advice: i32) -> i32 {
    0 // Succeed silently — advice is purely advisory.
}

/// Ensure that disk space is allocated for the file region
/// `[offset, offset+len)`.
///
/// POSIX: on success, returns 0.  On error, returns an error number
/// (NOT -1; unlike most POSIX functions, `posix_fallocate` returns
/// the error directly).
///
/// Our implementation uses `fstat` + `ftruncate` to extend the file
/// if `offset + len` exceeds the current size.  This doesn't truly
/// preallocate contiguous blocks (the filesystem may still allocate
/// lazily), but it guarantees the file is at least as large as
/// `offset + len` — sufficient for programs that use `posix_fallocate`
/// to avoid `ENOSPC` on later writes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_fallocate(fd: Fd, offset: OffT, len: OffT) -> i32 {
    // POSIX: EINVAL if offset < 0 or len <= 0.
    if offset < 0 || len <= 0 {
        return errno::EINVAL;
    }

    // Check that offset + len doesn't overflow.
    let Some(target_size) = offset.checked_add(len) else {
        return errno::EFBIG;
    };

    // Get the current file size.
    let mut stat_buf = Stat::zeroed();
    if fstat(fd, &raw mut stat_buf) < 0 {
        return errno::get_errno();
    }

    // If the file is already large enough, nothing to do.
    if stat_buf.st_size >= target_size {
        return 0;
    }

    // Extend the file to the required size.
    if ftruncate(fd, target_size) < 0 {
        return errno::get_errno();
    }

    0
}

// ---------------------------------------------------------------------------
// flock — advisory file locking
// ---------------------------------------------------------------------------

/// Lock operation: shared (read) lock.
pub const LOCK_SH: i32 = 1;
/// Lock operation: exclusive (write) lock.
pub const LOCK_EX: i32 = 2;
/// Lock operation: unlock.
pub const LOCK_UN: i32 = 8;
/// Lock operation modifier: non-blocking.
pub const LOCK_NB: i32 = 4;

/// Apply or remove an advisory lock on an open file.
///
/// Stub: always succeeds.  Our OS does not yet implement file locking
/// at the kernel level.  Programs that call `flock` at startup for
/// lock files will proceed normally (the lock is advisory and not
/// enforced).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn flock(_fd: Fd, _operation: i32) -> i32 {
    // Advisory locking not yet implemented in the kernel.
    // Return success so programs that create lock files don't fail.
    0
}

// ---------------------------------------------------------------------------
// lockf — POSIX file locking
// ---------------------------------------------------------------------------

/// Lock command: lock a section for exclusive use.
pub const F_LOCK: i32 = 1;
/// Lock command: non-blocking lock attempt.
pub const F_TLOCK: i32 = 2;
/// Lock command: unlock a section.
pub const F_ULOCK: i32 = 0;
/// Lock command: test if a section is locked.
pub const F_TEST: i32 = 3;

/// Lock a section of a file (POSIX `lockf`).
///
/// Stub: always succeeds.  Like `flock`, advisory file locking is not
/// yet enforced by the kernel.  Programs that use `lockf` for lock
/// files or serialization will proceed normally.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lockf(_fd: Fd, _cmd: i32, _len: OffT) -> i32 {
    // Advisory locking not yet implemented.
    0
}

// ---------------------------------------------------------------------------
// sendfile
// ---------------------------------------------------------------------------

/// Copy data between file descriptors (in-kernel optimization).
///
/// Copies up to `count` bytes from `in_fd` to `out_fd`.  If `offset`
/// is non-null, it specifies the starting offset in `in_fd` (and is
/// updated to reflect the new position); the file offset of `in_fd`
/// is NOT modified (matching Linux sendfile semantics).  If `offset`
/// is null, reads from the current file position and advances it.
///
/// Stub: performs the copy in userspace via pread/read + write loop.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sendfile(
    out_fd: Fd,
    in_fd: Fd,
    offset: *mut i64,
    count: usize,
) -> isize {
    let mut buf = [0u8; 4096];
    let mut total: usize = 0;

    if offset.is_null() {
        // No offset — read from current position (advances in_fd).
        // Because read() advances in_fd's position by the number of
        // bytes actually read, we must fully drain the buffer before
        // reading again — otherwise a short write would discard the
        // unwritten bytes (the file position has already moved past
        // them and we can't seek back on non-seekable fds like pipes).
        while total < count {
            let remaining = count.wrapping_sub(total);
            let chunk = if remaining < buf.len() { remaining } else { buf.len() };

            let nr = read(in_fd, buf.as_mut_ptr(), chunk);
            if nr < 0 {
                if total > 0 { break; }
                return -1;
            }
            if nr == 0 { break; }

            // Write all bytes that were read, retrying on short writes.
            let mut written: usize = 0;
            let to_write = nr as usize;
            while written < to_write {
                let nw = write(
                    out_fd,
                    unsafe { buf.as_ptr().add(written) },
                    to_write.wrapping_sub(written),
                );
                if nw < 0 {
                    if total > 0 || written > 0 {
                        total = total.wrapping_add(written);
                        return total as isize;
                    }
                    return -1;
                }
                if nw == 0 { break; } // Avoid infinite loop.
                written = written.wrapping_add(nw as usize);
            }

            total = total.wrapping_add(written);
        }
    } else {
        // Use pread to avoid modifying in_fd's file position.
        // SAFETY: offset is valid (caller contract).
        let mut cur_off = unsafe { *offset };

        while total < count {
            let remaining = count.wrapping_sub(total);
            let chunk = if remaining < buf.len() { remaining } else { buf.len() };

            let nr = pread(in_fd, buf.as_mut_ptr(), chunk, cur_off);
            if nr < 0 {
                if total > 0 { break; }
                return -1;
            }
            if nr == 0 { break; }

            // Write all bytes that were read, retrying on short writes.
            // Without this loop, a short write discards the unwritten
            // bytes — pread on the next iteration reads NEW data from
            // cur_off, not the leftover bytes from buf.
            let mut written: usize = 0;
            let to_write = nr as usize;
            while written < to_write {
                let nw = write(
                    out_fd,
                    unsafe { buf.as_ptr().add(written) },
                    to_write.wrapping_sub(written),
                );
                if nw < 0 {
                    if total > 0 || written > 0 {
                        total = total.wrapping_add(written);
                        cur_off = cur_off.wrapping_add(written as i64);
                        unsafe { *offset = cur_off; }
                        return total as isize;
                    }
                    return -1;
                }
                if nw == 0 { break; } // Avoid infinite loop.
                written = written.wrapping_add(nw as usize);
            }

            total = total.wrapping_add(written);
            cur_off = cur_off.wrapping_add(written as i64);
        }

        // Update caller's offset to reflect bytes transferred.
        // SAFETY: offset is valid.
        unsafe { *offset = cur_off; }
    }

    total as isize
}

// ---------------------------------------------------------------------------
// copy_file_range
// ---------------------------------------------------------------------------

/// Copy data between two files (in-kernel optimization).
///
/// Like `sendfile` but works between any two regular files.  `flags`
/// is reserved and must be 0.
///
/// When `off_in`/`off_out` is non-null, the corresponding fd's file
/// position is NOT modified (uses pread/pwrite internally); the offset
/// is updated to reflect the bytes transferred.  When null, reads/writes
/// from the current fd position and advances it.
///
/// Stub: performs userspace pread/read + pwrite/write copy.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn copy_file_range(
    fd_in: Fd,
    off_in: *mut i64,
    fd_out: Fd,
    off_out: *mut i64,
    len: usize,
    _flags: u32,
) -> isize {
    let mut buf = [0u8; 4096];
    let mut total: usize = 0;

    let mut in_pos = if off_in.is_null() { 0 } else { unsafe { *off_in } };
    let mut out_pos = if off_out.is_null() { 0 } else { unsafe { *off_out } };

    while total < len {
        let remaining = len.wrapping_sub(total);
        let chunk = if remaining < buf.len() { remaining } else { buf.len() };

        // Read: use pread when off_in is provided, else normal read.
        let nr = if off_in.is_null() {
            read(fd_in, buf.as_mut_ptr(), chunk)
        } else {
            pread(fd_in, buf.as_mut_ptr(), chunk, in_pos)
        };
        if nr <= 0 { break; }

        // Write all bytes that were read, retrying on short writes.
        // When off_in is null, read() has already advanced fd_in's
        // position by nr bytes — those bytes exist only in buf and
        // must be fully drained before reading again.
        let mut written: usize = 0;
        let to_write = nr as usize;
        while written < to_write {
            let nw = if off_out.is_null() {
                write(
                    fd_out,
                    unsafe { buf.as_ptr().add(written) },
                    to_write.wrapping_sub(written),
                )
            } else {
                pwrite(
                    fd_out,
                    unsafe { buf.as_ptr().add(written) },
                    to_write.wrapping_sub(written),
                    out_pos.wrapping_add(written as i64),
                )
            };
            if nw < 0 {
                if total > 0 || written > 0 {
                    total = total.wrapping_add(written);
                    // Update offsets for partial progress before returning.
                    in_pos = in_pos.wrapping_add(written as i64);
                    out_pos = out_pos.wrapping_add(written as i64);
                    if !off_in.is_null() { unsafe { *off_in = in_pos; } }
                    if !off_out.is_null() { unsafe { *off_out = out_pos; } }
                    return total as isize;
                }
                return -1;
            }
            if nw == 0 { break; }
            written = written.wrapping_add(nw as usize);
        }

        total = total.wrapping_add(written);
        in_pos = in_pos.wrapping_add(written as i64);
        out_pos = out_pos.wrapping_add(written as i64);
    }

    // Update caller's offsets to reflect bytes transferred.
    if !off_in.is_null() {
        // SAFETY: off_in is valid.
        unsafe { *off_in = in_pos; }
    }
    if !off_out.is_null() {
        // SAFETY: off_out is valid.
        unsafe { *off_out = out_pos; }
    }

    total as isize
}

// ---------------------------------------------------------------------------
// utimes / futimes / utimensat / futimens — timestamps (stubs)
// ---------------------------------------------------------------------------

/// `struct timeval` for `utimes` — seconds + microseconds.
#[repr(C)]
pub struct Timeval {
    /// Seconds.
    pub tv_sec: i64,
    /// Microseconds.
    pub tv_usec: i64,
}

/// Set file access and modification times (microsecond precision).
///
/// Stub: always returns 0.  Our filesystem doesn't track per-file
/// timestamps yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn utimes(_path: *const u8, _times: *const Timeval) -> i32 {
    0
}

/// Set file access and modification times on an open fd.
///
/// Stub: always returns 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn futimes(_fd: Fd, _times: *const Timeval) -> i32 {
    0
}

/// `UTIME_NOW` — set timestamp to current time.
pub const UTIME_NOW: i64 = (1 << 30) - 1;
/// `UTIME_OMIT` — leave timestamp unchanged.
pub const UTIME_OMIT: i64 = (1 << 30) - 2;

/// Set file timestamps with nanosecond precision (relative to dirfd).
///
/// Stub: always returns 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn utimensat(
    _dirfd: Fd,
    _path: *const u8,
    _times: *const crate::stat::Timespec,
    _flags: i32,
) -> i32 {
    0
}

/// Set file timestamps with nanosecond precision on an open fd.
///
/// Stub: always returns 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn futimens(_fd: Fd, _times: *const crate::stat::Timespec) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Translate POSIX open flags to our native flag word.
pub(crate) fn translate_open_flags(posix_flags: i32) -> u64 {
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

// ---------------------------------------------------------------------------
// creat — create a new file (POSIX, equivalent to open with O_CREAT|O_WRONLY|O_TRUNC)
// ---------------------------------------------------------------------------

/// Create a new file or truncate an existing file.
///
/// Equivalent to `open(path, O_CREAT | O_WRONLY | O_TRUNC, mode)`.
/// This is a POSIX function retained for compatibility; new code should
/// use `open()` directly.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn creat(path: *const u8, mode: ModeT) -> Fd {
    open(path, fcntl::O_CREAT | fcntl::O_WRONLY | fcntl::O_TRUNC, mode)
}

// ---------------------------------------------------------------------------
// LP64 aliases — 64-bit variants identical to regular versions
// ---------------------------------------------------------------------------
//
// On LP64 (our x86_64 target), off_t is already 64-bit, so the *64
// variants are identical.  These exist for programs compiled with
// _FILE_OFFSET_BITS=64 or that explicitly use the *64 interfaces.

/// `open64` — alias for `open` on LP64.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn open64(path: *const u8, flags: i32, mode: ModeT) -> Fd {
    open(path, flags, mode)
}

/// `lseek64` — alias for `lseek` on LP64.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lseek64(fd: Fd, offset: OffT, whence: i32) -> OffT {
    lseek(fd, offset, whence)
}

/// `stat64` — alias for `stat` on LP64.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn stat64(path: *const u8, statbuf: *mut crate::stat::Stat) -> i32 {
    stat(path, statbuf)
}

/// `fstat64` — alias for `fstat` on LP64.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fstat64(fd: Fd, statbuf: *mut crate::stat::Stat) -> i32 {
    fstat(fd, statbuf)
}

/// `lstat64` — alias for `lstat` on LP64.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lstat64(path: *const u8, statbuf: *mut crate::stat::Stat) -> i32 {
    lstat(path, statbuf)
}

// ---------------------------------------------------------------------------
// glibc __xstat family — internal stat wrappers
// ---------------------------------------------------------------------------
//
// glibc internally calls __xstat(ver, path, buf) instead of stat(path, buf).
// The `ver` argument selects the stat struct version (1 = old, 3 = current).
// On modern systems, `ver` is always 1 or 3; we ignore it and always use
// our current struct layout.

/// glibc internal: `__xstat(ver, path, buf)` → `stat(path, buf)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __xstat(_ver: i32, path: *const u8, statbuf: *mut crate::stat::Stat) -> i32 {
    stat(path, statbuf)
}

/// glibc internal: `__fxstat(ver, fd, buf)` → `fstat(fd, buf)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __fxstat(_ver: i32, fd: Fd, statbuf: *mut crate::stat::Stat) -> i32 {
    fstat(fd, statbuf)
}

/// glibc internal: `__lxstat(ver, path, buf)` → `lstat(path, buf)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __lxstat(_ver: i32, path: *const u8, statbuf: *mut crate::stat::Stat) -> i32 {
    lstat(path, statbuf)
}

/// glibc internal: 64-bit `__xstat64`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __xstat64(_ver: i32, path: *const u8, statbuf: *mut crate::stat::Stat) -> i32 {
    stat(path, statbuf)
}

/// glibc internal: 64-bit `__fxstat64`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __fxstat64(_ver: i32, fd: Fd, statbuf: *mut crate::stat::Stat) -> i32 {
    fstat(fd, statbuf)
}

/// glibc internal: 64-bit `__lxstat64`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __lxstat64(_ver: i32, path: *const u8, statbuf: *mut crate::stat::Stat) -> i32 {
    lstat(path, statbuf)
}

// ===========================================================================
// FORTIFY_SOURCE _chk wrappers
// ===========================================================================

/// `__read_chk` — fortified `read`.
///
/// `buflen` is the size of the buffer `buf` points to.  We ignore it
/// (no runtime overflow check) and delegate to `read`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __read_chk(fd: Fd, buf: *mut u8, count: SizeT, _buflen: SizeT) -> SsizeT {
    read(fd, buf, count)
}

/// `__pread_chk` — fortified `pread`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __pread_chk(
    fd: Fd,
    buf: *mut u8,
    count: SizeT,
    offset: OffT,
    _buflen: SizeT,
) -> SsizeT {
    pread(fd, buf, count, offset)
}

/// `__pread64_chk` — LP64 alias for `__pread_chk`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __pread64_chk(
    fd: Fd,
    buf: *mut u8,
    count: SizeT,
    offset: OffT,
    buflen: SizeT,
) -> SsizeT {
    __pread_chk(fd, buf, count, offset, buflen)
}

/// `__getcwd_chk` — fortified `getcwd`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __getcwd_chk(
    buf: *mut u8,
    size: SizeT,
    _buflen: SizeT,
) -> *mut u8 {
    crate::unistd::getcwd(buf, size)
}

/// `__realpath_chk` — fortified `realpath`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __realpath_chk(
    path: *const u8,
    resolved: *mut u8,
    _resolved_len: SizeT,
) -> *mut u8 {
    crate::unistd::realpath(path, resolved)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- translate_open_flags --

    #[test]
    fn translate_rdonly() {
        let flags = translate_open_flags(fcntl::O_RDONLY);
        // O_RDONLY = 0, so no access-mode bits set.
        assert_eq!(flags, 0);
    }

    #[test]
    fn translate_wronly() {
        let flags = translate_open_flags(fcntl::O_WRONLY);
        assert_eq!(flags & 0x3, 1); // O_WRONLY = 1.
    }

    #[test]
    fn translate_rdwr() {
        let flags = translate_open_flags(fcntl::O_RDWR);
        assert_eq!(flags & 0x3, 2); // O_RDWR = 2.
    }

    #[test]
    fn translate_creat_trunc() {
        let flags = translate_open_flags(
            fcntl::O_WRONLY | fcntl::O_CREAT | fcntl::O_TRUNC,
        );
        assert_ne!(flags & 0x40, 0, "O_CREAT bit");   // Bit 6.
        assert_ne!(flags & 0x200, 0, "O_TRUNC bit");  // Bit 9.
    }

    #[test]
    fn translate_append() {
        let flags = translate_open_flags(fcntl::O_APPEND);
        assert_ne!(flags & 0x400, 0, "O_APPEND bit"); // Bit 10.
    }

    #[test]
    fn translate_excl() {
        let flags = translate_open_flags(fcntl::O_CREAT | fcntl::O_EXCL);
        assert_ne!(flags & 0x40, 0, "O_CREAT bit");
        assert_ne!(flags & 0x80, 0, "O_EXCL bit"); // Bit 7.
    }

    #[test]
    fn translate_all_flags() {
        let flags = translate_open_flags(
            fcntl::O_RDWR | fcntl::O_CREAT | fcntl::O_TRUNC
            | fcntl::O_APPEND | fcntl::O_EXCL,
        );
        assert_eq!(flags & 0x3, 2);     // O_RDWR.
        assert_ne!(flags & 0x40, 0);    // O_CREAT.
        assert_ne!(flags & 0x80, 0);    // O_EXCL.
        assert_ne!(flags & 0x200, 0);   // O_TRUNC.
        assert_ne!(flags & 0x400, 0);   // O_APPEND.
    }

    #[test]
    fn translate_no_flags() {
        let flags = translate_open_flags(0);
        assert_eq!(flags, 0);
    }

    // -- Stub functions: verify they return expected values --

    #[test]
    fn test_chmod_succeeds() {
        assert_eq!(chmod(b"/tmp\0".as_ptr(), 0o755), 0);
    }

    #[test]
    fn test_fchmod_succeeds() {
        assert_eq!(fchmod(0, 0o644), 0);
    }

    #[test]
    fn test_chown_succeeds() {
        assert_eq!(chown(b"/tmp\0".as_ptr(), 0, 0), 0);
    }

    #[test]
    fn test_fchown_succeeds() {
        assert_eq!(fchown(0, 0, 0), 0);
    }

    #[test]
    fn test_lchown_succeeds() {
        assert_eq!(lchown(b"/link\0".as_ptr(), 0, 0), 0);
    }

    #[test]
    fn test_umask_returns_previous() {
        // Reset to known state.
        umask(0o022);
        // Setting a new mask returns the previous one.
        assert_eq!(umask(0o077), 0o022);
        // Now previous should be 0o077.
        assert_eq!(umask(0o000), 0o077);
        // And previous should be 0o000.
        assert_eq!(umask(0o022), 0o000);
    }

    #[test]
    fn test_umask_masks_high_bits() {
        // Reset to known state.
        umask(0o022);
        // Setting bits beyond the low 9 should be masked off.
        let prev = umask(0o70777); // Only 0o777 should stick.
        assert_eq!(prev, 0o022);
        let val = umask(0o022); // Read back what was stored.
        assert_eq!(val, 0o777);
    }

    #[test]
    fn test_get_umask_no_side_effect() {
        umask(0o137);
        let val = get_umask();
        assert_eq!(val, 0o137);
        // Reading should not change the value.
        assert_eq!(get_umask(), 0o137);
        // Clean up.
        umask(0o022);
    }

    #[test]
    fn test_posix_fadvise_succeeds() {
        assert_eq!(posix_fadvise(0, 0, 0, POSIX_FADV_NORMAL), 0);
        assert_eq!(posix_fadvise(0, 0, 0, POSIX_FADV_SEQUENTIAL), 0);
        assert_eq!(posix_fadvise(0, 0, 0, POSIX_FADV_RANDOM), 0);
    }

    #[test]
    fn test_posix_fallocate_invalid_offset() {
        // Negative offset → EINVAL (returned directly, not via errno).
        assert_eq!(posix_fallocate(0, -1, 4096), crate::errno::EINVAL);
    }

    #[test]
    fn test_posix_fallocate_invalid_len_zero() {
        // len == 0 → EINVAL.
        assert_eq!(posix_fallocate(0, 0, 0), crate::errno::EINVAL);
    }

    #[test]
    fn test_posix_fallocate_invalid_len_negative() {
        // len < 0 → EINVAL.
        assert_eq!(posix_fallocate(0, 0, -1), crate::errno::EINVAL);
    }

    #[test]
    fn test_posix_fallocate_overflow() {
        // offset + len overflows i64 → EFBIG.
        assert_eq!(
            posix_fallocate(0, i64::MAX, 1),
            crate::errno::EFBIG,
        );
    }

    #[test]
    fn test_flock_succeeds() {
        assert_eq!(flock(0, LOCK_SH), 0);
        assert_eq!(flock(0, LOCK_EX), 0);
        assert_eq!(flock(0, LOCK_UN), 0);
        assert_eq!(flock(0, LOCK_EX | LOCK_NB), 0);
    }

    // -- File locking constants match Linux --

    #[test]
    fn test_lock_constants() {
        assert_eq!(LOCK_SH, 1);
        assert_eq!(LOCK_EX, 2);
        assert_eq!(LOCK_NB, 4);
        assert_eq!(LOCK_UN, 8);
    }

    // -- posix_fadvise constants match Linux --

    #[test]
    fn test_fadv_constants() {
        assert_eq!(POSIX_FADV_NORMAL, 0);
        assert_eq!(POSIX_FADV_RANDOM, 1);
        assert_eq!(POSIX_FADV_SEQUENTIAL, 2);
        assert_eq!(POSIX_FADV_WILLNEED, 3);
        assert_eq!(POSIX_FADV_DONTNEED, 4);
        assert_eq!(POSIX_FADV_NOREUSE, 5);
    }

    // -- close_range edge cases --

    #[test]
    fn test_close_range_inverted() {
        // close_range with first > last should do nothing (no crash).
        let _ = close_range(100, 50, 0);
    }

    #[test]
    fn test_close_range_uint_max() {
        // Programs commonly call close_range(3, UINT_MAX, 0) to close
        // all fds from 3 upward.  This must not loop for 4 billion
        // iterations — it should cap at MAX_FDS.
        let _ = close_range(200, u32::MAX, 0);
        // If this returns in reasonable time, the cap works.
    }

    // -- build_at_path --

    #[test]
    fn test_build_at_path_basic() {
        let dir = b"/home/user";
        let rel = b"docs/file.txt\0";
        let mut out = [0u8; crate::unistd::PATH_MAX];
        let len = build_at_path(dir, dir.len(), rel.as_ptr(), &mut out);
        assert_eq!(&out[..len], b"/home/user/docs/file.txt");
        assert_eq!(out[len], 0); // Null-terminated.
    }

    #[test]
    fn test_build_at_path_dir_trailing_slash() {
        let dir = b"/tmp/";
        let rel = b"test.txt\0";
        let mut out = [0u8; crate::unistd::PATH_MAX];
        let len = build_at_path(dir, dir.len(), rel.as_ptr(), &mut out);
        // Should NOT double the slash: /tmp//test.txt → /tmp/test.txt
        assert_eq!(&out[..len], b"/tmp/test.txt");
    }

    #[test]
    fn test_build_at_path_empty_rel() {
        let dir = b"/home";
        let rel = b"\0";
        let mut out = [0u8; crate::unistd::PATH_MAX];
        let len = build_at_path(dir, dir.len(), rel.as_ptr(), &mut out);
        // Empty relative path → just dir + "/".
        assert_eq!(&out[..len], b"/home/");
    }

    #[test]
    fn test_build_at_path_null_rel() {
        let dir = b"/home";
        let mut out = [0u8; crate::unistd::PATH_MAX];
        let len = build_at_path(dir, dir.len(), core::ptr::null(), &mut out);
        assert_eq!(len, 0);
    }

    #[test]
    fn test_build_at_path_overflow() {
        // dir_len + rel_len exceeds PATH_MAX.
        let dir = [b'a'; 4000];
        let mut rel = [b'b'; 200];
        rel[199] = 0; // null-terminate
        let mut out = [0u8; crate::unistd::PATH_MAX];
        let len = build_at_path(&dir, dir.len(), rel.as_ptr(), &mut out);
        assert_eq!(len, 0, "should return 0 when result exceeds PATH_MAX");
    }

    #[test]
    fn test_build_at_path_dotdot_relative() {
        let dir = b"/home/user/project";
        let rel = b"../other\0";
        let mut out = [0u8; crate::unistd::PATH_MAX];
        let len = build_at_path(dir, dir.len(), rel.as_ptr(), &mut out);
        // build_at_path just concatenates — normalization happens later
        // in resolve_path when open() is called.
        assert_eq!(&out[..len], b"/home/user/project/../other");
    }

    // -- is_absolute_path --

    #[test]
    fn test_is_absolute_path_yes() {
        assert!(is_absolute_path(b"/foo\0".as_ptr()));
        assert!(is_absolute_path(b"/\0".as_ptr()));
    }

    #[test]
    fn test_is_absolute_path_no() {
        assert!(!is_absolute_path(b"foo\0".as_ptr()));
        assert!(!is_absolute_path(b".\0".as_ptr()));
        assert!(!is_absolute_path(b"\0".as_ptr()));  // Empty string.
    }

    #[test]
    fn test_is_absolute_path_null() {
        assert!(!is_absolute_path(core::ptr::null()));
    }

    // -- AT_* constants --

    #[test]
    fn test_at_fdcwd_value() {
        assert_eq!(AT_FDCWD, -100);
    }

    #[test]
    fn test_at_flag_values() {
        assert_eq!(AT_SYMLINK_NOFOLLOW, 0x100);
        assert_eq!(AT_REMOVEDIR, 0x200);
        assert_eq!(AT_SYMLINK_FOLLOW, 0x400);
        assert_eq!(AT_EMPTY_PATH, 0x1000);
        assert_eq!(AT_EACCESS, 0x200);
    }

    #[test]
    fn test_at_symlink_flags_distinct() {
        // AT_SYMLINK_NOFOLLOW and AT_SYMLINK_FOLLOW must be different bits.
        assert_ne!(AT_SYMLINK_NOFOLLOW, AT_SYMLINK_FOLLOW);
        assert_eq!(AT_SYMLINK_NOFOLLOW & AT_SYMLINK_FOLLOW, 0);
    }
}

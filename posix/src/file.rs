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
#[unsafe(no_mangle)]
pub extern "C" fn close(fd: Fd) -> i32 {
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn close_range(first: u32, last: u32, _flags: u32) -> i32 {
    let mut fd = first;
    while fd <= last {
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
#[unsafe(no_mangle)]
pub extern "C" fn closefrom(lowfd: i32) {
    // Use a reasonable upper bound — our fd table max is typically 256.
    let max_fd: i32 = 256;
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn faccessat(dirfd: i32, path: *const u8, mode: i32, _flags: i32) -> i32 {
    if dirfd != AT_FDCWD && !is_absolute_path(path) {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }
    access(path, mode)
}

// ---------------------------------------------------------------------------
// *at() functions
// ---------------------------------------------------------------------------
//
// These delegate to the non-*at version when dirfd == AT_FDCWD (-100) or
// when the path is absolute (POSIX: dirfd is ignored for absolute paths).
// Other dirfd values with relative paths return ENOSYS until we implement
// fchdir() or kernel-level *at() support.

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

/// AT_FDCWD: use the current working directory.
pub const AT_FDCWD: i32 = -100;
/// AT_SYMLINK_NOFOLLOW: do not follow symlinks.
pub const AT_SYMLINK_NOFOLLOW: i32 = 0x100;
/// AT_REMOVEDIR: unlinkat should remove a directory.
pub const AT_REMOVEDIR: i32 = 0x200;

/// Open a file relative to a directory fd.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[unsafe(no_mangle)]
pub extern "C" fn openat(dirfd: i32, path: *const u8, flags: i32, mode: ModeT) -> Fd {
    if dirfd != AT_FDCWD && !is_absolute_path(path) {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }
    open(path, flags, mode)
}

/// Get file status relative to a directory fd.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[unsafe(no_mangle)]
pub extern "C" fn fstatat(dirfd: i32, path: *const u8, buf: *mut Stat, _flags: i32) -> i32 {
    if dirfd != AT_FDCWD && !is_absolute_path(path) {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }
    stat(path, buf)
}

/// Remove a file or directory relative to a directory fd.
///
/// When `flags` includes `AT_REMOVEDIR`, acts like rmdir.
/// Otherwise acts like unlink.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[unsafe(no_mangle)]
pub extern "C" fn unlinkat(dirfd: i32, path: *const u8, flags: i32) -> i32 {
    if dirfd != AT_FDCWD && !is_absolute_path(path) {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }
    if flags & AT_REMOVEDIR != 0 {
        rmdir(path)
    } else {
        unlink(path)
    }
}

/// Rename a file relative to directory fds.
///
/// POSIX: each `dirfd` is ignored when its corresponding path is absolute.
#[unsafe(no_mangle)]
pub extern "C" fn renameat(
    olddirfd: i32,
    oldpath: *const u8,
    newdirfd: i32,
    newpath: *const u8,
) -> i32 {
    let old_needs_dirfd = olddirfd != AT_FDCWD && !is_absolute_path(oldpath);
    let new_needs_dirfd = newdirfd != AT_FDCWD && !is_absolute_path(newpath);
    if old_needs_dirfd || new_needs_dirfd {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }
    rename(oldpath, newpath)
}

/// Rename a file with flags (Linux extension).
///
/// `flags` can include `RENAME_NOREPLACE` (1), `RENAME_EXCHANGE` (2).
/// Our kernel doesn't support these flags yet, so non-zero flags
/// return EINVAL.  Zero flags delegates to `renameat`.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn mkdirat(dirfd: i32, path: *const u8, mode: ModeT) -> i32 {
    if dirfd != AT_FDCWD && !is_absolute_path(path) {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }
    mkdir(path, mode)
}

/// Read a symbolic link relative to a directory fd.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[unsafe(no_mangle)]
pub extern "C" fn readlinkat(
    dirfd: i32,
    path: *const u8,
    buf: *mut u8,
    bufsiz: SizeT,
) -> SsizeT {
    if dirfd != AT_FDCWD && !is_absolute_path(path) {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }
    readlink(path, buf, bufsiz)
}

/// Create a symbolic link relative to a directory fd.
///
/// POSIX: if `linkpath` is absolute, `newdirfd` is ignored.
/// Note: `target` is stored as-is (not resolved), so its absoluteness
/// doesn't affect whether we need `newdirfd`.
#[unsafe(no_mangle)]
pub extern "C" fn symlinkat(target: *const u8, newdirfd: i32, linkpath: *const u8) -> i32 {
    if newdirfd != AT_FDCWD && !is_absolute_path(linkpath) {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }
    symlink(target, linkpath)
}

/// Create a hard link relative to directory fds.
///
/// POSIX: each `dirfd` is ignored when its corresponding path is absolute.
#[unsafe(no_mangle)]
pub extern "C" fn linkat(
    olddirfd: i32,
    oldpath: *const u8,
    newdirfd: i32,
    newpath: *const u8,
    _flags: i32,
) -> i32 {
    let old_needs_dirfd = olddirfd != AT_FDCWD && !is_absolute_path(oldpath);
    let new_needs_dirfd = newdirfd != AT_FDCWD && !is_absolute_path(newpath);
    if old_needs_dirfd || new_needs_dirfd {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }
    link(oldpath, newpath)
}

/// Change file mode bits relative to a directory fd.
///
/// Stub: accepts silently.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[unsafe(no_mangle)]
pub extern "C" fn fchmodat(dirfd: i32, path: *const u8, mode: ModeT, _flags: i32) -> i32 {
    if dirfd != AT_FDCWD && !is_absolute_path(path) {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }
    chmod(path, mode)
}

/// Change file owner/group relative to a directory fd.
///
/// Stub: accepts silently.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[unsafe(no_mangle)]
pub extern "C" fn fchownat(
    dirfd: i32,
    path: *const u8,
    owner: UidT,
    group: GidT,
    _flags: i32,
) -> i32 {
    if dirfd != AT_FDCWD && !is_absolute_path(path) {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }
    chown(path, owner, group)
}

// ---------------------------------------------------------------------------
// chmod / fchmod / chown / fchown (stubs)
// ---------------------------------------------------------------------------

/// Change file mode bits.
///
/// Stub: our OS doesn't have file permissions yet.  Accepts silently.
///
/// Returns 0 (always succeeds).
#[unsafe(no_mangle)]
pub extern "C" fn chmod(_path: *const u8, _mode: ModeT) -> i32 {
    // No permission system yet — accept silently.
    0
}

/// Change file mode bits (by fd).
///
/// Stub: accepts silently.
#[unsafe(no_mangle)]
pub extern "C" fn fchmod(_fd: Fd, _mode: ModeT) -> i32 {
    0
}

/// Change file owner and group.
///
/// Stub: our OS doesn't have multi-user support.  Accepts silently.
#[unsafe(no_mangle)]
pub extern "C" fn chown(_path: *const u8, _owner: UidT, _group: GidT) -> i32 {
    0
}

/// Change file owner and group (by fd).
///
/// Stub: accepts silently.
#[unsafe(no_mangle)]
pub extern "C" fn fchown(_fd: Fd, _owner: UidT, _group: GidT) -> i32 {
    0
}

/// Change file owner and group (don't follow symlinks).
///
/// Like `chown`, but does not follow symbolic links — changes ownership
/// of the link itself rather than its target.  Stub: accepts silently.
#[unsafe(no_mangle)]
pub extern "C" fn lchown(_path: *const u8, _owner: UidT, _group: GidT) -> i32 {
    0
}

/// Set file mode creation mask.
///
/// Stub: returns 0o022 (previous mask) and ignores the new mask.
#[unsafe(no_mangle)]
pub extern "C" fn umask(_cmask: ModeT) -> ModeT {
    // No permission system yet — return a typical default mask.
    0o022
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
#[unsafe(no_mangle)]
pub extern "C" fn posix_fadvise(_fd: Fd, _offset: OffT, _len: OffT, _advice: i32) -> i32 {
    0 // Succeed silently — advice is purely advisory.
}

/// Preallocate file space.
///
/// Stub: returns 0 without actually preallocating.  The filesystem
/// layer doesn't support preallocation yet.
#[unsafe(no_mangle)]
pub extern "C" fn posix_fallocate(_fd: Fd, _offset: OffT, _len: OffT) -> i32 {
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn utimes(_path: *const u8, _times: *const Timeval) -> i32 {
    0
}

/// Set file access and modification times on an open fd.
///
/// Stub: always returns 0.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn futimens(_fd: Fd, _times: *const crate::stat::Timespec) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Translate POSIX open flags to our native flag word.
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

// ---------------------------------------------------------------------------
// creat — create a new file (POSIX, equivalent to open with O_CREAT|O_WRONLY|O_TRUNC)
// ---------------------------------------------------------------------------

/// Create a new file or truncate an existing file.
///
/// Equivalent to `open(path, O_CREAT | O_WRONLY | O_TRUNC, mode)`.
/// This is a POSIX function retained for compatibility; new code should
/// use `open()` directly.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn open64(path: *const u8, flags: i32, mode: ModeT) -> Fd {
    open(path, flags, mode)
}

/// `lseek64` — alias for `lseek` on LP64.
#[unsafe(no_mangle)]
pub extern "C" fn lseek64(fd: Fd, offset: OffT, whence: i32) -> OffT {
    lseek(fd, offset, whence)
}

/// `stat64` — alias for `stat` on LP64.
#[unsafe(no_mangle)]
pub extern "C" fn stat64(path: *const u8, statbuf: *mut crate::stat::Stat) -> i32 {
    stat(path, statbuf)
}

/// `fstat64` — alias for `fstat` on LP64.
#[unsafe(no_mangle)]
pub extern "C" fn fstat64(fd: Fd, statbuf: *mut crate::stat::Stat) -> i32 {
    fstat(fd, statbuf)
}

/// `lstat64` — alias for `lstat` on LP64.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn __xstat(_ver: i32, path: *const u8, statbuf: *mut crate::stat::Stat) -> i32 {
    stat(path, statbuf)
}

/// glibc internal: `__fxstat(ver, fd, buf)` → `fstat(fd, buf)`.
#[unsafe(no_mangle)]
pub extern "C" fn __fxstat(_ver: i32, fd: Fd, statbuf: *mut crate::stat::Stat) -> i32 {
    fstat(fd, statbuf)
}

/// glibc internal: `__lxstat(ver, path, buf)` → `lstat(path, buf)`.
#[unsafe(no_mangle)]
pub extern "C" fn __lxstat(_ver: i32, path: *const u8, statbuf: *mut crate::stat::Stat) -> i32 {
    lstat(path, statbuf)
}

/// glibc internal: 64-bit `__xstat64`.
#[unsafe(no_mangle)]
pub extern "C" fn __xstat64(_ver: i32, path: *const u8, statbuf: *mut crate::stat::Stat) -> i32 {
    stat(path, statbuf)
}

/// glibc internal: 64-bit `__fxstat64`.
#[unsafe(no_mangle)]
pub extern "C" fn __fxstat64(_ver: i32, fd: Fd, statbuf: *mut crate::stat::Stat) -> i32 {
    fstat(fd, statbuf)
}

/// glibc internal: 64-bit `__lxstat64`.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn __read_chk(fd: Fd, buf: *mut u8, count: SizeT, _buflen: SizeT) -> SsizeT {
    read(fd, buf, count)
}

/// `__pread_chk` — fortified `pread`.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn __getcwd_chk(
    buf: *mut u8,
    size: SizeT,
    _buflen: SizeT,
) -> *mut u8 {
    crate::unistd::getcwd(buf, size)
}

/// `__realpath_chk` — fortified `realpath`.
#[unsafe(no_mangle)]
pub extern "C" fn __realpath_chk(
    path: *const u8,
    resolved: *mut u8,
    _resolved_len: SizeT,
) -> *mut u8 {
    crate::unistd::realpath(path, resolved)
}

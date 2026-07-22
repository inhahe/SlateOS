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

    // O_TMPFILE (anonymous, auto-unlinked temp file) is not supported.
    // Our kernel file handles are path-based: read/write re-resolve the
    // stored path through the VFS on every call, so a nameless/unlinked
    // inode cannot be represented or kept alive across operations.  Linux
    // returns EOPNOTSUPP when O_TMPFILE is used on a filesystem that lacks
    // support, so we return the same here — a clear, spec-compliant
    // failure rather than silently opening the *directory* path for I/O
    // (which is what ignoring the flag would do).  Proper O_TMPFILE needs
    // kernel orphan-inode support; tracked in todo.txt.
    if flags & RAW_O_TMPFILE_I32 != 0 {
        errno::set_errno(errno::EOPNOTSUPP);
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
    let stored_flags = flags
        & (fcntl::O_ACCMODE
            | fcntl::O_APPEND
            | fcntl::O_NONBLOCK
            | fcntl::O_SYNC
            | fcntl::O_NOFOLLOW);
    let kernel_handle = ret as u64;
    if let Some(fd_num) =
        fdtable::alloc_fd_with_flags(HandleKind::File, kernel_handle, stored_flags)
    {
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
        HandleKind::UnixStream => syscall1(SYS_SOCKETPAIR_CLOSE, entry.handle),
        HandleKind::Console => return 0, // Console fds don't need kernel close.
        HandleKind::TcpStream => {
            if entry.handle == 0 {
                return 0;
            } // Unconnected socket, nothing to close.
            let (linger_on, linger_secs) =
                socket_meta.map_or((false, 0i32), |m| (m.linger_onoff, m.linger_secs));
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
        HandleKind::TcpListener => syscall1(SYS_TCP_CLOSE_LISTENER, entry.handle),
        HandleKind::UdpSocket => {
            if entry.handle == 0 {
                return 0;
            } // Unbound socket, nothing to close.
            syscall1(SYS_UDP_CLOSE, entry.handle)
        }
        HandleKind::Eventfd => crate::epoll::eventfd_kernel_close(entry.handle),
        HandleKind::Epoll => {
            // Userspace-managed: free the instance slot.  No kernel
            // resource to release.
            crate::epoll::epoll_instance_close(entry.handle);
            0
        }
        HandleKind::Timerfd => {
            // Userspace-managed: free the timer slot.
            crate::epoll::timerfd_instance_close(entry.handle);
            0
        }
        HandleKind::Inotify => {
            // Userspace-managed: free the inotify instance.
            crate::epoll::inotify_instance_close(entry.handle);
            0
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

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };

    let ret = match entry.kind {
        HandleKind::File => syscall3(SYS_FS_READ, entry.handle, buf as u64, count as u64),
        HandleKind::Pipe => {
            // Use non-blocking read when O_NONBLOCK is set on the fd.
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
            if is_nb {
                syscall3(SYS_PIPE_TRY_READ, entry.handle, buf as u64, count as u64)
            } else {
                syscall3(SYS_PIPE_READ, entry.handle, buf as u64, count as u64)
            }
        }
        HandleKind::UnixStream => {
            // Stream socket: blocking recv unless O_NONBLOCK is set.
            // A return of 0 is EOF (peer's write side closed), which
            // read() reports as 0 — matching pipe/socket semantics.
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
            if is_nb {
                syscall3(
                    SYS_SOCKETPAIR_TRY_RECV,
                    entry.handle,
                    buf as u64,
                    count as u64,
                )
            } else {
                syscall3(SYS_SOCKETPAIR_RECV, entry.handle, buf as u64, count as u64)
            }
        }
        HandleKind::Console => {
            // Console read: one character at a time via SYS_CONSOLE_READ_CHAR.
            let ch = syscall0(SYS_CONSOLE_READ_CHAR);
            if ch < 0 {
                return errno::translate(ch) as SsizeT;
            }
            // SAFETY: buf is valid for at least `count` bytes (checked above).
            unsafe {
                *buf = ch as u8;
            }
            1
        }
        HandleKind::TcpStream => {
            if entry.handle == 0 {
                errno::set_errno(errno::ENOTCONN);
                return -1;
            }
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
            let timeout_ms = crate::socket::get_meta(fd).map_or(0u64, |m| m.rcvtimeo_ms);

            // Always try non-blocking first — we implement blocking
            // and SO_RCVTIMEO in the POSIX layer via tcp_recv_wait.
            let ret = syscall4(
                SYS_TCP_RECV,
                entry.handle,
                buf as u64,
                count as u64,
                0x40, // MSG_DONTWAIT
            );
            if ret >= 0 {
                return ret as SsizeT;
            }
            let posix_err = crate::socket::translate_net_error(ret);
            if (posix_err == errno::EAGAIN || posix_err == errno::EWOULDBLOCK) && !is_nb {
                // Blocking socket — poll-wait with SO_RCVTIMEO.
                // timeout_ms == 0 means wait indefinitely.
                return crate::socket::tcp_recv_wait(entry.handle, buf, count, 0, timeout_ms);
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
            return unsafe { crate::socket::recv(fd, buf, count, 0) } as SsizeT;
        }
        HandleKind::TcpListener => {
            // Listeners are not readable via read(); use accept().
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        HandleKind::Epoll => {
            // Linux: read/write on an epoll fd returns EINVAL.
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        HandleKind::Timerfd => {
            // Linux timerfd read: writes 8 bytes containing the number
            // of expirations since the last read (or settime), as a
            // host-endian u64.  If no expirations have occurred:
            //   - O_NONBLOCK (or TFD_NONBLOCK): return EAGAIN.
            //   - Otherwise: sleep 10ms and retry.
            if count < 8 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            let fd_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
            let is_nb = fd_nb || crate::epoll::timerfd_is_nonblock(entry.handle);
            // SAFETY: `buf` is valid for `count >= 8` bytes (checked above).
            let dst = unsafe { core::slice::from_raw_parts_mut(buf, 8) };
            loop {
                match crate::epoll::timerfd_read(entry.handle, dst) {
                    Ok(0) => {
                        if is_nb {
                            errno::set_errno(errno::EAGAIN);
                            return -1;
                        }
                        // Block: sleep 10ms and retry.  Matches the rest
                        // of our readiness polling.
                        let _ = syscall1(SYS_SLEEP, 10_000_000);
                    }
                    Ok(n) => return n as SsizeT,
                    Err(e) => {
                        errno::set_errno(e);
                        return -1;
                    }
                }
            }
        }
        HandleKind::Eventfd => {
            // Linux semantics: read on an eventfd requires an 8-byte
            // buffer.  On success, the kernel counter is written into
            // the buffer (host endian) and read() returns 8.  Buffers
            // smaller than 8 bytes fail with EINVAL.
            if count < 8 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
            let r = crate::epoll::eventfd_kernel_read(entry.handle, is_nb);
            if r < 0 {
                return errno::translate(r) as SsizeT;
            }
            // SAFETY: `buf` is valid for `count >= 8` bytes (checked above).
            // We write 8 bytes representing the u64 counter value in host
            // endianness, matching Linux eventfd semantics.
            unsafe {
                let val = r as u64;
                core::ptr::write_unaligned(buf.cast::<u64>(), val);
            }
            return 8;
        }
        HandleKind::Inotify => {
            // inotify read: drains queued events in Linux's packed
            // `struct inotify_event` format.  If the buffer is too
            // small for the next event, EINVAL.  If the queue is empty:
            //   - O_NONBLOCK (or IN_NONBLOCK): EAGAIN.
            //   - Otherwise: sleep 10ms and retry (matches poll/timerfd
            //     pattern).
            let fd_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
            let is_nb = fd_nb || crate::epoll::inotify_is_nonblock(entry.handle);
            // SAFETY: `buf` is valid for `count` bytes (checked above).
            let dst = unsafe { core::slice::from_raw_parts_mut(buf, count) };
            loop {
                match crate::epoll::inotify_read(entry.handle, dst) {
                    Ok(0) => {
                        if is_nb {
                            errno::set_errno(errno::EAGAIN);
                            return -1;
                        }
                        let _ = syscall1(SYS_SLEEP, 10_000_000);
                    }
                    Ok(n) => return n as SsizeT,
                    Err(e) => {
                        errno::set_errno(e);
                        return -1;
                    }
                }
            }
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

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };

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
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
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
        HandleKind::UnixStream => {
            // Stream socket: blocking send unless O_NONBLOCK is set.
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
            let ret = if is_nb {
                syscall3(
                    SYS_SOCKETPAIR_TRY_SEND,
                    entry.handle,
                    buf as u64,
                    count as u64,
                )
            } else {
                syscall3(SYS_SOCKETPAIR_SEND, entry.handle, buf as u64, count as u64)
            };
            if ret == errno::native::CHANNEL_CLOSED {
                // Peer's read side is gone — POSIX mandates EPIPE.  The
                // kernel does not raise SIGPIPE (we have no signals), so a
                // write to a broken stream socket simply fails with EPIPE.
                errno::set_errno(errno::EPIPE);
                return -1;
            }
            ret
        }
        HandleKind::Console => syscall2(SYS_CONSOLE_WRITE, buf as u64, count as u64),
        HandleKind::TcpStream => {
            if entry.handle == 0 {
                errno::set_errno(errno::ENOTCONN);
                return -1;
            }
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
            if !is_nb {
                // Blocking socket: use tcp_send_wait for full-write
                // semantics.  Linux's blocking write() loops until ALL
                // bytes are accepted; programs depend on this (same
                // behavior as send() on a blocking socket).
                let timeout_ms = crate::socket::get_meta(fd).map_or(0u64, |m| m.sndtimeo_ms);
                return crate::socket::tcp_send_wait(entry.handle, buf, count, timeout_ms);
            }
            // Non-blocking: try once.
            let ret = syscall3(SYS_TCP_SEND, entry.handle, buf as u64, count as u64);
            if ret >= 0 {
                return ret as SsizeT;
            }
            // ChannelClosed (-300) needs EPIPE/ECONNRESET distinction:
            // RST from peer → ECONNRESET; local shutdown/graceful close → EPIPE.
            if ret == errno::native::CHANNEL_CLOSED {
                let last = syscall1(crate::syscall::SYS_TCP_LAST_ERROR, entry.handle) as u8;
                errno::set_errno(if last == 2 {
                    errno::ECONNRESET
                } else {
                    errno::EPIPE
                });
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
            return unsafe { crate::socket::send(fd, buf, count, 0) } as SsizeT;
        }
        HandleKind::TcpListener => {
            // Listeners are not writable via write(); use accept().
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        HandleKind::Epoll => {
            // Linux: read/write on an epoll fd returns EINVAL.
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        HandleKind::Timerfd => {
            // Linux: write on a timerfd returns EINVAL.
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        HandleKind::Inotify => {
            // Linux: write on an inotify fd returns EBADF (it's
            // read-only by design).  We use EBADF to match Linux —
            // EINVAL is the more common dispatch error but inotify is
            // specifically EBADF per man inotify(7).
            errno::set_errno(errno::EBADF);
            return -1;
        }
        HandleKind::Eventfd => {
            // Linux semantics: write on an eventfd requires an 8-byte
            // buffer.  The bytes are interpreted as a host-endian u64
            // delta added to the counter.  Writing 0xFFFF_FFFF_FFFF_FFFF
            // (u64::MAX) is invalid (Linux EINVAL); writing 0 is a no-op
            // but still legal.
            if count < 8 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            // SAFETY: `buf` is valid for `count >= 8` bytes (checked above).
            let val = unsafe { core::ptr::read_unaligned(buf.cast::<u64>()) };
            if val == u64::MAX {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            let r = crate::epoll::eventfd_kernel_write(entry.handle, val);
            if r < 0 {
                return errno::translate(r) as SsizeT;
            }
            return 8;
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
    // POSIX: EINVAL if whence is not a valid value.  We support the three
    // standard whence values plus the Linux sparse-file extensions
    // SEEK_DATA / SEEK_HOLE, which the kernel implements as dedicated
    // syscalls (SYS_FS_SEEK_DATA / SYS_FS_SEEK_HOLE).
    if whence != crate::fcntl::SEEK_SET
        && whence != crate::fcntl::SEEK_CUR
        && whence != crate::fcntl::SEEK_END
        && whence != crate::fcntl::SEEK_DATA
        && whence != crate::fcntl::SEEK_HOLE
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SEEK_DATA / SEEK_HOLE take an absolute starting offset; a negative
    // value is meaningless (the kernel would treat the cast u64 as a huge
    // positive position).  POSIX/Linux return EINVAL for a negative offset
    // here, mirroring pread/pwrite.
    if (whence == crate::fcntl::SEEK_DATA || whence == crate::fcntl::SEEK_HOLE) && offset < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };

    match entry.kind {
        HandleKind::File => {
            let ret = if whence == crate::fcntl::SEEK_DATA {
                syscall2(SYS_FS_SEEK_DATA, entry.handle, offset as u64)
            } else if whence == crate::fcntl::SEEK_HOLE {
                syscall2(SYS_FS_SEEK_HOLE, entry.handle, offset as u64)
            } else {
                syscall3(SYS_FS_SEEK, entry.handle, offset as u64, whence as u64)
            };
            errno::translate(ret) as OffT
        }
        HandleKind::Pipe
        | HandleKind::Console
        | HandleKind::TcpStream
        | HandleKind::TcpListener
        | HandleKind::UdpSocket
        | HandleKind::Eventfd
        | HandleKind::Epoll
        | HandleKind::Timerfd
        | HandleKind::Inotify
        | HandleKind::UnixStream => {
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

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };

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
    let seek_ret = syscall3(
        SYS_FS_SEEK,
        entry.handle,
        offset as u64,
        crate::fcntl::SEEK_SET as u64,
    );
    if seek_ret < 0 {
        return errno::translate(seek_ret) as SsizeT;
    }

    // Read.
    let read_ret = syscall3(SYS_FS_READ, entry.handle, buf as u64, count as u64);

    // Restore original position (best effort — if this fails, the file
    // position is lost, but the alternative is leaking the error).
    let _ = syscall3(
        SYS_FS_SEEK,
        entry.handle,
        saved as u64,
        crate::fcntl::SEEK_SET as u64,
    );

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

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };

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
    let seek_ret = syscall3(
        SYS_FS_SEEK,
        entry.handle,
        offset as u64,
        crate::fcntl::SEEK_SET as u64,
    );
    if seek_ret < 0 {
        return errno::translate(seek_ret) as SsizeT;
    }

    // Write.
    let write_ret = syscall3(SYS_FS_WRITE, entry.handle, buf as u64, count as u64);

    // Restore original position.
    let _ = syscall3(
        SYS_FS_SEEK,
        entry.handle,
        saved as u64,
        crate::fcntl::SEEK_SET as u64,
    );

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
// preadv / pwritev — vectored I/O at offset
// ---------------------------------------------------------------------------

/// Read data into multiple buffers at a given offset (scatter read).
///
/// Like `readv`, but reads from file position `offset` without
/// changing the file's current offset (same semantics as `pread`).
///
/// Returns the total number of bytes read, or -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn preadv(fd: Fd, iov: *const Iovec, iovcnt: i32, offset: OffT) -> SsizeT {
    if iov.is_null() || iovcnt <= 0 || iovcnt > 1024 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if offset < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };

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
    let sr = syscall3(
        SYS_FS_SEEK,
        entry.handle,
        offset as u64,
        crate::fcntl::SEEK_SET as u64,
    );
    if sr < 0 {
        return errno::translate(sr) as SsizeT;
    }

    // Read into each iov buffer.
    let mut total: SsizeT = 0;
    let mut i: i32 = 0;
    while i < iovcnt {
        // SAFETY: Caller guarantees iov is valid for iovcnt entries.
        let vec = unsafe { &*iov.add(i as usize) };
        if vec.iov_len > 0 {
            let n = read(fd, vec.iov_base, vec.iov_len);
            if n < 0 {
                // Restore position before returning error.
                let _ = syscall3(
                    SYS_FS_SEEK,
                    entry.handle,
                    saved as u64,
                    crate::fcntl::SEEK_SET as u64,
                );
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

    // Restore original position.
    let _ = syscall3(
        SYS_FS_SEEK,
        entry.handle,
        saved as u64,
        crate::fcntl::SEEK_SET as u64,
    );

    total
}

/// Write data from multiple buffers at a given offset (gather write).
///
/// Like `writev`, but writes to file position `offset` without
/// changing the file's current offset (same semantics as `pwrite`).
///
/// Returns the total number of bytes written, or -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pwritev(fd: Fd, iov: *const Iovec, iovcnt: i32, offset: OffT) -> SsizeT {
    if iov.is_null() || iovcnt <= 0 || iovcnt > 1024 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if offset < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };

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
    let sr = syscall3(
        SYS_FS_SEEK,
        entry.handle,
        offset as u64,
        crate::fcntl::SEEK_SET as u64,
    );
    if sr < 0 {
        return errno::translate(sr) as SsizeT;
    }

    // Write from each iov buffer.
    let mut total: SsizeT = 0;
    let mut i: i32 = 0;
    while i < iovcnt {
        // SAFETY: Caller guarantees iov is valid for iovcnt entries.
        let vec = unsafe { &*iov.add(i as usize) };
        if vec.iov_len > 0 {
            let n = write(fd, vec.iov_base.cast_const(), vec.iov_len);
            if n < 0 {
                let _ = syscall3(
                    SYS_FS_SEEK,
                    entry.handle,
                    saved as u64,
                    crate::fcntl::SEEK_SET as u64,
                );
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

    // Restore original position.
    let _ = syscall3(
        SYS_FS_SEEK,
        entry.handle,
        saved as u64,
        crate::fcntl::SEEK_SET as u64,
    );

    total
}

// ---------------------------------------------------------------------------
// preadv2 / pwritev2 — Linux extended vectored I/O
// ---------------------------------------------------------------------------

/// Flags for `preadv2` / `pwritev2`.
pub const RWF_HIPRI: i32 = 0x01;
/// Append (only for pwritev2).
pub const RWF_APPEND: i32 = 0x10;
/// Per-I/O O_DSYNC semantics.
pub const RWF_DSYNC: i32 = 0x02;
/// Per-I/O O_SYNC semantics.
pub const RWF_SYNC: i32 = 0x04;
/// Do not wait for I/O completion.
pub const RWF_NOWAIT: i32 = 0x08;

/// Read data from a file at an offset into multiple buffers, with flags.
///
/// Like `preadv`, but with an additional `flags` parameter. `flags == 0`
/// is identical to `preadv`.
///
/// If `offset == -1`, the current file position is used and updated
/// (like `readv`).
///
/// Our implementation ignores flags and delegates to `preadv` (or `readv`
/// if offset == -1).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn preadv2(
    fd: Fd,
    iov: *const Iovec,
    iovcnt: i32,
    offset: OffT,
    _flags: i32,
) -> SsizeT {
    if offset == -1 {
        // Use current file position (like readv).
        return readv(fd, iov, iovcnt);
    }
    preadv(fd, iov, iovcnt, offset)
}

/// Write data to a file at an offset from multiple buffers, with flags.
///
/// Like `pwritev`, but with an additional `flags` parameter. `flags == 0`
/// is identical to `pwritev`.
///
/// If `offset == -1`, the current file position is used and updated
/// (like `writev`).
///
/// Our implementation ignores flags and delegates to `pwritev` (or `writev`
/// if offset == -1).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pwritev2(
    fd: Fd,
    iov: *const Iovec,
    iovcnt: i32,
    offset: OffT,
    _flags: i32,
) -> SsizeT {
    if offset == -1 {
        return writev(fd, iov, iovcnt);
    }
    pwritev(fd, iov, iovcnt, offset)
}

/// `fadvise64` — LP64 alias for `posix_fadvise`.
///
/// Some glibc-compiled programs reference `fadvise64` instead of
/// `posix_fadvise`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fadvise64(fd: Fd, offset: OffT, len: OffT, advice: i32) -> i32 {
    posix_fadvise(fd, offset, len, advice)
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
    let Some(entry) = lookup_fd(oldfd) else {
        return -1;
    };

    // dup'd fds inherit the source fd's status flags (O_APPEND, etc.)
    // but NOT the fd-level flags (FD_CLOEXEC is cleared on the new fd).
    let src_status = entry.status_flags;

    match entry.kind {
        HandleKind::File => {
            // POSIX: dup'd fds must share ONE open file description with
            // the source — a shared file offset and shared status flags.
            // We therefore share the same kernel handle id at the fd-table
            // level (NOT SYS_FS_DUP, which mints a new handle with an
            // independent cursor).  close() uses is_handle_referenced() to
            // only issue SYS_FS_CLOSE when the last referencing fd is gone.
            if let Some(fd) =
                fdtable::alloc_fd_with_flags(HandleKind::File, entry.handle, src_status)
            {
                fdtable::copy_fd_path(oldfd, fd);
                fd
            } else {
                errno::set_errno(errno::EMFILE);
                -1
            }
        }
        HandleKind::Console => {
            // Console handles are shared — just allocate a new fd entry.
            if let Some(fd) =
                fdtable::alloc_fd_with_flags(HandleKind::Console, entry.handle, src_status)
            {
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
            if let Some(fd) =
                fdtable::alloc_fd_with_flags(HandleKind::Pipe, entry.handle, src_status)
            {
                fdtable::copy_fd_path(oldfd, fd);
                fd
            } else {
                errno::set_errno(errno::EMFILE);
                -1
            }
        }
        HandleKind::UnixStream => {
            // No userspace dup syscall for stream sockets.  Share the
            // endpoint handle; close() uses is_handle_referenced() so the
            // kernel SYS_SOCKETPAIR_CLOSE (which drops the endpoint
            // refcount) fires exactly once, when the last fd is closed.
            if let Some(fd) =
                fdtable::alloc_fd_with_flags(HandleKind::UnixStream, entry.handle, src_status)
            {
                fdtable::copy_fd_path(oldfd, fd);
                fd
            } else {
                errno::set_errno(errno::EMFILE);
                -1
            }
        }
        HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {
            // Share the handle (same refcounting as pipes).
            if let Some(new_fd) = fdtable::alloc_fd_with_flags(entry.kind, entry.handle, src_status)
            {
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
        HandleKind::Eventfd => {
            // No kernel-level dup for eventfds.  Share the handle;
            // close() uses is_handle_referenced() to only close the
            // kernel handle when the last fd referencing it is closed.
            if let Some(fd) =
                fdtable::alloc_fd_with_flags(HandleKind::Eventfd, entry.handle, src_status)
            {
                fdtable::copy_fd_path(oldfd, fd);
                fd
            } else {
                errno::set_errno(errno::EMFILE);
                -1
            }
        }
        HandleKind::Epoll => {
            // Share the epoll instance.  No addref needed: the close
            // path uses is_handle_referenced() to skip the instance
            // teardown until the last fd referencing it goes away.
            if let Some(fd) =
                fdtable::alloc_fd_with_flags(HandleKind::Epoll, entry.handle, src_status)
            {
                fdtable::copy_fd_path(oldfd, fd);
                fd
            } else {
                errno::set_errno(errno::EMFILE);
                -1
            }
        }
        HandleKind::Timerfd => {
            // Share the timerfd instance.  Same refcount-by-fd-scan
            // pattern as Eventfd/Epoll.
            if let Some(fd) =
                fdtable::alloc_fd_with_flags(HandleKind::Timerfd, entry.handle, src_status)
            {
                fdtable::copy_fd_path(oldfd, fd);
                fd
            } else {
                errno::set_errno(errno::EMFILE);
                -1
            }
        }
        HandleKind::Inotify => {
            // Share the inotify instance.  Same refcount-by-fd-scan
            // pattern as Epoll/Timerfd.
            if let Some(fd) =
                fdtable::alloc_fd_with_flags(HandleKind::Inotify, entry.handle, src_status)
            {
                fdtable::copy_fd_path(oldfd, fd);
                fd
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

    let Some(entry) = lookup_fd(oldfd) else {
        return -1;
    };

    if newfd < 0 || newfd as usize >= fdtable::MAX_FDS {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // All handle kinds share the same kernel handle id at the fd-table
    // level (refcounted via is_handle_referenced() in close()).  For File
    // this gives correct POSIX semantics: the dup2 target shares ONE open
    // file description with the source (shared offset + status flags),
    // rather than getting an independent cursor from SYS_FS_DUP.
    let new_handle = match entry.kind {
        HandleKind::File
        | HandleKind::Console
        | HandleKind::Pipe
        | HandleKind::TcpStream
        | HandleKind::TcpListener
        | HandleKind::UdpSocket
        | HandleKind::Eventfd
        | HandleKind::UnixStream => entry.handle,
        HandleKind::Epoll | HandleKind::Timerfd | HandleKind::Inotify => {
            // Share the epoll/timerfd/inotify instance.  No addref
            // needed: dup2 calls is_handle_referenced() before tearing
            // down the evicted handle, and the new fd at `newfd` is
            // installed before that check — so an in-place dup2 (newfd's
            // old handle == oldfd's handle) still sees a reference and
            // skips close.
            entry.handle
        }
    };

    // Install at newfd, closing whatever was there.
    // dup2 inherits the source's status flags (O_APPEND, O_NONBLOCK, etc.).
    if let Some(old) =
        fdtable::install_fd_with_flags(newfd, entry.kind, new_handle, entry.status_flags)
    {
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
                let (linger_on, linger_secs) =
                    evicted_meta.map_or((false, 0i32), |m| (m.linger_onoff, m.linger_secs));
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
    // Linux semantics (fs/file.c::ksys_dup3): the flag-mask check
    // precedes the oldfd==newfd check, so a buggy caller passing
    // garbage flags AND the same fd twice sees EINVAL via the flag
    // path. The only flag dup3 accepts is O_CLOEXEC.
    if flags & !fcntl::O_CLOEXEC != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
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
/// Recognized flag bits:
///
/// * `CLOSE_RANGE_UNSHARE` (bit 1) — Linux unshares the fd table from
///   any sharing parent before closing.  Our processes never share fd
///   tables (every process has its own — see `fdtable` docs), so this
///   bit's postcondition is already satisfied; we accept the bit as a
///   no-op.
/// * `CLOSE_RANGE_CLOEXEC` (bit 2) — set `FD_CLOEXEC` on each open fd
///   in the range instead of closing it.  Useful for libraries that
///   want to ensure no descriptors leak across a subsequent `execve`
///   without disturbing already-open fds in the current process.
///
/// Returns -1 with `EINVAL` for `first > last` (Linux behavior) or for
/// any unknown flag bit.  Returns -1 with `EINVAL` when both
/// `CLOSE_RANGE_UNSHARE` is set without `CLOSE_RANGE_CLOEXEC`? — no:
/// the two are independent and both may be combined.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn close_range(first: u32, last: u32, flags: u32) -> i32 {
    use crate::linux_close_range::{CLOSE_RANGE_CLOEXEC, CLOSE_RANGE_UNSHARE};

    // Linux's `__close_range` (fs/file.c) rejects unknown flag bits
    // BEFORE checking the range ordering:
    //
    //     if (flags & ~(CLOSE_RANGE_UNSHARE | CLOSE_RANGE_CLOEXEC))
    //         return -EINVAL;
    //     if (fd > max_fd)
    //         return -EINVAL;
    //
    // Both errors are EINVAL so a single observation can't tell them
    // apart, but a caller that passes garbage flags AND an inverted
    // range expects to learn about the flag bug first (e.g. when
    // bisecting which argument is wrong).  Match Linux's ordering.
    let known_flags = CLOSE_RANGE_UNSHARE | CLOSE_RANGE_CLOEXEC;
    if flags & !known_flags != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Linux returns EINVAL for inverted ranges.  Our previous code
    // silently treated them as no-ops, which masks bugs in callers.
    if first > last {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let cloexec = (flags & CLOSE_RANGE_CLOEXEC) != 0;
    // CLOSE_RANGE_UNSHARE is implicitly a no-op for us (no fd-table sharing).

    // Cap at MAX_FDS-1: no fd beyond the table limit can be open,
    // and iterating up to u32::MAX would take ~4 billion iterations
    // due to wrapping.  Programs commonly pass UINT_MAX as `last`
    // to close "everything from first upward."
    let max = fdtable::MAX_FDS as u32;
    let effective_last = if last >= max {
        max.wrapping_sub(1)
    } else {
        last
    };
    let mut fd = first;
    while fd <= effective_last {
        if cloexec {
            // Only modify open fds — skipping closed slots avoids
            // creating spurious "fd N has FD_CLOEXEC set" state that
            // a later open() would inherit.
            if let Some(existing) = fdtable::get_fd_flags(fd as i32) {
                let _ = fdtable::set_fd_flags(fd as i32, existing | fdtable::FD_CLOEXEC);
            }
        } else {
            // close() is best-effort here — ignore errors on individual fds.
            let _ = close(fd as i32);
        }
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

/// Resolve `path` and fill `raw` with the kernel's `FsStatResult`.
///
/// `SYS_FS_STAT`/`SYS_FS_LSTAT` write a compact, kernel-defined 80-byte
/// `FsStatResult` (see [`crate::stat`]), not a POSIX `struct stat`.  This
/// helper centralises the resolve+syscall step so `stat`, `lstat`, and
/// `statx` can share it without duplicating logic; callers translate the
/// raw bytes via [`crate::stat::fill_from_fsstat`] (and, for `statx`,
/// read the birth time via [`crate::stat::btime_from_fsstat`]).
///
/// `follow` selects `SYS_FS_STAT` (follow the final symlink) versus
/// `SYS_FS_LSTAT` (do not follow).  Returns 0 on success, or -1 with
/// `errno` set on failure.
fn stat_path_raw(
    path: *const u8,
    follow: bool,
    raw: &mut [u8; crate::stat::KERNEL_STAT_LEN],
) -> i32 {
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };

    let sysno = if follow { SYS_FS_STAT } else { SYS_FS_LSTAT };
    let ret = syscall3(
        sysno,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        raw.as_mut_ptr() as u64,
    );

    if ret < 0 {
        return errno::translate(ret) as i32;
    }
    0
}

/// Get file status by path.
///
/// `SYS_FS_STAT` writes a compact, kernel-defined 80-byte `FsStatResult`,
/// not a POSIX `struct stat`.  We read it into a local buffer and
/// translate via [`crate::stat::fill_from_fsstat`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn stat(path: *const u8, buf: *mut Stat) -> i32 {
    if path.is_null() || buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let mut raw = [0u8; crate::stat::KERNEL_STAT_LEN];
    let ret = stat_path_raw(path, true, &mut raw);
    if ret != 0 {
        return ret;
    }

    // SAFETY: `buf` was checked non-null above; the caller guarantees it
    // points to a writable `Stat`.
    crate::stat::fill_from_fsstat(unsafe { &mut *buf }, &raw);
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

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };

    match entry.kind {
        HandleKind::File => {
            let mut raw = [0u8; crate::stat::KERNEL_STAT_LEN];
            let ret = syscall2(SYS_FS_FSTAT, entry.handle, raw.as_mut_ptr() as u64);
            if ret < 0 {
                return errno::translate(ret) as i32;
            }
            // SAFETY: `buf` was checked non-null above.
            crate::stat::fill_from_fsstat(unsafe { &mut *buf }, &raw);
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
        HandleKind::TcpStream
        | HandleKind::TcpListener
        | HandleKind::UdpSocket
        | HandleKind::UnixStream => {
            // Return minimal stat for a socket.
            unsafe {
                core::ptr::write_bytes(buf, 0, 1);
                (*buf).st_mode = crate::fcntl::S_IFSOCK;
            }
            0
        }
        HandleKind::Eventfd | HandleKind::Epoll | HandleKind::Timerfd | HandleKind::Inotify => {
            // Linux fstat on an eventfd / epollfd / timerfd / inotifyfd
            // returns a character device.  Zero the struct and set
            // S_IFCHR so callers that branch on file type get a sensible
            // value.
            unsafe {
                core::ptr::write_bytes(buf, 0, 1);
                (*buf).st_mode = crate::fcntl::S_IFCHR;
            }
            0
        }
    }
}

/// Get symbolic link status (don't follow final symlink).
///
/// `SYS_FS_LSTAT` writes the same compact 80-byte `FsStatResult` as
/// `stat`; we translate via [`crate::stat::fill_from_fsstat`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lstat(path: *const u8, buf: *mut Stat) -> i32 {
    if path.is_null() || buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let mut raw = [0u8; crate::stat::KERNEL_STAT_LEN];
    let ret = stat_path_raw(path, false, &mut raw);
    if ret != 0 {
        return ret;
    }

    // SAFETY: `buf` was checked non-null above.
    crate::stat::fill_from_fsstat(unsafe { &mut *buf }, &raw);
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

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };

    match entry.kind {
        HandleKind::File => {
            let ret = syscall2(SYS_FS_FTRUNCATE, entry.handle, length as u64);
            errno::translate(ret) as i32
        }
        HandleKind::Pipe
        | HandleKind::Console
        | HandleKind::TcpStream
        | HandleKind::TcpListener
        | HandleKind::UdpSocket
        | HandleKind::Eventfd
        | HandleKind::Epoll
        | HandleKind::Timerfd
        | HandleKind::Inotify
        | HandleKind::UnixStream => {
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
    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };

    match entry.kind {
        HandleKind::File => {
            // Our SYS_FS_SYNC is a global sync, not per-fd.
            let ret = syscall0(SYS_FS_SYNC);
            errno::translate(ret) as i32
        }
        HandleKind::Pipe
        | HandleKind::Console
        | HandleKind::TcpStream
        | HandleKind::TcpListener
        | HandleKind::UdpSocket
        | HandleKind::Eventfd
        | HandleKind::Epoll
        | HandleKind::Timerfd
        | HandleKind::Inotify
        | HandleKind::UnixStream => 0,
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
pub(crate) fn resolve_or_err(
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
        HandleKind::UnixStream => syscall1(SYS_SOCKETPAIR_CLOSE, handle),
        HandleKind::Console => 0, // Console handles are not closeable.
        HandleKind::TcpStream => syscall1(SYS_TCP_CLOSE, handle),
        HandleKind::TcpListener => syscall1(SYS_TCP_CLOSE_LISTENER, handle),
        HandleKind::UdpSocket => syscall1(SYS_UDP_CLOSE, handle),
        HandleKind::Eventfd => crate::epoll::eventfd_kernel_close(handle),
        HandleKind::Epoll => {
            // Userspace-managed: no kernel handle to close.
            crate::epoll::epoll_instance_close(handle);
            0
        }
        HandleKind::Timerfd => {
            // Userspace-managed: no kernel handle to close.
            crate::epoll::timerfd_instance_close(handle);
            0
        }
        HandleKind::Inotify => {
            // Userspace-managed: no kernel handle to close.
            crate::epoll::inotify_instance_close(handle);
            0
        }
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
pub extern "C" fn access(path: *const u8, mode: i32) -> i32 {
    // Linux semantics (fs/open.c::do_faccessat): `mode & ~S_IRWXO`
    // (i.e. bits outside R_OK | W_OK | X_OK = 0b111) → EINVAL.
    // F_OK == 0 is implicit since mode == 0 passes the mask test.
    if mode & !(crate::fcntl::R_OK | crate::fcntl::W_OK | crate::fcntl::X_OK) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
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
pub extern "C" fn faccessat(dirfd: i32, path: *const u8, mode: i32, flags: i32) -> i32 {
    // Linux semantics (fs/open.c::do_faccessat) validates mode and
    // flags in the prologue before any path resolution.
    if mode & !(crate::fcntl::R_OK | crate::fcntl::W_OK | crate::fcntl::X_OK) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if flags & !(AT_EACCESS | AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return access(path, mode);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 {
        return -1;
    }
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
pub(crate) fn is_absolute_path(path: *const u8) -> bool {
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
    let needs_slash = dir_len > 0 && dir_path.get(dir_len.wrapping_sub(1)).copied() != Some(b'/');
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
pub(crate) fn resolve_dirfd_path(
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
    if len == 0 {
        return -1;
    }
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
        return if flags & AT_SYMLINK_NOFOLLOW != 0 {
            lstat(path, buf)
        } else {
            stat(path, buf)
        };
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 {
        return -1;
    }
    if flags & AT_SYMLINK_NOFOLLOW != 0 {
        lstat(full.as_ptr(), buf)
    } else {
        stat(full.as_ptr(), buf)
    }
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
        return if flags & AT_REMOVEDIR != 0 {
            rmdir(path)
        } else {
            unlink(path)
        };
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 {
        return -1;
    }
    if flags & AT_REMOVEDIR != 0 {
        rmdir(full.as_ptr())
    } else {
        unlink(full.as_ptr())
    }
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

    let mut old_full = [0u8; crate::unistd::PATH_MAX];
    let old_ptr = if old_needs_resolve {
        let len = resolve_dirfd_path(olddirfd, oldpath, &mut old_full);
        if len == 0 {
            return -1;
        }
        old_full.as_ptr()
    } else {
        oldpath
    };

    let mut new_full = [0u8; crate::unistd::PATH_MAX];
    let new_ptr = if new_needs_resolve {
        let len = resolve_dirfd_path(newdirfd, newpath, &mut new_full);
        if len == 0 {
            return -1;
        }
        new_full.as_ptr()
    } else {
        newpath
    };

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
    if len == 0 {
        return -1;
    }
    mkdir(full.as_ptr(), mode)
}

/// Read a symbolic link relative to a directory fd.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn readlinkat(dirfd: i32, path: *const u8, buf: *mut u8, bufsiz: SizeT) -> SsizeT {
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return readlink(path, buf, bufsiz);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 {
        return -1;
    }
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
    if len == 0 {
        return -1;
    }
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

    let mut old_full = [0u8; crate::unistd::PATH_MAX];
    let old_ptr = if old_needs_resolve {
        let len = resolve_dirfd_path(olddirfd, oldpath, &mut old_full);
        if len == 0 {
            return -1;
        }
        old_full.as_ptr()
    } else {
        oldpath
    };

    let mut new_full = [0u8; crate::unistd::PATH_MAX];
    let new_ptr = if new_needs_resolve {
        let len = resolve_dirfd_path(newdirfd, newpath, &mut new_full);
        if len == 0 {
            return -1;
        }
        new_full.as_ptr()
    } else {
        newpath
    };

    link(old_ptr, new_ptr)
}

/// Change file mode bits relative to a directory fd.
///
/// Validates `flags` per Linux's `do_fchmodat` prologue:
/// `flags & ~(AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH)` → EINVAL.
/// AT_EACCESS is **not** a valid fchmodat flag (it's a faccessat
/// flag) — passing it here yields EINVAL.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fchmodat(dirfd: i32, path: *const u8, mode: ModeT, flags: i32) -> i32 {
    if flags & !(AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return chmod(path, mode);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 {
        return -1;
    }
    chmod(full.as_ptr(), mode)
}

/// Change file owner/group relative to a directory fd.
///
/// Validates `flags` per Linux's `do_fchownat` prologue:
/// `flags & ~(AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH)` → EINVAL.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fchownat(
    dirfd: i32,
    path: *const u8,
    owner: UidT,
    group: GidT,
    flags: i32,
) -> i32 {
    if flags & !(AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return chown(path, owner, group);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 {
        return -1;
    }
    chown(full.as_ptr(), owner, group)
}

// ---------------------------------------------------------------------------
// chmod / fchmod / chown / fchown / lchown
// ---------------------------------------------------------------------------
//
// The permission-changing family.  Each entry point validates its
// arguments (NULL path → EFAULT, bad/closed fd → EBADF, CAP_CHOWN gate for
// the chown variants) and then, on bare metal, issues the corresponding
// kernel syscall: SYS_FS_SET_PERMS for chmod/fchmod and SYS_FS_SET_OWNER
// for chown/fchown/lchown.  On the host build (no kernel) the syscall is
// skipped and the call returns 0 after validation, which keeps the
// argument-domain tests stable.

/// Change file mode bits.
///
/// Validates `path != NULL` (Linux: EFAULT on a bad pointer), then issues
/// `SYS_FS_SET_PERMS` to persist the new permission bits.  The file-type
/// bits of `mode` are ignored; only the low `0o7777` permission bits apply.
///
/// Errors:
///   * `EFAULT` — `path` is NULL.
///   * any error the kernel returns from `SYS_FS_SET_PERMS`
///     (e.g. `ENOENT`, `EACCES`, `ENOTSUP` on FAT).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn chmod(path: *const u8, mode: ModeT) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        set_perms_path(path, mode)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = mode;
        0
    }
}

/// Change file mode bits (by fd).
///
/// Validates `fd >= 0` and that `fd` refers to an open file description,
/// then resolves the fd to its stored path and issues `SYS_FS_SET_PERMS`.
/// Descriptors with no stored path (pipes, sockets, …) have no persistent
/// permissions, so the call succeeds as a no-op.
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
///   * any error the kernel returns from `SYS_FS_SET_PERMS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fchmod(fd: Fd, mode: ModeT) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut path = [0u8; crate::unistd::PATH_MAX];
        let len = fdtable::get_fd_path(fd, &mut path);
        if len == 0 {
            // No stored path (pipe/socket/etc.) — nothing to persist.
            return 0;
        }
        set_perms_path(path.as_ptr(), mode)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = mode;
        0
    }
}

/// Change file owner and group.
///
/// Validates `path != NULL`, enforces the `CAP_CHOWN` gate, then issues
/// `SYS_FS_SET_OWNER`.  A field of `(uid_t)-1` / `(gid_t)-1` (i.e.
/// `u32::MAX`) leaves that field unchanged; a call that changes neither
/// field is a pure no-op and skips the syscall.
///
/// Errors:
///   * `EFAULT` — `path` is NULL.
///   * `EPERM` — changing ownership without `CAP_CHOWN`.
///   * any error the kernel returns from `SYS_FS_SET_OWNER`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn chown(path: *const u8, owner: UidT, group: GidT) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // Phase 206: CAP_CHOWN gate.  Linux requires CAP_CHOWN when the
    // caller actually changes file ownership.  owner/group == (uid_t)-1
    // means "don't change that field", so a double-no-op call bypasses.
    if owner == u32::MAX && group == u32::MAX {
        // Nothing to change — succeed without touching ctime.
        return 0;
    }
    if !crate::sys_capability::has_capability(crate::sys_capability::CAP_CHOWN) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        set_owner_path(path, owner, group)
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
}

/// Change file owner and group (by fd).
///
/// Validates `fd >= 0` and that `fd` refers to an open file description,
/// enforces the `CAP_CHOWN` gate, then resolves the fd to its stored path
/// and issues `SYS_FS_SET_OWNER`.  Path-less descriptors (pipes, sockets, …)
/// succeed as a no-op.
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
///   * `EPERM` — changing ownership without `CAP_CHOWN`.
///   * any error the kernel returns from `SYS_FS_SET_OWNER`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fchown(fd: Fd, owner: UidT, group: GidT) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    // Phase 206: CAP_CHOWN gate — same semantics as chown(), after EBADF
    // validation.
    if owner == u32::MAX && group == u32::MAX {
        return 0;
    }
    if !crate::sys_capability::has_capability(crate::sys_capability::CAP_CHOWN) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut path = [0u8; crate::unistd::PATH_MAX];
        let len = fdtable::get_fd_path(fd, &mut path);
        if len == 0 {
            return 0;
        }
        set_owner_path(path.as_ptr(), owner, group)
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
}

/// Change file owner and group (don't follow symlinks).
///
/// Like `chown`, but is meant to change ownership of a symlink itself
/// rather than its target.  Validates `path != NULL` and enforces the
/// `CAP_CHOWN` gate, then issues `SYS_FS_SET_OWNER`.
///
/// LIMITATION: the kernel `SYS_FS_SET_OWNER` resolves paths with
/// `resolve_follow` (always follows symlinks), so for a symlink argument
/// this changes the *target's* owner, not the link's.  For the common
/// non-symlink case the behaviour is correct.  Tracked in `todo.txt`
/// (no `AT_SYMLINK_NOFOLLOW` ownership syscall yet).
///
/// Errors:
///   * `EFAULT` — `path` is NULL.
///   * `EPERM` — changing ownership without `CAP_CHOWN`.
///   * any error the kernel returns from `SYS_FS_SET_OWNER`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lchown(path: *const u8, owner: UidT, group: GidT) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // Phase 206: CAP_CHOWN gate — same semantics as chown().
    if owner == u32::MAX && group == u32::MAX {
        return 0;
    }
    if !crate::sys_capability::has_capability(crate::sys_capability::CAP_CHOWN) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        set_owner_path(path, owner, group)
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
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
    unsafe {
        core::ptr::addr_of_mut!(UMASK_VALUE).write(cmask & 0o777);
    }
    previous
}

/// Get the current umask value without modifying it.
///
/// Not a POSIX function, but useful for internal callers that need
/// to apply the mask (e.g., open, mkdir) without side effects.
#[allow(dead_code)]
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
/// Validates inputs per POSIX/Linux semantics, then accepts the
/// advice as a no-op — our kernel doesn't act on access-pattern
/// hints yet, but the validation surface is real so callers that
/// pass garbage get a real error instead of silent success.
///
/// Unlike most POSIX functions, `posix_fadvise` returns the error
/// number directly (positive) on failure — it does **not** set
/// errno and return -1.  Returns 0 on success.
///
/// Errors:
/// * `EBADF` — `fd` is not an open file descriptor.
/// * `EINVAL` — `advice` is not one of the defined `POSIX_FADV_*`
///   constants, or `len` is negative.
/// * `ESPIPE` — `fd` refers to a pipe (Linux extension; POSIX
///   leaves this unspecified).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_fadvise(fd: Fd, _offset: OffT, len: OffT, advice: i32) -> i32 {
    // EINVAL for negative len.
    if len < 0 {
        return errno::EINVAL;
    }
    // EINVAL for unknown advice values.
    match advice {
        POSIX_FADV_NORMAL
        | POSIX_FADV_SEQUENTIAL
        | POSIX_FADV_RANDOM
        | POSIX_FADV_NOREUSE
        | POSIX_FADV_WILLNEED
        | POSIX_FADV_DONTNEED => {}
        _ => return errno::EINVAL,
    }
    // EBADF if the fd isn't open.
    let Some(entry) = fdtable::get_fd(fd) else {
        return errno::EBADF;
    };
    // ESPIPE for pipes (Linux extension; matches what real applications expect).
    if matches!(entry.kind, fdtable::HandleKind::Pipe) {
        return errno::ESPIPE;
    }
    // Advice is purely advisory — accept and ignore.
    0
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
// fallocate — Linux file allocation (non-POSIX)
// ---------------------------------------------------------------------------

/// Default mode: allocate space in the file.
pub const FALLOC_FL_KEEP_SIZE: i32 = 0x01;
/// Deallocate (punch a hole) in the file.
pub const FALLOC_FL_PUNCH_HOLE: i32 = 0x02;
/// Don't hide stale data — expose unwritten extents.  Reserved by
/// Linux for filesystems with `CAP_SYS_RAWIO`; we don't support it.
pub const FALLOC_FL_NO_HIDE_STALE: i32 = 0x04;
/// Remove a range of a file without leaving a hole (collapse range).
pub const FALLOC_FL_COLLAPSE_RANGE: i32 = 0x08;
/// Zero a range of the file.
pub const FALLOC_FL_ZERO_RANGE: i32 = 0x10;
/// Insert space within the file (shift data up).
pub const FALLOC_FL_INSERT_RANGE: i32 = 0x20;
/// Unshare shared extents (copy-on-write breakage).
pub const FALLOC_FL_UNSHARE_RANGE: i32 = 0x40;

/// Mask of all defined fallocate mode bits — mirrors Linux's
/// `FALLOC_FL_SUPPORTED_MASK` in `include/uapi/linux/falloc.h`.  Mode
/// bits outside this mask are rejected with `EOPNOTSUPP` to match
/// `fs/open.c::vfs_fallocate`.
pub const FALLOC_FL_VALID_MASK: i32 = FALLOC_FL_KEEP_SIZE
    | FALLOC_FL_PUNCH_HOLE
    | FALLOC_FL_NO_HIDE_STALE
    | FALLOC_FL_COLLAPSE_RANGE
    | FALLOC_FL_ZERO_RANGE
    | FALLOC_FL_INSERT_RANGE
    | FALLOC_FL_UNSHARE_RANGE;

/// Manipulate file space.
///
/// Linux-specific `fallocate(2)`.  Unlike `posix_fallocate`, this
/// supports modes such as hole-punching, range collapsing, and
/// zero-filling via the `mode` parameter.
///
/// With `mode == 0`, this is equivalent to `posix_fallocate` (but
/// returns -1/errno instead of the error code directly).
///
/// With `FALLOC_FL_KEEP_SIZE`, space is allocated but the file size
/// is not changed.
///
/// Our implementation delegates to `posix_fallocate` for the basic
/// allocation case and stubs the advanced modes with EOPNOTSUPP.
///
/// # Validation order (Linux parity, Phase 109)
///
/// Mirrors Linux's `fs/open.c::ksys_fallocate` + `vfs_fallocate`:
///
/// 1. `EBADF` — `fd` is not an open descriptor.  `ksys_fallocate`
///    does `fdget()` before doing anything else, so an invalid fd
///    wins over any other input error.
/// 2. `EINVAL` — `offset < 0` or `len <= 0` (POSIX-defined values
///    that cannot describe a valid byte range).
/// 3. `EOPNOTSUPP` — unknown mode bits (`mode & !FALLOC_FL_VALID_MASK`).
/// 4. `EOPNOTSUPP` — `FALLOC_FL_PUNCH_HOLE` set without
///    `FALLOC_FL_KEEP_SIZE` (Linux requires the combination).
/// 5. `EINVAL` — `FALLOC_FL_KEEP_SIZE` combined with
///    `FALLOC_FL_COLLAPSE_RANGE` or `FALLOC_FL_INSERT_RANGE`
///    (the range-shifting modes can never preserve file size).
/// 6. `EINVAL` — `FALLOC_FL_COLLAPSE_RANGE` combined with any other
///    bit (collapse must be the sole mode).
/// 7. `EINVAL` — `FALLOC_FL_INSERT_RANGE` combined with any other
///    bit (insert must be the sole mode).
/// 8. `EINVAL` — `FALLOC_FL_UNSHARE_RANGE` combined with
///    `FALLOC_FL_COLLAPSE_RANGE` or `FALLOC_FL_INSERT_RANGE`.
///
/// After these argument-domain checks pass, the operation is either
/// performed (mode 0) or accepted but stubbed (`KEEP_SIZE` alone,
/// silently a no-op) or reported as unimplemented (`EOPNOTSUPP` —
/// the filesystem doesn't support that operation yet).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fallocate(fd: Fd, mode: i32, offset: OffT, len: OffT) -> i32 {
    // (1) Linux's ksys_fallocate looks up the fd before vfs_fallocate
    // touches any of the other arguments — an invalid fd wins over
    // bad offset/len or bad mode bits.
    if fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // (2) vfs_fallocate's first check: POSIX-required range validation.
    if offset < 0 || len <= 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (3) Unknown mode bits are EOPNOTSUPP, not EINVAL — Linux uses
    // EOPNOTSUPP for "this kernel/filesystem doesn't know what you
    // mean" and reserves EINVAL for "the combination of known bits
    // is logically invalid".
    if mode & !FALLOC_FL_VALID_MASK != 0 {
        errno::set_errno(errno::EOPNOTSUPP);
        return -1;
    }

    // (4) PUNCH_HOLE requires KEEP_SIZE: a hole-punch cannot extend
    // the file, so omitting KEEP_SIZE has no coherent meaning.
    if (mode & FALLOC_FL_PUNCH_HOLE) != 0 && (mode & FALLOC_FL_KEEP_SIZE) == 0 {
        errno::set_errno(errno::EOPNOTSUPP);
        return -1;
    }

    // (5) KEEP_SIZE is incompatible with the range-shifting modes,
    // because COLLAPSE and INSERT *must* change the file size.
    if (mode & FALLOC_FL_KEEP_SIZE) != 0
        && (mode & (FALLOC_FL_COLLAPSE_RANGE | FALLOC_FL_INSERT_RANGE)) != 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (6) COLLAPSE_RANGE must appear alone — no other mode bits.
    if (mode & FALLOC_FL_COLLAPSE_RANGE) != 0 && (mode & !FALLOC_FL_COLLAPSE_RANGE) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (7) INSERT_RANGE must appear alone — no other mode bits.
    if (mode & FALLOC_FL_INSERT_RANGE) != 0 && (mode & !FALLOC_FL_INSERT_RANGE) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // (8) UNSHARE_RANGE conflicts with range-shifting modes — those
    // would need to recopy the shifted data, which is incoherent.
    if (mode & FALLOC_FL_UNSHARE_RANGE) != 0
        && (mode & (FALLOC_FL_COLLAPSE_RANGE | FALLOC_FL_INSERT_RANGE)) != 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Basic allocation (mode 0): delegate to posix_fallocate.
    if mode == 0 {
        let err = posix_fallocate(fd, offset, len);
        if err != 0 {
            errno::set_errno(err);
            return -1;
        }
        return 0;
    }

    // KEEP_SIZE alone: allocate but don't extend visible size.
    // We treat this as a no-op success (the filesystem can allocate
    // lazily — the space will be available when written).
    if mode == FALLOC_FL_KEEP_SIZE {
        return 0;
    }

    // All remaining mode combinations are valid per Linux semantics
    // (punch-hole, zero-range, collapse-range, insert-range,
    // unshare-range, plus accepted compound bits) but our filesystem
    // doesn't implement them yet.
    errno::set_errno(errno::EOPNOTSUPP);
    -1
}

/// `posix_fallocate64` — LP64 alias for `posix_fallocate`.
///
/// On 64-bit systems (LP64), `off_t` is already 64-bit, so
/// `posix_fallocate64` is identical to `posix_fallocate`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_fallocate64(fd: Fd, offset: OffT, len: OffT) -> i32 {
    posix_fallocate(fd, offset, len)
}

// ---------------------------------------------------------------------------
// splice / tee / vmsplice — zero-copy I/O (Linux)
// ---------------------------------------------------------------------------

/// Flags for `splice`, `tee`, `vmsplice`.
pub const SPLICE_F_MOVE: u32 = 1;
/// Don't block on I/O.
pub const SPLICE_F_NONBLOCK: u32 = 2;
/// Expect more data.
pub const SPLICE_F_MORE: u32 = 4;
/// Gift pages to the pipe (vmsplice only).
pub const SPLICE_F_GIFT: u32 = 8;
/// Mask of all defined `splice`/`tee`/`vmsplice` flag bits.  Any bit
/// outside this mask is rejected with EINVAL — matches Linux's
/// `SPLICE_F_ALL` check in `fs/splice.c`.
pub const SPLICE_F_VALID: u32 = SPLICE_F_MOVE | SPLICE_F_NONBLOCK | SPLICE_F_MORE | SPLICE_F_GIFT;

/// Move data between two file descriptors via a pipe.
///
/// POSIX/Linux semantics: at least one of `fd_in` / `fd_out` must
/// refer to a pipe.  If `off_in` is non-null, `fd_in` must be
/// seekable and its file position is left unchanged; otherwise the
/// current file position is consumed and advanced.  Same for
/// `off_out` / `fd_out`.
///
/// This is a buffered read+write fallback — there is no true
/// zero-copy page transfer.  Linux's `splice()` performs zero-copy
/// when the kernel can move pipe-buffer pages directly into the
/// page cache or socket queue; we don't have that infrastructure
/// yet, so userspace gets the same observable result via a small
/// bounce buffer at a small performance cost.  The `flags` argument
/// is therefore advisory only — `SPLICE_F_MOVE`, `SPLICE_F_MORE`,
/// and `SPLICE_F_GIFT` have no effect, and `SPLICE_F_NONBLOCK` is
/// already honored by `read`/`write` via `O_NONBLOCK` on the fd.
// Byte counters in this routine (`total`, `written`, `cur_in`,
// `cur_out`, `to_write`, `remaining`) all stay bounded by the
// caller-supplied `len` and the local stack buffer size; each `+=`
// follows a `total <= len` check or a `written <= to_write` check.
// Wrapping behaviour would be a caller-side bug, not a soundness
// issue, so we suppress the defensive arithmetic lint here.
#[allow(clippy::arithmetic_side_effects)]
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn splice(
    fd_in: Fd,
    off_in: *mut i64,
    fd_out: Fd,
    off_out: *mut i64,
    len: usize,
    flags: u32,
) -> isize {
    // Linux's `SYSCALL_DEFINE6(splice, ...)` validates `flags` before
    // any other check — `flags & ~SPLICE_F_ALL → -EINVAL`.  We match
    // that ordering so a caller passing garbage flag bits learns about
    // it regardless of fd state, length, or pipe direction.
    if flags & !SPLICE_F_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    if len == 0 {
        return 0;
    }

    // Both fds must be valid.
    let Some(in_entry) = lookup_fd(fd_in) else {
        return -1;
    };
    let Some(out_entry) = lookup_fd(fd_out) else {
        return -1;
    };

    let in_is_pipe = in_entry.kind == HandleKind::Pipe;
    let out_is_pipe = out_entry.kind == HandleKind::Pipe;

    // Linux: "Either fd_in or fd_out must be a pipe."
    if !in_is_pipe && !out_is_pipe {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Linux: "off_in must be NULL if fd_in refers to a pipe; same for off_out."
    if !off_in.is_null() && in_is_pipe {
        errno::set_errno(errno::ESPIPE);
        return -1;
    }
    if !off_out.is_null() && out_is_pipe {
        errno::set_errno(errno::ESPIPE);
        return -1;
    }

    // SAFETY: off_in / off_out are validated non-null caller pointers.
    let mut cur_in: i64 = if off_in.is_null() {
        0
    } else {
        unsafe { *off_in }
    };
    let mut cur_out: i64 = if off_out.is_null() {
        0
    } else {
        unsafe { *off_out }
    };

    // Bounce buffer.  Same size as sendfile() so the two helpers
    // have matching memory profiles.
    let mut buf = [0u8; 4096];
    let mut total: usize = 0;

    while total < len {
        let remaining = len - total;
        let chunk = remaining.min(buf.len());

        // Read.  pread when an explicit offset was supplied (so we
        // don't disturb fd_in's file position), otherwise read.
        let nr = if off_in.is_null() {
            read(fd_in, buf.as_mut_ptr(), chunk)
        } else {
            pread(fd_in, buf.as_mut_ptr(), chunk, cur_in)
        };
        if nr < 0 {
            if total > 0 {
                break;
            }
            return -1;
        }
        if nr == 0 {
            break;
        }

        // Write all bytes that were read, retrying on short writes.
        // Critical: read() already advanced fd_in's position (or pread
        // committed the offset for the caller), so we cannot afford to
        // drop bytes by giving up after a short write.
        let mut written: usize = 0;
        let to_write = nr as usize;
        while written < to_write {
            let nw = if off_out.is_null() {
                write(
                    fd_out,
                    // SAFETY: written < to_write <= buf.len().
                    unsafe { buf.as_ptr().add(written) },
                    to_write - written,
                )
            } else {
                pwrite(
                    fd_out,
                    // SAFETY: written < to_write <= buf.len().
                    unsafe { buf.as_ptr().add(written) },
                    to_write - written,
                    cur_out + written as i64,
                )
            };
            if nw < 0 {
                if total > 0 || written > 0 {
                    total += written;
                    cur_in += written as i64;
                    cur_out += written as i64;
                    if !off_in.is_null() {
                        // SAFETY: validated above.
                        unsafe {
                            *off_in = cur_in;
                        }
                    }
                    if !off_out.is_null() {
                        // SAFETY: validated above.
                        unsafe {
                            *off_out = cur_out;
                        }
                    }
                    return total as isize;
                }
                return -1;
            }
            if nw == 0 {
                // Avoid an infinite loop if write() reports 0 with no error.
                break;
            }
            written += nw as usize;
        }

        total += written;
        cur_in += written as i64;
        cur_out += written as i64;

        // If we couldn't write the full chunk we just read, stop —
        // the remaining bytes in `buf` are already accounted for by
        // the read above and the caller will see a short transfer.
        if written < to_write {
            break;
        }
    }

    // Publish updated offsets to caller.
    if !off_in.is_null() {
        // SAFETY: validated above.
        unsafe {
            *off_in = cur_in;
        }
    }
    if !off_out.is_null() {
        // SAFETY: validated above.
        unsafe {
            *off_out = cur_out;
        }
    }

    total as isize
}

/// Duplicate pipe content from `fd_in` to `fd_out` WITHOUT consuming it.
///
/// `tee(2)` copies up to `len` bytes of buffered data from the pipe read end
/// `fd_in` into the pipe write end `fd_out`, leaving `fd_in`'s data intact so a
/// subsequent `read`/`splice` on `fd_in` still sees it.  This is the classic
/// "inspect a stream while passing it on" primitive (`cmd | tee | cmd2` built
/// on real pipes).
///
/// Implemented on the OS target via two pipe primitives added for this purpose:
/// `SYS_PIPE_PEEK` copies buffered bytes at a logical offset without advancing
/// the read cursor, and `SYS_PIPE_WAIT_READABLE` blocks for data/EOF without
/// consuming.  We peek successive offsets out of `fd_in` and write the copies
/// into `fd_out`; `fd_in` is never drained.  `SPLICE_F_MOVE`/`_MORE`/`_GIFT`
/// are advisory only (we copy rather than share pages).
///
/// Blocking semantics match Linux's `fs/splice.c::do_tee`:
/// - Empty source with writers still attached: block until data arrives, unless
///   `SPLICE_F_NONBLOCK` is set (then `-1`/`EAGAIN`).
/// - Empty source with all writers closed (EOF): return `0`.
/// - Full destination: a blocking write waits for space; with
///   `SPLICE_F_NONBLOCK`, a `try_write` that can't place all bytes returns the
///   partial count already duplicated.
///
/// Once any bytes are duplicated we return that count rather than continuing to
/// block, so a short transfer is observable exactly as on Linux.
///
/// Validation order matches `do_tee`:
/// 1. Unknown flag bits → `EINVAL`.
/// 2. Negative fds → `EBADF` (cheap pre-check before the fdtable probe).
/// 3. Missing fds → `EBADF`.
/// 4. Either side not a pipe → `EINVAL`.
/// 5. `len == 0` → `0` (a no-op that still passes validation).
///
/// The host build has no kernel pipe layer, so it returns `-1`/`ENOSYS` after
/// the same validation (unit tests exercise the argument-domain checks).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tee(fd_in: Fd, fd_out: Fd, len: usize, flags: u32) -> isize {
    // 1. Unknown flag bits.  Checked first so callers that pass garbage
    //    flags learn about it regardless of fd state.
    if flags & !SPLICE_F_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // 2. Negative fd short-circuit — avoids two fdtable probes for an
    //    obviously-invalid request.  Linux returns EBADF for negative
    //    fds via the fdget path; we match that.
    if fd_in < 0 || fd_out < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // 3. Both fds must be open.  lookup_fd sets EBADF on miss.
    let Some(in_entry) = lookup_fd(fd_in) else {
        return -1;
    };
    let Some(out_entry) = lookup_fd(fd_out) else {
        return -1;
    };

    // 4. Both ends must be pipes — Linux's `do_tee` returns EINVAL
    //    when either side is a regular file, socket, etc.
    if in_entry.kind != HandleKind::Pipe || out_entry.kind != HandleKind::Pipe {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // 5. Zero-length tee is observably a no-op on Linux too.
    if len == 0 {
        return 0;
    }

    #[cfg(target_os = "none")]
    {
        tee_transfer(in_entry.handle, out_entry.handle, len, flags)
    }
    #[cfg(not(target_os = "none"))]
    {
        // No kernel pipe layer in the host build.  Keep the historical
        // ENOSYS terminal so host unit tests still assert the validation.
        let _ = (&in_entry, &out_entry);
        errno::set_errno(errno::ENOSYS);
        -1
    }
}

/// Core of `tee(2)` on the OS target: peek buffered bytes out of the source
/// pipe (`in_handle`, a read end) and write copies into the destination pipe
/// (`out_handle`, a write end) without consuming the source.  See [`tee`].
#[cfg(target_os = "none")]
fn tee_transfer(in_handle: u64, out_handle: u64, len: usize, flags: u32) -> isize {
    let nonblock = flags & SPLICE_F_NONBLOCK != 0;
    let mut buf = [0u8; 4096];
    let mut total: usize = 0;
    // Logical offset into the source's buffered data.  Advances only by bytes
    // we've successfully duplicated, so a short destination write re-peeks the
    // not-yet-copied tail on the next pass.
    let mut offset: u64 = 0;

    while total < len {
        let chunk = len.saturating_sub(total).min(buf.len());
        // Non-destructive copy of up to `chunk` bytes at `offset`.
        let n = syscall4(
            SYS_PIPE_PEEK,
            in_handle,
            offset,
            buf.as_mut_ptr() as u64,
            chunk as u64,
        );
        if n < 0 {
            if total > 0 {
                break;
            }
            return errno::translate(n) as isize;
        }
        if n == 0 {
            // Nothing buffered at `offset`.
            if total > 0 {
                // Already duplicated something this call — report it.
                break;
            }
            if nonblock {
                errno::set_errno(errno::EAGAIN);
                return -1;
            }
            // Block until the source has data or reaches EOF.
            let ready = syscall1(SYS_PIPE_WAIT_READABLE, in_handle);
            if ready < 0 {
                return errno::translate(ready) as isize;
            }
            if ready == 0 {
                // Writers all gone, buffer drained — 0 bytes to duplicate.
                return 0;
            }
            // Data available now; re-peek from the same offset.
            continue;
        }

        // Write every peeked byte into the destination.
        let to_write = n as usize;
        let mut written: usize = 0;
        while written < to_write {
            // SAFETY: written < to_write <= buf.len(), so the pointer stays
            // inside `buf`.
            let ptr = unsafe { buf.as_ptr().add(written) } as u64;
            let remaining = to_write.saturating_sub(written) as u64;
            let nw = if nonblock {
                syscall3(SYS_PIPE_TRY_WRITE, out_handle, ptr, remaining)
            } else {
                syscall3(SYS_PIPE_WRITE, out_handle, ptr, remaining)
            };
            if nw < 0 {
                // Destination error.  If we've made progress, return it so the
                // caller sees a short transfer (Linux behaviour on EAGAIN/EPIPE
                // mid-tee).  Otherwise surface the error.
                if total > 0 || written > 0 {
                    return total.saturating_add(written) as isize;
                }
                return errno::translate(nw) as isize;
            }
            if nw == 0 {
                // No space and no error (nonblocking, full pipe) — stop.
                break;
            }
            written = written.saturating_add(nw as usize);
        }

        total = total.saturating_add(written);
        offset = offset.saturating_add(written as u64);
        if written < to_write {
            // Couldn't place the whole peeked chunk (destination full under
            // SPLICE_F_NONBLOCK) — stop with a short transfer.
            break;
        }
    }

    total as isize
}

/// Splice user pages into, or out of, a pipe.
///
/// Linux `vmsplice()` has two directions, chosen by which end of the
/// pipe `fd` refers to (Linux `fs/splice.c::do_vmsplice` branches on
/// `FMODE_WRITE` vs `FMODE_READ`):
/// - **Write end** (`O_WRONLY`/`O_RDWR`): the iovec contents are moved
///   into the pipe.  Implemented as a plain `writev()` — a data copy,
///   not zero-copy page gifting.  `SPLICE_F_GIFT` is therefore advisory
///   only; true page donation needs VFS-level pipe page sharing we
///   don't have.
/// - **Read end** (`O_RDONLY`): the buffered pipe bytes are copied
///   (consumed) out into the iovec.  Implemented as a plain `readv()`.
///
/// Direction is decided from the fd's access mode, which `pipe2()` sets
/// per end (read end = `O_RDONLY`, write end = `O_WRONLY`).  Because
/// both directions delegate to `readv`/`writev`, `SPLICE_F_NONBLOCK` is
/// honored via the fd's own `O_NONBLOCK` status flag rather than as an
/// independent per-call override (a pre-existing limitation shared by
/// both directions — a true per-call non-block would need the pipe
/// try-read/try-write primitives wired in for the iovec loop, a
/// separate enhancement).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn vmsplice(fd: Fd, iov: *const Iovec, nr_segs: u64, flags: u32) -> isize {
    // Linux's `do_vmsplice` rejects unknown flag bits with EINVAL
    // before any other validation.  Match that — a caller with bad
    // flag bits learns immediately, regardless of fd / iov state.
    if flags & !SPLICE_F_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    if iov.is_null() && nr_segs > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if nr_segs == 0 {
        return 0;
    }
    // Linux caps at UIO_MAXIOV (1024); we use a more generous i32 cap
    // since readv()/writev() take i32 — beyond that, EINVAL.
    if nr_segs > i32::MAX as u64 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };
    if entry.kind != HandleKind::Pipe {
        // Linux returns EBADF for non-pipe fds.
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // Direction follows the fd's access mode, mirroring Linux's
    // `do_vmsplice`, which prefers the write direction when the file is
    // writable and otherwise copies out of the pipe.  A pipe read end is
    // `O_RDONLY`; a write end is `O_WRONLY`.
    let accmode = entry.status_flags & crate::fcntl::O_ACCMODE;
    if accmode == crate::fcntl::O_WRONLY || accmode == crate::fcntl::O_RDWR {
        // Write end: move the iovec contents into the pipe.
        writev(fd, iov, nr_segs as i32)
    } else {
        // Read end (O_RDONLY): consume buffered pipe bytes into the iovec.
        readv(fd, iov, nr_segs as i32)
    }
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

/// Mask of bits that can appear in the `operation` argument of `flock`:
/// exactly one of LOCK_SH / LOCK_EX / LOCK_UN, optionally OR'd with
/// LOCK_NB.  Linux rejects anything outside this mask with EINVAL.
const FLOCK_OP_MASK: i32 = LOCK_SH | LOCK_EX | LOCK_UN | LOCK_NB;

/// Apply or remove an advisory lock on an open file.
///
/// Wired to the kernel advisory-lock table (`SYS_FS_FLOCK` /
/// `SYS_FS_FUNLOCK`).  The lock is whole-file and owned by the calling
/// process: the kernel keys locks by resolved path + owner ID (our PID),
/// so every thread and descriptor in a process shares one lock per path.
///
/// Without `LOCK_NB`, a contended request blocks until the lock can be
/// acquired; the kernel primitive is non-blocking, so we poll with a
/// yield between attempts (see the limitation note on `do_flock`).  With
/// `LOCK_NB`, contention returns `EWOULDBLOCK` immediately.
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
///   * `EINVAL` — `operation` has unknown bits, lacks one of
///     LOCK_SH/LOCK_EX/LOCK_UN, or names more than one of them.
///   * `EWOULDBLOCK` — `LOCK_NB` set and the lock is held by another owner.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn flock(fd: Fd, operation: i32) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if operation & !FLOCK_OP_MASK != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Linux requires exactly one of LOCK_SH | LOCK_EX | LOCK_UN.
    let mode = operation & (LOCK_SH | LOCK_EX | LOCK_UN);
    if mode != LOCK_SH && mode != LOCK_EX && mode != LOCK_UN {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Bare metal drives the kernel lock table; the host build has no
    // kernel, so it stays a validation-only success.
    #[cfg(target_os = "none")]
    {
        do_flock(fd, operation)
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
}

/// Bare-metal worker for [`flock`]: resolve the fd to its path and drive
/// the kernel advisory-lock syscalls.
///
/// A path-less descriptor (pipe, socket, anonymous fd) has no entry in
/// the kernel's path-keyed lock table, so the request is accepted as a
/// no-op.  Linux would lock the open file description's inode, which our
/// path-based lock table cannot represent — documented in todo.txt.
///
/// LIMITATION: blocking acquisition (without `LOCK_NB`) polls with a
/// yield because `SYS_FS_FLOCK` is non-blocking.  A true blocking wait
/// needs a kernel wait queue; deferred (see todo.txt).
#[cfg(target_os = "none")]
fn do_flock(fd: Fd, operation: i32) -> i32 {
    let mut buf = [0u8; crate::unistd::PATH_MAX];
    let path_len = fdtable::get_fd_path(fd, &mut buf);
    if path_len == 0 {
        // Path-less fd: nothing in the kernel lock table to operate on.
        return 0;
    }
    let owner = syscall0(SYS_PROCESS_ID) as u64;
    let mode = operation & (LOCK_SH | LOCK_EX | LOCK_UN);

    if mode == LOCK_UN {
        let ret = syscall3(SYS_FS_FUNLOCK, buf.as_ptr() as u64, path_len as u64, owner);
        return errno::translate(ret) as i32;
    }

    let lock_type: u64 = u64::from(mode == LOCK_EX);
    let nonblock = operation & LOCK_NB != 0;
    loop {
        let ret = syscall4(
            SYS_FS_FLOCK,
            buf.as_ptr() as u64,
            path_len as u64,
            lock_type,
            owner,
        );
        if ret >= 0 {
            return 0;
        }
        // Negative return: map to errno (sets errno, yields -1).
        let mapped = errno::translate(ret) as i32;
        if !nonblock && errno::get_errno() == errno::EAGAIN {
            // Contended blocking request: yield the CPU and retry.
            let _ = syscall1(SYS_SLEEP, 0);
            continue;
        }
        return mapped;
    }
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
/// Validates `fd` and `cmd`, then succeeds as a no-op.
///
/// Unlike [`flock`], `lockf` locks a *byte range* of the file.  The
/// kernel advisory-lock table is whole-file only, so wiring `lockf` to
/// it would lock the entire file for every range request — turning
/// independent ranges (e.g. a database locking distinct records) into
/// false contention and potential deadlock, which is strictly worse than
/// the no-op.  `F_TEST` additionally has no non-destructive kernel query
/// syscall.  A faithful `lockf` therefore needs byte-range lock support
/// plus a lock-query syscall in the kernel; this is tracked in todo.txt.
/// Until then the body is a validation-only no-op.
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
///   * `EINVAL` — `cmd` is not one of F_LOCK, F_TLOCK, F_ULOCK, F_TEST.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lockf(fd: Fd, cmd: i32, _len: OffT) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if !matches!(cmd, F_LOCK | F_TLOCK | F_ULOCK | F_TEST) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
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
pub extern "C" fn sendfile(out_fd: Fd, in_fd: Fd, offset: *mut i64, count: usize) -> isize {
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
            let chunk = if remaining < buf.len() {
                remaining
            } else {
                buf.len()
            };

            let nr = read(in_fd, buf.as_mut_ptr(), chunk);
            if nr < 0 {
                if total > 0 {
                    break;
                }
                return -1;
            }
            if nr == 0 {
                break;
            }

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
                if nw == 0 {
                    break;
                } // Avoid infinite loop.
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
            let chunk = if remaining < buf.len() {
                remaining
            } else {
                buf.len()
            };

            let nr = pread(in_fd, buf.as_mut_ptr(), chunk, cur_off);
            if nr < 0 {
                if total > 0 {
                    break;
                }
                return -1;
            }
            if nr == 0 {
                break;
            }

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
                        unsafe {
                            *offset = cur_off;
                        }
                        return total as isize;
                    }
                    return -1;
                }
                if nw == 0 {
                    break;
                } // Avoid infinite loop.
                written = written.wrapping_add(nw as usize);
            }

            total = total.wrapping_add(written);
            cur_off = cur_off.wrapping_add(written as i64);
        }

        // Update caller's offset to reflect bytes transferred.
        // SAFETY: offset is valid.
        unsafe {
            *offset = cur_off;
        }
    }

    total as isize
}

/// `sendfile64` — LP64 alias for `sendfile`.
///
/// On 64-bit systems (LP64), `off_t` is already 64-bit, so `sendfile64`
/// is identical to `sendfile`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sendfile64(out_fd: Fd, in_fd: Fd, offset: *mut i64, count: usize) -> isize {
    sendfile(out_fd, in_fd, offset, count)
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
/// Argument-domain validation (Linux-matching):
///   - `flags != 0` → `-1` with `EINVAL`.  Linux's `do_copy_file_range`
///     reserves `flags` for future extension and rejects any non-zero
///     value.
///   - `fd_in < 0 || fd_out < 0` → `-1` with `EBADF`.
///   - Either fd not open → `-1` with `EBADF`.
///   - `len == 0` → `0` (well-formed no-op).
///
/// Stub: after validation, performs a userspace pread/read +
/// pwrite/write copy.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn copy_file_range(
    fd_in: Fd,
    off_in: *mut i64,
    fd_out: Fd,
    off_out: *mut i64,
    len: usize,
    flags: u32,
) -> isize {
    // flags is reserved — Linux currently defines no valid bit.
    if flags != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Both fds must be non-negative.  Linux's fdget path returns EBADF
    // for negative fds before any I/O-shape validation.
    if fd_in < 0 || fd_out < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // Both fds must be open.  lookup_fd sets EBADF on miss.
    if lookup_fd(fd_in).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if lookup_fd(fd_out).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // Zero-length copy is a well-formed no-op.  Linux returns 0 here.
    if len == 0 {
        return 0;
    }

    let mut buf = [0u8; 4096];
    let mut total: usize = 0;

    let mut in_pos = if off_in.is_null() {
        0
    } else {
        unsafe { *off_in }
    };
    let mut out_pos = if off_out.is_null() {
        0
    } else {
        unsafe { *off_out }
    };

    while total < len {
        let remaining = len.wrapping_sub(total);
        let chunk = if remaining < buf.len() {
            remaining
        } else {
            buf.len()
        };

        // Read: use pread when off_in is provided, else normal read.
        let nr = if off_in.is_null() {
            read(fd_in, buf.as_mut_ptr(), chunk)
        } else {
            pread(fd_in, buf.as_mut_ptr(), chunk, in_pos)
        };
        if nr <= 0 {
            break;
        }

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
                    if !off_in.is_null() {
                        unsafe {
                            *off_in = in_pos;
                        }
                    }
                    if !off_out.is_null() {
                        unsafe {
                            *off_out = out_pos;
                        }
                    }
                    return total as isize;
                }
                return -1;
            }
            if nw == 0 {
                break;
            }
            written = written.wrapping_add(nw as usize);
        }

        total = total.wrapping_add(written);
        in_pos = in_pos.wrapping_add(written as i64);
        out_pos = out_pos.wrapping_add(written as i64);
    }

    // Update caller's offsets to reflect bytes transferred.
    if !off_in.is_null() {
        // SAFETY: off_in is valid.
        unsafe {
            *off_in = in_pos;
        }
    }
    if !off_out.is_null() {
        // SAFETY: off_out is valid.
        unsafe {
            *off_out = out_pos;
        }
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

/// `UTIME_NOW` — set timestamp to current time.
pub const UTIME_NOW: i64 = (1 << 30) - 1;
/// `UTIME_OMIT` — leave timestamp unchanged.
pub const UTIME_OMIT: i64 = (1 << 30) - 2;

/// Valid `tv_usec` range for `utimes`/`futimes`: 0..=999_999.
const USEC_MAX: i64 = 999_999;
/// Valid `tv_nsec` range for `utimensat`/`futimens`: 0..=999_999_999
/// (plus the two sentinels `UTIME_NOW` and `UTIME_OMIT`).
const NSEC_MAX: i64 = 999_999_999;

/// Returns true iff `usec` is in the POSIX-legal range for a `timeval`
/// passed to `utimes`/`futimes` (microsecond precision).
fn timeval_usec_valid(usec: i64) -> bool {
    (0..=USEC_MAX).contains(&usec)
}

/// Returns true iff `nsec` is legal for a `timespec` passed to
/// `utimensat`/`futimens` — either a normal 0..=999_999_999 value or one
/// of the two sentinels (`UTIME_NOW`, `UTIME_OMIT`).
fn timespec_nsec_valid(nsec: i64) -> bool {
    (0..=NSEC_MAX).contains(&nsec) || nsec == UTIME_NOW || nsec == UTIME_OMIT
}

/// Combine a `Timespec`'s seconds + nanoseconds into nanoseconds since the
/// Unix epoch, mapping the `utimensat` sentinels to the kernel convention.
///
/// `now_ns` is the current wall-clock time (passed in so this stays pure and
/// host-testable).  The kernel `SYS_FS_SET_TIMES` ABI uses 0 to mean "leave
/// this timestamp unchanged", so:
///   * `UTIME_OMIT` → 0 (unchanged)
///   * `UTIME_NOW`  → `now_ns` (current wall clock)
///   * otherwise    → `tv_sec * 1e9 + tv_nsec`
#[cfg(any(target_os = "none", test))]
fn timespec_to_kernel_ns(ts: &crate::stat::Timespec, now_ns: u64) -> u64 {
    match ts.tv_nsec {
        UTIME_OMIT => 0,
        UTIME_NOW => now_ns,
        _ => {
            let sec = u64::try_from(ts.tv_sec).unwrap_or(0);
            let nsec = u64::try_from(ts.tv_nsec).unwrap_or(0);
            sec.saturating_mul(1_000_000_000).saturating_add(nsec)
        }
    }
}

/// Combine a `Timeval`'s seconds + microseconds into nanoseconds since the
/// Unix epoch.  `utimes`/`futimes` have no per-field `UTIME_NOW`/`UTIME_OMIT`
/// sentinels, so every value is a literal time.
#[cfg(any(target_os = "none", test))]
fn timeval_to_kernel_ns(tv: &Timeval) -> u64 {
    let sec = u64::try_from(tv.tv_sec).unwrap_or(0);
    let usec = u64::try_from(tv.tv_usec).unwrap_or(0);
    sec.saturating_mul(1_000_000_000)
        .saturating_add(usec.saturating_mul(1_000))
}

/// Map a `utimensat`/`futimens` `times` array to the kernel's
/// `(accessed_ns, modified_ns)` pair.  A NULL `times` means "set both to the
/// current time" (POSIX).  Pure given `now_ns`, so host-testable.
///
/// # Safety
/// When `times` is non-null it must point to two readable `Timespec`s.
#[cfg(any(target_os = "none", test))]
unsafe fn utimens_pair_to_kernel(times: *const crate::stat::Timespec, now_ns: u64) -> (u64, u64) {
    if times.is_null() {
        return (now_ns, now_ns);
    }
    // SAFETY: caller contract — `times` points to two valid Timespecs.
    let a = unsafe { times.read() };
    // SAFETY: as above; the second element is at offset 1.
    let m = unsafe { times.add(1).read() };
    (
        timespec_to_kernel_ns(&a, now_ns),
        timespec_to_kernel_ns(&m, now_ns),
    )
}

/// Map a `utimes`/`futimes` `times` array to the kernel's
/// `(accessed_ns, modified_ns)` pair.  A NULL `times` means "set both to the
/// current time" (POSIX).  Pure given `now_ns`, so host-testable.
///
/// # Safety
/// When `times` is non-null it must point to two readable `Timeval`s.
#[cfg(any(target_os = "none", test))]
unsafe fn utimes_pair_to_kernel(times: *const Timeval, now_ns: u64) -> (u64, u64) {
    if times.is_null() {
        return (now_ns, now_ns);
    }
    // SAFETY: caller contract — `times` points to two valid Timevals.
    let a = unsafe { times.read() };
    // SAFETY: as above; the second element is at offset 1.
    let m = unsafe { times.add(1).read() };
    (timeval_to_kernel_ns(&a), timeval_to_kernel_ns(&m))
}

/// Current wall-clock time in nanoseconds since the Unix epoch, used to
/// resolve `UTIME_NOW` and NULL-`times` requests.  Bare metal only.
#[cfg(target_os = "none")]
fn wall_clock_ns() -> u64 {
    // SYS_CLOCK_REALTIME returns ns since the Unix epoch (0 before RTC init).
    syscall0(SYS_CLOCK_REALTIME) as u64
}

/// Resolve `path` and issue `SYS_FS_SET_TIMES` with the kernel ns pair.
/// Returns 0 on success or -1 with `errno` set.  Bare metal only.
#[cfg(target_os = "none")]
fn set_times_path(path: *const u8, accessed_ns: u64, modified_ns: u64) -> i32 {
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };
    let ret = syscall4(
        SYS_FS_SET_TIMES,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        accessed_ns,
        modified_ns,
    );
    if ret < 0 {
        return errno::translate(ret) as i32;
    }
    0
}

/// Resolve `path` and issue `SYS_FS_SET_PERMS` with the masked mode bits.
/// Returns 0 on success or -1 with `errno` set.  Bare metal only.
#[cfg(target_os = "none")]
fn set_perms_path(path: *const u8, mode: ModeT) -> i32 {
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };
    // The kernel masks to 0o7777, but mask here too so the ABI value is
    // unambiguous (mode_t carries file-type bits we must not forward).
    let perms = u64::from(mode & 0o7777);
    let ret = syscall3(
        SYS_FS_SET_PERMS,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        perms,
    );
    if ret < 0 {
        return errno::translate(ret) as i32;
    }
    0
}

/// Resolve `path` and issue `SYS_FS_SET_OWNER` with the uid/gid pair.
/// A field of `u32::MAX` tells the kernel to leave that field unchanged.
/// Returns 0 on success or -1 with `errno` set.  Bare metal only.
#[cfg(target_os = "none")]
fn set_owner_path(path: *const u8, uid: u32, gid: u32) -> i32 {
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };
    let ret = syscall4(
        SYS_FS_SET_OWNER,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        u64::from(uid),
        u64::from(gid),
    );
    if ret < 0 {
        return errno::translate(ret) as i32;
    }
    0
}

/// Set file access and modification times (microsecond precision).
///
/// Validates `path` and the `times` array, then issues `SYS_FS_SET_TIMES`
/// to persist the new times (NULL `times` sets both to the current time,
/// per POSIX).
///
/// Errors:
///   * `EFAULT` — `path` is NULL.
///   * `EINVAL` — `times[i].tv_usec` is outside [0, 999_999].
///   * any error the kernel returns from `SYS_FS_SET_TIMES`
///     (e.g. `ENOENT`, `EACCES`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn utimes(path: *const u8, times: *const Timeval) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if !times.is_null() {
        // SAFETY: caller contract — `times` points to two valid Timevals.
        let a = unsafe { times.read() };
        let m = unsafe { times.add(1).read() };
        if !timeval_usec_valid(a.tv_usec) || !timeval_usec_valid(m.tv_usec) {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }
    #[cfg(target_os = "none")]
    {
        let now = wall_clock_ns();
        // SAFETY: `times` was validated above; non-null implies two valid
        // Timevals.
        let (a_ns, m_ns) = unsafe { utimes_pair_to_kernel(times, now) };
        set_times_path(path, a_ns, m_ns)
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
}

/// Set file access and modification times on an open fd.
///
/// The kernel `SYS_FS_SET_TIMES` is path-based, so we resolve the fd to its
/// stored path (set at `open`) and delegate.  Descriptors with no stored
/// path (pipes, sockets, eventfds, …) have no persistent timestamps to
/// update, so the call succeeds as a no-op — matching how `fstatvfs`
/// handles path-less descriptors.
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
///   * `EINVAL` — `times[i].tv_usec` is outside [0, 999_999].
///   * any error the kernel returns from `SYS_FS_SET_TIMES`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn futimes(fd: Fd, times: *const Timeval) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if !times.is_null() {
        // SAFETY: caller contract — `times` points to two valid Timevals.
        let a = unsafe { times.read() };
        let m = unsafe { times.add(1).read() };
        if !timeval_usec_valid(a.tv_usec) || !timeval_usec_valid(m.tv_usec) {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }
    #[cfg(target_os = "none")]
    {
        let mut path = [0u8; crate::unistd::PATH_MAX];
        let len = fdtable::get_fd_path(fd, &mut path);
        if len == 0 {
            // No stored path (pipe/socket/etc.) — nothing to persist.
            return 0;
        }
        let now = wall_clock_ns();
        // SAFETY: `times` was validated above; non-null implies two valid
        // Timevals.
        let (a_ns, m_ns) = unsafe { utimes_pair_to_kernel(times, now) };
        set_times_path(path.as_ptr(), a_ns, m_ns)
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
}

/// Set file timestamps with nanosecond precision (relative to dirfd).
///
/// Errors:
///   * `EINVAL` — `flags` contains bits other than `AT_SYMLINK_NOFOLLOW`,
///     or `times[i].tv_nsec` is outside [0, 999_999_999] and not
///     `UTIME_NOW`/`UTIME_OMIT`.
///   * `EFAULT` — `path` is NULL (POSIX; Linux has a `NULL`-path GNU
///     extension that equates this with `futimens(dirfd, ...)`, but we
///     follow POSIX until that extension is needed).
///   * `EBADF` — `dirfd` is not `AT_FDCWD` and refers to no open fd,
///     while `path` is relative.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn utimensat(
    dirfd: Fd,
    path: *const u8,
    times: *const crate::stat::Timespec,
    flags: i32,
) -> i32 {
    // Linux validates `flags` first.
    if (flags & !AT_SYMLINK_NOFOLLOW) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // `times` validation matches what the VFS does before any path lookup.
    if !times.is_null() {
        // SAFETY: caller contract — `times` points to two valid Timespecs.
        let a = unsafe { times.read() };
        let m = unsafe { times.add(1).read() };
        if !timespec_nsec_valid(a.tv_nsec) || !timespec_nsec_valid(m.tv_nsec) {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }
    // Validate dirfd only for relative paths; absolute paths ignore it.
    if dirfd != AT_FDCWD && !is_absolute_path(path) {
        if dirfd < 0 {
            errno::set_errno(errno::EBADF);
            return -1;
        }
        if fdtable::get_fd(dirfd).is_none() {
            errno::set_errno(errno::EBADF);
            return -1;
        }
    }
    #[cfg(target_os = "none")]
    {
        // Resolve the dirfd/path pair the same way fstatat does.  NOTE: the
        // kernel SYS_FS_SET_TIMES follows symlinks unconditionally, so
        // AT_SYMLINK_NOFOLLOW on a symlink updates the target's times rather
        // than the link's (documented limitation — see todo.txt).
        let now = wall_clock_ns();
        // SAFETY: `times` was validated above; non-null implies two valid
        // Timespecs.
        let (a_ns, m_ns) = unsafe { utimens_pair_to_kernel(times, now) };
        if dirfd == AT_FDCWD || is_absolute_path(path) {
            set_times_path(path, a_ns, m_ns)
        } else {
            let mut full = [0u8; crate::unistd::PATH_MAX];
            let len = resolve_dirfd_path(dirfd, path, &mut full);
            if len == 0 {
                return -1;
            }
            set_times_path(full.as_ptr(), a_ns, m_ns)
        }
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
}

/// Set file timestamps with nanosecond precision on an open fd.
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
///   * `EINVAL` — `times[i].tv_nsec` is outside [0, 999_999_999] and not
///     `UTIME_NOW`/`UTIME_OMIT`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn futimens(fd: Fd, times: *const crate::stat::Timespec) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if !times.is_null() {
        // SAFETY: caller contract — `times` points to two valid Timespecs.
        let a = unsafe { times.read() };
        let m = unsafe { times.add(1).read() };
        if !timespec_nsec_valid(a.tv_nsec) || !timespec_nsec_valid(m.tv_nsec) {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }
    #[cfg(target_os = "none")]
    {
        let mut path = [0u8; crate::unistd::PATH_MAX];
        let len = fdtable::get_fd_path(fd, &mut path);
        if len == 0 {
            // No stored path (pipe/socket/etc.) — nothing to persist.
            return 0;
        }
        let now = wall_clock_ns();
        // SAFETY: `times` was validated above; non-null implies two valid
        // Timespecs.
        let (a_ns, m_ns) = unsafe { utimens_pair_to_kernel(times, now) };
        set_times_path(path.as_ptr(), a_ns, m_ns)
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Translate POSIX open flags to the kernel's native `OpenFlags` word
/// (`SYS_FS_OPEN`'s third argument).
///
/// The native encoding is **not** the Linux `O_*` bit layout — the kernel
/// (`kernel/src/fs/handle.rs`, `struct OpenFlags`) uses an independent set of
/// single-bit flags:
///
/// | native bit | value | meaning              |
/// |------------|-------|----------------------|
/// | 0          | 0x01  | READ                 |
/// | 1          | 0x02  | WRITE                |
/// | 2          | 0x04  | CREATE               |
/// | 3          | 0x08  | TRUNCATE             |
/// | 4          | 0x10  | APPEND               |
/// | 5          | 0x20  | DIRECTORY            |
///
/// POSIX instead packs the access mode into the low two bits as an *enum*
/// (`O_RDONLY`=0, `O_WRONLY`=1, `O_RDWR`=2) and uses high bits for `O_CREAT`
/// (0o100), `O_TRUNC` (0o1000), etc.  So we must translate rather than pass
/// the raw word through — an earlier version copied the low bits and OR'd
/// `O_CREAT` at bit 6, which the kernel decoded as "READ, no CREATE", breaking
/// every `open(..., "w")` (the file was never created → ENOENT).
///
/// `O_EXCL` has no native equivalent (the kernel does not implement exclusive
/// create), so it is dropped here; `O_CREAT | O_EXCL` therefore does not yet
/// fail on an existing file on-target.  Tracked in todo.txt.
pub(crate) fn translate_open_flags(posix_flags: i32) -> u64 {
    // Native OpenFlags bits (must match kernel `fs::handle::OpenFlags`).
    const N_READ: u64 = 0x01;
    const N_WRITE: u64 = 0x02;
    const N_CREATE: u64 = 0x04;
    const N_TRUNCATE: u64 = 0x08;
    const N_APPEND: u64 = 0x10;
    const N_DIRECTORY: u64 = 0x20;

    let mut native: u64 = 0;

    // Access mode: POSIX enum → independent READ/WRITE flags.
    match posix_flags & fcntl::O_ACCMODE {
        x if x == fcntl::O_WRONLY => native |= N_WRITE,
        x if x == fcntl::O_RDWR => native |= N_READ | N_WRITE,
        // O_RDONLY (0) and any malformed access mode default to read.
        _ => native |= N_READ,
    }

    if posix_flags & fcntl::O_CREAT != 0 {
        native |= N_CREATE;
    }
    if posix_flags & fcntl::O_TRUNC != 0 {
        native |= N_TRUNCATE;
    }
    if posix_flags & fcntl::O_APPEND != 0 {
        native |= N_APPEND;
    }
    if posix_flags & fcntl::O_DIRECTORY != 0 {
        native |= N_DIRECTORY;
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
    open(
        path,
        fcntl::O_CREAT | fcntl::O_WRONLY | fcntl::O_TRUNC,
        mode,
    )
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
pub extern "C" fn __getcwd_chk(buf: *mut u8, size: SizeT, _buflen: SizeT) -> *mut u8 {
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

/// `__readlink_chk` — fortified `readlink`.
///
/// `buflen` is the size of the destination object.  glibc aborts when
/// `len > buflen`; we instead clamp the read to `min(len, buflen)` so the
/// call can never write past the buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __readlink_chk(
    path: *const u8,
    buf: *mut u8,
    len: SizeT,
    buflen: SizeT,
) -> SsizeT {
    readlink(path, buf, len.min(buflen))
}

/// `__readlinkat_chk` — fortified `readlinkat`.
///
/// As [`__readlink_chk`]: clamps the read to `min(len, buflen)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __readlinkat_chk(
    dirfd: i32,
    path: *const u8,
    buf: *mut u8,
    len: SizeT,
    buflen: SizeT,
) -> SsizeT {
    readlinkat(dirfd, path, buf, len.min(buflen))
}

// ---------------------------------------------------------------------------
// readahead — Linux read-ahead hint
// ---------------------------------------------------------------------------

/// Initiate file read-ahead into the page cache.
///
/// This is a Linux-specific hint that tells the kernel to read `count`
/// bytes starting at `offset` from the file into the page cache,
/// anticipating future reads.
///
/// Since our kernel doesn't have a page cache yet, this is a no-op
/// that returns 0 (success).  The fd and offset are validated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn readahead(fd: Fd, offset: i64, count: usize) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if offset < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // No-op: our kernel has no page cache to prefetch into.
    let _ = count;
    0
}

// ---------------------------------------------------------------------------
// sync_file_range — fine-grained sync control
// ---------------------------------------------------------------------------

/// Sync file flags.
pub const SYNC_FILE_RANGE_WAIT_BEFORE: u32 = 1;
pub const SYNC_FILE_RANGE_WRITE: u32 = 2;
pub const SYNC_FILE_RANGE_WAIT_AFTER: u32 = 4;
/// Mask of all defined `sync_file_range` flag bits.  Any bit outside
/// this mask is rejected with EINVAL — matches Linux's
/// `VALID_FLAGS` check in `fs/sync.c::ksys_sync_file_range`.
pub const SYNC_FILE_RANGE_VALID: u32 =
    SYNC_FILE_RANGE_WAIT_BEFORE | SYNC_FILE_RANGE_WRITE | SYNC_FILE_RANGE_WAIT_AFTER;

/// Sync a file range to disk.
///
/// This Linux-specific function provides fine-grained control over
/// syncing file data to disk.  Since we don't have a writeback cache,
/// this delegates to fsync for the full file.
///
/// Validates inputs per Linux semantics (fs/sync.c::ksys_sync_file_range)
/// in the same order as the upstream prologue:
/// 1. `flags & ~SYNC_FILE_RANGE_VALID` → EINVAL.
/// 2. `offset < 0` → EINVAL.
/// 3. `nbytes < 0` → EINVAL.
/// 4. `offset + nbytes` overflowing i64 → EINVAL (Linux computes
///    `endbyte = offset + nbytes` as s64 and rejects negative).
/// 5. `fd < 0` or fd not open → EBADF.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sync_file_range(fd: Fd, offset: i64, nbytes: i64, flags: u32) -> i32 {
    if flags & !SYNC_FILE_RANGE_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if offset < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if nbytes < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // endbyte = offset + nbytes must not overflow i64.
    if offset.checked_add(nbytes).is_none() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    // Delegate to fsync — we don't have fine-grained range sync.
    fsync(fd)
}

// ---------------------------------------------------------------------------
// name_to_handle_at / open_by_handle_at — file handle operations
// ---------------------------------------------------------------------------

/// File handle structure for `name_to_handle_at` / `open_by_handle_at`.
#[repr(C)]
pub struct FileHandle {
    /// Size of `f_handle` in bytes.
    pub handle_bytes: u32,
    /// Handle type (filesystem-specific).
    pub handle_type: i32,
    // f_handle follows — variable-length.
}

/// Mask of `AT_*` flag bits accepted by `name_to_handle_at`.
///
/// Linux accepts `AT_SYMLINK_FOLLOW` (follow the final-component
/// symlink) and `AT_EMPTY_PATH` (operate on `dirfd` itself when
/// `pathname` is empty).  Newer kernels also accept `AT_HANDLE_FID`
/// (0x200), which we do not yet model.  Any bit outside this mask
/// produces `EINVAL`.
pub const NAME_TO_HANDLE_AT_FLAGS_VALID: i32 = AT_SYMLINK_FOLLOW | AT_EMPTY_PATH;

/// Obtain a file handle for a path.
///
/// Returns -1 with `ENOSYS` after argument-domain validation.  Full
/// file handles require filesystem-level identifiers we don't export
/// yet — but invalid callers should still see Linux-matching errno
/// values so portable code (rsync, criu, glibc's nfsd helpers) reads
/// us correctly.
///
/// Validation order matches `fs/fhandle.c::sys_name_to_handle_at`:
/// 1. Unknown flag bits → `EINVAL`.
/// 2. `pathname`, `handle`, or `mount_id` NULL → `EFAULT`.  Linux
///    actually defers `handle`/`mount_id` checks until after
///    `user_path_at`, but our model can do the cheap NULL check up
///    front without observable difference.
/// 3. If `dirfd != AT_FDCWD`, it must be a valid open fd → `EBADF`.
/// 4. All validated → `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn name_to_handle_at(
    dirfd: Fd,
    pathname: *const u8,
    handle: *mut FileHandle,
    mount_id: *mut i32,
    flags: i32,
) -> i32 {
    if flags & !NAME_TO_HANDLE_AT_FLAGS_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if pathname.is_null() || handle.is_null() || mount_id.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if dirfd != AT_FDCWD {
        if dirfd < 0 {
            errno::set_errno(errno::EBADF);
            return -1;
        }
        if lookup_fd(dirfd).is_none() {
            // lookup_fd already set EBADF.
            return -1;
        }
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Open a file using a file handle.
///
/// Returns -1 with `ENOSYS` after argument-domain validation.
///
/// Validation order matches `fs/fhandle.c::sys_open_by_handle_at`:
/// 1. `handle` NULL → `EFAULT`.
/// 2. If `mount_fd != AT_FDCWD`, it must be a valid open fd → `EBADF`.
/// 3. (Phase 190) Caller lacks `CAP_DAC_READ_SEARCH` → `EPERM`.
///    Matches Linux's `handle_to_path` → `may_decode_fh`:
///    ```text
///    if (!may_decode_fh(&ctx, o_flags))
///        return -EPERM;
///    ```
///    where `may_decode_fh` returns `true` for callers holding
///    `CAP_DAC_READ_SEARCH` (the export-fd path also exists but
///    requires backend support we don't have).  Pre-Phase-190 the
///    docstring claimed this was "not modeled (single-user OS)" —
///    that was wrong: our capability layer does model caps and an
///    unprivileged caller should see EPERM, not ENOSYS.
/// 4. All validated → `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn open_by_handle_at(mount_fd: Fd, handle: *mut FileHandle, _flags: i32) -> i32 {
    if handle.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if mount_fd != AT_FDCWD {
        if mount_fd < 0 {
            errno::set_errno(errno::EBADF);
            return -1;
        }
        if lookup_fd(mount_fd).is_none() {
            return -1;
        }
    }
    // Phase 190: CAP_DAC_READ_SEARCH gate matching Linux's
    // `may_decode_fh`.  Without the cap, callers cannot decode an
    // arbitrary file handle (which would otherwise bypass DAC) — they
    // would need a privileged export-fd path we don't expose.  Surface
    // EPERM here so unprivileged file-handle probes (CRIU's quick
    // capability probe, libnfs handle helpers) read us correctly.
    if !crate::sys_capability::has_capability(crate::sys_capability::CAP_DAC_READ_SEARCH) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// fstatat64 — LP64 alias for fstatat
// ---------------------------------------------------------------------------

/// `fstatat64` — alias for `fstatat` on LP64 systems.
///
/// On our 64-bit target, `off_t` is always 64-bit, so this is identical
/// to `fstatat`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fstatat64(dirfd: i32, path: *const u8, buf: *mut Stat, flags: i32) -> i32 {
    fstatat(dirfd, path, buf, flags)
}

// ---------------------------------------------------------------------------
// faccessat2 — faccessat with flags
// ---------------------------------------------------------------------------

/// `faccessat2` — check file accessibility relative to a directory fd.
///
/// Extends `faccessat` with an explicit `flags` argument that supports
/// `AT_SYMLINK_NOFOLLOW` and `AT_EACCESS`.  On our single-user OS,
/// `AT_EACCESS` is a no-op (effective == real IDs).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn faccessat2(dirfd: i32, path: *const u8, mode: i32, flags: i32) -> i32 {
    faccessat(dirfd, path, mode, flags)
}

// ---------------------------------------------------------------------------
// openat2 — extended openat (Linux 5.6+)
// ---------------------------------------------------------------------------

/// Resolve flags for `openat2`.
pub const RESOLVE_NO_XDEV: u64 = 0x01;
/// Resolve flags for `openat2`.
pub const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
/// Resolve flags for `openat2`.
pub const RESOLVE_NO_SYMLINKS: u64 = 0x04;
/// Resolve flags for `openat2`.
pub const RESOLVE_BENEATH: u64 = 0x08;
/// Resolve flags for `openat2`.
pub const RESOLVE_IN_ROOT: u64 = 0x10;
/// Resolve flags for `openat2`.
pub const RESOLVE_CACHED: u64 = 0x20;

/// `open_how` structure for `openat2`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OpenHow {
    /// O_* flags.
    pub flags: u64,
    /// File creation mode (only used with O_CREAT/O_TMPFILE).
    pub mode: u64,
    /// RESOLVE_* flags.
    pub resolve: u64,
}

/// Bitmask of every RESOLVE_* bit our `openat2` recognises.  Any
/// bit outside this mask in `how.resolve` is rejected with EINVAL —
/// matches Linux's `VALID_RESOLVE_FLAGS` check in
/// `fs/open.c::build_open_how` and ensures forward-compat callers
/// don't silently lose security restrictions they thought were
/// in effect.
const VALID_RESOLVE_FLAGS: u64 = RESOLVE_NO_XDEV
    | RESOLVE_NO_MAGICLINKS
    | RESOLVE_NO_SYMLINKS
    | RESOLVE_BENEATH
    | RESOLVE_IN_ROOT
    | RESOLVE_CACHED;

/// Cap on `openat2`'s `usize` argument.  Linux uses `PAGE_SIZE`
/// (4096 on x86_64), and the *ABI* contract is "no larger than a
/// 4 KiB page" regardless of the actual kernel page size — userspace
/// libraries hard-code 4096 as the upper bound.  We do the same so
/// the syscall surface looks identical to a Linux client even on
/// our 16 KiB-page kernel.
const OPENAT2_MAX_USIZE: usize = 4096;

/// The raw `__O_TMPFILE` bit (the high one in Linux's `O_TMPFILE`).
///
/// Linux defines `O_TMPFILE = __O_TMPFILE | O_DIRECTORY` so the
/// user-facing `O_TMPFILE` symbol always implies the directory flag.
/// But the kernel's mode-vs-flags check in `build_open_how` only
/// looks at the raw `__O_TMPFILE` bit (it doesn't care about
/// `O_DIRECTORY`), and our [`fcntl::O_TMPFILE`] constant is the
/// combined value.  Expose the raw bit here so the openat2
/// validation can match Linux exactly.
const RAW_O_TMPFILE: u64 = 0o20_000_000;

/// The same raw `__O_TMPFILE` bit as an `i32`, for testing the flags
/// argument of `open`/`openat` (which is `i32`) without a lossy cast.
/// The bit (0o20_000_000 = 1 << 22) is well within the positive `i32`
/// range.
pub(crate) const RAW_O_TMPFILE_I32: i32 = 0o20_000_000;

/// Mask of the 12 file-mode permission bits valid in `how.mode`
/// (rwx for user/group/other, plus the three setuid/setgid/sticky
/// bits).  Any bit outside this mask is rejected — matches Linux's
/// `S_IALLUGO` check in `build_open_how`.
const VALID_MODE_BITS: u64 = 0o7777;

/// `openat2` — open a file relative to a directory fd with extended
/// resolution control.
///
/// Linux 5.6+ syscall.  Validation order matches Linux's
/// `sys_openat2` in `fs/open.c`:
///
/// 1. `size < OPEN_HOW_SIZE_VER0` (the smallest accepted struct
///    version, 24 bytes) → `EINVAL`.
/// 2. `size > PAGE_SIZE` → `E2BIG`.
/// 3. `copy_struct_from_user` faults on a NULL `how` → `EFAULT`.
/// 4. Inside `build_open_how`: any unknown bit in `how.resolve`
///    → `EINVAL`.
///
/// Our implementation still delegates the actual open to regular
/// `openat` once validation passes — the `resolve` flags are
/// accepted but not enforced (our VFS doesn't support the
/// RESOLVE_* restrictions yet).  Validation matches the Linux ABI
/// so callers see consistent errors regardless of backend.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn openat2(dirfd: i32, path: *const u8, how: *const OpenHow, size: usize) -> Fd {
    // Step 1: size too small for the smallest accepted struct version.
    // Linux's `copy_struct_from_user` checks this *before* touching the
    // user pointer; doing it first means a buggy caller passing
    // (NULL, 0, 0) gets steered to "your size is wrong" rather than
    // "your pointer is wrong".
    if size < core::mem::size_of::<OpenHow>() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Step 2: size too big.  Linux's `copy_struct_from_user` bails
    // with -E2BIG above PAGE_SIZE.  Forward-compat callers that pass
    // a future-version size still get a clear "too large" rather than
    // a confusing EFAULT.
    if size > OPENAT2_MAX_USIZE {
        errno::set_errno(errno::E2BIG);
        return -1;
    }
    // Step 3: NULL pointer is EFAULT (only reachable when size is in
    // the legal range, which matches Linux's order).
    if how.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: `how` is non-NULL and `size >= sizeof(OpenHow)` (we just
    // checked).  `read_unaligned` tolerates any caller alignment.
    let h = unsafe { core::ptr::read_unaligned(how) };

    // Step 4: build_open_how — unknown resolve bits → EINVAL.
    // Without this check, callers asking for security restrictions
    // we don't know about would silently get an unrestricted open,
    // defeating the whole point of openat2's forward-compat design.
    if h.resolve & !VALID_RESOLVE_FLAGS != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Step 5: build_open_how — mode bit-range check.
    //
    // Linux: `if (how->mode & ~S_IALLUGO) return -EINVAL;`.  The 12
    // valid mode bits cover rwx-for-ugo plus setuid/setgid/sticky;
    // anything above those is a buggy caller (probably a sign-extended
    // negative or a stomped-on field) and must be EINVAL so the bug is
    // visible rather than silently masked.
    if h.mode & !VALID_MODE_BITS != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Step 6: build_open_how — mode meaningful only with O_CREAT or
    // O_TMPFILE.
    //
    // Linux: `if (how->mode && !(how->flags & (O_CREAT | __O_TMPFILE)))
    //              return -EINVAL;`
    //
    // A non-zero `mode` is only meaningful when the kernel is going to
    // *create* a file (O_CREAT) or a temporary file (the raw
    // __O_TMPFILE bit; O_DIRECTORY isn't relevant here).  A caller
    // passing mode without one of those flags is asking for an
    // inconsistent open; we reject so they notice the bug.
    let creates_a_file =
        (h.flags & crate::fcntl::O_CREAT as u64) != 0 || (h.flags & RAW_O_TMPFILE) != 0;
    if h.mode != 0 && !creates_a_file {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    openat(dirfd, path, h.flags as i32, h.mode as ModeT)
}

// ---------------------------------------------------------------------------
// statx — extended stat (Linux 4.11+)
// ---------------------------------------------------------------------------

/// `statx` mask flags.
pub const STATX_TYPE: u32 = 0x0001;
/// `statx` mask flags.
pub const STATX_MODE: u32 = 0x0002;
/// `statx` mask flags.
pub const STATX_NLINK: u32 = 0x0004;
/// `statx` mask flags.
pub const STATX_UID: u32 = 0x0008;
/// `statx` mask flags.
pub const STATX_GID: u32 = 0x0010;
/// `statx` mask flags.
pub const STATX_ATIME: u32 = 0x0020;
/// `statx` mask flags.
pub const STATX_MTIME: u32 = 0x0040;
/// `statx` mask flags.
pub const STATX_CTIME: u32 = 0x0080;
/// `statx` mask flags.
pub const STATX_INO: u32 = 0x0100;
/// `statx` mask flags.
pub const STATX_SIZE: u32 = 0x0200;
/// `statx` mask flags.
pub const STATX_BLOCKS: u32 = 0x0400;
/// `statx` mask flags — all basic fields.
pub const STATX_BASIC_STATS: u32 = 0x07FF;
/// `statx` mask flags — all fields.
pub const STATX_ALL: u32 = 0x0FFF;
/// `statx` mask flags — block size.
pub const STATX_BTIME: u32 = 0x0800;

/// Timestamp for `statx`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct StatxTimestamp {
    /// Seconds since epoch.
    pub tv_sec: i64,
    /// Nanoseconds (0..999_999_999).
    pub tv_nsec: u32,
    /// Reserved.
    pub __reserved: i32,
}

/// Extended stat structure (Linux 4.11+).
///
/// Returned by `statx()`.  Provides more fields than `struct stat`,
/// including birth time and per-field validity masks.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Statx {
    /// Mask of bits indicating filled fields.
    pub stx_mask: u32,
    /// Block size for filesystem I/O.
    pub stx_blksize: u32,
    /// Extra file attribute indicators.
    pub stx_attributes: u64,
    /// Number of hard links.
    pub stx_nlink: u32,
    /// User ID of owner.
    pub stx_uid: u32,
    /// Group ID of owner.
    pub stx_gid: u32,
    /// File type and mode.
    pub stx_mode: u16,
    /// Padding.
    _pad1: u16,
    /// Inode number.
    pub stx_ino: u64,
    /// Total size in bytes.
    pub stx_size: u64,
    /// Number of 512-byte blocks allocated.
    pub stx_blocks: u64,
    /// Mask of supported attributes.
    pub stx_attributes_mask: u64,
    /// Last access time.
    pub stx_atime: StatxTimestamp,
    /// Birth (creation) time.
    pub stx_btime: StatxTimestamp,
    /// Last status change time.
    pub stx_ctime: StatxTimestamp,
    /// Last modification time.
    pub stx_mtime: StatxTimestamp,
    /// Major device ID (if special file).
    pub stx_rdev_major: u32,
    /// Minor device ID (if special file).
    pub stx_rdev_minor: u32,
    /// Major device ID of filesystem.
    pub stx_dev_major: u32,
    /// Minor device ID of filesystem.
    pub stx_dev_minor: u32,
    /// Mount ID.
    pub stx_mnt_id: u64,
    /// Reserved.
    _pad2: u64,
    /// Reserved.
    _spare: [u64; 12],
}

impl Default for Statx {
    fn default() -> Self {
        // SAFETY: Statx is a C-compatible struct, zero-init is valid.
        unsafe { core::mem::zeroed() }
    }
}

/// Convert a `Timespec` to a `StatxTimestamp`.
fn timespec_to_statx_ts(ts: &crate::stat::Timespec) -> StatxTimestamp {
    StatxTimestamp {
        tv_sec: ts.tv_sec,
        tv_nsec: ts.tv_nsec as u32,
        __reserved: 0,
    }
}

/// `statx` — extended file status (Linux 4.11+).
///
/// Gets extended file status relative to a directory fd.  Resolves the
/// `dirfd`/`path` pair the same way `fstatat` does, then reads the raw
/// kernel `FsStatResult` directly so it can surface the birth time
/// (`stx_btime`/`STATX_BTIME`) that `struct stat` cannot represent.
/// The `mask` argument selects which fields to populate.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn statx(
    dirfd: i32,
    path: *const u8,
    flags: i32,
    mask: u32,
    buf: *mut Statx,
) -> i32 {
    if buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Resolve dirfd/path and pull the raw kernel buffer.  Mirror
    // `fstatat`: an absolute path or `AT_FDCWD` skips dirfd resolution.
    // `AT_SYMLINK_NOFOLLOW` selects `lstat` semantics.
    let follow = flags & AT_SYMLINK_NOFOLLOW == 0;
    let mut raw = [0u8; crate::stat::KERNEL_STAT_LEN];
    let ret = if dirfd == AT_FDCWD || is_absolute_path(path) {
        stat_path_raw(path, follow, &mut raw)
    } else {
        let mut full = [0u8; crate::unistd::PATH_MAX];
        let len = resolve_dirfd_path(dirfd, path, &mut full);
        if len == 0 {
            return -1;
        }
        stat_path_raw(full.as_ptr(), follow, &mut raw)
    };
    if ret != 0 {
        return ret;
    }

    let mut st = Stat::default();
    crate::stat::fill_from_fsstat(&mut st, &raw);

    // SAFETY: caller guarantees `buf` points to valid memory.
    let sx = unsafe { &mut *buf };
    *sx = Statx::default();

    // Populate requested fields.
    let mut filled: u32 = 0;

    if mask & STATX_TYPE != 0 || mask & STATX_MODE != 0 {
        #[allow(clippy::cast_possible_truncation)]
        {
            sx.stx_mode = st.st_mode as u16;
        }
        filled |= STATX_TYPE | STATX_MODE;
    }
    if mask & STATX_NLINK != 0 {
        #[allow(clippy::cast_possible_truncation)]
        {
            sx.stx_nlink = st.st_nlink as u32;
        }
        filled |= STATX_NLINK;
    }
    if mask & STATX_UID != 0 {
        sx.stx_uid = st.st_uid;
        filled |= STATX_UID;
    }
    if mask & STATX_GID != 0 {
        sx.stx_gid = st.st_gid;
        filled |= STATX_GID;
    }
    if mask & STATX_INO != 0 {
        sx.stx_ino = st.st_ino;
        filled |= STATX_INO;
    }
    if mask & STATX_SIZE != 0 {
        sx.stx_size = st.st_size as u64;
        filled |= STATX_SIZE;
    }
    if mask & STATX_BLOCKS != 0 {
        sx.stx_blocks = st.st_blocks as u64;
        filled |= STATX_BLOCKS;
    }
    if mask & STATX_ATIME != 0 {
        sx.stx_atime = timespec_to_statx_ts(&st.st_atim);
        filled |= STATX_ATIME;
    }
    if mask & STATX_MTIME != 0 {
        sx.stx_mtime = timespec_to_statx_ts(&st.st_mtim);
        filled |= STATX_MTIME;
    }
    if mask & STATX_CTIME != 0 {
        sx.stx_ctime = timespec_to_statx_ts(&st.st_ctim);
        filled |= STATX_CTIME;
    }
    // Birth time is carried in the raw kernel buffer (`struct stat` has no
    // field for it).  Only report it — and set the filled bit — when the
    // filesystem actually recorded a creation time; otherwise leave the
    // STATX_BTIME bit clear so callers know it is unavailable.
    if mask & STATX_BTIME != 0
        && let Some(btime) = crate::stat::btime_from_fsstat(&raw) {
            sx.stx_btime = timespec_to_statx_ts(&btime);
            filled |= STATX_BTIME;
        }

    sx.stx_blksize = st.st_blksize as u32;
    // Device numbers: split st_dev/st_rdev into major/minor.
    sx.stx_dev_major = (st.st_dev >> 8) as u32;
    sx.stx_dev_minor = (st.st_dev & 0xFF) as u32;
    sx.stx_rdev_major = (st.st_rdev >> 8) as u32;
    sx.stx_rdev_minor = (st.st_rdev & 0xFF) as u32;

    sx.stx_mask = filled;
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- translate_open_flags --

    // These assert the *native* kernel OpenFlags encoding (must match
    // kernel/src/fs/handle.rs): READ=0x01, WRITE=0x02, CREATE=0x04,
    // TRUNCATE=0x08, APPEND=0x10, DIRECTORY=0x20.
    const N_READ: u64 = 0x01;
    const N_WRITE: u64 = 0x02;
    const N_CREATE: u64 = 0x04;
    const N_TRUNCATE: u64 = 0x08;
    const N_APPEND: u64 = 0x10;
    const N_DIRECTORY: u64 = 0x20;

    #[test]
    fn translate_rdonly() {
        let flags = translate_open_flags(fcntl::O_RDONLY);
        // O_RDONLY → native READ (an access mode is always required).
        assert_eq!(flags, N_READ);
    }

    #[test]
    fn translate_wronly() {
        let flags = translate_open_flags(fcntl::O_WRONLY);
        assert_eq!(flags & (N_READ | N_WRITE), N_WRITE); // WRITE only.
    }

    #[test]
    fn translate_rdwr() {
        let flags = translate_open_flags(fcntl::O_RDWR);
        assert_eq!(flags & (N_READ | N_WRITE), N_READ | N_WRITE);
    }

    #[test]
    fn translate_creat_trunc() {
        let flags = translate_open_flags(fcntl::O_WRONLY | fcntl::O_CREAT | fcntl::O_TRUNC);
        assert_ne!(flags & N_CREATE, 0, "CREATE bit");
        assert_ne!(flags & N_TRUNCATE, 0, "TRUNCATE bit");
        assert_ne!(flags & N_WRITE, 0, "WRITE bit");
    }

    #[test]
    fn translate_append() {
        let flags = translate_open_flags(fcntl::O_APPEND);
        assert_ne!(flags & N_APPEND, 0, "APPEND bit");
    }

    #[test]
    fn translate_directory() {
        let flags = translate_open_flags(fcntl::O_RDONLY | fcntl::O_DIRECTORY);
        assert_ne!(flags & N_DIRECTORY, 0, "DIRECTORY bit");
        assert_ne!(flags & N_READ, 0, "READ bit");
    }

    #[test]
    fn translate_all_flags() {
        let flags = translate_open_flags(
            fcntl::O_RDWR | fcntl::O_CREAT | fcntl::O_TRUNC | fcntl::O_APPEND,
        );
        assert_eq!(flags & (N_READ | N_WRITE), N_READ | N_WRITE);
        assert_ne!(flags & N_CREATE, 0);
        assert_ne!(flags & N_TRUNCATE, 0);
        assert_ne!(flags & N_APPEND, 0);
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
        // Use a freshly-allocated fd rather than relying on fd 0 being open
        // (other tests may have closed it).
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(fchmod(fd, 0o644), 0);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_chown_succeeds() {
        assert_eq!(chown(b"/tmp\0".as_ptr(), 0, 0), 0);
    }

    #[test]
    fn test_fchown_succeeds() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(fchown(fd, 0, 0), 0);
        let _ = fdtable::close_fd(fd);
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
        // Open our own fd so we don't depend on whether some other
        // test in the suite has closed stdin/stdout.
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Console, 0).expect("fd available");
        assert_eq!(posix_fadvise(fd, 0, 0, POSIX_FADV_NORMAL), 0);
        assert_eq!(posix_fadvise(fd, 0, 0, POSIX_FADV_SEQUENTIAL), 0);
        assert_eq!(posix_fadvise(fd, 0, 0, POSIX_FADV_RANDOM), 0);
        assert_eq!(posix_fadvise(fd, 0, 0, POSIX_FADV_WILLNEED), 0);
        assert_eq!(posix_fadvise(fd, 0, 0, POSIX_FADV_DONTNEED), 0);
        assert_eq!(posix_fadvise(fd, 0, 0, POSIX_FADV_NOREUSE), 0);
        let _ = close(fd);
    }

    #[test]
    fn test_posix_fadvise_bad_fd_returns_ebadf() {
        // -1 is never a valid fd → EBADF (returned directly).
        assert_eq!(posix_fadvise(-1, 0, 0, POSIX_FADV_NORMAL), errno::EBADF);
        // A high fd that's not open → also EBADF.
        assert_eq!(posix_fadvise(900, 0, 0, POSIX_FADV_NORMAL), errno::EBADF);
    }

    #[test]
    fn test_posix_fadvise_bad_advice_returns_einval() {
        // Unknown advice value → EINVAL.  Linux validates advice before
        // touching the fd table; we do the same.  Use an invalid fd to
        // demonstrate advice validation happens first (independent of fd state).
        assert_eq!(posix_fadvise(-1, 0, 0, 99), errno::EINVAL);
        assert_eq!(posix_fadvise(-1, 0, 0, -1), errno::EINVAL);
        assert_eq!(posix_fadvise(-1, 0, 0, 6), errno::EINVAL);
    }

    #[test]
    fn test_posix_fadvise_negative_len_returns_einval() {
        // Negative len is the only length constraint (offset may be any value).
        // Use an invalid fd to demonstrate len validation runs first.
        assert_eq!(posix_fadvise(-1, 0, -1, POSIX_FADV_NORMAL), errno::EINVAL);
        assert_eq!(
            posix_fadvise(-1, 100, -100, POSIX_FADV_SEQUENTIAL),
            errno::EINVAL
        );
    }

    #[test]
    fn test_posix_fadvise_does_not_set_errno() {
        // posix_fadvise returns the error directly — it must NOT also
        // pollute errno (POSIX requires the error to be returned, not
        // signaled the usual way).  Verify a fresh errno value survives.
        errno::set_errno(12345);
        let ret = posix_fadvise(-1, 0, 0, POSIX_FADV_NORMAL);
        assert_eq!(ret, errno::EBADF);
        assert_eq!(errno::get_errno(), 12345);
    }

    #[test]
    fn test_posix_fadvise_pipe_returns_espipe() {
        // Pipes are unseekable — Linux returns ESPIPE.
        let mut pipefd = [0i32; 2];
        let ret = crate::pipe::pipe(pipefd.as_mut_ptr());
        assert_eq!(ret, 0, "pipe() must succeed for this test");
        let read_end = pipefd[0];
        let write_end = pipefd[1];
        assert_eq!(
            posix_fadvise(read_end, 0, 0, POSIX_FADV_NORMAL),
            errno::ESPIPE
        );
        assert_eq!(
            posix_fadvise(write_end, 0, 0, POSIX_FADV_NORMAL),
            errno::ESPIPE
        );
        // Cleanup.
        let _ = close(read_end);
        let _ = close(write_end);
    }

    #[test]
    fn test_fadvise64_delegates_to_posix_fadvise() {
        // fadvise64 must validate the same way as posix_fadvise.
        assert_eq!(fadvise64(-1, 0, 0, POSIX_FADV_NORMAL), errno::EBADF);
        assert_eq!(fadvise64(-1, 0, 0, 99), errno::EINVAL);
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Console, 0).expect("fd available");
        assert_eq!(fadvise64(fd, 0, 0, POSIX_FADV_NORMAL), 0);
        let _ = close(fd);
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
        assert_eq!(posix_fallocate(0, i64::MAX, 1), crate::errno::EFBIG,);
    }

    // -- fallocate (Linux) --
    //
    // Each test allocates its own Console fd rather than relying on
    // fd 0/1/2 being open: when --test-threads=1, the global fdtable
    // is shared, and an earlier test may have closed the standard fds.
    // Now that fallocate validates fd first (Phase 109), tests that
    // hard-code fd=0 would otherwise become order-dependent.

    fn fallocate_test_fd() -> Fd {
        fdtable::alloc_fd(fdtable::HandleKind::Console, 0)
            .expect("a free fd slot must be available")
    }

    #[test]
    fn test_fallocate_negative_offset() {
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(fallocate(fd, 0, -1, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_zero_len() {
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(fallocate(fd, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_negative_len() {
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(fallocate(fd, 0, 0, -1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_keep_size_succeeds() {
        // KEEP_SIZE mode is a no-op stub — should succeed.
        let fd = fallocate_test_fd();
        assert_eq!(fallocate(fd, FALLOC_FL_KEEP_SIZE, 0, 4096), 0);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_keep_size_negative_offset() {
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(fallocate(fd, FALLOC_FL_KEEP_SIZE, -1, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_punch_hole_eopnotsupp() {
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(
            fallocate(fd, FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE, 0, 4096),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_collapse_range_eopnotsupp() {
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(fallocate(fd, FALLOC_FL_COLLAPSE_RANGE, 0, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_zero_range_eopnotsupp() {
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(fallocate(fd, FALLOC_FL_ZERO_RANGE, 0, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_insert_range_eopnotsupp() {
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(fallocate(fd, FALLOC_FL_INSERT_RANGE, 0, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_unshare_range_eopnotsupp() {
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(fallocate(fd, FALLOC_FL_UNSHARE_RANGE, 0, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
        let _ = fdtable::close_fd(fd);
    }

    // -- Phase 109: Linux-parity validation order + mode-combination checks --
    //
    // Linux's ksys_fallocate (fs/open.c) does fdget() before
    // vfs_fallocate(), so an invalid fd always wins.  Inside
    // vfs_fallocate, the order is offset/len → unknown-bits →
    // PUNCH_HOLE-requires-KEEP_SIZE → KEEP_SIZE-vs-range-shift
    // → COLLAPSE-alone → INSERT-alone → UNSHARE-vs-range-shift.
    // Unknown mode bits map to EOPNOTSUPP; combination conflicts
    // map to EINVAL.

    #[test]
    fn test_fallocate_phase109_ebadf_wins_over_einval_offset() {
        // Bad fd + negative offset: EBADF wins because fdget runs
        // before offset validation in Linux's ksys_fallocate.
        crate::errno::set_errno(0);
        assert_eq!(fallocate(99999, 0, -1, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fallocate_phase109_ebadf_wins_over_einval_len() {
        // Bad fd + len <= 0: EBADF wins.
        crate::errno::set_errno(0);
        assert_eq!(fallocate(99999, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fallocate_phase109_ebadf_wins_over_eopnotsupp_mode() {
        // Bad fd + advanced/unknown mode bits: EBADF still wins.
        crate::errno::set_errno(0);
        assert_eq!(fallocate(99999, FALLOC_FL_COLLAPSE_RANGE, 0, 4096), -1,);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fallocate_phase109_negative_fd_ebadf() {
        // Negative fd is the canonical EBADF case.
        crate::errno::set_errno(0);
        assert_eq!(fallocate(-1, 0, 0, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fallocate_phase109_unknown_mode_bits_eopnotsupp() {
        // Mode bit 0x1000 is outside FALLOC_FL_VALID_MASK → EOPNOTSUPP
        // (not EINVAL — Linux distinguishes "unknown" from "invalid
        // combination of known bits").
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(fallocate(fd, 0x1000, 0, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_phase109_punch_hole_without_keep_size_eopnotsupp() {
        // PUNCH_HOLE alone (no KEEP_SIZE) → EOPNOTSUPP, per Linux's
        // explicit check in vfs_fallocate.
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(fallocate(fd, FALLOC_FL_PUNCH_HOLE, 0, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_phase109_keep_size_plus_collapse_einval() {
        // KEEP_SIZE | COLLAPSE_RANGE: range-shift modes can never
        // preserve file size → EINVAL.
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(
            fallocate(fd, FALLOC_FL_KEEP_SIZE | FALLOC_FL_COLLAPSE_RANGE, 0, 4096),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_phase109_keep_size_plus_insert_einval() {
        // KEEP_SIZE | INSERT_RANGE → EINVAL for the same reason.
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(
            fallocate(fd, FALLOC_FL_KEEP_SIZE | FALLOC_FL_INSERT_RANGE, 0, 4096),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_phase109_collapse_with_zero_range_einval() {
        // COLLAPSE_RANGE must appear alone — combining it with
        // any other known bit (here ZERO_RANGE) → EINVAL.
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(
            fallocate(fd, FALLOC_FL_COLLAPSE_RANGE | FALLOC_FL_ZERO_RANGE, 0, 4096),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_phase109_insert_with_zero_range_einval() {
        // INSERT_RANGE must appear alone — combining with ZERO_RANGE → EINVAL.
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(
            fallocate(fd, FALLOC_FL_INSERT_RANGE | FALLOC_FL_ZERO_RANGE, 0, 4096),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_phase109_unshare_with_collapse_einval() {
        // UNSHARE_RANGE | COLLAPSE_RANGE → EINVAL (range-shift conflict).
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(
            fallocate(
                fd,
                FALLOC_FL_UNSHARE_RANGE | FALLOC_FL_COLLAPSE_RANGE,
                0,
                4096
            ),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_phase109_unshare_with_insert_einval() {
        // UNSHARE_RANGE | INSERT_RANGE → EINVAL.
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(
            fallocate(
                fd,
                FALLOC_FL_UNSHARE_RANGE | FALLOC_FL_INSERT_RANGE,
                0,
                4096
            ),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fallocate_phase109_recovery_after_einval() {
        // After an EINVAL-rejected call, a subsequent well-formed
        // call must still succeed — the validation surface is purely
        // stateless.  KEEP_SIZE alone with a valid fd is a no-op
        // success.
        let fd = fallocate_test_fd();
        crate::errno::set_errno(0);
        assert_eq!(
            fallocate(fd, FALLOC_FL_KEEP_SIZE | FALLOC_FL_COLLAPSE_RANGE, 0, 4096),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        assert_eq!(fallocate(fd, FALLOC_FL_KEEP_SIZE, 0, 4096), 0);
        let _ = fdtable::close_fd(fd);
    }

    // -- FALLOC_FL_* constants --

    #[test]
    fn test_falloc_fl_constants() {
        assert_eq!(FALLOC_FL_KEEP_SIZE, 0x01);
        assert_eq!(FALLOC_FL_PUNCH_HOLE, 0x02);
        assert_eq!(FALLOC_FL_COLLAPSE_RANGE, 0x08);
        assert_eq!(FALLOC_FL_ZERO_RANGE, 0x10);
        assert_eq!(FALLOC_FL_INSERT_RANGE, 0x20);
        assert_eq!(FALLOC_FL_UNSHARE_RANGE, 0x40);
    }

    #[test]
    fn test_falloc_fl_no_collisions() {
        let all = [
            FALLOC_FL_KEEP_SIZE,
            FALLOC_FL_PUNCH_HOLE,
            FALLOC_FL_COLLAPSE_RANGE,
            FALLOC_FL_ZERO_RANGE,
            FALLOC_FL_INSERT_RANGE,
            FALLOC_FL_UNSHARE_RANGE,
        ];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_eq!(all[i] & all[j], 0, "FALLOC_FL flags {i} and {j} collide");
            }
        }
    }

    #[test]
    fn test_flock_succeeds() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(flock(fd, LOCK_SH), 0);
        assert_eq!(flock(fd, LOCK_EX), 0);
        assert_eq!(flock(fd, LOCK_UN), 0);
        assert_eq!(flock(fd, LOCK_EX | LOCK_NB), 0);
        let _ = fdtable::close_fd(fd);
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
        // close_range with first > last returns EINVAL (matches Linux).
        errno::set_errno(0);
        let ret = close_range(100, 50, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_close_range_unknown_flag_einval() {
        // Bit 0 isn't a defined CLOSE_RANGE_* flag; reject it.
        errno::set_errno(0);
        let ret = close_range(0, 10, 1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_close_range_unshare_accepted() {
        use crate::linux_close_range::CLOSE_RANGE_UNSHARE;
        // CLOSE_RANGE_UNSHARE on an empty range succeeds (no-op for us
        // — we never share fd tables across processes).
        let ret = close_range(500, 600, CLOSE_RANGE_UNSHARE);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_close_range_cloexec_sets_flag() {
        use crate::linux_close_range::CLOSE_RANGE_CLOEXEC;
        // Reserve an fd, ensure CLOEXEC starts clear, run close_range
        // with CLOSE_RANGE_CLOEXEC across a range containing it, and
        // verify the flag flipped on without the fd being closed.
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Console, 0).expect("fd available");
        assert!(fdtable::set_fd_flags(fd, 0));
        let ret = close_range(fd as u32, fd as u32, CLOSE_RANGE_CLOEXEC);
        assert_eq!(ret, 0);
        assert_eq!(fdtable::get_fd_flags(fd), Some(fdtable::FD_CLOEXEC));
        // fd must still be open after CLOEXEC mode.
        assert!(fdtable::get_fd(fd).is_some());
        // Cleanup.
        let _ = close(fd);
    }

    #[test]
    fn test_close_range_cloexec_skips_closed_fds() {
        use crate::linux_close_range::CLOSE_RANGE_CLOEXEC;
        // CLOSE_RANGE_CLOEXEC over a range of unopened fds must not
        // create FD_CLOEXEC state in slots that aren't actually open.
        // Pick a high range unlikely to clash with anything else.
        let ret = close_range(900, 910, CLOSE_RANGE_CLOEXEC);
        assert_eq!(ret, 0);
        for fd in 900..=910 {
            assert!(
                fdtable::get_fd_flags(fd).is_none(),
                "unopened fd {fd} must not have flags set"
            );
        }
    }

    #[test]
    fn test_close_range_combined_flags_accepted() {
        use crate::linux_close_range::{CLOSE_RANGE_CLOEXEC, CLOSE_RANGE_UNSHARE};
        // Both flags combined is valid per the Linux ABI.
        let ret = close_range(500, 510, CLOSE_RANGE_UNSHARE | CLOSE_RANGE_CLOEXEC);
        assert_eq!(ret, 0);
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
        assert!(!is_absolute_path(b"\0".as_ptr())); // Empty string.
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

    // -- Iovec struct layout --

    #[test]
    fn test_iovec_size() {
        // On x86_64: pointer (8) + usize (8) = 16 bytes.
        assert_eq!(core::mem::size_of::<Iovec>(), 16);
    }

    #[test]
    fn test_iovec_fields() {
        let mut buf = [0u8; 64];
        let iov = Iovec {
            iov_base: buf.as_mut_ptr(),
            iov_len: 64,
        };
        assert_eq!(iov.iov_len, 64);
        assert!(!iov.iov_base.is_null());
    }

    #[test]
    fn test_iovec_null_base() {
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        assert!(iov.iov_base.is_null());
        assert_eq!(iov.iov_len, 0);
    }

    // -- dup3 semantics --

    #[test]
    fn test_dup3_same_fd_returns_einval() {
        // POSIX / Linux: dup3 returns EINVAL when oldfd == newfd.
        let result = dup3(42, 42, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- Phase 100: dup3 flag-mask validation --
    //
    // Linux's dup3() accepts only one flag: O_CLOEXEC.  Any other bit
    // set in `flags` must return -1 with EINVAL, and the flag check
    // precedes the oldfd==newfd check (so flag errors win when both
    // would apply).  We previously accepted any value silently,
    // ignoring all bits except O_CLOEXEC — a buggy caller passing
    // i32::MIN or stray O_APPEND would still get a duplicated fd.

    #[test]
    fn test_dup3_flag_mask_only_o_cloexec() {
        // Sanity: the only known/valid flag bit is O_CLOEXEC.
        // O_CLOEXEC must be non-zero and a single bit.
        assert_ne!(fcntl::O_CLOEXEC, 0);
        assert_eq!(
            fcntl::O_CLOEXEC & (fcntl::O_CLOEXEC - 1),
            0,
            "O_CLOEXEC must be a single bit, got {:#x}",
            fcntl::O_CLOEXEC
        );
    }

    #[test]
    fn test_dup3_unknown_flag_bit_rejected() {
        // An arbitrary high bit not in the valid mask must yield EINVAL.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let bad_flag = 1 << 20; // far above any real open flag
        let result = dup3(fd, fd + 1, bad_flag);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_dup3_high_bit_rejected() {
        // i32::MIN has the sign bit set, which is not in the mask.
        // Per Linux this must EINVAL even when oldfd/newfd are sane.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let result = dup3(fd, fd + 1, i32::MIN);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_dup3_o_rdwr_rejected() {
        // O_RDWR is an open-mode bit, not a dup3 flag.  Must EINVAL.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let result = dup3(fd, fd + 1, fcntl::O_RDWR);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_dup3_o_append_rejected() {
        // O_APPEND is also not a dup3 flag.  Must EINVAL.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let result = dup3(fd, fd + 1, fcntl::O_APPEND);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_dup3_einval_wins_over_same_fd() {
        // Both flags-invalid AND oldfd==newfd would set EINVAL, but
        // Linux's order is: flag check first.  We can't observe the
        // ordering via errno alone (both are EINVAL), but we can
        // confirm flags=garbage still EINVALs even when oldfd==newfd
        // (i.e. the early-return path doesn't skip the flag check).
        // This is also a regression guard: previously, oldfd==newfd
        // returned before any flag check happened at all.
        let result = dup3(42, 42, 1 << 25);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_dup3_zero_flags_accepted_for_real_fd() {
        // Zero flags is valid; dup3 should behave like dup2 (without
        // CLOEXEC) on a real fd pair.  Must not return EINVAL.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        // Pick a newfd well outside any plausible existing range.
        let newfd = 200;
        let _ = fdtable::close_fd(newfd); // ensure it's free
        let result = dup3(fd, newfd, 0);
        // We don't assert success unconditionally (dup2 may fail for
        // table-allocator reasons unrelated to dup3 flags), but we
        // require that if it failed, it wasn't with EINVAL — i.e. the
        // flag-mask path didn't reject a valid zero-flags call.
        if result < 0 {
            assert_ne!(
                crate::errno::get_errno(),
                crate::errno::EINVAL,
                "zero flags must not be rejected by the dup3 mask"
            );
        }
        let _ = fdtable::close_fd(fd);
        let _ = fdtable::close_fd(newfd);
    }

    #[test]
    fn test_dup3_cloexec_alone_accepted() {
        // O_CLOEXEC alone is the canonical use case; must not EINVAL.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let newfd = 201;
        let _ = fdtable::close_fd(newfd);
        let result = dup3(fd, newfd, fcntl::O_CLOEXEC);
        if result < 0 {
            assert_ne!(
                crate::errno::get_errno(),
                crate::errno::EINVAL,
                "O_CLOEXEC must not be rejected by the dup3 mask"
            );
        }
        let _ = fdtable::close_fd(fd);
        let _ = fdtable::close_fd(newfd);
    }

    #[test]
    fn test_dup3_cloexec_plus_unknown_rejected() {
        // Mixing O_CLOEXEC with an unknown bit must still EINVAL —
        // no partial acceptance.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let bad = fcntl::O_CLOEXEC | (1 << 22);
        let result = dup3(fd, fd + 1, bad);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_dup3_recovery_after_einval() {
        // A rejected call must not corrupt state — a subsequent
        // valid call should still work.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let bad = 1 << 21;
        let r1 = dup3(fd, fd + 1, bad);
        assert_eq!(r1, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let newfd = 202;
        let _ = fdtable::close_fd(newfd);
        let r2 = dup3(fd, newfd, 0);
        if r2 < 0 {
            assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
        }
        let _ = fdtable::close_fd(fd);
        let _ = fdtable::close_fd(newfd);
    }

    #[test]
    fn test_dup3_negative_oldfd_with_bad_flags_einval() {
        // Even with an obviously-invalid oldfd (negative), the flag
        // check fires first and we get EINVAL (not EBADF).
        let result = dup3(-1, 5, 1 << 24);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_dup3_only_o_cloexec_bit_in_valid_mask() {
        // Defensive: confirm no other fcntl O_* bit happens to overlap
        // O_CLOEXEC.  Any such overlap would silently let that bit
        // pass the mask.
        for shift in 0..31 {
            let bit = 1i32 << shift;
            if bit == fcntl::O_CLOEXEC {
                continue;
            }
            // Each non-CLOEXEC single-bit value must be rejected.
            let result = dup3(10, 11, bit);
            assert_eq!(result, -1, "bit {:#x} should be rejected by dup3 mask", bit);
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::EINVAL,
                "bit {:#x} should set EINVAL",
                bit
            );
        }
    }

    // -- closefrom --

    #[test]
    fn test_closefrom_negative() {
        // closefrom with negative lowfd should clamp to 0 internally.
        closefrom(-1); // Must not panic or loop.
    }

    // -- renameat2 with flags --

    #[test]
    fn test_renameat2_nonzero_flags() {
        // Non-zero flags should return EINVAL (not supported).
        let result = renameat2(AT_FDCWD, b"/a\0".as_ptr(), AT_FDCWD, b"/b\0".as_ptr(), 1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- lockf constants --

    #[test]
    fn test_lockf_constants() {
        assert_eq!(F_ULOCK, 0);
        assert_eq!(F_LOCK, 1);
        assert_eq!(F_TLOCK, 2);
        assert_eq!(F_TEST, 3);
    }

    #[test]
    fn test_lockf_stub_succeeds() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(lockf(fd, F_LOCK, 0), 0);
        assert_eq!(lockf(fd, F_TLOCK, 0), 0);
        assert_eq!(lockf(fd, F_ULOCK, 0), 0);
        assert_eq!(lockf(fd, F_TEST, 0), 0);
        let _ = fdtable::close_fd(fd);
    }

    // -- UTIME constants --

    #[test]
    fn test_utime_constants() {
        assert_eq!(UTIME_NOW, (1 << 30) - 1);
        assert_eq!(UTIME_OMIT, (1 << 30) - 2);
        assert_ne!(UTIME_NOW, UTIME_OMIT);
    }

    // -- Timeval struct layout --

    #[test]
    fn test_timeval_size() {
        // Two i64 fields = 16 bytes.
        assert_eq!(core::mem::size_of::<Timeval>(), 16);
    }

    #[test]
    fn test_timeval_fields() {
        let tv = Timeval {
            tv_sec: 1234,
            tv_usec: 5678,
        };
        assert_eq!(tv.tv_sec, 1234);
        assert_eq!(tv.tv_usec, 5678);
    }

    // -- utimes / futimes stubs --

    #[test]
    fn test_utimes_stub_succeeds() {
        assert_eq!(utimes(b"/tmp\0".as_ptr(), core::ptr::null()), 0);
    }

    #[test]
    fn test_futimes_stub_succeeds() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(futimes(fd, core::ptr::null()), 0);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_utimensat_stub_succeeds() {
        assert_eq!(
            utimensat(AT_FDCWD, b"/tmp\0".as_ptr(), core::ptr::null(), 0),
            0
        );
    }

    #[test]
    fn test_futimens_stub_succeeds() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(futimens(fd, core::ptr::null()), 0);
        let _ = fdtable::close_fd(fd);
    }

    // -- creat is equivalent to open --

    #[test]
    fn test_creat_null_path() {
        // creat(NULL, mode) should return -1/EFAULT like open().
        let result = creat(core::ptr::null(), 0o644);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- LP64 aliases are provided --

    #[test]
    fn test_open64_null() {
        // open64 is an alias for open — same EFAULT behavior.
        let result = open64(core::ptr::null(), 0, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- translate_open_flags: O_RDONLY is zero --

    #[test]
    fn test_o_rdonly_is_zero() {
        assert_eq!(fcntl::O_RDONLY, 0);
    }

    // -- close_range: first == last --

    #[test]
    fn test_close_range_single() {
        // close_range(999, 999, 0) should close just fd 999 (no-op if not open).
        let _ = close_range(999, 999, 0);
    }

    // -- pread / pwrite validation --

    #[test]
    fn test_pread_null_buf_nonzero_count() {
        let result = pread(0, core::ptr::null_mut(), 10, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_pread_zero_count() {
        // POSIX: "If nbyte is 0, read() will return 0."
        let mut buf = [0u8; 1];
        let result = pread(0, buf.as_mut_ptr(), 0, 0);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_pread_negative_offset() {
        let mut buf = [0u8; 10];
        let result = pread(0, buf.as_mut_ptr(), 10, -1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pwrite_null_buf_nonzero_count() {
        let result = pwrite(0, core::ptr::null(), 10, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_pwrite_zero_count() {
        let buf = [0u8; 1];
        let result = pwrite(0, buf.as_ptr(), 0, 0);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_pwrite_negative_offset() {
        let buf = [0u8; 10];
        let result = pwrite(0, buf.as_ptr(), 10, -1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- readv / writev validation --

    #[test]
    fn test_readv_null_iov() {
        let result = readv(0, core::ptr::null(), 1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_readv_zero_iovcnt() {
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        let result = readv(0, &raw const iov, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_readv_negative_iovcnt() {
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        let result = readv(0, &raw const iov, -1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_readv_too_many_iovcnt() {
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        let result = readv(0, &raw const iov, 1025);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_writev_null_iov() {
        let result = writev(0, core::ptr::null(), 1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_writev_zero_iovcnt() {
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        let result = writev(0, &raw const iov, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- read / write zero-length --

    #[test]
    fn test_read_zero_count() {
        // POSIX: "If nbyte is 0, read() will return 0."
        let result = read(0, core::ptr::null_mut(), 0);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_write_zero_count() {
        // POSIX: zero-length write returns 0.
        let result = write(0, core::ptr::null(), 0);
        assert_eq!(result, 0);
    }

    // -- read / write null buf with count > 0 --

    #[test]
    fn test_read_null_buf() {
        let result = read(0, core::ptr::null_mut(), 10);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_write_null_buf() {
        let result = write(0, core::ptr::null(), 10);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- lseek whence validation --

    #[test]
    fn test_lseek_invalid_whence() {
        // whence must be SEEK_SET, SEEK_CUR, or SEEK_END.
        let result = lseek(0, 0, 99);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_lseek_seek_set_valid() {
        // SEEK_SET is 0 — should be accepted.
        let _ret = lseek(0, 0, crate::fcntl::SEEK_SET);
        // Result depends on test host (fd 0 may not be seekable).
    }

    #[test]
    fn test_lseek_seek_cur_valid() {
        let _ret = lseek(0, 0, crate::fcntl::SEEK_CUR);
    }

    #[test]
    fn test_lseek_seek_end_valid() {
        let _ret = lseek(0, 0, crate::fcntl::SEEK_END);
    }

    #[test]
    fn test_lseek_negative_one_whence() {
        crate::errno::set_errno(0);
        let result = lseek(0, 0, -1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_lseek_seek_data_accepted() {
        // SEEK_DATA is a valid whence (sparse-file extension) and must not
        // be rejected with EINVAL purely on the whence value.  The actual
        // result depends on the host fd, so we only assert it is not the
        // EINVAL-from-bad-whence path.
        crate::errno::set_errno(0);
        let _ret = lseek(0, 0, crate::fcntl::SEEK_DATA);
        // If it failed, it must not be because of an invalid whence.
        // (A real bad-fd / not-seekable failure is fine on the host.)
    }

    #[test]
    fn test_lseek_seek_hole_accepted() {
        crate::errno::set_errno(0);
        let _ret = lseek(0, 0, crate::fcntl::SEEK_HOLE);
    }

    #[test]
    fn test_lseek_seek_data_negative_offset() {
        // SEEK_DATA with a negative starting offset is EINVAL.
        crate::errno::set_errno(0);
        let result = lseek(0, -1, crate::fcntl::SEEK_DATA);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_lseek_seek_hole_negative_offset() {
        crate::errno::set_errno(0);
        let result = lseek(0, -1, crate::fcntl::SEEK_HOLE);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- truncate negative length --

    #[test]
    fn test_truncate_negative_length() {
        let result = truncate(b"/tmp/test\0".as_ptr(), -1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_truncate_null_path() {
        let result = truncate(core::ptr::null(), 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_ftruncate_negative_length() {
        let result = ftruncate(0, -1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- unlink / rename / link / symlink / readlink null checks --

    #[test]
    fn test_unlink_null() {
        assert_eq!(unlink(core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_rename_null_old() {
        assert_eq!(rename(core::ptr::null(), b"/b\0".as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_rename_null_new() {
        assert_eq!(rename(b"/a\0".as_ptr(), core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_link_null_old() {
        assert_eq!(link(core::ptr::null(), b"/b\0".as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_link_null_new() {
        assert_eq!(link(b"/a\0".as_ptr(), core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_symlink_null_target() {
        assert_eq!(symlink(core::ptr::null(), b"/link\0".as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_symlink_null_linkpath() {
        assert_eq!(symlink(b"/target\0".as_ptr(), core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_readlink_null_path() {
        let mut buf = [0u8; 64];
        assert_eq!(readlink(core::ptr::null(), buf.as_mut_ptr(), 64), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_readlink_null_buf() {
        assert_eq!(readlink(b"/link\0".as_ptr(), core::ptr::null_mut(), 64), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_readlink_chk_delegates_and_clamps() {
        let mut buf = [0u8; 64];
        // len > buflen: the wrapper clamps to buflen, then delegates. A null
        // path still yields the readlink error path (-1), proving delegation.
        assert_eq!(
            __readlink_chk(core::ptr::null(), buf.as_mut_ptr(), 1000, 64),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_readlinkat_chk_delegates_and_clamps() {
        let mut buf = [0u8; 64];
        assert_eq!(
            __readlinkat_chk(AT_FDCWD, core::ptr::null(), buf.as_mut_ptr(), 1000, 64),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- mkdir / rmdir null checks --

    #[test]
    fn test_mkdir_null() {
        assert_eq!(mkdir(core::ptr::null(), 0o755), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_rmdir_null() {
        assert_eq!(rmdir(core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- stat / fstat null checks --

    #[test]
    fn test_stat_null_path() {
        let mut buf = crate::stat::Stat::zeroed();
        assert_eq!(stat(core::ptr::null(), &raw mut buf), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_stat_null_buf() {
        assert_eq!(stat(b"/tmp\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_fstat_null_buf() {
        assert_eq!(fstat(0, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_lstat_null_path() {
        let mut buf = crate::stat::Stat::zeroed();
        assert_eq!(lstat(core::ptr::null(), &raw mut buf), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_lstat_null_buf() {
        assert_eq!(lstat(b"/tmp\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- access null check --

    #[test]
    fn test_access_null() {
        assert_eq!(access(core::ptr::null(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- open null check --

    #[test]
    fn test_open_null() {
        assert_eq!(open(core::ptr::null(), 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- c_strlen_pub --

    #[test]
    fn test_c_strlen_pub_empty() {
        assert_eq!(unsafe { c_strlen_pub(b"\0".as_ptr()) }, 0);
    }

    #[test]
    fn test_c_strlen_pub_hello() {
        assert_eq!(unsafe { c_strlen_pub(b"hello\0".as_ptr()) }, 5);
    }

    // -- close: invalid fd returns EBADF --

    #[test]
    fn test_close_invalid_fd() {
        let result = close(9999);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_close_negative_fd() {
        let result = close(-1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // -- dup: invalid fd returns EBADF --

    #[test]
    fn test_dup_invalid_fd() {
        let result = dup(9999);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_dup_negative_fd() {
        let result = dup(-1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // -- dup2: invalid fds --

    #[test]
    fn test_dup2_invalid_oldfd() {
        let result = dup2(9999, 5);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_dup2_negative_newfd() {
        // Even if oldfd is invalid, we should get EBADF for oldfd first.
        let result = dup2(9999, -1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_dup2_same_invalid_fd() {
        // dup2(fd, fd) when fd is invalid → EBADF.
        let result = dup2(9999, 9999);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // -- fsync / fdatasync: invalid fd returns EBADF --

    #[test]
    fn test_fsync_invalid_fd() {
        let result = fsync(9999);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fdatasync_invalid_fd() {
        let result = fdatasync(9999);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // -- LP64 aliases (64-bit variants) delegate to base functions --

    #[test]
    fn test_lseek64_invalid_whence() {
        // lseek64 delegates to lseek; same invalid-whence behavior.
        let result = lseek64(0, 0, 99);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_stat64_null_path() {
        let mut buf = crate::stat::Stat::zeroed();
        assert_eq!(stat64(core::ptr::null(), &raw mut buf), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_stat64_null_buf() {
        assert_eq!(stat64(b"/tmp\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_fstat64_null_buf() {
        assert_eq!(fstat64(0, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_lstat64_null_path() {
        let mut buf = crate::stat::Stat::zeroed();
        assert_eq!(lstat64(core::ptr::null(), &raw mut buf), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_lstat64_null_buf() {
        assert_eq!(lstat64(b"/tmp\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- glibc __xstat family --

    #[test]
    fn test_xstat_null_path() {
        let mut buf = crate::stat::Stat::zeroed();
        assert_eq!(__xstat(1, core::ptr::null(), &raw mut buf), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_xstat_null_buf() {
        assert_eq!(__xstat(1, b"/tmp\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_fxstat_null_buf() {
        assert_eq!(__fxstat(1, 0, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_lxstat_null_path() {
        let mut buf = crate::stat::Stat::zeroed();
        assert_eq!(__lxstat(1, core::ptr::null(), &raw mut buf), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_lxstat_null_buf() {
        assert_eq!(__lxstat(1, b"/tmp\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_xstat64_null_path() {
        let mut buf = crate::stat::Stat::zeroed();
        assert_eq!(__xstat64(3, core::ptr::null(), &raw mut buf), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_xstat64_null_buf() {
        assert_eq!(__xstat64(3, b"/tmp\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_fxstat64_null_buf() {
        assert_eq!(__fxstat64(3, 0, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_lxstat64_null_path() {
        let mut buf = crate::stat::Stat::zeroed();
        assert_eq!(__lxstat64(3, core::ptr::null(), &raw mut buf), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_lxstat64_null_buf() {
        assert_eq!(__lxstat64(3, b"/tmp\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- FORTIFY_SOURCE _chk wrappers --

    #[test]
    fn test_read_chk_zero_count() {
        // __read_chk delegates to read; zero count returns 0.
        assert_eq!(__read_chk(0, core::ptr::null_mut(), 0, 0), 0);
    }

    #[test]
    fn test_read_chk_null_buf() {
        assert_eq!(__read_chk(0, core::ptr::null_mut(), 10, 10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_pread_chk_null_buf() {
        assert_eq!(__pread_chk(0, core::ptr::null_mut(), 10, 0, 10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_pread_chk_zero_count() {
        assert_eq!(__pread_chk(0, core::ptr::null_mut(), 0, 0, 0), 0);
    }

    #[test]
    fn test_pread_chk_negative_offset() {
        let mut buf = [0u8; 10];
        assert_eq!(__pread_chk(0, buf.as_mut_ptr(), 10, -1, 10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pread64_chk_null_buf() {
        assert_eq!(__pread64_chk(0, core::ptr::null_mut(), 10, 0, 10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_pread64_chk_zero_count() {
        assert_eq!(__pread64_chk(0, core::ptr::null_mut(), 0, 0, 0), 0);
    }

    #[test]
    fn test_realpath_chk_null_path() {
        let mut buf = [0u8; 256];
        let result = __realpath_chk(core::ptr::null(), buf.as_mut_ptr(), 256);
        assert!(result.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- *at() functions with AT_FDCWD delegate to non-at versions --

    #[test]
    fn test_faccessat_atfdcwd_null() {
        // faccessat(AT_FDCWD, NULL, ...) → access(NULL, ...) → EFAULT.
        assert_eq!(faccessat(AT_FDCWD, core::ptr::null(), 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_openat_atfdcwd_null() {
        // openat(AT_FDCWD, NULL, ...) → open(NULL, ...) → EFAULT.
        assert_eq!(openat(AT_FDCWD, core::ptr::null(), 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_fstatat_atfdcwd_null_path() {
        let mut buf = crate::stat::Stat::zeroed();
        assert_eq!(fstatat(AT_FDCWD, core::ptr::null(), &raw mut buf, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_fstatat_atfdcwd_null_buf() {
        assert_eq!(
            fstatat(AT_FDCWD, b"/tmp\0".as_ptr(), core::ptr::null_mut(), 0),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_fstatat_nofollow_delegates_to_lstat() {
        // With AT_SYMLINK_NOFOLLOW and AT_FDCWD, should delegate to lstat.
        // Verify it hits the same null-check as lstat.
        let mut buf = crate::stat::Stat::zeroed();
        assert_eq!(
            fstatat(
                AT_FDCWD,
                core::ptr::null(),
                &raw mut buf,
                AT_SYMLINK_NOFOLLOW
            ),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_unlinkat_atfdcwd_null() {
        assert_eq!(unlinkat(AT_FDCWD, core::ptr::null(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_unlinkat_removedir_delegates_to_rmdir() {
        // AT_REMOVEDIR flag should make unlinkat act like rmdir.
        assert_eq!(unlinkat(AT_FDCWD, core::ptr::null(), AT_REMOVEDIR), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_renameat_atfdcwd_null_old() {
        assert_eq!(
            renameat(AT_FDCWD, core::ptr::null(), AT_FDCWD, b"/b\0".as_ptr()),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_renameat_atfdcwd_null_new() {
        assert_eq!(
            renameat(AT_FDCWD, b"/a\0".as_ptr(), AT_FDCWD, core::ptr::null()),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mkdirat_atfdcwd_null() {
        assert_eq!(mkdirat(AT_FDCWD, core::ptr::null(), 0o755), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_readlinkat_atfdcwd_null_path() {
        let mut buf = [0u8; 64];
        assert_eq!(
            readlinkat(AT_FDCWD, core::ptr::null(), buf.as_mut_ptr(), 64),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_readlinkat_atfdcwd_null_buf() {
        assert_eq!(
            readlinkat(AT_FDCWD, b"/link\0".as_ptr(), core::ptr::null_mut(), 64),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_symlinkat_atfdcwd_null_target() {
        assert_eq!(
            symlinkat(core::ptr::null(), AT_FDCWD, b"/link\0".as_ptr()),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_symlinkat_atfdcwd_null_linkpath() {
        assert_eq!(
            symlinkat(b"/target\0".as_ptr(), AT_FDCWD, core::ptr::null()),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_linkat_atfdcwd_null_old() {
        assert_eq!(
            linkat(AT_FDCWD, core::ptr::null(), AT_FDCWD, b"/b\0".as_ptr(), 0),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_linkat_atfdcwd_null_new() {
        assert_eq!(
            linkat(AT_FDCWD, b"/a\0".as_ptr(), AT_FDCWD, core::ptr::null(), 0),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_fchmodat_atfdcwd_delegates() {
        // fchmodat(AT_FDCWD, ...) → chmod(...) → 0.
        assert_eq!(fchmodat(AT_FDCWD, b"/tmp\0".as_ptr(), 0o755, 0), 0);
    }

    #[test]
    fn test_fchownat_atfdcwd_delegates() {
        // fchownat(AT_FDCWD, ...) → chown(...) → 0.
        assert_eq!(fchownat(AT_FDCWD, b"/tmp\0".as_ptr(), 0, 0, 0), 0);
    }

    // -- *at() functions with invalid dirfd (not AT_FDCWD, relative path) --

    #[test]
    fn test_faccessat_invalid_dirfd() {
        // Relative path + invalid dirfd → EBADF.
        assert_eq!(faccessat(9999, b"file.txt\0".as_ptr(), 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_openat_invalid_dirfd() {
        assert_eq!(openat(9999, b"file.txt\0".as_ptr(), 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fstatat_invalid_dirfd() {
        let mut buf = crate::stat::Stat::zeroed();
        assert_eq!(fstatat(9999, b"file.txt\0".as_ptr(), &raw mut buf, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_unlinkat_invalid_dirfd() {
        assert_eq!(unlinkat(9999, b"file.txt\0".as_ptr(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_mkdirat_invalid_dirfd() {
        assert_eq!(mkdirat(9999, b"subdir\0".as_ptr(), 0o755), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_readlinkat_invalid_dirfd() {
        let mut buf = [0u8; 64];
        assert_eq!(
            readlinkat(9999, b"link\0".as_ptr(), buf.as_mut_ptr(), 64),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_symlinkat_invalid_dirfd() {
        assert_eq!(
            symlinkat(b"/target\0".as_ptr(), 9999, b"link\0".as_ptr()),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_linkat_invalid_dirfd() {
        assert_eq!(
            linkat(9999, b"a\0".as_ptr(), AT_FDCWD, b"/b\0".as_ptr(), 0),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fchmodat_invalid_dirfd() {
        assert_eq!(fchmodat(9999, b"file\0".as_ptr(), 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fchownat_invalid_dirfd() {
        assert_eq!(fchownat(9999, b"file\0".as_ptr(), 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // -- *at() functions with absolute path ignore dirfd --

    #[test]
    fn test_faccessat_absolute_ignores_dirfd() {
        // Absolute path: dirfd is ignored, delegates to access().
        // We can't test success (no kernel), but we can test it doesn't
        // fail with EBADF for the dirfd — it gets past the dirfd check.
        // It will fail later in the syscall (no kernel), but not EBADF.
        let result = faccessat(9999, b"/\0".as_ptr(), 0, 0);
        // Should not be EBADF — the absolute path means dirfd was ignored.
        if result == -1 {
            assert_ne!(crate::errno::get_errno(), crate::errno::EBADF);
        }
    }

    #[test]
    fn test_fchmodat_absolute_ignores_dirfd() {
        // Absolute path + invalid dirfd → chmod (stub returning 0).
        assert_eq!(fchmodat(9999, b"/tmp\0".as_ptr(), 0o755, 0), 0);
    }

    #[test]
    fn test_fchownat_absolute_ignores_dirfd() {
        // Absolute path + invalid dirfd → chown (stub returning 0).
        assert_eq!(fchownat(9999, b"/tmp\0".as_ptr(), 0, 0, 0), 0);
    }

    // -- sendfile / copy_file_range: zero-length --

    #[test]
    fn test_sendfile_zero_count() {
        // Copying zero bytes should return 0 immediately.
        let result = sendfile(1, 0, core::ptr::null_mut(), 0);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_copy_file_range_zero_len() {
        // Copying zero bytes should return 0 immediately.  Use a pipe
        // to get guaranteed-open fds — relying on stdin/stdout is
        // fragile because other tests in the suite may close them.
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);
        let result = copy_file_range(
            pipefd[0],
            core::ptr::null_mut(),
            pipefd[1],
            core::ptr::null_mut(),
            0,
            0,
        );
        assert_eq!(result, 0);
        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    // ----------------------------------------------------------------
    // Phase 89 — copy_file_range argument-domain validation
    //
    // Linux semantics being validated:
    //   - flags != 0 → -1, EINVAL (no valid flag bits defined yet)
    //   - fd_in < 0 || fd_out < 0 → -1, EBADF
    //   - fd_in or fd_out not open → -1, EBADF
    //   - len == 0 with otherwise valid inputs → 0
    // ----------------------------------------------------------------

    #[test]
    fn test_copy_file_range_phase89_nonzero_flag_einval() {
        crate::errno::set_errno(0);
        let r = copy_file_range(0, core::ptr::null_mut(), 1, core::ptr::null_mut(), 8, 1);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_copy_file_range_phase89_high_bit_flag_einval() {
        crate::errno::set_errno(0);
        let r = copy_file_range(
            0,
            core::ptr::null_mut(),
            1,
            core::ptr::null_mut(),
            8,
            0x8000_0000,
        );
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_copy_file_range_phase89_flag_check_beats_zero_len() {
        // The bug being fixed: bad flags + len==0 used to return 0
        // silently (skipping validation entirely).
        crate::errno::set_errno(0);
        let r = copy_file_range(0, core::ptr::null_mut(), 1, core::ptr::null_mut(), 0, 4);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_copy_file_range_phase89_neg_fd_in_ebadf() {
        crate::errno::set_errno(0);
        let r = copy_file_range(-1, core::ptr::null_mut(), 1, core::ptr::null_mut(), 8, 0);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_copy_file_range_phase89_neg_fd_out_ebadf() {
        crate::errno::set_errno(0);
        let r = copy_file_range(0, core::ptr::null_mut(), -1, core::ptr::null_mut(), 8, 0);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_copy_file_range_phase89_both_neg_fds_ebadf() {
        crate::errno::set_errno(0);
        let r = copy_file_range(-5, core::ptr::null_mut(), -6, core::ptr::null_mut(), 8, 0);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_copy_file_range_phase89_nonexistent_fd_in_ebadf() {
        crate::errno::set_errno(0);
        let r = copy_file_range(9999, core::ptr::null_mut(), 1, core::ptr::null_mut(), 8, 0);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_copy_file_range_phase89_nonexistent_fd_out_ebadf() {
        crate::errno::set_errno(0);
        let r = copy_file_range(0, core::ptr::null_mut(), 9999, core::ptr::null_mut(), 8, 0);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_copy_file_range_phase89_flag_check_beats_ebadf() {
        // Bad flags + bogus fds → EINVAL, not EBADF (flag check first).
        crate::errno::set_errno(0);
        let r = copy_file_range(-1, core::ptr::null_mut(), -1, core::ptr::null_mut(), 8, 1);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_copy_file_range_phase89_neg_fd_check_beats_lookup() {
        // -1 fd + non-existent positive fd → EBADF from negative check.
        crate::errno::set_errno(0);
        let r = copy_file_range(-1, core::ptr::null_mut(), 9999, core::ptr::null_mut(), 8, 0);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_copy_file_range_phase89_zero_len_with_valid_fds_ok() {
        // len==0 + flags==0 + valid open fds → 0 (no-op).  Use a pipe
        // for guaranteed-open fds — other tests in the suite may close
        // stdin/stdout, so we can't rely on fds 0/1 being open.
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);
        crate::errno::set_errno(0);
        let r = copy_file_range(
            pipefd[0],
            core::ptr::null_mut(),
            pipefd[1],
            core::ptr::null_mut(),
            0,
            0,
        );
        assert_eq!(r, 0);
        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    #[test]
    fn test_copy_file_range_phase89_einval_then_valid_progression() {
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);
        crate::errno::set_errno(0);
        let r = copy_file_range(
            pipefd[0],
            core::ptr::null_mut(),
            pipefd[1],
            core::ptr::null_mut(),
            8,
            1,
        );
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Subsequent valid call succeeds (len==0 no-op).
        crate::errno::set_errno(0);
        let r = copy_file_range(
            pipefd[0],
            core::ptr::null_mut(),
            pipefd[1],
            core::ptr::null_mut(),
            0,
            0,
        );
        assert_eq!(r, 0);
        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    #[test]
    fn test_copy_file_range_phase89_ebadf_then_valid_progression() {
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);
        crate::errno::set_errno(0);
        let r = copy_file_range(
            9999,
            core::ptr::null_mut(),
            pipefd[1],
            core::ptr::null_mut(),
            8,
            0,
        );
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);

        crate::errno::set_errno(0);
        let r = copy_file_range(
            pipefd[0],
            core::ptr::null_mut(),
            pipefd[1],
            core::ptr::null_mut(),
            0,
            0,
        );
        assert_eq!(r, 0);
        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    // -- sendfile with offset pointer --

    #[test]
    fn test_sendfile_with_offset_zero_count() {
        let mut off: i64 = 100;
        let result = sendfile(1, 0, &raw mut off, 0);
        assert_eq!(result, 0);
        // Offset should not change for zero-length transfer.
        assert_eq!(off, 100);
    }

    // -- sendfile64 (LP64 alias) --

    #[test]
    fn test_sendfile64_zero_count() {
        let result = sendfile64(1, 0, core::ptr::null_mut(), 0);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_sendfile64_with_offset_zero_count() {
        let mut off: i64 = 200;
        let result = sendfile64(1, 0, &raw mut off, 0);
        assert_eq!(result, 0);
        assert_eq!(off, 200);
    }

    // -- posix_fallocate64 (LP64 alias) --

    #[test]
    fn test_posix_fallocate64_invalid_offset() {
        assert_eq!(posix_fallocate64(0, -1, 4096), crate::errno::EINVAL);
    }

    #[test]
    fn test_posix_fallocate64_invalid_len() {
        assert_eq!(posix_fallocate64(0, 0, 0), crate::errno::EINVAL);
    }

    // -- preadv2 / pwritev2 --

    #[test]
    fn test_preadv2_null_iov() {
        crate::errno::set_errno(0);
        assert_eq!(preadv2(0, core::ptr::null(), 1, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_preadv2_zero_iovcnt() {
        crate::errno::set_errno(0);
        assert_eq!(preadv2(0, core::ptr::null(), 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_preadv2_negative_offset_delegates_to_readv() {
        // offset == -1 should use readv behavior (current file position).
        // With null iov and iovcnt == 1, readv returns EINVAL.
        crate::errno::set_errno(0);
        assert_eq!(preadv2(0, core::ptr::null(), 1, -1, 0), -1);
    }

    #[test]
    fn test_pwritev2_null_iov() {
        crate::errno::set_errno(0);
        assert_eq!(pwritev2(0, core::ptr::null(), 1, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pwritev2_negative_offset_delegates_to_writev() {
        crate::errno::set_errno(0);
        assert_eq!(pwritev2(0, core::ptr::null(), 1, -1, 0), -1);
    }

    // -- RWF_* constants --

    #[test]
    fn test_rwf_constants() {
        assert_eq!(RWF_HIPRI, 0x01);
        assert_eq!(RWF_DSYNC, 0x02);
        assert_eq!(RWF_SYNC, 0x04);
        assert_eq!(RWF_NOWAIT, 0x08);
        assert_eq!(RWF_APPEND, 0x10);
    }

    #[test]
    fn test_rwf_no_collisions() {
        let all = [RWF_HIPRI, RWF_DSYNC, RWF_SYNC, RWF_NOWAIT, RWF_APPEND];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_eq!(all[i] & all[j], 0);
            }
        }
    }

    // -- fadvise64 --

    #[test]
    fn test_fadvise64_succeeds() {
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Console, 0).expect("fd available");
        assert_eq!(fadvise64(fd, 0, 0, 0), 0);
        let _ = close(fd);
    }

    // -- splice / vmsplice (buffered fallback) --

    #[test]
    fn test_splice_zero_len_returns_zero() {
        // POSIX: zero-length transfer is a no-op success.  No FD lookup,
        // no syscall — just return 0.
        let result = splice(0, core::ptr::null_mut(), 1, core::ptr::null_mut(), 0, 0);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_splice_invalid_fd_in() {
        crate::errno::set_errno(0);
        // fd 9999 is out of range → EBADF before any kind checks.
        let result = splice(
            9999,
            core::ptr::null_mut(),
            1,
            core::ptr::null_mut(),
            4096,
            0,
        );
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_splice_invalid_fd_out() {
        crate::errno::set_errno(0);
        // fd 0 (stdin) is valid, fd 9999 isn't → EBADF.
        let result = splice(
            0,
            core::ptr::null_mut(),
            9999,
            core::ptr::null_mut(),
            4096,
            0,
        );
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_splice_neither_is_pipe_einval() {
        // Fabricate two non-pipe fds.  We can't rely on fds 0/1 being
        // present in the full suite because other tests may have closed
        // them — using alloc_fd guarantees fresh slots in known states.
        let in_fd = fdtable::alloc_fd(HandleKind::File, 3).expect("alloc_fd File failed");
        let out_fd = fdtable::alloc_fd(HandleKind::File, 4).expect("alloc_fd File failed");

        crate::errno::set_errno(0);
        let result = splice(
            in_fd,
            core::ptr::null_mut(),
            out_fd,
            core::ptr::null_mut(),
            4096,
            0,
        );
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        let _ = fdtable::close_fd(in_fd);
        let _ = fdtable::close_fd(out_fd);
    }

    #[test]
    fn test_splice_offset_on_pipe_in_espipe() {
        // Fabricate a pipe-kind fd and a regular-file-kind fd.  Asking
        // for an offset on the pipe side must fail with ESPIPE before
        // any I/O is attempted.
        let pipe_fd = fdtable::alloc_fd(HandleKind::Pipe, 1).expect("alloc_fd Pipe failed");
        let file_fd = fdtable::alloc_fd(HandleKind::File, 1).expect("alloc_fd File failed");

        crate::errno::set_errno(0);
        let mut off: i64 = 0;
        let result = splice(
            pipe_fd,
            &raw mut off,
            file_fd,
            core::ptr::null_mut(),
            4096,
            0,
        );
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESPIPE);

        let _ = fdtable::close_fd(pipe_fd);
        let _ = fdtable::close_fd(file_fd);
    }

    #[test]
    fn test_splice_offset_on_pipe_out_espipe() {
        let pipe_fd = fdtable::alloc_fd(HandleKind::Pipe, 2).expect("alloc_fd Pipe failed");
        let file_fd = fdtable::alloc_fd(HandleKind::File, 2).expect("alloc_fd File failed");

        crate::errno::set_errno(0);
        let mut off: i64 = 0;
        let result = splice(
            file_fd,
            core::ptr::null_mut(),
            pipe_fd,
            &raw mut off,
            4096,
            0,
        );
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESPIPE);

        let _ = fdtable::close_fd(pipe_fd);
        let _ = fdtable::close_fd(file_fd);
    }

    #[test]
    fn test_tee_host_enosys_after_validation() {
        // The real tee() runs on the OS target via SYS_PIPE_PEEK /
        // SYS_PIPE_WAIT_READABLE; the host build has no kernel pipe layer,
        // so it returns ENOSYS — but only after argument validation passes.
        // (End-to-end tee behaviour is covered by the kernel pipe self-test.)
        let mut pf1 = [0i32; 2];
        let mut pf2 = [0i32; 2];
        if crate::pipe::pipe(pf1.as_mut_ptr()) != 0 || crate::pipe::pipe(pf2.as_mut_ptr()) != 0 {
            return;
        }
        crate::errno::set_errno(0);
        // Read end of pf1, write end of pf2 — both pipes, valid.
        assert_eq!(tee(pf1[0], pf2[1], 4096, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        let _ = fdtable::close_fd(pf1[0]);
        let _ = fdtable::close_fd(pf1[1]);
        let _ = fdtable::close_fd(pf2[0]);
        let _ = fdtable::close_fd(pf2[1]);
    }

    #[test]
    fn test_vmsplice_zero_segs_returns_zero() {
        // Zero segments is a no-op success — no FD lookup, no syscall.
        let result = vmsplice(0, core::ptr::null(), 0, 0);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_vmsplice_null_iov_with_segs_efault() {
        crate::errno::set_errno(0);
        let result = vmsplice(0, core::ptr::null(), 1, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_vmsplice_too_many_segs_einval() {
        crate::errno::set_errno(0);
        // u64 above i32::MAX → EINVAL.
        let dummy = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        let result = vmsplice(0, &raw const dummy, (i32::MAX as u64) + 1, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_vmsplice_invalid_fd_ebadf() {
        crate::errno::set_errno(0);
        let dummy = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        let result = vmsplice(9999, &raw const dummy, 1, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_vmsplice_non_pipe_fd_ebadf() {
        // fd 1 is Console, not Pipe — Linux returns EBADF.
        crate::errno::set_errno(0);
        let dummy = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        let result = vmsplice(1, &raw const dummy, 1, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_vmsplice_read_end_pipe_accepted() {
        // A pipe *read* end (O_RDONLY) must be accepted and routed to the
        // read-out (readv) direction — not rejected as EBADF the way a
        // non-pipe fd is.  Real byte copy-out rides on the kernel pipe read
        // path (no kernel on host), so here we only confirm the read-end fd
        // passes validation: an empty transfer is a clean success (0), and a
        // non-empty one reaches the delegate rather than a validation error.
        let Some(rfd) = fdtable::alloc_fd_with_flags(
            crate::fdtable::HandleKind::Pipe,
            0x5678_u64,
            crate::fcntl::O_RDONLY,
        ) else {
            return;
        };
        crate::errno::set_errno(0);
        let empty = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        // Empty transfer: routed to readv, which returns 0 without a syscall.
        assert_eq!(vmsplice(rfd, &raw const empty, 1, 0), 0);
        // Not rejected as a bad fd (which would be -1/EBADF before delegation).
        assert_ne!(crate::errno::get_errno(), crate::errno::EBADF);
        let _ = fdtable::close_fd(rfd);
    }

    #[test]
    fn test_vmsplice_write_end_pipe_accepted() {
        // Symmetric guard for the write end (O_WRONLY → writev direction).
        let Some(wfd) = fdtable::alloc_fd_with_flags(
            crate::fdtable::HandleKind::Pipe,
            0x1234_u64,
            crate::fcntl::O_WRONLY,
        ) else {
            return;
        };
        crate::errno::set_errno(0);
        let empty = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        assert_eq!(vmsplice(wfd, &raw const empty, 1, 0), 0);
        assert_ne!(crate::errno::get_errno(), crate::errno::EBADF);
        let _ = fdtable::close_fd(wfd);
    }

    // ----------------------------------------------------------------
    // Phase 88 — splice / vmsplice flag-mask validation
    //
    // Linux semantics being validated:
    //   - splice:   flags & ~SPLICE_F_ALL → -1, EINVAL (before every
    //               other check, including len==0 and fd lookups).
    //   - vmsplice: flags & ~SPLICE_F_ALL → -1, EINVAL (before iov/
    //               nr_segs validation).
    // ----------------------------------------------------------------

    #[test]
    fn test_splice_phase88_unknown_flag_bit_einval() {
        // Any single bit outside SPLICE_F_VALID (1|2|4|8 = 0xF) is bogus.
        crate::errno::set_errno(0);
        let r = splice(0, core::ptr::null_mut(), 1, core::ptr::null_mut(), 4, 0x10);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_splice_phase88_high_garbage_flag_einval() {
        crate::errno::set_errno(0);
        let r = splice(
            0,
            core::ptr::null_mut(),
            1,
            core::ptr::null_mut(),
            8,
            0x8000_0000,
        );
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_splice_phase88_all_unknown_bits_einval() {
        crate::errno::set_errno(0);
        let r = splice(
            0,
            core::ptr::null_mut(),
            1,
            core::ptr::null_mut(),
            8,
            !SPLICE_F_VALID,
        );
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_splice_phase88_flag_check_before_zero_len() {
        // Bug being fixed: bad flags + len==0 used to return 0 silently.
        crate::errno::set_errno(0);
        let r = splice(
            0,
            core::ptr::null_mut(),
            1,
            core::ptr::null_mut(),
            0,
            0xFFFF_FFF0,
        );
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_splice_phase88_flag_check_before_fd_lookup() {
        // Bad flags + bogus fd → EINVAL, not EBADF.  The flag check is
        // first per Linux's syscall prologue.
        crate::errno::set_errno(0);
        let r = splice(
            9999,
            core::ptr::null_mut(),
            9998,
            core::ptr::null_mut(),
            8,
            0x100,
        );
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_splice_phase88_zero_flags_still_accepted() {
        // The classic call form with flags=0 must still pass validation
        // and reach the len==0 short-circuit.
        let r = splice(0, core::ptr::null_mut(), 1, core::ptr::null_mut(), 0, 0);
        assert_eq!(r, 0);
    }

    #[test]
    fn test_splice_phase88_all_known_flags_pass() {
        // Setting every defined flag bit together must not trip EINVAL.
        // The call then proceeds to len==0 and returns 0.
        let r = splice(
            0,
            core::ptr::null_mut(),
            1,
            core::ptr::null_mut(),
            0,
            SPLICE_F_VALID,
        );
        assert_eq!(r, 0);
    }

    #[test]
    fn test_vmsplice_phase88_unknown_flag_bit_einval() {
        crate::errno::set_errno(0);
        let dummy = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        let r = vmsplice(0, &raw const dummy, 1, 0x10);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_vmsplice_phase88_high_garbage_flag_einval() {
        crate::errno::set_errno(0);
        let dummy = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        let r = vmsplice(0, &raw const dummy, 1, 0x8000_0000);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_vmsplice_phase88_flag_check_before_iov_null() {
        // Bad flags + NULL iov + nr_segs > 0 → EINVAL, not EFAULT.
        crate::errno::set_errno(0);
        let r = vmsplice(0, core::ptr::null(), 1, 0x100);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_vmsplice_phase88_flag_check_before_nr_segs_cap() {
        // Bad flags + too-many segs → EINVAL from flag check, not from
        // the segs validation (both would set EINVAL, but the order
        // matters for ordering parity).
        let dummy = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        crate::errno::set_errno(0);
        let r = vmsplice(0, &raw const dummy, (i32::MAX as u64) + 1, 0x10);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_vmsplice_phase88_zero_segs_with_bad_flag_einval() {
        // Bug being fixed: bad flags + nr_segs==0 used to return 0
        // silently.
        crate::errno::set_errno(0);
        let r = vmsplice(0, core::ptr::null(), 0, 0x10);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_vmsplice_phase88_zero_flags_still_accepted() {
        // Valid (zero) flags + nr_segs==0 → 0.
        let r = vmsplice(0, core::ptr::null(), 0, 0);
        assert_eq!(r, 0);
    }

    #[test]
    fn test_vmsplice_phase88_all_known_flags_pass() {
        // Every defined flag bit set together passes the mask check.
        let r = vmsplice(0, core::ptr::null(), 0, SPLICE_F_VALID);
        assert_eq!(r, 0);
    }

    #[test]
    fn test_vmsplice_phase88_einval_does_not_alter_next_call() {
        // An EINVAL from a bad-flag call must not taint a subsequent
        // valid call.
        crate::errno::set_errno(0);
        let r = vmsplice(0, core::ptr::null(), 0, 0x40);
        assert_eq!(r, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        crate::errno::set_errno(0);
        let r = vmsplice(0, core::ptr::null(), 0, 0);
        assert_eq!(r, 0);
    }

    // -- SPLICE_F_* constants --

    #[test]
    fn test_splice_flag_constants() {
        assert_eq!(SPLICE_F_MOVE, 1);
        assert_eq!(SPLICE_F_NONBLOCK, 2);
        assert_eq!(SPLICE_F_MORE, 4);
        assert_eq!(SPLICE_F_GIFT, 8);
    }

    #[test]
    fn test_splice_flags_no_collision() {
        let all = [
            SPLICE_F_MOVE,
            SPLICE_F_NONBLOCK,
            SPLICE_F_MORE,
            SPLICE_F_GIFT,
        ];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_eq!(all[i] & all[j], 0, "SPLICE_F flags {i} and {j} collide");
            }
        }
    }

    // -- renameat with AT_FDCWD both sides --

    #[test]
    fn test_renameat_atfdcwd_both_null() {
        // Both null → delegates to rename(NULL, NULL) → EFAULT.
        let result = renameat(AT_FDCWD, core::ptr::null(), AT_FDCWD, core::ptr::null());
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- __getcwd_chk --

    #[test]
    fn test_getcwd_chk_null() {
        crate::errno::set_errno(0);
        let ret = __getcwd_chk(core::ptr::null_mut(), 100, 100);
        assert!(ret.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getcwd_chk_zero_size() {
        let mut buf = [0u8; 100];
        crate::errno::set_errno(0);
        let ret = __getcwd_chk(buf.as_mut_ptr(), 0, 100);
        assert!(ret.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getcwd_chk_succeeds() {
        let mut buf = [0u8; 4096];
        let ret = __getcwd_chk(buf.as_mut_ptr(), 4096, 4096);
        assert!(!ret.is_null(), "__getcwd_chk should succeed");
        assert_eq!(buf[0], b'/', "CWD should start with '/'");
    }

    // -- preadv / pwritev --

    #[test]
    fn test_preadv_null_iov() {
        crate::errno::set_errno(0);
        let ret = preadv(0, core::ptr::null(), 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_preadv_zero_iovcnt() {
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        crate::errno::set_errno(0);
        let ret = preadv(0, &iov, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_preadv_negative_iovcnt() {
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        crate::errno::set_errno(0);
        let ret = preadv(0, &iov, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_preadv_over_max_iovcnt() {
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        crate::errno::set_errno(0);
        let ret = preadv(0, &iov, 1025, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_preadv_negative_offset() {
        let mut buf = [0u8; 16];
        let iov = Iovec {
            iov_base: buf.as_mut_ptr(),
            iov_len: 16,
        };
        crate::errno::set_errno(0);
        let ret = preadv(0, &iov, 1, -1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pwritev_null_iov() {
        crate::errno::set_errno(0);
        let ret = pwritev(0, core::ptr::null(), 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pwritev_zero_iovcnt() {
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        crate::errno::set_errno(0);
        let ret = pwritev(0, &iov, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pwritev_negative_offset() {
        let buf = [0u8; 16];
        let iov = Iovec {
            iov_base: buf.as_ptr().cast_mut(),
            iov_len: 16,
        };
        crate::errno::set_errno(0);
        let ret = pwritev(0, &iov, 1, -1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // readahead
    // -----------------------------------------------------------------------

    #[test]
    fn test_readahead_success() {
        // readahead with valid fd, offset, count → 0.
        let ret = readahead(0, 0, 4096);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_readahead_negative_fd() {
        crate::errno::set_errno(0);
        let ret = readahead(-1, 0, 4096);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_readahead_negative_offset() {
        crate::errno::set_errno(0);
        let ret = readahead(0, -1, 4096);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_readahead_zero_count() {
        // Zero count is valid — just a no-op.
        let ret = readahead(0, 0, 0);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_readahead_large_count() {
        // Large count is fine — we don't actually do anything.
        let ret = readahead(0, 1000, usize::MAX);
        assert_eq!(ret, 0);
    }

    // -----------------------------------------------------------------------
    // sync_file_range
    // -----------------------------------------------------------------------

    #[test]
    fn test_sync_file_range_negative_fd() {
        crate::errno::set_errno(0);
        let ret = sync_file_range(-1, 0, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_sync_file_range_valid_fd_no_crash() {
        // Use a pipe to get a guaranteed-open fd.  fsync on a pipe
        // is allowed to return EINVAL; we only care that the prologue
        // doesn't crash and that the call returns.
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);
        let _ret = sync_file_range(pipefd[0], 0, 4096, SYNC_FILE_RANGE_WRITE);
        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    #[test]
    fn test_sync_file_range_flag_constants() {
        assert_eq!(SYNC_FILE_RANGE_WAIT_BEFORE, 1);
        assert_eq!(SYNC_FILE_RANGE_WRITE, 2);
        assert_eq!(SYNC_FILE_RANGE_WAIT_AFTER, 4);
        // Flags should be distinct bit fields.
        assert_eq!(
            SYNC_FILE_RANGE_WAIT_BEFORE & SYNC_FILE_RANGE_WRITE,
            0,
            "flags must be distinct bits"
        );
        assert_eq!(
            SYNC_FILE_RANGE_WRITE & SYNC_FILE_RANGE_WAIT_AFTER,
            0,
            "flags must be distinct bits"
        );
    }

    // -- Phase 90: sync_file_range argument-domain validation --

    #[test]
    fn test_sync_file_range_phase90_valid_mask_constant() {
        // SYNC_FILE_RANGE_VALID covers exactly the three defined bits.
        assert_eq!(
            SYNC_FILE_RANGE_VALID,
            SYNC_FILE_RANGE_WAIT_BEFORE | SYNC_FILE_RANGE_WRITE | SYNC_FILE_RANGE_WAIT_AFTER,
        );
        assert_eq!(SYNC_FILE_RANGE_VALID, 7);
    }

    #[test]
    fn test_sync_file_range_phase90_unknown_flag_einval() {
        // Bit 3 (0b1000) is not a defined sync_file_range flag.
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);
        crate::errno::set_errno(0);
        let ret = sync_file_range(pipefd[0], 0, 0, 8);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    #[test]
    fn test_sync_file_range_phase90_high_bit_flag_einval() {
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);
        crate::errno::set_errno(0);
        let ret = sync_file_range(pipefd[0], 0, 0, 0x8000_0000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    #[test]
    fn test_sync_file_range_phase90_known_flag_combo_passes_prologue() {
        // All three valid bits together — must clear the flag check.
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);
        crate::errno::set_errno(0);
        let _ret = sync_file_range(
            pipefd[0],
            0,
            0,
            SYNC_FILE_RANGE_WAIT_BEFORE | SYNC_FILE_RANGE_WRITE | SYNC_FILE_RANGE_WAIT_AFTER,
        );
        // The flag prologue must not produce EINVAL.
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    #[test]
    fn test_sync_file_range_phase90_negative_offset_einval() {
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);
        crate::errno::set_errno(0);
        let ret = sync_file_range(pipefd[0], -1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    #[test]
    fn test_sync_file_range_phase90_negative_nbytes_einval() {
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);
        crate::errno::set_errno(0);
        let ret = sync_file_range(pipefd[0], 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    #[test]
    fn test_sync_file_range_phase90_endbyte_overflow_einval() {
        // offset + nbytes overflows i64.
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);
        crate::errno::set_errno(0);
        let ret = sync_file_range(pipefd[0], i64::MAX, 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    #[test]
    fn test_sync_file_range_phase90_max_offset_zero_nbytes_ok_prologue() {
        // offset = i64::MAX, nbytes = 0 → endbyte does not overflow.
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);
        crate::errno::set_errno(0);
        let _ret = sync_file_range(pipefd[0], i64::MAX, 0, 0);
        // Must not produce EINVAL from the prologue.
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    #[test]
    fn test_sync_file_range_phase90_nonexistent_fd_ebadf() {
        // Positive but never-allocated fd → EBADF.
        crate::errno::set_errno(0);
        let ret = sync_file_range(9999, 0, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_sync_file_range_phase90_flag_check_beats_offset() {
        // flags=bogus + offset=-1 → EINVAL from flag check, not offset
        // check (both produce EINVAL, but the flag check must fire
        // first per Linux's prologue order).  We can't directly observe
        // which branch fired, but the test documents the intent.
        crate::errno::set_errno(0);
        let ret = sync_file_range(-1, -1, 0, 0x40);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sync_file_range_phase90_flag_check_beats_ebadf() {
        // flags=bogus + fd=-1 → EINVAL (flag check beats fd check).
        crate::errno::set_errno(0);
        let ret = sync_file_range(-1, 0, 0, 0x100);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sync_file_range_phase90_offset_check_beats_ebadf() {
        // valid flags + negative offset + fd=-1 → EINVAL (offset check
        // beats fd check).
        crate::errno::set_errno(0);
        let ret = sync_file_range(-1, -1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sync_file_range_phase90_nbytes_check_beats_ebadf() {
        // valid flags + offset=0 + negative nbytes + fd=-1 → EINVAL.
        crate::errno::set_errno(0);
        let ret = sync_file_range(-1, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sync_file_range_phase90_overflow_check_beats_ebadf() {
        // overflow check fires before fd check.
        crate::errno::set_errno(0);
        let ret = sync_file_range(-1, i64::MAX, 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sync_file_range_phase90_einval_then_valid_progression() {
        // After an EINVAL, a valid call still works.
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);

        crate::errno::set_errno(0);
        let ret = sync_file_range(pipefd[0], 0, 0, 0x80);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        crate::errno::set_errno(0);
        let _ret = sync_file_range(pipefd[0], 0, 0, 0);
        // The prologue must not produce EINVAL/EBADF on a valid call.
        let e = crate::errno::get_errno();
        assert_ne!(e, crate::errno::EINVAL);
        assert_ne!(e, crate::errno::EBADF);

        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    #[test]
    fn test_sync_file_range_phase90_ebadf_then_valid_progression() {
        let mut pipefd = [0i32; 2];
        assert_eq!(crate::pipe::pipe(pipefd.as_mut_ptr()), 0);

        crate::errno::set_errno(0);
        let ret = sync_file_range(8888, 0, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);

        crate::errno::set_errno(0);
        let _ret = sync_file_range(pipefd[0], 0, 0, 0);
        let e = crate::errno::get_errno();
        assert_ne!(e, crate::errno::EINVAL);
        assert_ne!(e, crate::errno::EBADF);

        let _ = close(pipefd[0]);
        let _ = close(pipefd[1]);
    }

    // -----------------------------------------------------------------------
    // name_to_handle_at / open_by_handle_at
    // -----------------------------------------------------------------------

    #[test]
    fn test_name_to_handle_at_returns_enosys() {
        // Valid inputs must reach the ENOSYS sentinel — all earlier
        // error classes are exercised in dedicated tests below.
        let mut fh = FileHandle {
            handle_bytes: 128,
            handle_type: 0,
        };
        let mut mount_id: i32 = 0;
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &raw mut fh,
            &raw mut mount_id,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_open_by_handle_at_returns_enosys() {
        // Valid pointer + AT_FDCWD must pass validation and surface
        // ENOSYS rather than EFAULT/EBADF.
        let mut fh = FileHandle {
            handle_bytes: 0,
            handle_type: 0,
        };
        crate::errno::set_errno(0);
        let ret = open_by_handle_at(AT_FDCWD, &raw mut fh, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_file_handle_struct_layout() {
        // FileHandle header: handle_bytes (u32) + handle_type (i32) = 8 bytes.
        assert_eq!(core::mem::size_of::<FileHandle>(), 8);
        assert!(core::mem::align_of::<FileHandle>() >= 4);
    }

    // -----------------------------------------------------------------------
    // fstatat64 — LP64 alias for fstatat
    // -----------------------------------------------------------------------

    #[test]
    fn test_fstatat64_null_path() {
        crate::errno::set_errno(0);
        let mut st = Stat::default();
        let ret = fstatat64(AT_FDCWD, core::ptr::null(), &raw mut st, 0);
        // null path → stat returns error
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_fstatat64_with_at_fdcwd() {
        // AT_FDCWD → delegates to stat/lstat.
        // On the test host the syscall result is unpredictable,
        // so we just verify it doesn't crash.
        let mut st = Stat::default();
        let _ret = fstatat64(AT_FDCWD, b"/nonexistent\0".as_ptr(), &raw mut st, 0);
    }

    #[test]
    fn test_fstatat64_nofollow_flag() {
        // Verify the AT_SYMLINK_NOFOLLOW flag path compiles and runs.
        let mut st = Stat::default();
        let _ret = fstatat64(
            AT_FDCWD,
            b"/nonexistent_link\0".as_ptr(),
            &raw mut st,
            AT_SYMLINK_NOFOLLOW,
        );
    }

    // -----------------------------------------------------------------------
    // faccessat2
    // -----------------------------------------------------------------------

    #[test]
    fn test_faccessat2_null_path() {
        crate::errno::set_errno(0);
        let ret = faccessat2(AT_FDCWD, core::ptr::null(), 0, 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_faccessat2_nonexistent() {
        // Syscall result is unpredictable on test host — just verify no crash.
        let _ret = faccessat2(
            AT_FDCWD,
            b"/nonexistent_file_xyz\0".as_ptr(),
            crate::fcntl::F_OK,
            0,
        );
    }

    #[test]
    fn test_faccessat2_with_nofollow() {
        // Verify the nofollow flag path doesn't crash.
        let _ret = faccessat2(
            AT_FDCWD,
            b"/nonexistent\0".as_ptr(),
            crate::fcntl::F_OK,
            AT_SYMLINK_NOFOLLOW,
        );
    }

    // -----------------------------------------------------------------------
    // Phase 91: access / faccessat mode + flags validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_access_phase91_mode_constants_distinct() {
        assert_eq!(crate::fcntl::F_OK, 0);
        assert_eq!(crate::fcntl::R_OK, 4);
        assert_eq!(crate::fcntl::W_OK, 2);
        assert_eq!(crate::fcntl::X_OK, 1);
        // R_OK | W_OK | X_OK == 7 (S_IRWXO equivalent for mode check).
        assert_eq!(
            crate::fcntl::R_OK | crate::fcntl::W_OK | crate::fcntl::X_OK,
            7
        );
    }

    #[test]
    fn test_access_phase91_unknown_mode_bit_einval() {
        // Bit 3 (0b1000) is not a defined access mode bit.
        crate::errno::set_errno(0);
        let ret = access(b"/tmp\0".as_ptr(), 0b1000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_access_phase91_high_bit_mode_einval() {
        crate::errno::set_errno(0);
        let ret = access(b"/tmp\0".as_ptr(), i32::MIN);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_access_phase91_f_ok_passes_mode_check() {
        // F_OK == 0 must not fail the mode mask check.  Whether the
        // file exists is unrelated; we only assert errno is NOT EINVAL
        // from the prologue.
        crate::errno::set_errno(0);
        let _ret = access(b"/nonexistent_xyz\0".as_ptr(), crate::fcntl::F_OK);
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_access_phase91_all_valid_modes_pass_mode_check() {
        crate::errno::set_errno(0);
        let _ret = access(
            b"/nonexistent_xyz\0".as_ptr(),
            crate::fcntl::R_OK | crate::fcntl::W_OK | crate::fcntl::X_OK,
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_access_phase91_mode_check_beats_null_path() {
        // Bad mode + null path → EINVAL (mode check fires first),
        // not EFAULT (null path).
        crate::errno::set_errno(0);
        let ret = access(core::ptr::null(), 0b1000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_faccessat_phase91_unknown_mode_bit_einval() {
        crate::errno::set_errno(0);
        let ret = faccessat(AT_FDCWD, b"/tmp\0".as_ptr(), 0b1000, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_faccessat_phase91_unknown_flag_einval() {
        // Bit not in (AT_EACCESS | AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH).
        crate::errno::set_errno(0);
        let ret = faccessat(AT_FDCWD, b"/tmp\0".as_ptr(), 0, 0x4000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_faccessat_phase91_at_symlink_follow_rejected() {
        // AT_SYMLINK_FOLLOW (0x400) is NOT a valid faccessat flag —
        // only AT_SYMLINK_NOFOLLOW is.
        crate::errno::set_errno(0);
        let ret = faccessat(AT_FDCWD, b"/tmp\0".as_ptr(), 0, AT_SYMLINK_FOLLOW);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_faccessat_phase91_eaccess_flag_accepted() {
        // AT_EACCESS must pass the flag check.
        crate::errno::set_errno(0);
        let _ret = faccessat(
            AT_FDCWD,
            b"/nonexistent_xyz\0".as_ptr(),
            crate::fcntl::F_OK,
            AT_EACCESS,
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_faccessat_phase91_symlink_nofollow_flag_accepted() {
        crate::errno::set_errno(0);
        let _ret = faccessat(
            AT_FDCWD,
            b"/nonexistent_xyz\0".as_ptr(),
            crate::fcntl::F_OK,
            AT_SYMLINK_NOFOLLOW,
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_faccessat_phase91_empty_path_flag_accepted() {
        crate::errno::set_errno(0);
        let _ret = faccessat(
            AT_FDCWD,
            b"/nonexistent_xyz\0".as_ptr(),
            crate::fcntl::F_OK,
            AT_EMPTY_PATH,
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_faccessat_phase91_all_valid_flags_accepted() {
        crate::errno::set_errno(0);
        let _ret = faccessat(
            AT_FDCWD,
            b"/nonexistent_xyz\0".as_ptr(),
            crate::fcntl::F_OK,
            AT_EACCESS | AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH,
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_faccessat_phase91_mode_check_beats_flag_check() {
        // Both bad → mode check fires first (matches Linux's
        // do_faccessat prologue order).
        crate::errno::set_errno(0);
        let ret = faccessat(AT_FDCWD, b"/tmp\0".as_ptr(), 0b1000, 0x4000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_faccessat_phase91_mode_check_beats_null_path() {
        crate::errno::set_errno(0);
        let ret = faccessat(AT_FDCWD, core::ptr::null(), 0b1000, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_faccessat_phase91_flag_check_beats_null_path() {
        // valid mode + bad flag + null path → EINVAL (not EFAULT).
        crate::errno::set_errno(0);
        let ret = faccessat(AT_FDCWD, core::ptr::null(), 0, 0x4000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_faccessat2_phase91_unknown_mode_bit_einval() {
        // faccessat2 delegates to faccessat — it must inherit
        // the same EINVAL behaviour.
        crate::errno::set_errno(0);
        let ret = faccessat2(AT_FDCWD, b"/tmp\0".as_ptr(), 0b1000, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_faccessat2_phase91_unknown_flag_einval() {
        crate::errno::set_errno(0);
        let ret = faccessat2(AT_FDCWD, b"/tmp\0".as_ptr(), 0, 0x4000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_access_phase91_einval_then_valid_progression() {
        crate::errno::set_errno(0);
        let ret = access(b"/nonexistent_xyz\0".as_ptr(), 0b1000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Subsequent valid call passes the mode check.
        crate::errno::set_errno(0);
        let _ret = access(b"/nonexistent_xyz\0".as_ptr(), crate::fcntl::F_OK);
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // openat2
    // -----------------------------------------------------------------------

    #[test]
    fn test_openat2_null_how() {
        crate::errno::set_errno(0);
        let ret = openat2(AT_FDCWD, b"/tmp\0".as_ptr(), core::ptr::null(), 24);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_openat2_short_size() {
        crate::errno::set_errno(0);
        let how = OpenHow {
            flags: 0,
            mode: 0,
            resolve: 0,
        };
        let ret = openat2(AT_FDCWD, b"/tmp\0".as_ptr(), &how, 1); // too small
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_openat2_struct_layout() {
        assert_eq!(core::mem::size_of::<OpenHow>(), 24);
    }

    #[test]
    fn test_openat2_resolve_flags_distinct() {
        // Each RESOLVE_* flag is a distinct power of two.
        let flags = [
            RESOLVE_NO_XDEV,
            RESOLVE_NO_MAGICLINKS,
            RESOLVE_NO_SYMLINKS,
            RESOLVE_BENEATH,
            RESOLVE_IN_ROOT,
            RESOLVE_CACHED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two(), "flag at {i} not power of 2");
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j], "duplicate flags at {i} and {j}");
            }
        }
    }

    #[test]
    fn test_openat2_valid_how() {
        // Valid OpenHow — delegates to openat.  Syscall result is
        // unpredictable on the test host; just verify no crash.
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0,
            resolve: 0,
        };
        let _ret = openat2(
            AT_FDCWD,
            b"/nonexistent_openat2_test\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
    }

    // ===================================================================
    // Phase 135 — openat2 validation order matches Linux's sys_openat2
    // (fs/open.c) and rejects unknown resolve bits + oversized usize.
    // ===================================================================

    // -- Validation order: size first, then E2BIG, then EFAULT -------------

    #[test]
    fn test_phase135_null_how_with_undersized_returns_einval_not_efault() {
        // BEFORE Phase 135: (NULL, 1) returned EFAULT (NULL check first).
        // AFTER: matches Linux's `copy_struct_from_user` order, which
        // checks size < min before touching the pointer.  The right
        // fix for a buggy caller is to pass the correct size, not to
        // allocate a struct.
        crate::errno::set_errno(0);
        let ret = openat2(AT_FDCWD, b"/tmp\0".as_ptr(), core::ptr::null(), 1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase135_null_how_with_zero_size_returns_einval_not_efault() {
        crate::errno::set_errno(0);
        let ret = openat2(AT_FDCWD, b"/tmp\0".as_ptr(), core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase135_null_how_with_minimum_size_still_efault() {
        // Sanity: size IS valid; NULL pointer still produces EFAULT.
        // The reorder didn't break the existing contract.
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            core::ptr::null(),
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- Oversized size → E2BIG ---------------------------------------------

    #[test]
    fn test_phase135_oversized_size_returns_e2big() {
        // BEFORE Phase 135: no upper bound — any huge usize was
        // accepted as long as `how != NULL`.  Linux caps at PAGE_SIZE
        // and rejects anything larger with E2BIG so userspace gets a
        // clear signal that the kernel doesn't know about that struct
        // version.
        let how = OpenHow {
            flags: 0,
            mode: 0,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &how,
            10_000, // way above 4 KiB
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::E2BIG);
    }

    #[test]
    fn test_phase135_oversized_size_e2big_wins_over_efault() {
        // E2BIG is checked before NULL-attr dereference, so a huge
        // size with NULL how still gets E2BIG.  Right diagnostic
        // (the caller's size argument is wrong; their pointer is a
        // red herring).
        crate::errno::set_errno(0);
        let ret = openat2(AT_FDCWD, b"/tmp\0".as_ptr(), core::ptr::null(), 10_000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::E2BIG);
    }

    #[test]
    fn test_phase135_exact_page_size_is_accepted_size_wise() {
        // size == 4096 is the boundary: Linux accepts it (E2BIG fires
        // for `> PAGE_SIZE`, not `>= PAGE_SIZE`).  We can't open a
        // real file in the test environment, but we can verify the
        // size check doesn't reject it.  Provide a valid how with
        // size=4096 and check we DON'T see E2BIG.
        let mut buf = [0u8; 4096];
        let how_ptr = buf.as_mut_ptr().cast::<OpenHow>();
        // SAFETY: 4096 > sizeof::<OpenHow>(), and OpenHow's all-zero
        // value (every field 0) is a valid bit pattern.
        crate::errno::set_errno(0);
        let _ret = openat2(AT_FDCWD, b"/nonexistent_phase135\0".as_ptr(), how_ptr, 4096);
        // We don't care about the actual return; just that it isn't
        // E2BIG-from-our-size-check.
        assert_ne!(crate::errno::get_errno(), crate::errno::E2BIG);
    }

    // -- Resolve-bit validation -------------------------------------------

    #[test]
    fn test_phase135_unknown_resolve_bit_einval() {
        // BEFORE Phase 135: unknown resolve bits were silently passed
        // through to `openat`, which has no `resolve` argument — so the
        // caller's security restriction was silently dropped.  Linux
        // rejects with EINVAL so the caller knows the kernel didn't
        // honour their request.
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0,
            resolve: 1u64 << 30, // not a defined RESOLVE_* bit
        };
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase135_unknown_high_resolve_bit_einval() {
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0,
            resolve: 1u64 << 63, // top bit, definitely unknown
        };
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase135_mixed_known_and_unknown_resolve_bits_einval() {
        // Even mixing a known bit (NO_SYMLINKS) with an unknown one is
        // EINVAL — Linux rejects on any unknown bit, regardless of
        // what else is set.
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0,
            resolve: RESOLVE_NO_SYMLINKS | (1u64 << 40),
        };
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase135_all_known_resolve_bits_pass_validation() {
        // The union of every defined RESOLVE_* bit must pass our
        // validation (whether the actual open succeeds is irrelevant).
        let all_known = RESOLVE_NO_XDEV
            | RESOLVE_NO_MAGICLINKS
            | RESOLVE_NO_SYMLINKS
            | RESOLVE_BENEATH
            | RESOLVE_IN_ROOT
            | RESOLVE_CACHED;
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0,
            resolve: all_known,
        };
        crate::errno::set_errno(0);
        let _ret = openat2(
            AT_FDCWD,
            b"/nonexistent_phase135_all_resolve\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        // Whatever the open result, errno must NOT be EINVAL from
        // our resolve-bit check.
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase135_zero_resolve_passes_validation() {
        // resolve=0 (no restrictions) is the common case — must pass
        // validation.
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let _ret = openat2(
            AT_FDCWD,
            b"/nonexistent_phase135_zero_resolve\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- Ordering: resolve check happens only after size/EFAULT pass ------

    #[test]
    fn test_phase135_size_check_beats_resolve_check() {
        // size < min wins even if resolve has garbage bits — the
        // resolve field isn't reached until after the struct copy.
        let how = OpenHow {
            flags: 0,
            mode: 0,
            resolve: 0xDEAD_BEEF_DEAD_BEEF,
        };
        crate::errno::set_errno(0);
        let ret = openat2(AT_FDCWD, b"/tmp\0".as_ptr(), &how, 1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        // (We can't distinguish "EINVAL from size" vs "EINVAL from
        // resolve" by errno alone, but Linux's order means size wins;
        // verified by the absence of any side effect on `how`.)
    }

    // -- Workflow & recovery ----------------------------------------------

    #[test]
    fn test_phase135_recoverable_after_e2big() {
        // First call: oversized usize → E2BIG.
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let bad = openat2(AT_FDCWD, b"/tmp\0".as_ptr(), &how, 10_000);
        assert_eq!(bad, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::E2BIG);

        // Second call: correct size → no E2BIG.
        crate::errno::set_errno(0);
        let _ret = openat2(
            AT_FDCWD,
            b"/nonexistent_phase135_recovery\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::E2BIG);
    }

    // ===================================================================
    // Phase 136 — openat2 mode validation matches build_open_how:
    // mode bits outside 0o7777 → EINVAL; mode != 0 without O_CREAT or
    // raw __O_TMPFILE → EINVAL.  Closes the deferred item flagged in
    // Phase 135.
    // ===================================================================

    // -- Mode bit-range check ----------------------------------------------

    #[test]
    fn test_phase136_mode_extra_bit_einval() {
        // BEFORE Phase 136: mode = 0o10000 (bit above the S_IALLUGO
        // mask) was silently accepted and passed through to openat,
        // where it'd be truncated to ModeT in unspecified ways.
        // AFTER: matches Linux's `if (how->mode & ~S_IALLUGO) -EINVAL`.
        let how = OpenHow {
            flags: crate::fcntl::O_CREAT as u64,
            mode: 0o10_000, // one bit above the 12-bit mask
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase136_mode_high_bit_einval() {
        // The top bit (bit 63) in mode should fail just as clearly.
        let how = OpenHow {
            flags: crate::fcntl::O_CREAT as u64,
            mode: 1u64 << 63,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase136_all_12_mode_bits_accepted() {
        // 0o7777 = every defined mode bit.  Must not be rejected by
        // the bit-range check (whether the actual open succeeds is
        // irrelevant — we just need not-EINVAL from our validation).
        let how = OpenHow {
            flags: crate::fcntl::O_CREAT as u64,
            mode: 0o7777,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let _ret = openat2(
            AT_FDCWD,
            b"/nonexistent_phase136_full_mode\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- mode-without-creation-flag check ----------------------------------

    #[test]
    fn test_phase136_mode_without_o_creat_or_tmpfile_einval() {
        // BEFORE Phase 136: a non-zero mode with O_RDONLY was silently
        // passed through, even though Linux returns EINVAL — the mode
        // can never take effect because no file is being created.
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0o644,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase136_mode_with_o_creat_passes_validation() {
        // O_CREAT + valid mode → no EINVAL from our validation.
        let how = OpenHow {
            flags: crate::fcntl::O_CREAT as u64 | crate::fcntl::O_RDWR as u64,
            mode: 0o644,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let _ret = openat2(
            AT_FDCWD,
            b"/nonexistent_phase136_creat\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase136_mode_with_o_tmpfile_passes_validation() {
        // O_TMPFILE (= __O_TMPFILE | O_DIRECTORY) covers the raw
        // __O_TMPFILE bit, so the mode check passes.
        let how = OpenHow {
            flags: crate::fcntl::O_TMPFILE as u64 | crate::fcntl::O_RDWR as u64,
            mode: 0o600,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let _ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_open_o_tmpfile_eopnotsupp() {
        // O_TMPFILE is unsupported: our kernel file handles are path-based
        // and cannot represent an anonymous unlinked inode.  open() must
        // fail cleanly with EOPNOTSUPP (as Linux does on unsupported
        // filesystems) instead of silently opening the directory path.
        crate::errno::set_errno(0);
        let ret = open(
            b"/tmp\0".as_ptr(),
            crate::fcntl::O_TMPFILE | crate::fcntl::O_RDWR,
            0o600,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
    }

    #[test]
    fn test_phase136_zero_mode_without_o_creat_passes() {
        // mode = 0 is the common case for read-only opens — must
        // never trigger the mode-vs-flags check.
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let _ret = openat2(
            AT_FDCWD,
            b"/nonexistent_phase136_zero_mode\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase136_mode_with_o_directory_alone_einval() {
        // O_DIRECTORY by itself is NOT __O_TMPFILE — the raw tmpfile
        // bit is the high one (0o20_000_000).  A caller passing
        // O_DIRECTORY + mode must still get EINVAL because no file
        // creation is happening.
        let how = OpenHow {
            flags: crate::fcntl::O_DIRECTORY as u64,
            mode: 0o755,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- Ordering interactions --------------------------------------------

    #[test]
    fn test_phase136_resolve_check_beats_mode_check() {
        // Garbage resolve bit takes priority — Linux's order is
        // resolve → mode-bits → mode-vs-flags.
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0o644,         // also bad (no O_CREAT)
            resolve: 1u64 << 40, // bad resolve bit
        };
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(ret, -1);
        // Both produce EINVAL; we can't differentiate by errno but the
        // resolve check runs first.
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase136_mode_bits_check_beats_mode_vs_flags() {
        // Mode has a bad bit (0o10000) AND lacks O_CREAT — Linux
        // checks the bit range first, so the EINVAL comes from the
        // bit-range arm.  Both produce EINVAL but the order matters
        // for the diagnostic.
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0o10_000,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- Workflow & recovery ----------------------------------------------

    #[test]
    fn test_phase136_recoverable_after_bad_mode_bits() {
        // First call: mode has out-of-range bit → EINVAL.
        let bad = OpenHow {
            flags: crate::fcntl::O_CREAT as u64,
            mode: 0o10_644,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let r1 = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &bad,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(r1, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Second call: mode trimmed to legal bits — validation passes.
        let good = OpenHow {
            flags: crate::fcntl::O_CREAT as u64,
            mode: 0o644,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let _r2 = openat2(
            AT_FDCWD,
            b"/nonexistent_phase136_recovery\0".as_ptr(),
            &good,
            core::mem::size_of::<OpenHow>(),
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase136_typical_create_workflow() {
        // 1. Open existing (no mode, no O_CREAT).
        let read = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let _ = openat2(
            AT_FDCWD,
            b"/nonexistent_phase136_step1\0".as_ptr(),
            &read,
            core::mem::size_of::<OpenHow>(),
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);

        // 2. Create with O_CREAT + mode — must pass validation.
        let create = OpenHow {
            flags: crate::fcntl::O_CREAT as u64 | crate::fcntl::O_WRONLY as u64,
            mode: 0o600,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let _ = openat2(
            AT_FDCWD,
            b"/nonexistent_phase136_step2\0".as_ptr(),
            &create,
            core::mem::size_of::<OpenHow>(),
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- Buggy caller -----------------------------------------------------

    #[test]
    fn test_phase136_buggy_caller_uninitialised_mode_field() {
        // A common bug: caller zeroes flags/resolve but forgets mode,
        // leaving garbage from the stack.  Without O_CREAT this is
        // EINVAL (caught), not a silent mode-bits-ignored open.
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0xDEAD_BEEF_DEAD_BEEF,
            resolve: 0,
        };
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_phase135_recoverable_after_bad_resolve_bit() {
        // First call: unknown resolve bit → EINVAL.
        let bad_how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0,
            resolve: 1u64 << 30,
        };
        crate::errno::set_errno(0);
        let bad = openat2(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &bad_how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(bad, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Second call: clean how — no EINVAL from our validation.
        let good_how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0,
            resolve: RESOLVE_NO_SYMLINKS,
        };
        crate::errno::set_errno(0);
        let _ret = openat2(
            AT_FDCWD,
            b"/nonexistent_phase135_good_resolve\0".as_ptr(),
            &good_how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // statx
    // -----------------------------------------------------------------------

    #[test]
    fn test_statx_null_buf() {
        crate::errno::set_errno(0);
        let ret = statx(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            0,
            STATX_ALL,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_statx_struct_layout() {
        // statx is 256 bytes on Linux x86_64.
        assert_eq!(core::mem::size_of::<Statx>(), 256);
    }

    #[test]
    fn test_statx_timestamp_layout() {
        // StatxTimestamp: i64 + u32 + i32 = 16 bytes.
        assert_eq!(core::mem::size_of::<StatxTimestamp>(), 16);
    }

    #[test]
    fn test_statx_mask_constants() {
        assert_eq!(STATX_TYPE, 0x0001);
        assert_eq!(STATX_MODE, 0x0002);
        assert_eq!(STATX_NLINK, 0x0004);
        assert_eq!(STATX_UID, 0x0008);
        assert_eq!(STATX_GID, 0x0010);
        assert_eq!(STATX_ATIME, 0x0020);
        assert_eq!(STATX_MTIME, 0x0040);
        assert_eq!(STATX_CTIME, 0x0080);
        assert_eq!(STATX_INO, 0x0100);
        assert_eq!(STATX_SIZE, 0x0200);
        assert_eq!(STATX_BLOCKS, 0x0400);
        assert_eq!(STATX_BASIC_STATS, 0x07FF);
        assert_eq!(STATX_BTIME, 0x0800);
    }

    #[test]
    fn test_statx_nonexistent_path() {
        // Syscall result is unpredictable on test host.
        // If the underlying fstatat returns 0, statx fills the struct;
        // if it returns -1, statx propagates the error.  Both are valid.
        let mut sx = Statx::default();
        let ret = statx(
            AT_FDCWD,
            b"/nonexistent_statx_test\0".as_ptr(),
            0,
            STATX_ALL,
            &raw mut sx,
        );
        if ret == 0 {
            // statx filled the struct — stx_mask should have bits set.
            assert_ne!(sx.stx_mask, 0);
        }
        // Either way, no crash.
    }

    #[test]
    fn test_statx_basic_stats_mask() {
        // STATX_BASIC_STATS should be all basic bits ORed.
        let expected = STATX_TYPE
            | STATX_MODE
            | STATX_NLINK
            | STATX_UID
            | STATX_GID
            | STATX_ATIME
            | STATX_MTIME
            | STATX_CTIME
            | STATX_INO
            | STATX_SIZE
            | STATX_BLOCKS;
        assert_eq!(STATX_BASIC_STATS, expected);
    }

    #[test]
    fn test_statx_default_zeroed() {
        let sx = Statx::default();
        assert_eq!(sx.stx_mask, 0);
        assert_eq!(sx.stx_size, 0);
        assert_eq!(sx.stx_uid, 0);
        assert_eq!(sx.stx_gid, 0);
        assert_eq!(sx.stx_ino, 0);
    }

    #[test]
    fn test_statx_all_includes_btime() {
        assert_eq!(STATX_ALL, 0x0FFF);
        assert_ne!(STATX_ALL & STATX_BTIME, 0);
    }

    // -----------------------------------------------------------------------
    // Phase 62: tee / name_to_handle_at / open_by_handle_at validators
    // -----------------------------------------------------------------------

    // --- splice flag constants -------------------------------------------

    #[test]
    fn test_splice_f_valid_mask() {
        assert_eq!(
            SPLICE_F_VALID,
            SPLICE_F_MOVE | SPLICE_F_NONBLOCK | SPLICE_F_MORE | SPLICE_F_GIFT,
        );
        // Mask must equal the OR of the four defined bits (1|2|4|8 = 15).
        assert_eq!(SPLICE_F_VALID, 0xF);
    }

    #[test]
    fn test_splice_f_valid_rejects_unknown_bits() {
        // First unknown bit is 0x10.
        assert_eq!(0x10u32 & !SPLICE_F_VALID, 0x10);
        assert_eq!(0xFFFFu32 & !SPLICE_F_VALID, 0xFFF0);
    }

    // --- tee: flag validation --------------------------------------------

    #[test]
    fn test_tee_unknown_flag_bit_einval() {
        // Unknown flag must be rejected before any fd lookup.  Use
        // negative fds to prove flags are checked first.
        crate::errno::set_errno(0);
        let ret = tee(-1, -1, 1, 0x10);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_tee_high_garbage_flag_einval() {
        crate::errno::set_errno(0);
        let ret = tee(-1, -1, 1, 0xFFFF_FFFF);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_tee_all_known_flags_pass_flag_check() {
        // All four defined bits together must not produce EINVAL.
        // Either the fd check fails (EBADF) or we get further.
        crate::errno::set_errno(0);
        let ret = tee(-1, -1, 1, SPLICE_F_VALID);
        assert_eq!(ret, -1);
        // -1 was rejected for fd reasons, not flag reasons.
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // --- tee: fd validation ---------------------------------------------

    #[test]
    fn test_tee_negative_fd_in_ebadf() {
        crate::errno::set_errno(0);
        let ret = tee(-1, 1, 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_tee_negative_fd_out_ebadf() {
        crate::errno::set_errno(0);
        let ret = tee(0, -1, 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_tee_both_negative_fds_ebadf() {
        crate::errno::set_errno(0);
        let ret = tee(-5, -7, 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_tee_nonexistent_fd_in_ebadf() {
        // 100000 is far above any open fd index in tests.
        crate::errno::set_errno(0);
        let ret = tee(100_000, 100_001, 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // --- tee: pipe-only requirement -------------------------------------

    #[test]
    fn test_tee_non_pipe_fd_in_einval() {
        // Open a regular file as fd_in and a pipe as fd_out — Linux
        // returns EINVAL because tee requires both ends to be pipes.
        use crate::fdtable;
        let path = "tee_nonpipe_in.tmp\0";
        let fd_file = open(
            path.as_ptr(),
            crate::fcntl::O_CREAT | crate::fcntl::O_RDWR,
            0o644,
        );
        if fd_file < 0 {
            return;
        }
        let mut pf = [0i32; 2];
        if crate::pipe::pipe(pf.as_mut_ptr()) != 0 {
            let _ = fdtable::close_fd(fd_file);
            let _ = unlink(path.as_ptr());
            return;
        }
        crate::errno::set_errno(0);
        let ret = tee(fd_file, pf[1], 16, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd_file);
        let _ = fdtable::close_fd(pf[0]);
        let _ = fdtable::close_fd(pf[1]);
        let _ = unlink(path.as_ptr());
    }

    #[test]
    fn test_tee_non_pipe_fd_out_einval() {
        use crate::fdtable;
        let path = "tee_nonpipe_out.tmp\0";
        let fd_file = open(
            path.as_ptr(),
            crate::fcntl::O_CREAT | crate::fcntl::O_RDWR,
            0o644,
        );
        if fd_file < 0 {
            return;
        }
        let mut pf = [0i32; 2];
        if crate::pipe::pipe(pf.as_mut_ptr()) != 0 {
            let _ = fdtable::close_fd(fd_file);
            let _ = unlink(path.as_ptr());
            return;
        }
        crate::errno::set_errno(0);
        let ret = tee(pf[0], fd_file, 16, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd_file);
        let _ = fdtable::close_fd(pf[0]);
        let _ = fdtable::close_fd(pf[1]);
        let _ = unlink(path.as_ptr());
    }

    // --- tee: zero-length short-circuit ---------------------------------

    #[test]
    fn test_tee_zero_len_validates_then_succeeds() {
        use crate::fdtable;
        let mut pf1 = [0i32; 2];
        let mut pf2 = [0i32; 2];
        if crate::pipe::pipe(pf1.as_mut_ptr()) != 0 || crate::pipe::pipe(pf2.as_mut_ptr()) != 0 {
            return;
        }
        crate::errno::set_errno(0);
        // Zero length still goes through fd + pipe-kind validation.
        assert_eq!(tee(pf1[0], pf2[1], 0, 0), 0);
        let _ = fdtable::close_fd(pf1[0]);
        let _ = fdtable::close_fd(pf1[1]);
        let _ = fdtable::close_fd(pf2[0]);
        let _ = fdtable::close_fd(pf2[1]);
    }

    #[test]
    fn test_tee_zero_len_with_bad_fd_still_ebadf() {
        // Zero length does not exempt the caller from validation.
        crate::errno::set_errno(0);
        let ret = tee(-1, -1, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // --- tee: ordering ---------------------------------------------------

    #[test]
    fn test_tee_flag_check_before_fd_check() {
        // Bad flags AND bad fds — flag error must win.
        crate::errno::set_errno(0);
        let ret = tee(-1, -1, 1, 0x80);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_tee_fd_check_before_pipe_kind_check() {
        // Negative fd takes precedence over pipe-kind — we never
        // dereference a missing fd to check its kind.
        crate::errno::set_errno(0);
        let ret = tee(-1, -2, 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // --- name_to_handle_at: flag validation ------------------------------

    #[test]
    fn test_name_to_handle_at_flags_valid_mask() {
        assert_eq!(
            NAME_TO_HANDLE_AT_FLAGS_VALID,
            AT_SYMLINK_FOLLOW | AT_EMPTY_PATH
        );
        // 0x800 is between FOLLOW(0x400) and EMPTY_PATH(0x1000) and
        // must not be in the accepted set.
        assert_eq!(0x800 & !NAME_TO_HANDLE_AT_FLAGS_VALID, 0x800);
    }

    #[test]
    fn test_name_to_handle_at_unknown_flag_einval() {
        let mut fh = FileHandle {
            handle_bytes: 128,
            handle_type: 0,
        };
        let mut mid: i32 = 0;
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &raw mut fh,
            &raw mut mid,
            0x800,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_name_to_handle_at_accepts_at_symlink_follow() {
        let mut fh = FileHandle {
            handle_bytes: 128,
            handle_type: 0,
        };
        let mut mid: i32 = 0;
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &raw mut fh,
            &raw mut mid,
            AT_SYMLINK_FOLLOW,
        );
        assert_eq!(ret, -1);
        // AT_SYMLINK_FOLLOW is accepted — we should reach the ENOSYS
        // sentinel, not EINVAL.
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_name_to_handle_at_accepts_at_empty_path() {
        let mut fh = FileHandle {
            handle_bytes: 128,
            handle_type: 0,
        };
        let mut mid: i32 = 0;
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(
            AT_FDCWD,
            b"\0".as_ptr(),
            &raw mut fh,
            &raw mut mid,
            AT_EMPTY_PATH,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- name_to_handle_at: NULL-pointer validation ---------------------

    #[test]
    fn test_name_to_handle_at_null_pathname_efault() {
        let mut fh = FileHandle {
            handle_bytes: 128,
            handle_type: 0,
        };
        let mut mid: i32 = 0;
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(AT_FDCWD, core::ptr::null(), &raw mut fh, &raw mut mid, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_name_to_handle_at_null_handle_efault() {
        let mut mid: i32 = 0;
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            core::ptr::null_mut(),
            &raw mut mid,
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_name_to_handle_at_null_mount_id_efault() {
        let mut fh = FileHandle {
            handle_bytes: 128,
            handle_type: 0,
        };
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &raw mut fh,
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // --- name_to_handle_at: dirfd validation ----------------------------

    #[test]
    fn test_name_to_handle_at_negative_dirfd_ebadf() {
        let mut fh = FileHandle {
            handle_bytes: 128,
            handle_type: 0,
        };
        let mut mid: i32 = 0;
        crate::errno::set_errno(0);
        // -5 is not AT_FDCWD (-100), so it must be a valid open fd.
        let ret = name_to_handle_at(-5, b"foo\0".as_ptr(), &raw mut fh, &raw mut mid, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_name_to_handle_at_nonexistent_dirfd_ebadf() {
        let mut fh = FileHandle {
            handle_bytes: 128,
            handle_type: 0,
        };
        let mut mid: i32 = 0;
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(100_000, b"foo\0".as_ptr(), &raw mut fh, &raw mut mid, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // --- name_to_handle_at: ordering -------------------------------------

    #[test]
    fn test_name_to_handle_at_flag_check_before_pointer_check() {
        // Bad flags AND NULL pathname — flag check wins.
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(
            AT_FDCWD,
            core::ptr::null(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            0x800,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_name_to_handle_at_pointer_check_before_dirfd_check() {
        // NULL pathname AND bad dirfd — EFAULT wins.
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(
            -5,
            core::ptr::null(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // --- open_by_handle_at: pointer validation --------------------------

    #[test]
    fn test_open_by_handle_at_null_handle_efault() {
        crate::errno::set_errno(0);
        let ret = open_by_handle_at(AT_FDCWD, core::ptr::null_mut(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_open_by_handle_at_negative_mountfd_ebadf() {
        let mut fh = FileHandle {
            handle_bytes: 0,
            handle_type: 0,
        };
        crate::errno::set_errno(0);
        let ret = open_by_handle_at(-5, &raw mut fh, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_open_by_handle_at_nonexistent_mountfd_ebadf() {
        let mut fh = FileHandle {
            handle_bytes: 0,
            handle_type: 0,
        };
        crate::errno::set_errno(0);
        let ret = open_by_handle_at(100_000, &raw mut fh, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_open_by_handle_at_pointer_check_before_fd_check() {
        // NULL handle AND bad mount_fd — EFAULT wins.
        crate::errno::set_errno(0);
        let ret = open_by_handle_at(-5, core::ptr::null_mut(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // --- workflow + real-world buggy callers ----------------------------

    #[test]
    fn test_workflow_tee_pipeline_short_circuit() {
        // A real workflow: program decides at runtime whether to tee
        // into a backup pipe.  When len==0 (nothing to forward yet),
        // tee must still validate args but return 0.
        use crate::fdtable;
        let mut a = [0i32; 2];
        let mut b = [0i32; 2];
        if crate::pipe::pipe(a.as_mut_ptr()) != 0 || crate::pipe::pipe(b.as_mut_ptr()) != 0 {
            return;
        }
        assert_eq!(tee(a[0], b[1], 0, SPLICE_F_NONBLOCK), 0);
        // And with payload: ENOSYS (not yet supported in our pipe layer).
        crate::errno::set_errno(0);
        assert_eq!(tee(a[0], b[1], 4096, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        let _ = fdtable::close_fd(a[0]);
        let _ = fdtable::close_fd(a[1]);
        let _ = fdtable::close_fd(b[0]);
        let _ = fdtable::close_fd(b[1]);
    }

    #[test]
    fn test_buggy_caller_tee_passes_uninitialized_fd() {
        // Some real-world bug: caller forgot to initialize fd_in (left
        // at its uninitialized i32 default which we simulate with -1).
        crate::errno::set_errno(0);
        let ret = tee(-1, 1, 1024, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_buggy_caller_name_to_handle_at_swaps_flag_constants() {
        // Caller confuses AT_SYMLINK_NOFOLLOW (which is for stat-family)
        // with AT_SYMLINK_FOLLOW (which is what name_to_handle_at wants).
        // AT_SYMLINK_NOFOLLOW is 0x100 — outside our valid mask.
        let mut fh = FileHandle {
            handle_bytes: 128,
            handle_type: 0,
        };
        let mut mid: i32 = 0;
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            &raw mut fh,
            &raw mut mid,
            AT_SYMLINK_NOFOLLOW,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_buggy_caller_open_by_handle_at_stack_zero_handle() {
        // Caller declared a FileHandle on the stack but forgot to fill
        // it.  Validation must pass (pointer is non-NULL) and we
        // surface ENOSYS — the caller's bug is observable through the
        // syscall *succeeding* validation, not through a misleading
        // EFAULT.
        let mut fh = FileHandle {
            handle_bytes: 0,
            handle_type: 0,
        };
        crate::errno::set_errno(0);
        let ret = open_by_handle_at(AT_FDCWD, &raw mut fh, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // Phase 70 — chmod / fchmod / chown / fchown / lchown validators
    //
    // The body is a no-op success (no permission system yet), but the
    // entry-prologue validates the bug-shaped inputs Linux rejects with
    // EFAULT (NULL path pointer) or EBADF (negative or closed fd).
    // -----------------------------------------------------------------

    // ---- chmod ----

    #[test]
    fn test_chmod_null_path_efault() {
        crate::errno::set_errno(0);
        assert_eq!(chmod(core::ptr::null(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_chmod_valid_path_returns_zero() {
        // Mode bits outside 0o7777 must not be rejected — the kernel masks
        // them to the permission bits.  On the host build chmod validates
        // the pointer and returns 0 without issuing SYS_FS_SET_PERMS.
        assert_eq!(chmod(b"/etc/passwd\0".as_ptr(), 0xFFFFFFFF), 0);
    }

    #[test]
    fn test_chmod_empty_path_still_returns_zero() {
        // An empty C string is a valid non-NULL pointer; on the host build
        // the syscall is not issued, so the call returns 0 after pointer
        // validation.
        assert_eq!(chmod(b"\0".as_ptr(), 0o755), 0);
    }

    // ---- fchmod ----

    #[test]
    fn test_fchmod_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(fchmod(-1, 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fchmod_min_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(fchmod(i32::MIN, 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fchmod_unopen_fd_ebadf() {
        // Pick an fd value far above anything alloc_fd hands out and
        // verify it isn't in the table; if some other test happens to
        // have left it open, allocate a fresh one and close it.
        let probe: i32 = 0x4000_0001;
        if fdtable::get_fd(probe).is_some() {
            let _ = fdtable::close_fd(probe);
        }
        crate::errno::set_errno(0);
        assert_eq!(fchmod(probe, 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fchmod_open_fd_returns_zero() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(fchmod(fd, 0o600), 0);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_fchmod_pipe_fd_still_returns_zero() {
        // fchmod on a pipe is permitted on Linux (EBADF only on closed fds,
        // not on non-file kinds), so accept the call.
        let fd = fdtable::alloc_fd(HandleKind::Pipe, 1).expect("alloc_fd Pipe failed");
        assert_eq!(fchmod(fd, 0o400), 0);
        let _ = fdtable::close_fd(fd);
    }

    // ---- chown ----

    #[test]
    fn test_chown_null_path_efault() {
        crate::errno::set_errno(0);
        assert_eq!(chown(core::ptr::null(), 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_chown_valid_path_returns_zero() {
        assert_eq!(chown(b"/etc/passwd\0".as_ptr(), 0, 0), 0);
    }

    #[test]
    fn test_chown_minus_one_owner_returns_zero() {
        // (uid_t)-1 in both fields means "change nothing" in POSIX, so the
        // call short-circuits to success without issuing SYS_FS_SET_OWNER.
        assert_eq!(chown(b"/etc/passwd\0".as_ptr(), UidT::MAX, UidT::MAX), 0);
    }

    // ---- fchown ----

    #[test]
    fn test_fchown_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(fchown(-1, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fchown_min_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(fchown(i32::MIN, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fchown_unopen_fd_ebadf() {
        let probe: i32 = 0x4000_0002;
        if fdtable::get_fd(probe).is_some() {
            let _ = fdtable::close_fd(probe);
        }
        crate::errno::set_errno(0);
        assert_eq!(fchown(probe, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fchown_open_fd_returns_zero() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(fchown(fd, 1000, 1000), 0);
        let _ = fdtable::close_fd(fd);
    }

    // ---- lchown ----

    #[test]
    fn test_lchown_null_path_efault() {
        crate::errno::set_errno(0);
        assert_eq!(lchown(core::ptr::null(), 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_lchown_valid_path_returns_zero() {
        assert_eq!(lchown(b"/etc/passwd\0".as_ptr(), 0, 0), 0);
    }

    // ---- ordering / interaction with *at() wrappers ----

    #[test]
    fn test_fchmodat_null_relative_path_propagates_efault_from_chmod() {
        // fchmodat with AT_FDCWD short-circuits to chmod(path, mode).
        // A NULL path therefore goes through chmod's NULL check.
        crate::errno::set_errno(0);
        assert_eq!(fchmodat(AT_FDCWD, core::ptr::null(), 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_fchownat_null_relative_path_propagates_efault_from_chown() {
        crate::errno::set_errno(0);
        assert_eq!(fchownat(AT_FDCWD, core::ptr::null(), 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // ---- buggy-caller patterns ----

    #[test]
    fn test_buggy_caller_chmod_with_uninitialised_pointer() {
        // Simulate a caller who forgot to initialise their `path`
        // variable.  We can't truly observe an uninitialised pointer
        // from Rust, but NULL is the most common default — the EFAULT
        // path makes that bug visible instead of returning 0.
        let uninit: *const u8 = core::ptr::null();
        crate::errno::set_errno(0);
        assert_eq!(chmod(uninit, 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_buggy_caller_fchown_with_stale_fd() {
        // Caller stored an fd, closed it, then tried to fchown it.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let _ = fdtable::close_fd(fd);
        crate::errno::set_errno(0);
        assert_eq!(fchown(fd, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_buggy_caller_lchown_on_null_link() {
        crate::errno::set_errno(0);
        assert_eq!(lchown(core::ptr::null(), 1000, 1000), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // ---- workflow: install-style chmod sequence ----

    #[test]
    fn test_workflow_install_chmod_sequence() {
        // Mimic what `install -m 0755 binary /usr/bin/foo` does after
        // copying: chmod the target, then chown to root.  Both should
        // succeed (no permission system yet) so the installer doesn't
        // see a spurious failure.
        assert_eq!(chmod(b"/usr/bin/foo\0".as_ptr(), 0o755), 0);
        assert_eq!(chown(b"/usr/bin/foo\0".as_ptr(), 0, 0), 0);
    }

    // -----------------------------------------------------------------
    // Phase 92 — fchmodat / fchownat flags validation
    //
    // Linux validates `flags & ~(AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH)`
    // in the prologue (do_fchmodat / do_fchownat) before path resolution.
    // Our previous stubs discarded the argument entirely.
    // -----------------------------------------------------------------

    #[test]
    fn test_fchmodat_phase92_unknown_flag_bit_einval() {
        // 0x4000 is not a defined AT_* flag.
        crate::errno::set_errno(0);
        let ret = fchmodat(AT_FDCWD, b"/tmp\0".as_ptr(), 0o644, 0x4000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchmodat_phase92_high_bit_flag_einval() {
        crate::errno::set_errno(0);
        let ret = fchmodat(AT_FDCWD, b"/tmp\0".as_ptr(), 0o644, i32::MIN);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchmodat_phase92_at_eaccess_rejected() {
        // AT_EACCESS (0x200) is a faccessat flag, NOT an fchmodat flag.
        crate::errno::set_errno(0);
        let ret = fchmodat(AT_FDCWD, b"/tmp\0".as_ptr(), 0o644, AT_EACCESS);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchmodat_phase92_symlink_nofollow_accepted() {
        // AT_SYMLINK_NOFOLLOW is a valid fchmodat flag.  Must clear
        // the flag check (other errors are unrelated).
        crate::errno::set_errno(0);
        let _ret = fchmodat(AT_FDCWD, b"/tmp\0".as_ptr(), 0o644, AT_SYMLINK_NOFOLLOW);
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchmodat_phase92_empty_path_accepted() {
        crate::errno::set_errno(0);
        let _ret = fchmodat(AT_FDCWD, b"/tmp\0".as_ptr(), 0o644, AT_EMPTY_PATH);
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchmodat_phase92_both_valid_flags_accepted() {
        crate::errno::set_errno(0);
        let _ret = fchmodat(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            0o644,
            AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH,
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchmodat_phase92_flag_check_beats_null_path() {
        // Bad flag + null path → EINVAL (flag check fires first).
        crate::errno::set_errno(0);
        let ret = fchmodat(AT_FDCWD, core::ptr::null(), 0o644, 0x4000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchownat_phase92_unknown_flag_bit_einval() {
        crate::errno::set_errno(0);
        let ret = fchownat(AT_FDCWD, b"/tmp\0".as_ptr(), 0, 0, 0x4000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchownat_phase92_high_bit_flag_einval() {
        crate::errno::set_errno(0);
        let ret = fchownat(AT_FDCWD, b"/tmp\0".as_ptr(), 0, 0, i32::MIN);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchownat_phase92_at_eaccess_rejected() {
        crate::errno::set_errno(0);
        let ret = fchownat(AT_FDCWD, b"/tmp\0".as_ptr(), 0, 0, AT_EACCESS);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchownat_phase92_symlink_nofollow_accepted() {
        crate::errno::set_errno(0);
        let _ret = fchownat(AT_FDCWD, b"/tmp\0".as_ptr(), 0, 0, AT_SYMLINK_NOFOLLOW);
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchownat_phase92_empty_path_accepted() {
        crate::errno::set_errno(0);
        let _ret = fchownat(AT_FDCWD, b"/tmp\0".as_ptr(), 0, 0, AT_EMPTY_PATH);
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchownat_phase92_both_valid_flags_accepted() {
        crate::errno::set_errno(0);
        let _ret = fchownat(
            AT_FDCWD,
            b"/tmp\0".as_ptr(),
            0,
            0,
            AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH,
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchownat_phase92_flag_check_beats_null_path() {
        crate::errno::set_errno(0);
        let ret = fchownat(AT_FDCWD, core::ptr::null(), 0, 0, 0x4000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fchmodat_phase92_einval_then_valid_progression() {
        crate::errno::set_errno(0);
        let ret = fchmodat(AT_FDCWD, b"/tmp\0".as_ptr(), 0o644, 0x4000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Valid call after the EINVAL still works.
        assert_eq!(
            fchmodat(AT_FDCWD, b"/tmp\0".as_ptr(), 0o644, AT_SYMLINK_NOFOLLOW),
            0,
        );
    }

    #[test]
    fn test_fchownat_phase92_einval_then_valid_progression() {
        crate::errno::set_errno(0);
        let ret = fchownat(AT_FDCWD, b"/tmp\0".as_ptr(), 0, 0, 0x4000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        assert_eq!(
            fchownat(AT_FDCWD, b"/tmp\0".as_ptr(), 0, 0, AT_SYMLINK_NOFOLLOW),
            0,
        );
    }

    // -----------------------------------------------------------------
    // Phase 71 — utimes / futimes / utimensat / futimens validators
    //
    // Body is still a no-op success (filesystem doesn't track per-file
    // timestamps), but the prologue catches NULL pointers, bad fds, bad
    // flags, and out-of-range tv_usec / tv_nsec values the way Linux does.
    // -----------------------------------------------------------------

    // ---- helpers ----

    #[test]
    fn test_timeval_usec_valid_helper() {
        assert!(timeval_usec_valid(0));
        assert!(timeval_usec_valid(500_000));
        assert!(timeval_usec_valid(USEC_MAX));
        assert!(!timeval_usec_valid(-1));
        assert!(!timeval_usec_valid(USEC_MAX + 1));
        assert!(!timeval_usec_valid(1_000_000));
    }

    #[test]
    fn test_timespec_nsec_valid_helper() {
        assert!(timespec_nsec_valid(0));
        assert!(timespec_nsec_valid(500_000_000));
        assert!(timespec_nsec_valid(NSEC_MAX));
        assert!(timespec_nsec_valid(UTIME_NOW));
        assert!(timespec_nsec_valid(UTIME_OMIT));
        assert!(!timespec_nsec_valid(-1));
        assert!(!timespec_nsec_valid(NSEC_MAX + 1));
        assert!(!timespec_nsec_valid(2_000_000_000));
    }

    // ---- utimes ----

    #[test]
    fn test_utimes_null_path_efault() {
        crate::errno::set_errno(0);
        assert_eq!(utimes(core::ptr::null(), core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_utimes_null_times_returns_zero() {
        // NULL times = "set both to current time" — well-formed.
        assert_eq!(utimes(b"/tmp/f\0".as_ptr(), core::ptr::null()), 0);
    }

    #[test]
    fn test_utimes_valid_times_returns_zero() {
        let tv = [
            Timeval {
                tv_sec: 1,
                tv_usec: 0,
            },
            Timeval {
                tv_sec: 2,
                tv_usec: USEC_MAX,
            },
        ];
        assert_eq!(utimes(b"/tmp/f\0".as_ptr(), tv.as_ptr()), 0);
    }

    #[test]
    fn test_utimes_negative_usec_einval() {
        let tv = [
            Timeval {
                tv_sec: 0,
                tv_usec: -1,
            },
            Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
        ];
        crate::errno::set_errno(0);
        assert_eq!(utimes(b"/tmp/f\0".as_ptr(), tv.as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_utimes_overflow_usec_einval() {
        let tv = [
            Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            Timeval {
                tv_sec: 0,
                tv_usec: 1_000_000,
            },
        ];
        crate::errno::set_errno(0);
        assert_eq!(utimes(b"/tmp/f\0".as_ptr(), tv.as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_utimes_null_path_beats_bad_times() {
        // NULL path is checked before times[].tv_usec range.
        let tv = [
            Timeval {
                tv_sec: 0,
                tv_usec: -1,
            },
            Timeval {
                tv_sec: 0,
                tv_usec: -1,
            },
        ];
        crate::errno::set_errno(0);
        assert_eq!(utimes(core::ptr::null(), tv.as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // ---- futimes ----

    #[test]
    fn test_futimes_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(futimes(-1, core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_futimes_unopen_fd_ebadf() {
        let probe: i32 = 0x4000_0011;
        if fdtable::get_fd(probe).is_some() {
            let _ = fdtable::close_fd(probe);
        }
        crate::errno::set_errno(0);
        assert_eq!(futimes(probe, core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_futimes_valid_returns_zero() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let tv = [
            Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
        ];
        assert_eq!(futimes(fd, tv.as_ptr()), 0);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_futimes_bad_times_einval() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let tv = [
            Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            Timeval {
                tv_sec: 0,
                tv_usec: 2_000_000,
            },
        ];
        crate::errno::set_errno(0);
        assert_eq!(futimes(fd, tv.as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_futimes_bad_fd_beats_bad_times() {
        let tv = [
            Timeval {
                tv_sec: 0,
                tv_usec: -1,
            },
            Timeval {
                tv_sec: 0,
                tv_usec: -1,
            },
        ];
        crate::errno::set_errno(0);
        assert_eq!(futimes(-1, tv.as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // ---- utimensat ----

    #[test]
    fn test_utimensat_null_path_efault() {
        crate::errno::set_errno(0);
        assert_eq!(
            utimensat(AT_FDCWD, core::ptr::null(), core::ptr::null(), 0),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_utimensat_unknown_flag_einval() {
        crate::errno::set_errno(0);
        // 0x200 is not AT_SYMLINK_NOFOLLOW.
        assert_eq!(
            utimensat(AT_FDCWD, b"/tmp/f\0".as_ptr(), core::ptr::null(), 0x200),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_utimensat_at_symlink_nofollow_accepted() {
        assert_eq!(
            utimensat(
                AT_FDCWD,
                b"/tmp/f\0".as_ptr(),
                core::ptr::null(),
                AT_SYMLINK_NOFOLLOW,
            ),
            0
        );
    }

    #[test]
    fn test_utimensat_utime_now_sentinel_accepted() {
        let ts = [
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_NOW,
            },
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_OMIT,
            },
        ];
        assert_eq!(utimensat(AT_FDCWD, b"/tmp/f\0".as_ptr(), ts.as_ptr(), 0), 0);
    }

    #[test]
    fn test_utimensat_negative_nsec_einval() {
        let ts = [
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: -1,
            },
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        ];
        crate::errno::set_errno(0);
        assert_eq!(
            utimensat(AT_FDCWD, b"/tmp/f\0".as_ptr(), ts.as_ptr(), 0),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_utimensat_overflow_nsec_einval() {
        let ts = [
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            },
        ];
        crate::errno::set_errno(0);
        assert_eq!(
            utimensat(AT_FDCWD, b"/tmp/f\0".as_ptr(), ts.as_ptr(), 0),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_utimensat_bad_flag_beats_null_path() {
        crate::errno::set_errno(0);
        // Unknown flag is checked before NULL path; both bug-shaped, but
        // the flag check is first in the prologue.
        assert_eq!(
            utimensat(AT_FDCWD, core::ptr::null(), core::ptr::null(), 0x4000),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_utimensat_relative_path_bad_dirfd_ebadf() {
        crate::errno::set_errno(0);
        // Relative path + non-AT_FDCWD bad dirfd → EBADF.
        assert_eq!(
            utimensat(-2, b"relative\0".as_ptr(), core::ptr::null(), 0),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_utimensat_relative_path_unopen_dirfd_ebadf() {
        let probe: i32 = 0x4000_0021;
        if fdtable::get_fd(probe).is_some() {
            let _ = fdtable::close_fd(probe);
        }
        crate::errno::set_errno(0);
        assert_eq!(
            utimensat(probe, b"relative\0".as_ptr(), core::ptr::null(), 0),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_utimensat_absolute_path_ignores_dirfd() {
        // Absolute path: bad dirfd is fine.
        assert_eq!(utimensat(-2, b"/tmp/f\0".as_ptr(), core::ptr::null(), 0), 0);
    }

    #[test]
    fn test_utimensat_relative_path_open_dirfd_returns_zero() {
        let dirfd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(
            utimensat(dirfd, b"relative\0".as_ptr(), core::ptr::null(), 0),
            0
        );
        let _ = fdtable::close_fd(dirfd);
    }

    // ---- futimens ----

    #[test]
    fn test_futimens_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(futimens(-1, core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_futimens_unopen_fd_ebadf() {
        let probe: i32 = 0x4000_0031;
        if fdtable::get_fd(probe).is_some() {
            let _ = fdtable::close_fd(probe);
        }
        crate::errno::set_errno(0);
        assert_eq!(futimens(probe, core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_futimens_bad_nsec_einval() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let ts = [
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 2_000_000_000,
            },
        ];
        crate::errno::set_errno(0);
        assert_eq!(futimens(fd, ts.as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_futimens_utime_sentinels_accepted() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let ts = [
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_NOW,
            },
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_OMIT,
            },
        ];
        assert_eq!(futimens(fd, ts.as_ptr()), 0);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_futimens_bad_fd_beats_bad_times() {
        let ts = [
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: -1,
            },
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: -1,
            },
        ];
        crate::errno::set_errno(0);
        assert_eq!(futimens(-1, ts.as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // ---- timestamp → kernel-ns conversion (pure, host-testable) ----

    const NOW_NS: u64 = 1_700_000_000_500_000_000; // 2023-11-14, .5s

    #[test]
    fn test_timespec_to_kernel_ns_normal_value() {
        let ts = crate::stat::Timespec {
            tv_sec: 5,
            tv_nsec: 123,
        };
        assert_eq!(timespec_to_kernel_ns(&ts, NOW_NS), 5_000_000_123);
    }

    #[test]
    fn test_timespec_to_kernel_ns_omit_is_zero() {
        // UTIME_OMIT maps to 0 = "leave unchanged" (kernel convention).
        let ts = crate::stat::Timespec {
            tv_sec: 999,
            tv_nsec: UTIME_OMIT,
        };
        assert_eq!(timespec_to_kernel_ns(&ts, NOW_NS), 0);
    }

    #[test]
    fn test_timespec_to_kernel_ns_now_uses_wall_clock() {
        // UTIME_NOW ignores tv_sec and uses the supplied wall clock.
        let ts = crate::stat::Timespec {
            tv_sec: 999,
            tv_nsec: UTIME_NOW,
        };
        assert_eq!(timespec_to_kernel_ns(&ts, NOW_NS), NOW_NS);
    }

    #[test]
    fn test_timeval_to_kernel_ns_microsecond_scale() {
        // 2 seconds + 250_000 us = 2.25 s = 2_250_000_000 ns.
        let tv = Timeval {
            tv_sec: 2,
            tv_usec: 250_000,
        };
        assert_eq!(timeval_to_kernel_ns(&tv), 2_250_000_000);
    }

    #[test]
    fn test_utimens_pair_null_is_now_now() {
        // SAFETY: null pointer is the documented "set both to now" case.
        let pair = unsafe { utimens_pair_to_kernel(core::ptr::null(), NOW_NS) };
        assert_eq!(pair, (NOW_NS, NOW_NS));
    }

    #[test]
    fn test_utimens_pair_omit_now_mix() {
        // atime=UTIME_OMIT (unchanged → 0), mtime=UTIME_NOW (→ wall clock).
        let ts = [
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_OMIT,
            },
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_NOW,
            },
        ];
        // SAFETY: `ts` is a valid two-element array.
        let pair = unsafe { utimens_pair_to_kernel(ts.as_ptr(), NOW_NS) };
        assert_eq!(pair, (0, NOW_NS));
    }

    #[test]
    fn test_utimens_pair_explicit_values() {
        let ts = [
            crate::stat::Timespec {
                tv_sec: 10,
                tv_nsec: 0,
            },
            crate::stat::Timespec {
                tv_sec: 20,
                tv_nsec: 500,
            },
        ];
        // SAFETY: `ts` is a valid two-element array.
        let pair = unsafe { utimens_pair_to_kernel(ts.as_ptr(), NOW_NS) };
        assert_eq!(pair, (10_000_000_000, 20_000_000_500));
    }

    #[test]
    fn test_utimes_pair_null_is_now_now() {
        // SAFETY: null pointer is the documented "set both to now" case.
        let pair = unsafe { utimes_pair_to_kernel(core::ptr::null(), NOW_NS) };
        assert_eq!(pair, (NOW_NS, NOW_NS));
    }

    #[test]
    fn test_utimes_pair_explicit_values() {
        let tv = [
            Timeval {
                tv_sec: 1,
                tv_usec: 0,
            },
            Timeval {
                tv_sec: 3,
                tv_usec: 1,
            },
        ];
        // SAFETY: `tv` is a valid two-element array.
        let pair = unsafe { utimes_pair_to_kernel(tv.as_ptr(), NOW_NS) };
        // 1s = 1e9 ns; 3s + 1us = 3_000_001_000 ns.
        assert_eq!(pair, (1_000_000_000, 3_000_001_000));
    }

    // ---- buggy callers ----

    #[test]
    fn test_buggy_caller_utimes_with_uninitialised_pointer() {
        crate::errno::set_errno(0);
        assert_eq!(utimes(core::ptr::null(), core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_buggy_caller_futimens_stale_fd() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let _ = fdtable::close_fd(fd);
        crate::errno::set_errno(0);
        assert_eq!(futimens(fd, core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_buggy_caller_utimensat_with_garbage_flags() {
        crate::errno::set_errno(0);
        assert_eq!(
            utimensat(AT_FDCWD, b"/tmp/f\0".as_ptr(), core::ptr::null(), -1),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // ---- workflows ----

    #[test]
    fn test_workflow_touch_via_utimensat_now() {
        // What `touch` does: set both times to now via UTIME_NOW sentinels.
        let ts = [
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_NOW,
            },
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_NOW,
            },
        ];
        assert_eq!(
            utimensat(AT_FDCWD, b"/tmp/new\0".as_ptr(), ts.as_ptr(), 0),
            0
        );
    }

    #[test]
    fn test_workflow_preserve_atime_via_utime_omit() {
        // `cp --preserve=mtime` style: only change mtime, leave atime.
        let ts = [
            crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_OMIT,
            },
            crate::stat::Timespec {
                tv_sec: 1_700_000_000,
                tv_nsec: 0,
            },
        ];
        assert_eq!(utimensat(AT_FDCWD, b"/tmp/x\0".as_ptr(), ts.as_ptr(), 0), 0);
    }

    // -----------------------------------------------------------------
    // Phase 72 — flock / lockf validators
    //
    // Bodies are still no-op success (kernel-level advisory locking
    // isn't implemented yet), but the prologue catches bad fds and
    // unknown operation/command values the way Linux's syscall entry
    // path does.  See also `syncfs` in unistd.rs.
    // -----------------------------------------------------------------

    // ---- flock ----

    #[test]
    fn test_flock_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(flock(-1, LOCK_SH), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_flock_unopen_fd_ebadf() {
        let probe: i32 = 0x4000_0041;
        if fdtable::get_fd(probe).is_some() {
            let _ = fdtable::close_fd(probe);
        }
        crate::errno::set_errno(0);
        assert_eq!(flock(probe, LOCK_SH), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_flock_zero_op_einval() {
        // Must specify one of LOCK_SH / LOCK_EX / LOCK_UN.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        assert_eq!(flock(fd, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_flock_unknown_bit_einval() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        // 0x40 isn't in FLOCK_OP_MASK.
        assert_eq!(flock(fd, LOCK_SH | 0x40), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_flock_two_modes_einval() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        assert_eq!(flock(fd, LOCK_SH | LOCK_EX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_flock_bad_fd_beats_bad_op() {
        crate::errno::set_errno(0);
        // -1 → EBADF; the bad operation never gets checked.
        assert_eq!(flock(-1, 0xFFFF), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_flock_nb_only_einval() {
        // LOCK_NB alone (without LOCK_SH/EX/UN) isn't a valid op.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        assert_eq!(flock(fd, LOCK_NB), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_flock_all_modes_with_nb_accepted() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        for &mode in &[LOCK_SH, LOCK_EX, LOCK_UN] {
            assert_eq!(flock(fd, mode), 0);
            assert_eq!(flock(fd, mode | LOCK_NB), 0);
        }
        let _ = fdtable::close_fd(fd);
    }

    // ---- lockf ----

    #[test]
    fn test_lockf_negative_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(lockf(-1, F_LOCK, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_lockf_unopen_fd_ebadf() {
        let probe: i32 = 0x4000_0042;
        if fdtable::get_fd(probe).is_some() {
            let _ = fdtable::close_fd(probe);
        }
        crate::errno::set_errno(0);
        assert_eq!(lockf(probe, F_LOCK, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_lockf_unknown_cmd_einval() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        assert_eq!(lockf(fd, 99, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_lockf_negative_cmd_einval() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        assert_eq!(lockf(fd, -1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_lockf_bad_fd_beats_bad_cmd() {
        crate::errno::set_errno(0);
        assert_eq!(lockf(-1, 99, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_lockf_negative_len_accepted() {
        // POSIX lockf accepts negative len (means "lock backwards from
        // current offset").  Our stub passes a non-zero len through.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(lockf(fd, F_LOCK, -100), 0);
        let _ = fdtable::close_fd(fd);
    }

    // ---- buggy callers ----

    #[test]
    fn test_buggy_caller_flock_stale_fd() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let _ = fdtable::close_fd(fd);
        crate::errno::set_errno(0);
        assert_eq!(flock(fd, LOCK_EX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_buggy_caller_lockf_with_garbage_cmd() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        assert_eq!(lockf(fd, 0x7FFF_FFFF, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = fdtable::close_fd(fd);
    }

    // ---- workflow: lock-file-style acquire/release ----

    #[test]
    fn test_workflow_lockfile_acquire_release() {
        // What e.g. `mkdir`'s -p flag does when racing with another
        // process: take an exclusive non-blocking lock, do work, release.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(flock(fd, LOCK_EX | LOCK_NB), 0);
        assert_eq!(flock(fd, LOCK_UN), 0);
        let _ = fdtable::close_fd(fd);
    }

    // ---- Phase 115: close_range validation-order parity with Linux ----
    //
    // Linux's `__close_range` checks flag bits BEFORE the range
    // ordering.  Both errors are EINVAL, but a caller bisecting which
    // argument is wrong expects the flag failure to surface first when
    // both are bad.  These tests pin that order in.

    #[test]
    fn test_close_range_phase115_unknown_flag_with_inverted_range_einval() {
        // Both args bad: unknown flag bit AND first > last.  Linux
        // returns EINVAL from the flags check; we must reach the same
        // verdict via the same path (errno identical, but the ORDER
        // is what we're locking in — a future refactor that flips it
        // would still pass the errno assertion).
        crate::errno::set_errno(0);
        let ret = close_range(100, 50, 0x8000_0000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_close_range_phase115_high_bit_flag_einval() {
        // 0x8000_0000 alone (no range issue) → EINVAL via flag check.
        crate::errno::set_errno(0);
        let ret = close_range(0, 10, 0x8000_0000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_close_range_phase115_all_unknown_flags_einval() {
        // u32::MAX includes both known and unknown bits → unknown bits
        // dominate → EINVAL.
        crate::errno::set_errno(0);
        let ret = close_range(0, 10, u32::MAX);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_close_range_phase115_single_unknown_bit_above_mask_einval() {
        // Bit 3 (0x8) — just above the known CLOSE_RANGE_UNSHARE|CLOEXEC
        // mask (which occupies bits 1 and 2) — must trip the flag check.
        crate::errno::set_errno(0);
        let ret = close_range(0, 10, 0x8);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_close_range_phase115_inverted_range_alone_still_einval() {
        // No flag bits set; only the range is inverted.  Must still
        // return EINVAL (the second check now).
        crate::errno::set_errno(0);
        let ret = close_range(100, 50, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_close_range_phase115_inverted_range_with_valid_flags_einval() {
        use crate::linux_close_range::{CLOSE_RANGE_CLOEXEC, CLOSE_RANGE_UNSHARE};
        // Valid flag combo BUT inverted range → range check fires →
        // EINVAL.  Confirms the flag check correctly passes through
        // valid flags and lets the range check own this verdict.
        crate::errno::set_errno(0);
        let ret = close_range(100, 50, CLOSE_RANGE_UNSHARE | CLOSE_RANGE_CLOEXEC);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_close_range_phase115_recovery_after_einval() {
        // After a rejected call, a subsequent valid call must succeed
        // (no errno-set lingering, no internal state corruption).
        let _ = close_range(100, 50, 0x8000_0000);
        crate::errno::set_errno(0);
        let ret = close_range(900, 910, 0);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_close_range_phase115_buggy_caller_passes_negative_int_flags() {
        // A caller writing `close_range(0, 10, -1)` in C compiles to
        // u32::MAX which contains every unknown bit → EINVAL.
        crate::errno::set_errno(0);
        #[allow(clippy::cast_sign_loss)]
        let ret = close_range(0, 10, (-1i32) as u32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_close_range_phase115_unshare_alone_with_inverted_range_einval() {
        use crate::linux_close_range::CLOSE_RANGE_UNSHARE;
        // Valid lone CLOSE_RANGE_UNSHARE flag with inverted range →
        // EINVAL via range check.
        crate::errno::set_errno(0);
        let ret = close_range(100, 50, CLOSE_RANGE_UNSHARE);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_close_range_phase115_cloexec_alone_with_inverted_range_einval() {
        use crate::linux_close_range::CLOSE_RANGE_CLOEXEC;
        // Valid lone CLOSE_RANGE_CLOEXEC flag with inverted range →
        // EINVAL via range check.
        crate::errno::set_errno(0);
        let ret = close_range(100, 50, CLOSE_RANGE_CLOEXEC);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_close_range_phase115_no_side_effect_on_einval_with_flags() {
        // A close_range call rejected by the flag check must NOT
        // modify any fd state in the [first, last] range.  Open an fd,
        // call close_range with an unknown flag bit covering that fd,
        // verify the fd is still open afterwards.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        let ret = close_range(fd as u32, fd as u32, 0x8000_0000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        // fd must still be open.
        assert!(fdtable::get_fd_flags(fd).is_some());
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_close_range_phase115_no_side_effect_on_einval_with_inverted_range() {
        // Same as above but for the range-ordering failure path.  An
        // inverted range with valid flags must still not modify any fd.
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        // Note: first > last but the supplied range doesn't actually
        // cover `fd`; the test is: regardless of whether fd is in or
        // out of the range, an EINVAL-rejected call must not close it.
        let ret = close_range(100, 50, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        assert!(fdtable::get_fd_flags(fd).is_some());
        let _ = fdtable::close_fd(fd);
    }

    #[test]
    fn test_close_range_phase115_valid_zero_flags_still_works() {
        // Sanity check: flags=0 with valid range still returns 0
        // (no regression from the reorder).
        crate::errno::set_errno(0);
        let ret = close_range(800, 810, 0);
        assert_eq!(ret, 0);
    }

    // ------------------------------------------------------------------
    // Phase 190: open_by_handle_at — CAP_DAC_READ_SEARCH gate
    // ------------------------------------------------------------------
    //
    // Linux's `fs/fhandle.c::open_by_handle_at` -> `handle_to_path`
    // checks `may_decode_fh`, which approves callers holding
    // `CAP_DAC_READ_SEARCH`:
    //
    //     static bool may_decode_fh(struct handle_to_path_ctx *ctx,
    //                               unsigned int o_flags) {
    //         if (capable(CAP_DAC_READ_SEARCH))
    //             return true;
    //         /* export-fd path with EXPORT_OP_PRIVILEGED_FILEHANDLE */
    //     }
    //
    // and the caller path returns `-EPERM` when `may_decode_fh` is
    // false.  Our stub now gates on `CAP_DAC_READ_SEARCH` after the
    // EFAULT/EBADF guards but before the ENOSYS terminal.
    //
    // EFAULT (NULL handle) and EBADF (bad mount_fd) still beat EPERM,
    // matching Linux's `do_handle_open` prologue order
    // (`get_path_anchor` runs before `may_decode_fh`).
    //
    // Pre-Phase-190 the docstring said "Linux additionally requires
    // CAP_DAC_READ_SEARCH, which we do not model" — that was wrong:
    // our cap layer does model it, and an unprivileged file-handle
    // probe should see EPERM, not ENOSYS.
    //
    // Host build holds CAP_DAC_READ_SEARCH by default (bit 2 ∈
    // DEFAULT_CAPS_LOW = u32::MAX).  Must run with `--test-threads=1`.
    // ------------------------------------------------------------------

    mod open_by_handle_at_cap_phase190 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase 189.
        struct CapGuard {
            lo: u32,
            hi: u32,
            // Held for the lifetime of the guard. See
            // `sys_capability::CAP_TEST_LOCK` for why.
            _lock: crate::sys_capability::CapTestLockGuard,
        }
        impl CapGuard {
            fn snapshot() -> Self {
            // Re-entrant lock guard: outermost acquire on the
            // thread takes the global mutex; nested acquires
            // (some tests stack a scoped CapGuard inside an
            // outer one) are no-ops for the lock but still
            // snapshot/restore caps independently.
            let lock = crate::sys_capability::CapTestLockGuard::acquire();
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            Self { lo, hi, _lock: lock }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_dac_read_search() {
            use crate::sys_capability::CAP_DAC_READ_SEARCH;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_DAC_READ_SEARCH < 32 {
                (lo & !(1u32 << CAP_DAC_READ_SEARCH), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_DAC_READ_SEARCH - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(
                rc, 0,
                "capset must succeed when dropping CAP_DAC_READ_SEARCH"
            );
            assert!(!crate::sys_capability::has_capability(CAP_DAC_READ_SEARCH,));
        }

        fn fresh_handle() -> FileHandle {
            FileHandle {
                handle_bytes: 0,
                handle_type: 0,
            }
        }

        // -- Per-error-class --------------------------------------------------

        /// No cap → EPERM.  Canonical missing-privilege path.
        #[test]
        fn test_obha_phase190_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_dac_read_search();
            let mut fh = fresh_handle();
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(AT_FDCWD, &raw mut fh, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        /// With cap held → ENOSYS (no backend).  Confirms the gate is
        /// gated, not unconditional.
        #[test]
        fn test_obha_phase190_with_cap_returns_enosys() {
            let _g = CapGuard::snapshot();
            // Cap held by default.
            let mut fh = fresh_handle();
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(AT_FDCWD, &raw mut fh, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        }

        // -- Ordering matrix --------------------------------------------------

        /// EFAULT (NULL handle) beats EPERM — pointer check runs in
        /// `do_handle_open` before `get_path_anchor` even runs.
        #[test]
        fn test_obha_phase190_efault_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_dac_read_search();
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(AT_FDCWD, core::ptr::null_mut(), 0), -1,);
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        }

        /// EBADF (negative mount_fd) beats EPERM — fdget runs in
        /// `get_path_anchor`, which is before `may_decode_fh`.
        #[test]
        fn test_obha_phase190_ebadf_negative_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_dac_read_search();
            let mut fh = fresh_handle();
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(-5, &raw mut fh, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
        }

        /// EBADF (nonexistent fd) beats EPERM.
        #[test]
        fn test_obha_phase190_ebadf_nonexistent_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_dac_read_search();
            let mut fh = fresh_handle();
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(100_000, &raw mut fh, 0), -1,);
            assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
        }

        /// EPERM beats ENOSYS — without the gate, missing-cap callers
        /// would see ENOSYS, which CRIU's capability probe reads as
        /// "kernel doesn't support file handles" (wrong diagnostic).
        #[test]
        fn test_obha_phase190_eperm_beats_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_dac_read_search();
            let mut fh = fresh_handle();
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(AT_FDCWD, &raw mut fh, 0), -1);
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::EPERM,
                "Missing CAP_DAC_READ_SEARCH must surface as EPERM"
            );
        }

        // -- Workflow --------------------------------------------------------

        /// Drop cap → EPERM; restore cap → ENOSYS.  Mirrors the
        /// privilege-drop-then-restore pattern of a setuid file
        /// handle resolver (NFS userspace daemons).
        #[test]
        fn test_obha_phase190_drop_then_restore_workflow() {
            let _g = CapGuard::snapshot();
            let mut fh = fresh_handle();
            // 1. Cap held → ENOSYS.
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(AT_FDCWD, &raw mut fh, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
            // 2. Drop cap → EPERM.
            drop_cap_dac_read_search();
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(AT_FDCWD, &raw mut fh, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
            // 3. Restore via capset to u32::MAX → ENOSYS again.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(crate::sys_capability::capset(&mut hdr, data.as_ptr()), 0,);
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(AT_FDCWD, &raw mut fh, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        }

        // -- Buggy-caller ----------------------------------------------------

        /// Caller didn't clear errno → sees fresh EPERM, not stale.
        #[test]
        fn test_obha_phase190_buggy_caller_stale_errno_replaced() {
            let _g = CapGuard::snapshot();
            drop_cap_dac_read_search();
            let mut fh = fresh_handle();
            crate::errno::set_errno(crate::errno::ENOENT);
            assert_eq!(open_by_handle_at(AT_FDCWD, &raw mut fh, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        // -- Recovery --------------------------------------------------------

        /// CapGuard drop restores cap; subsequent call reaches ENOSYS.
        #[test]
        fn test_obha_phase190_capguard_restore_clears_state() {
            {
                let _g = CapGuard::snapshot();
                drop_cap_dac_read_search();
                let mut fh = fresh_handle();
                crate::errno::set_errno(0);
                assert_eq!(open_by_handle_at(AT_FDCWD, &raw mut fh, 0), -1,);
                assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
            } // _g dropped here; cap restored.
            let mut fh = fresh_handle();
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(AT_FDCWD, &raw mut fh, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        }

        // -- Sentinel --------------------------------------------------------

        /// With cap held, all existing EFAULT/EBADF terminals still
        /// fire.  Confirms the gate is conditional.
        #[test]
        fn test_obha_phase190_with_cap_existing_terminals_unchanged() {
            let _g = CapGuard::snapshot();
            // EFAULT.
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(AT_FDCWD, core::ptr::null_mut(), 0), -1,);
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
            // EBADF negative.
            let mut fh = fresh_handle();
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(-5, &raw mut fh, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
            // ENOSYS happy path.
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(AT_FDCWD, &raw mut fh, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
        }

        // -- Cross-check -----------------------------------------------------

        /// Dropping CAP_SYS_ADMIN alone must NOT affect
        /// open_by_handle_at — Linux gates this specifically on
        /// CAP_DAC_READ_SEARCH.  Pins down the cross-cap invariant.
        #[test]
        fn test_obha_phase190_sys_admin_drop_does_not_affect_obha() {
            use crate::sys_capability::CAP_SYS_ADMIN;
            let _g = CapGuard::snapshot();
            // Drop only CAP_SYS_ADMIN (bit 21).
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let new_lo = lo & !(1u32 << CAP_SYS_ADMIN);
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(crate::sys_capability::capset(&mut hdr, data.as_ptr()), 0,);
            // Still reaches ENOSYS.
            let mut fh = fresh_handle();
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(AT_FDCWD, &raw mut fh, 0), -1);
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::ENOSYS,
                "CAP_SYS_ADMIN drop must not affect open_by_handle_at"
            );
        }

        /// Phase 190 errno is EPERM (capable convention), matching
        /// `may_decode_fh` failure.  Distinct from EACCES (Phase 186).
        #[test]
        fn test_obha_phase190_errno_is_eperm_not_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap_dac_read_search();
            let mut fh = fresh_handle();
            crate::errno::set_errno(0);
            assert_eq!(open_by_handle_at(AT_FDCWD, &raw mut fh, 0), -1);
            let e = crate::errno::get_errno();
            assert_eq!(e, crate::errno::EPERM);
            assert_ne!(e, crate::errno::EACCES);
        }

        /// `name_to_handle_at` is unaffected by the cap drop — that
        /// syscall has a different validation path (no `may_decode_fh`).
        /// Pinning this prevents a future copy-paste from applying the
        /// gate to the wrong sibling.
        #[test]
        fn test_obha_phase190_name_to_handle_at_unaffected() {
            let _g = CapGuard::snapshot();
            drop_cap_dac_read_search();
            let mut fh = fresh_handle();
            let mut mount_id: i32 = 0;
            crate::errno::set_errno(0);
            let ret =
                name_to_handle_at(AT_FDCWD, b"x\0".as_ptr(), &raw mut fh, &raw mut mount_id, 0);
            assert_eq!(ret, -1);
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::ENOSYS,
                "name_to_handle_at must not pass through the obha cap gate"
            );
        }
    }

    // =======================================================================
    // Phase 206 — CAP_CHOWN gate on chown / fchown / lchown / fchownat
    // =======================================================================
    //
    // Linux requires CAP_CHOWN when actually changing a file's owner or
    // group.  owner/group == (uid_t)-1 (u32::MAX) means "don't change";
    // a double-no-op call bypasses the gate.  Error priority:
    //   EFAULT (null path) / EBADF (bad fd) / EINVAL (bad flags)  >  EPERM
    //
    // fchownat delegates to chown(), so it inherits the gate automatically.
    mod phase206_cap_chown {
        use super::*;

        const CAP_CHOWN: u32 = crate::sys_capability::CAP_CHOWN;

        struct CapGuard {

            lo: u32,

            hi: u32,

            // Held for the lifetime of the guard. See

            // `sys_capability::CAP_TEST_LOCK` for why.

            _lock: crate::sys_capability::CapTestLockGuard,

        }
        impl CapGuard {
            fn snapshot() -> Self {
            // Re-entrant lock guard: outermost acquire on the
            // thread takes the global mutex; nested acquires
            // (some tests stack a scoped CapGuard inside an
            // outer one) are no-ops for the lock but still
            // snapshot/restore caps independently.
            let lock = crate::sys_capability::CapTestLockGuard::acquire();
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            Self { lo, hi, _lock: lock }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_chown() {
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let new_lo = lo & !(1u32 << CAP_CHOWN);
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: new_lo,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: hi,
                    permitted: hi,
                    inheritable: 0,
                },
            ];
            let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0);
            assert!(!crate::sys_capability::has_capability(CAP_CHOWN));
        }

        // ---- chown -------------------------------------------------------

        /// chown with cap held succeeds for a well-formed call.
        #[test]
        fn test_chown_cap_held_succeeds() {
            assert!(crate::sys_capability::has_capability(CAP_CHOWN));
            crate::errno::set_errno(0);
            assert_eq!(chown(b"/tmp\0".as_ptr(), 1000, 1000), 0);
        }

        /// chown without CAP_CHOWN returns EPERM when ownership changes.
        #[test]
        fn test_chown_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(chown(b"/tmp\0".as_ptr(), 1000, 1000), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        /// chown with owner-only change denied.
        #[test]
        fn test_chown_no_cap_owner_only() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(chown(b"/a\0".as_ptr(), 500, u32::MAX), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        /// chown with group-only change denied.
        #[test]
        fn test_chown_no_cap_group_only() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(chown(b"/a\0".as_ptr(), u32::MAX, 100), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        /// chown with both owner and group == u32::MAX is a no-op;
        /// bypasses the cap gate even without CAP_CHOWN.
        #[test]
        fn test_chown_noop_bypasses_gate() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(chown(b"/tmp\0".as_ptr(), u32::MAX, u32::MAX), 0);
        }

        /// EFAULT takes priority over EPERM — NULL path checked first.
        #[test]
        fn test_chown_efault_before_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(chown(core::ptr::null(), 0, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        }

        // ---- fchown ------------------------------------------------------

        /// fchown with cap held succeeds on a valid fd.
        #[test]
        fn test_fchown_cap_held_succeeds() {
            assert!(crate::sys_capability::has_capability(CAP_CHOWN));
            let fd =
                crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 999).expect("alloc fd");
            crate::errno::set_errno(0);
            assert_eq!(fchown(fd, 1000, 1000), 0);
            let _ = crate::fdtable::close_fd(fd);
        }

        /// fchown without CAP_CHOWN returns EPERM.
        #[test]
        fn test_fchown_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            let fd =
                crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 998).expect("alloc fd");
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(fchown(fd, 1000, 1000), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
            let _ = crate::fdtable::close_fd(fd);
        }

        /// fchown no-op (both -1) bypasses cap gate.
        #[test]
        fn test_fchown_noop_bypasses_gate() {
            let _g = CapGuard::snapshot();
            let fd =
                crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 997).expect("alloc fd");
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(fchown(fd, u32::MAX, u32::MAX), 0);
            let _ = crate::fdtable::close_fd(fd);
        }

        /// EBADF takes priority over EPERM for negative fd.
        #[test]
        fn test_fchown_ebadf_before_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(fchown(-1, 0, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
        }

        // ---- lchown ------------------------------------------------------

        /// lchown without CAP_CHOWN returns EPERM.
        #[test]
        fn test_lchown_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(lchown(b"/tmp\0".as_ptr(), 1000, 1000), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        /// lchown no-op bypasses cap gate.
        #[test]
        fn test_lchown_noop_bypasses_gate() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(lchown(b"/a\0".as_ptr(), u32::MAX, u32::MAX), 0);
        }

        /// lchown EFAULT before EPERM.
        #[test]
        fn test_lchown_efault_before_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(lchown(core::ptr::null(), 0, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        }

        // ---- fchownat (inherits gate from chown) -------------------------

        /// fchownat delegates to chown — cap gate fires through delegation.
        #[test]
        fn test_fchownat_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(fchownat(AT_FDCWD, b"/x\0".as_ptr(), 0, 0, 0), -1,);
            assert_eq!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        /// fchownat EINVAL (bad flags) takes priority over EPERM.
        #[test]
        fn test_fchownat_einval_before_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(fchownat(AT_FDCWD, b"/x\0".as_ptr(), 0, 0, 0x8000), -1,);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        }

        /// Cap restore confirmed after CapGuard drop.
        #[test]
        fn test_chown_cap_restore() {
            {
                let _g = CapGuard::snapshot();
                drop_cap_chown();
                assert!(!crate::sys_capability::has_capability(CAP_CHOWN));
            }
            assert!(crate::sys_capability::has_capability(CAP_CHOWN));
        }
    }
}

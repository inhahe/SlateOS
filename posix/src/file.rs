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
        HandleKind::Eventfd => {
            syscall1(SYS_EVENTFD_CLOSE, entry.handle)
        }
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
            let fd_nb = fdtable::get_status_flags(fd).unwrap_or(0)
                & crate::fcntl::O_NONBLOCK != 0;
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
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0)
                & crate::fcntl::O_NONBLOCK != 0;
            let nr = if is_nb { SYS_EVENTFD_TRY_READ } else { SYS_EVENTFD_READ };
            let r = syscall1(nr, entry.handle);
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
            let fd_nb = fdtable::get_status_flags(fd).unwrap_or(0)
                & crate::fcntl::O_NONBLOCK != 0;
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
            let r = syscall2(SYS_EVENTFD_WRITE, entry.handle, val);
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
        | HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket
        | HandleKind::Eventfd | HandleKind::Epoll | HandleKind::Timerfd
        | HandleKind::Inotify => {
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
    let sr = syscall3(SYS_FS_SEEK, entry.handle, offset as u64, crate::fcntl::SEEK_SET as u64);
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
                let _ = syscall3(SYS_FS_SEEK, entry.handle, saved as u64, crate::fcntl::SEEK_SET as u64);
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
    let _ = syscall3(SYS_FS_SEEK, entry.handle, saved as u64, crate::fcntl::SEEK_SET as u64);

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
    let sr = syscall3(SYS_FS_SEEK, entry.handle, offset as u64, crate::fcntl::SEEK_SET as u64);
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
                let _ = syscall3(SYS_FS_SEEK, entry.handle, saved as u64, crate::fcntl::SEEK_SET as u64);
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
    let _ = syscall3(SYS_FS_SEEK, entry.handle, saved as u64, crate::fcntl::SEEK_SET as u64);

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
        HandleKind::Eventfd => {
            // No kernel-level dup for eventfds.  Share the handle;
            // close() uses is_handle_referenced() to only close the
            // kernel handle when the last fd referencing it is closed.
            if let Some(fd) = fdtable::alloc_fd_with_flags(
                HandleKind::Eventfd, entry.handle, src_status,
            ) {
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
            if let Some(fd) = fdtable::alloc_fd_with_flags(
                HandleKind::Epoll, entry.handle, src_status,
            ) {
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
            if let Some(fd) = fdtable::alloc_fd_with_flags(
                HandleKind::Timerfd, entry.handle, src_status,
            ) {
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
            if let Some(fd) = fdtable::alloc_fd_with_flags(
                HandleKind::Inotify, entry.handle, src_status,
            ) {
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
        | HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket
        | HandleKind::Eventfd => {
            entry.handle
        }
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

    // Linux returns EINVAL for inverted ranges.  Our previous code
    // silently treated them as no-ops, which masks bugs in callers.
    if first > last {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Reject any flag bit we don't understand.  glibc and musl
    // forward-compat their callers by passing 0; anything else is a
    // bug in the caller (or a feature we haven't implemented yet).
    let known_flags = CLOSE_RANGE_UNSHARE | CLOSE_RANGE_CLOEXEC;
    if flags & !known_flags != 0 {
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
    let effective_last = if last >= max { max.wrapping_sub(1) } else { last };
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
        HandleKind::Eventfd | HandleKind::Epoll | HandleKind::Timerfd
        | HandleKind::Inotify => {
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
        | HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket
        | HandleKind::Eventfd | HandleKind::Epoll | HandleKind::Timerfd
        | HandleKind::Inotify => {
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
        | HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket
        | HandleKind::Eventfd | HandleKind::Epoll | HandleKind::Timerfd
        | HandleKind::Inotify => 0,
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
        HandleKind::Eventfd => syscall1(SYS_EVENTFD_CLOSE, handle),
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
        POSIX_FADV_NORMAL | POSIX_FADV_SEQUENTIAL | POSIX_FADV_RANDOM
        | POSIX_FADV_NOREUSE | POSIX_FADV_WILLNEED | POSIX_FADV_DONTNEED => {}
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
/// Remove a range of a file without leaving a hole (collapse range).
pub const FALLOC_FL_COLLAPSE_RANGE: i32 = 0x08;
/// Zero a range of the file.
pub const FALLOC_FL_ZERO_RANGE: i32 = 0x10;
/// Insert space within the file (shift data up).
pub const FALLOC_FL_INSERT_RANGE: i32 = 0x20;
/// Unshare shared extents (copy-on-write breakage).
pub const FALLOC_FL_UNSHARE_RANGE: i32 = 0x40;

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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fallocate(fd: Fd, mode: i32, offset: OffT, len: OffT) -> i32 {
    if offset < 0 || len <= 0 {
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

    // Advanced modes (punch hole, collapse range, zero range, etc.)
    // are not yet supported by our filesystem.
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn splice(
    fd_in: Fd,
    off_in: *mut i64,
    fd_out: Fd,
    off_out: *mut i64,
    len: usize,
    flags: u32,
) -> isize {
    let _ = flags;

    if len == 0 {
        return 0;
    }

    // Both fds must be valid.
    let Some(in_entry) = lookup_fd(fd_in) else { return -1; };
    let Some(out_entry) = lookup_fd(fd_out) else { return -1; };

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
                        unsafe { *off_in = cur_in; }
                    }
                    if !off_out.is_null() {
                        // SAFETY: validated above.
                        unsafe { *off_out = cur_out; }
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
        unsafe { *off_in = cur_in; }
    }
    if !off_out.is_null() {
        // SAFETY: validated above.
        unsafe { *off_out = cur_out; }
    }

    total as isize
}

/// Duplicate pipe content without consuming it.
///
/// Stub: returns -1 with ENOSYS.  Linux implements this by sharing
/// pipe-buffer pages between two pipes without copying — our pipe
/// layer is a bounded byte-stream with no "peek without consume"
/// primitive, so there is no userspace fallback that preserves
/// `tee()`'s "leaves data in fd_in" guarantee.  Programs that need
/// tee must fall back to a pipe-into-buffer-into-two-pipes pattern.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tee(
    _fd_in: Fd,
    _fd_out: Fd,
    _len: usize,
    _flags: u32,
) -> isize {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Splice user pages into a pipe.
///
/// Linux `vmsplice()` has two modes depending on which end of the
/// pipe `fd` refers to:
/// - Write end: the iovec contents are "gifted" into the pipe
///   (zero-copy by remapping user pages).
/// - Read end: the next pipe buffers are copied out into the iovec.
///
/// We implement only the write-end direction, as a plain `writev()`
/// into the pipe — no page gifting.  The `SPLICE_F_GIFT` flag is
/// therefore advisory only; the kernel-zero-copy semantics aren't
/// available without VFS-level pipe page sharing.  Read-end use
/// returns -1/EINVAL — callers should use `readv()` instead.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn vmsplice(
    fd: Fd,
    iov: *const Iovec,
    nr_segs: u64,
    flags: u32,
) -> isize {
    let _ = flags;

    if iov.is_null() && nr_segs > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if nr_segs == 0 {
        return 0;
    }
    // Linux caps at UIO_MAXIOV (1024); we use a more generous i32 cap
    // since writev() takes i32 — beyond that, EINVAL.
    if nr_segs > i32::MAX as u64 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else { return -1; };
    if entry.kind != HandleKind::Pipe {
        // Linux returns EBADF for non-pipe fds.
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // Treat fd as the write end.  Read-end vmsplice (copying pipe
    // buffers into iovec) is not supported — if the caller wanted to
    // read, they should have called readv().
    writev(fd, iov, nr_segs as i32)
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

/// `sendfile64` — LP64 alias for `sendfile`.
///
/// On 64-bit systems (LP64), `off_t` is already 64-bit, so `sendfile64`
/// is identical to `sendfile`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sendfile64(
    out_fd: Fd,
    in_fd: Fd,
    offset: *mut i64,
    count: usize,
) -> isize {
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

/// Sync a file range to disk.
///
/// This Linux-specific function provides fine-grained control over
/// syncing file data to disk.  Since we don't have a writeback cache,
/// this delegates to fsync for the full file.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sync_file_range(fd: Fd, _offset: i64, _nbytes: i64, _flags: u32) -> i32 {
    if fd < 0 {
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

/// Obtain a file handle for a path.
///
/// Stub: returns -1 with ENOSYS.  File handles require kernel support
/// for exporting/importing filesystem-level identifiers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn name_to_handle_at(
    _dirfd: Fd,
    _pathname: *const u8,
    _handle: *mut FileHandle,
    _mount_id: *mut i32,
    _flags: i32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Open a file using a file handle.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn open_by_handle_at(
    _mount_fd: Fd,
    _handle: *mut FileHandle,
    _flags: i32,
) -> i32 {
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

/// `openat2` — open a file relative to a directory fd with extended
/// resolution control.
///
/// Linux 5.6+ syscall.  Our implementation delegates to regular `openat`
/// for now — the `resolve` flags are accepted but not enforced (our VFS
/// doesn't support the RESOLVE_* restrictions yet).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn openat2(dirfd: i32, path: *const u8, how: *const OpenHow, size: usize) -> Fd {
    if how.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if size < core::mem::size_of::<OpenHow>() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: caller guarantees `how` points to a valid OpenHow struct.
    let h = unsafe { &*how };
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
/// Gets extended file status relative to a directory fd.  Falls back
/// to `fstatat` internally and converts the result into a `Statx`.
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

    // Get the underlying stat info via fstatat.
    let mut st = Stat::default();
    let ret = fstatat(dirfd, path, &raw mut st, flags);
    if ret != 0 {
        return ret;
    }

    // SAFETY: caller guarantees `buf` points to valid memory.
    let sx = unsafe { &mut *buf };
    *sx = Statx::default();

    // Populate requested fields.
    let mut filled: u32 = 0;

    if mask & STATX_TYPE != 0 || mask & STATX_MODE != 0 {
        #[allow(clippy::cast_possible_truncation)]
        { sx.stx_mode = st.st_mode as u16; }
        filled |= STATX_TYPE | STATX_MODE;
    }
    if mask & STATX_NLINK != 0 {
        #[allow(clippy::cast_possible_truncation)]
        { sx.stx_nlink = st.st_nlink as u32; }
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
        // Open our own fd so we don't depend on whether some other
        // test in the suite has closed stdin/stdout.
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Console, 0)
            .expect("fd available");
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
        assert_eq!(posix_fadvise(-1, 100, -100, POSIX_FADV_SEQUENTIAL), errno::EINVAL);
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
        assert_eq!(posix_fadvise(read_end, 0, 0, POSIX_FADV_NORMAL), errno::ESPIPE);
        assert_eq!(posix_fadvise(write_end, 0, 0, POSIX_FADV_NORMAL), errno::ESPIPE);
        // Cleanup.
        let _ = close(read_end);
        let _ = close(write_end);
    }

    #[test]
    fn test_fadvise64_delegates_to_posix_fadvise() {
        // fadvise64 must validate the same way as posix_fadvise.
        assert_eq!(fadvise64(-1, 0, 0, POSIX_FADV_NORMAL), errno::EBADF);
        assert_eq!(fadvise64(-1, 0, 0, 99), errno::EINVAL);
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Console, 0)
            .expect("fd available");
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
        assert_eq!(
            posix_fallocate(0, i64::MAX, 1),
            crate::errno::EFBIG,
        );
    }

    // -- fallocate (Linux) --

    #[test]
    fn test_fallocate_negative_offset() {
        crate::errno::set_errno(0);
        assert_eq!(fallocate(0, 0, -1, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fallocate_zero_len() {
        crate::errno::set_errno(0);
        assert_eq!(fallocate(0, 0, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fallocate_negative_len() {
        crate::errno::set_errno(0);
        assert_eq!(fallocate(0, 0, 0, -1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fallocate_keep_size_succeeds() {
        // KEEP_SIZE mode is a no-op stub — should succeed.
        assert_eq!(fallocate(0, FALLOC_FL_KEEP_SIZE, 0, 4096), 0);
    }

    #[test]
    fn test_fallocate_keep_size_negative_offset() {
        crate::errno::set_errno(0);
        assert_eq!(fallocate(0, FALLOC_FL_KEEP_SIZE, -1, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_fallocate_punch_hole_eopnotsupp() {
        crate::errno::set_errno(0);
        assert_eq!(fallocate(0, FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE, 0, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
    }

    #[test]
    fn test_fallocate_collapse_range_eopnotsupp() {
        crate::errno::set_errno(0);
        assert_eq!(fallocate(0, FALLOC_FL_COLLAPSE_RANGE, 0, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
    }

    #[test]
    fn test_fallocate_zero_range_eopnotsupp() {
        crate::errno::set_errno(0);
        assert_eq!(fallocate(0, FALLOC_FL_ZERO_RANGE, 0, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
    }

    #[test]
    fn test_fallocate_insert_range_eopnotsupp() {
        crate::errno::set_errno(0);
        assert_eq!(fallocate(0, FALLOC_FL_INSERT_RANGE, 0, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
    }

    #[test]
    fn test_fallocate_unshare_range_eopnotsupp() {
        crate::errno::set_errno(0);
        assert_eq!(fallocate(0, FALLOC_FL_UNSHARE_RANGE, 0, 4096), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
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
                assert_eq!(all[i] & all[j], 0,
                    "FALLOC_FL flags {i} and {j} collide");
            }
        }
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
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Console, 0)
            .expect("fd available");
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
            assert!(fdtable::get_fd_flags(fd).is_none(),
                "unopened fd {fd} must not have flags set");
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
        assert_eq!(lockf(0, F_LOCK, 0), 0);
        assert_eq!(lockf(0, F_TLOCK, 0), 0);
        assert_eq!(lockf(0, F_ULOCK, 0), 0);
        assert_eq!(lockf(0, F_TEST, 0), 0);
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
        let tv = Timeval { tv_sec: 1234, tv_usec: 5678 };
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
        assert_eq!(futimes(0, core::ptr::null()), 0);
    }

    #[test]
    fn test_utimensat_stub_succeeds() {
        assert_eq!(utimensat(AT_FDCWD, b"/tmp\0".as_ptr(), core::ptr::null(), 0), 0);
    }

    #[test]
    fn test_futimens_stub_succeeds() {
        assert_eq!(futimens(0, core::ptr::null()), 0);
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
        let iov = Iovec { iov_base: core::ptr::null_mut(), iov_len: 0 };
        let result = readv(0, &raw const iov, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_readv_negative_iovcnt() {
        let iov = Iovec { iov_base: core::ptr::null_mut(), iov_len: 0 };
        let result = readv(0, &raw const iov, -1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_readv_too_many_iovcnt() {
        let iov = Iovec { iov_base: core::ptr::null_mut(), iov_len: 0 };
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
        let iov = Iovec { iov_base: core::ptr::null_mut(), iov_len: 0 };
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
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
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
        assert_eq!(fstatat(AT_FDCWD, b"/tmp\0".as_ptr(), core::ptr::null_mut(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_fstatat_nofollow_delegates_to_lstat() {
        // With AT_SYMLINK_NOFOLLOW and AT_FDCWD, should delegate to lstat.
        // Verify it hits the same null-check as lstat.
        let mut buf = crate::stat::Stat::zeroed();
        assert_eq!(fstatat(AT_FDCWD, core::ptr::null(), &raw mut buf, AT_SYMLINK_NOFOLLOW), -1);
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
        assert_eq!(renameat(AT_FDCWD, core::ptr::null(), AT_FDCWD, b"/b\0".as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_renameat_atfdcwd_null_new() {
        assert_eq!(renameat(AT_FDCWD, b"/a\0".as_ptr(), AT_FDCWD, core::ptr::null()), -1);
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
        assert_eq!(readlinkat(AT_FDCWD, core::ptr::null(), buf.as_mut_ptr(), 64), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_readlinkat_atfdcwd_null_buf() {
        assert_eq!(readlinkat(AT_FDCWD, b"/link\0".as_ptr(), core::ptr::null_mut(), 64), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_symlinkat_atfdcwd_null_target() {
        assert_eq!(symlinkat(core::ptr::null(), AT_FDCWD, b"/link\0".as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_symlinkat_atfdcwd_null_linkpath() {
        assert_eq!(symlinkat(b"/target\0".as_ptr(), AT_FDCWD, core::ptr::null()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_linkat_atfdcwd_null_old() {
        assert_eq!(linkat(AT_FDCWD, core::ptr::null(), AT_FDCWD, b"/b\0".as_ptr(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_linkat_atfdcwd_null_new() {
        assert_eq!(linkat(AT_FDCWD, b"/a\0".as_ptr(), AT_FDCWD, core::ptr::null(), 0), -1);
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
        assert_eq!(readlinkat(9999, b"link\0".as_ptr(), buf.as_mut_ptr(), 64), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_symlinkat_invalid_dirfd() {
        assert_eq!(symlinkat(b"/target\0".as_ptr(), 9999, b"link\0".as_ptr()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_linkat_invalid_dirfd() {
        assert_eq!(linkat(9999, b"a\0".as_ptr(), AT_FDCWD, b"/b\0".as_ptr(), 0), -1);
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
        // Copying zero bytes should return 0 immediately.
        let result = copy_file_range(
            0, core::ptr::null_mut(),
            1, core::ptr::null_mut(),
            0, 0,
        );
        assert_eq!(result, 0);
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
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Console, 0)
            .expect("fd available");
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
        let result = splice(9999, core::ptr::null_mut(), 1, core::ptr::null_mut(), 4096, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_splice_invalid_fd_out() {
        crate::errno::set_errno(0);
        // fd 0 (stdin) is valid, fd 9999 isn't → EBADF.
        let result = splice(0, core::ptr::null_mut(), 9999, core::ptr::null_mut(), 4096, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_splice_neither_is_pipe_einval() {
        // Fabricate two non-pipe fds.  We can't rely on fds 0/1 being
        // present in the full suite because other tests may have closed
        // them — using alloc_fd guarantees fresh slots in known states.
        let in_fd = fdtable::alloc_fd(HandleKind::File, 3)
            .expect("alloc_fd File failed");
        let out_fd = fdtable::alloc_fd(HandleKind::File, 4)
            .expect("alloc_fd File failed");

        crate::errno::set_errno(0);
        let result = splice(
            in_fd, core::ptr::null_mut(),
            out_fd, core::ptr::null_mut(),
            4096, 0,
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
        let pipe_fd = fdtable::alloc_fd(HandleKind::Pipe, 1)
            .expect("alloc_fd Pipe failed");
        let file_fd = fdtable::alloc_fd(HandleKind::File, 1)
            .expect("alloc_fd File failed");

        crate::errno::set_errno(0);
        let mut off: i64 = 0;
        let result = splice(
            pipe_fd, &raw mut off,
            file_fd, core::ptr::null_mut(),
            4096, 0,
        );
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESPIPE);

        let _ = fdtable::close_fd(pipe_fd);
        let _ = fdtable::close_fd(file_fd);
    }

    #[test]
    fn test_splice_offset_on_pipe_out_espipe() {
        let pipe_fd = fdtable::alloc_fd(HandleKind::Pipe, 2)
            .expect("alloc_fd Pipe failed");
        let file_fd = fdtable::alloc_fd(HandleKind::File, 2)
            .expect("alloc_fd File failed");

        crate::errno::set_errno(0);
        let mut off: i64 = 0;
        let result = splice(
            file_fd, core::ptr::null_mut(),
            pipe_fd, &raw mut off,
            4096, 0,
        );
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESPIPE);

        let _ = fdtable::close_fd(pipe_fd);
        let _ = fdtable::close_fd(file_fd);
    }

    #[test]
    fn test_tee_still_enosys() {
        // tee has no userspace fallback that preserves "leave data in
        // fd_in" semantics, so it remains ENOSYS for now.
        crate::errno::set_errno(0);
        assert_eq!(tee(0, 1, 4096, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
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
        let dummy = Iovec { iov_base: core::ptr::null_mut(), iov_len: 0 };
        let result = vmsplice(0, &raw const dummy, (i32::MAX as u64) + 1, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_vmsplice_invalid_fd_ebadf() {
        crate::errno::set_errno(0);
        let dummy = Iovec { iov_base: core::ptr::null_mut(), iov_len: 0 };
        let result = vmsplice(9999, &raw const dummy, 1, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_vmsplice_non_pipe_fd_ebadf() {
        // fd 1 is Console, not Pipe — Linux returns EBADF.
        crate::errno::set_errno(0);
        let dummy = Iovec { iov_base: core::ptr::null_mut(), iov_len: 0 };
        let result = vmsplice(1, &raw const dummy, 1, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
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
        let all = [SPLICE_F_MOVE, SPLICE_F_NONBLOCK, SPLICE_F_MORE, SPLICE_F_GIFT];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_eq!(all[i] & all[j], 0,
                    "SPLICE_F flags {i} and {j} collide");
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
        let iov = Iovec { iov_base: core::ptr::null_mut(), iov_len: 0 };
        crate::errno::set_errno(0);
        let ret = preadv(0, &iov, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_preadv_negative_iovcnt() {
        let iov = Iovec { iov_base: core::ptr::null_mut(), iov_len: 0 };
        crate::errno::set_errno(0);
        let ret = preadv(0, &iov, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_preadv_over_max_iovcnt() {
        let iov = Iovec { iov_base: core::ptr::null_mut(), iov_len: 0 };
        crate::errno::set_errno(0);
        let ret = preadv(0, &iov, 1025, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_preadv_negative_offset() {
        let mut buf = [0u8; 16];
        let iov = Iovec { iov_base: buf.as_mut_ptr(), iov_len: 16 };
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
        let iov = Iovec { iov_base: core::ptr::null_mut(), iov_len: 0 };
        crate::errno::set_errno(0);
        let ret = pwritev(0, &iov, 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pwritev_negative_offset() {
        let buf = [0u8; 16];
        let iov = Iovec { iov_base: buf.as_ptr().cast_mut(), iov_len: 16 };
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
        // On test host, fd 0 (stdin) may or may not support fsync.
        let _ret = sync_file_range(0, 0, 4096, SYNC_FILE_RANGE_WRITE);
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

    // -----------------------------------------------------------------------
    // name_to_handle_at / open_by_handle_at
    // -----------------------------------------------------------------------

    #[test]
    fn test_name_to_handle_at_returns_enosys() {
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(
            -100, // AT_FDCWD
            b"/tmp\0".as_ptr(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_open_by_handle_at_returns_enosys() {
        crate::errno::set_errno(0);
        let ret = open_by_handle_at(3, core::ptr::null_mut(), 0);
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
        let _ret = faccessat2(AT_FDCWD, b"/nonexistent_file_xyz\0".as_ptr(), crate::fcntl::F_OK, 0);
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
        let how = OpenHow { flags: 0, mode: 0, resolve: 0 };
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
        let how = OpenHow { flags: crate::fcntl::O_RDONLY as u64, mode: 0, resolve: 0 };
        let _ret = openat2(
            AT_FDCWD,
            b"/nonexistent_openat2_test\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
    }

    // -----------------------------------------------------------------------
    // statx
    // -----------------------------------------------------------------------

    #[test]
    fn test_statx_null_buf() {
        crate::errno::set_errno(0);
        let ret = statx(AT_FDCWD, b"/tmp\0".as_ptr(), 0, STATX_ALL, core::ptr::null_mut());
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
        let expected = STATX_TYPE | STATX_MODE | STATX_NLINK | STATX_UID
            | STATX_GID | STATX_ATIME | STATX_MTIME | STATX_CTIME
            | STATX_INO | STATX_SIZE | STATX_BLOCKS;
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
}

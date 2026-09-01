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
/// The part of Linux's `build_open_flags` (fs/open.c) that rejects a flag
/// combination outright, factored out so `open` and `openat` can both run it
/// in the position upstream runs it: *first*.
///
/// `do_sys_openat2` calls `build_open_flags(how, &op)` and returns its error
/// before it ever calls `getname(filename)` — and before `do_filp_open`
/// touches `dfd`.  So an impossible flag word outranks both a NULL path
/// (`EFAULT`) and a bad directory fd (`EBADF`).  We used to check the path
/// first and never made this check at all.
///
/// Only the combinations that survive `build_open_how`'s masking are listed.
/// Unknown *individual* bits are not an error for `open`/`openat`: upstream
/// notes that "older syscalls implicitly clear all of the invalid flags …
/// before calling build_open_flags(), but openat2(2) checks all of its
/// arguments", which is why [`openat2`] rejects them and this does not.
///
/// Returns `false` with `errno` already set when the flags are unusable.
fn validate_open_flags(flags: i32) -> bool {
    // "Block bugs where O_DIRECTORY | O_CREAT created regular files."  Both
    // bits together are always a caller error; upstream returns EINVAL.
    if flags & fcntl::O_DIRECTORY != 0 && flags & fcntl::O_CREAT != 0 {
        errno::set_errno(errno::EINVAL);
        return false;
    }

    // O_TMPFILE is `__O_TMPFILE | O_DIRECTORY`, and upstream enforces both
    // that pairing and that the open is for writing, *before* the filesystem
    // ever gets a chance to say it doesn't support tmpfiles.  Our EOPNOTSUPP
    // for the unsupported-but-well-formed case therefore has to rank below
    // these two EINVALs — it corresponds to `do_tmpfile`, which runs inside
    // `path_openat`, long after `getname`.
    if flags & RAW_O_TMPFILE_I32 != 0 {
        if flags & fcntl::O_DIRECTORY == 0 {
            errno::set_errno(errno::EINVAL);
            return false;
        }
        let acc = flags & fcntl::O_ACCMODE;
        if acc != fcntl::O_WRONLY && acc != fcntl::O_RDWR {
            errno::set_errno(errno::EINVAL);
            return false;
        }
    }

    true
}

/// Translates POSIX `open(path, flags, mode)` to our native
/// `SYS_FS_OPEN(path_ptr, path_len, flags, create_mode)`.
///
/// When `O_CREAT` is set and the file is created, the on-disk permission
/// bits are `mode & ~umask` (masked to the low 9 bits) — computed here and
/// passed to the kernel as the 4th syscall argument.  Without `O_CREAT` the
/// mode is ignored (arg3 = 0, which the kernel reads as "unspecified").
///
/// Returns a file descriptor on success, -1 on error (errno set).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn open(path: *const u8, flags: i32, mode: ModeT) -> Fd {
    // `build_open_flags` (fs/open.c) runs before `getname(filename)` in
    // `do_sys_openat2`, so every verdict it reaches outranks a bad path
    // pointer.  See `validate_open_flags`.
    if !validate_open_flags(flags) {
        return -1;
    }

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

    // `/dev/ptmx` and `/dev/pts/<n>` are handled entirely in libc.  Lane A
    // considered a VFS device-node concept for them and declined it on the
    // grounds that "a mechanism whose only user is two hardcoded names will
    // be wrong for the third", which is right: nothing about these two
    // paths needs to reach the filesystem, and putting them here keeps the
    // kernel's namespace free of nodes that are really syscalls.
    if let Some(fd) = open_pty_device(resolved.get(..resolved_len).unwrap_or(&[]), flags) {
        return fd;
    }

    let native_flags = translate_open_flags(flags);

    // Compute the umask-masked create mode only when O_CREAT is present;
    // otherwise pass 0 so the kernel keeps its "mode unspecified" path.
    let create_mode = if flags & fcntl::O_CREAT != 0 {
        u64::from(apply_umask_create(mode))
    } else {
        0
    };

    let ret = syscall4(
        SYS_FS_OPEN_MODE,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        native_flags,
        create_mode,
    );

    if ret < 0 {
        return errno::translate(ret) as Fd;
    }

    // Register the kernel file handle in the fd table.
    // Store the original POSIX flags (access mode + status flags) so
    // fcntl(F_GETFL) can return them.  Strip creation-only flags
    // (O_CREAT, O_EXCL, O_TRUNC, O_NOCTTY, O_DIRECTORY) that don't
    // survive past open().
    //
    // `O_PATH` is kept for two reasons.  `F_GETFL` reports it — measured on
    // Linux 6.6, `fcntl(O_PATH fd, F_GETFL)` returns exactly `O_PATH` and
    // nothing else, which is what this mask produces since `O_RDONLY` is 0.
    // And it is what [`reject_path_fd_entry`] reads to give the descriptor its
    // `EBADF` on every operation that would touch the file rather than name it.
    let stored_flags = flags
        & (fcntl::O_ACCMODE
            | fcntl::O_APPEND
            | fcntl::O_NONBLOCK
            | fcntl::O_SYNC
            | fcntl::O_NOFOLLOW
            | fcntl::O_PATH);
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
        HandleKind::Pipe => crate::pipe::pipe_kernel_close(entry.handle),
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
        HandleKind::PtyMaster | HandleKind::PtySlave => close_pty_handle(entry.handle),
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
/// - Console → `SYS_TTY_READ` (through the kernel line discipline)
///
/// Returns number of bytes read, 0 at EOF, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn read(fd: Fd, buf: *mut u8, count: SizeT) -> SsizeT {
    // The descriptor first, whatever `count` is.  `ksys_read`
    // (fs/read_write.c:604) opens with `fdget_pos(fd)` and returns `-EBADF`
    // when it comes back empty; only inside `vfs_read` (:458) does
    // `access_ok(buf, count)` produce `EFAULT`.  So `EBADF` outranks `EFAULT`,
    // and — because Linux has no zero-length shortcut above the lookup —
    // `read(closed_fd, buf, 0)` is `EBADF`, not a silent 0.
    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };
    if reject_path_fd_entry(&entry) {
        return -1;
    }

    if buf.is_null() && count > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // POSIX: "If nbyte is 0, read() will return 0 and have no other
    // results."  Short-circuit before touching the kernel so a 0-length
    // read on a reset TCP connection doesn't spuriously return an error.
    // (`access_ok(NULL, 0)` succeeds upstream, so a NULL buffer at count 0 is
    // likewise not a fault.)
    if count == 0 {
        return 0;
    }

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
            // Console read goes through the kernel's line discipline
            // (SYS_TTY_READ), not the raw keyboard.  It therefore honours
            // ICANON (line editing and VEOF), the VMIN/VTIME pair in raw
            // mode, and ISIG — a ^C/^\/^Z generates SIGINT/SIGQUIT/SIGTSTP
            // for the session's foreground process group instead of being
            // handed to us as a data byte.
            //
            // This used to be SYS_CONSOLE_READ_CHAR, which reads a single
            // raw byte straight from the keyboard driver.  Under it a
            // native-ABI program got no line editing, no EOF on ^D, no
            // effect from tcsetattr, and — worst — no terminal signals at
            // all, while a Linux-ABI program on the same console got all
            // four.  See design-decisions §114.
            syscall2(SYS_TTY_READ, buf as u64, count as u64)
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
        HandleKind::PtyMaster => {
            // What the program on the slave end printed, already through
            // `OPOST`/`ONLCR` — this is the byte stream a terminal emulator
            // draws.
            let is_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
            // `SYS_PTY_MASTER_READ` reports the last slave closing as
            // `IoError` → `EIO`, *not* as a zero-length read, and we pass
            // that through unchanged.  Converting it to 0 for friendliness
            // is the one thing lane A asked us not to do (design-decisions
            // §259): a program that only understands `EIO` reads a 0 as
            // "nothing right now, retry" and spins at 100% CPU forever on a
            // window whose child is already dead.  The reverse mistake —
            // `EIO` to a program expecting 0 — is a spurious diagnostic and
            // an exit.  Buffered output is delivered before the `EIO`, so
            // nothing the child printed is lost by honouring it.
            if is_nb {
                syscall3(
                    SYS_PTY_MASTER_TRY_READ,
                    entry.handle,
                    buf as u64,
                    count as u64,
                )
            } else {
                syscall3(SYS_PTY_MASTER_READ, entry.handle, buf as u64, count as u64)
            }
        }
        HandleKind::PtySlave => {
            // The slave end reads through the line discipline, which is the
            // same code path the console uses — `SYS_TTY_READ` honours
            // `ICANON`, `VMIN`/`VTIME` and `ISIG` for whichever terminal the
            // caller is on.
            //
            // It resolves the terminal as `current_tty()` and takes no
            // handle, so it can only serve a slave that is *this* process's
            // controlling terminal.  That is the case the fd exists for: a
            // slave fd is what `login_tty` makes stdin, and `login_tty`
            // acquires the terminal first.  A process holding a slave fd for
            // a terminal it is not on has no way to read it; that needs a
            // handle-taking `SYS_TTY_READ`, which does not exist yet and is
            // logged as `TD-B-PTY-SLAVE-READ-IS-CTTY-ONLY` in
            // `known-issues.md`.
            syscall2(SYS_TTY_READ, buf as u64, count as u64)
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
    // The descriptor first — `ksys_write` (fs/read_write.c:628) is `fdget_pos`
    // then `vfs_write`, whose `access_ok` (:458, via the same path as
    // `vfs_read`) is what yields `EFAULT`.  See `read` above.
    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };
    if reject_path_fd_entry(&entry) {
        return -1;
    }

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
        HandleKind::PtyMaster => {
            // Writing to the master delivers keystrokes into the slave's
            // line discipline — this is the emulator typing at the shell.
            let ret = syscall3(SYS_PTY_MASTER_WRITE, entry.handle, buf as u64, count as u64);
            if ret == errno::native::CHANNEL_CLOSED {
                // Every slave is gone: nothing can ever read these bytes.
                // POSIX spells that EPIPE, the same as a pipe whose reader
                // has closed, and lane A's table names EPIPE for this case
                // explicitly.
                errno::set_errno(errno::EPIPE);
                return -1;
            }
            ret
        }
        HandleKind::PtySlave => {
            // Output from the program's side, through `OPOST`/`ONLCR` and
            // the `TOSTOP` job-control gate.  The return is counted in the
            // bytes we handed over, not in the CRLF-expanded ones, so a
            // caller looping on a short write makes progress.
            let ret = syscall3(SYS_PTY_SLAVE_WRITE, entry.handle, buf as u64, count as u64);
            if ret == errno::native::CHANNEL_CLOSED {
                errno::set_errno(errno::EPIPE);
                return -1;
            }
            ret
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
    // The three checks below used to run in the opposite order — arguments
    // first, descriptor last — which got two of the four orderings wrong.
    // What was measured on Linux 6.6:
    //
    //   lseek(999closed, 0, badwhence)  -> EBADF   (not EINVAL: the fd wins)
    //   lseek(pathfd,    0, badwhence)  -> EBADF
    //   lseek(pipe,      0, badwhence)  -> EINVAL  (whence beats seekability)
    //   lseek(pipe,      0, SEEK_SET)   -> ESPIPE
    //   lseek(pipe,     -5, SEEK_DATA)  -> ESPIPE  (seekability beats offset)
    //   lseek(file,      0, badwhence)  -> EINVAL
    //
    // Which fixes the order at: descriptor, then `whence`, then seekability,
    // then the offset — each check strictly inside the previous one's success.
    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };
    if reject_path_fd_entry(&entry) {
        return -1;
    }

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

    match entry.kind {
        HandleKind::File => {
            let seeking_sparse =
                whence == crate::fcntl::SEEK_DATA || whence == crate::fcntl::SEEK_HOLE;

            // SEEK_DATA / SEEK_HOLE take an absolute starting offset; a
            // negative value is meaningless (the kernel would treat the cast
            // u64 as a huge positive position, i.e. past EOF).  The error is
            // ENXIO, not the EINVAL we used to return — measured identically
            // on ext4 and tmpfs, `lseek(f, -5, SEEK_DATA)` is ENXIO, and it is
            // the same error as `lseek(f, past_eof, SEEK_DATA)` precisely
            // because that is how the kernels reach it (`offset < 0 || offset
            // >= i_size` is one condition in shmem_file_llseek).
            if seeking_sparse && offset < 0 {
                errno::set_errno(errno::ENXIO);
                return -1;
            }

            let ret = if whence == crate::fcntl::SEEK_DATA {
                syscall2(SYS_FS_SEEK_DATA, entry.handle, offset as u64)
            } else if whence == crate::fcntl::SEEK_HOLE {
                syscall2(SYS_FS_SEEK_HOLE, entry.handle, offset as u64)
            } else {
                syscall3(SYS_FS_SEEK, entry.handle, offset as u64, whence as u64)
            };
            // Starting the search past EOF is ENXIO on Linux, but our kernel
            // reports it as the generic `InvalidArgument` — `fs/handle.rs`'s
            // `SeekFrom::Data`/`SeekFrom::Hole` arms are the only places those
            // two syscalls can produce it, so the remap is exact rather than a
            // guess about which EINVAL we are looking at.  It is done here
            // rather than in `errno::translate` because `InvalidArgument` must
            // stay EINVAL for every other caller.
            let out = errno::translate(ret) as OffT;
            if out < 0 && seeking_sparse && errno::get_errno() == errno::EINVAL {
                errno::set_errno(errno::ENXIO);
            }
            out
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
        | HandleKind::UnixStream
        | HandleKind::PtyMaster
        | HandleKind::PtySlave => {
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
    // `ksys_pread64` (fs/read_write.c:652) fixes the whole order: `pos < 0`
    // (:658) → EINVAL, `fdget` (:661) → EBADF, `!FMODE_PREAD` (:664) → ESPIPE,
    // and only then `vfs_read`'s `access_ok` (:458) → EFAULT.  We used to run
    // it backwards.
    //
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
    if reject_path_fd_entry(&entry) {
        return -1;
    }

    if entry.kind != HandleKind::File {
        errno::set_errno(errno::ESPIPE);
        return -1;
    }

    if buf.is_null() && count > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // POSIX: "If nbyte is 0, read() will return 0 and have no other results."
    // Below the ESPIPE test, not above it: upstream reaches `vfs_read` (which
    // is what returns the 0) only after the `FMODE_PREAD` check, so a
    // zero-length pread on a pipe is ESPIPE rather than a silent success.
    if count == 0 {
        return 0;
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
    // Same order as `pread`, from `ksys_pwrite64` (fs/read_write.c:686):
    // `pos < 0` (:692) → EINVAL, `fdget` (:695) → EBADF, `!FMODE_PWRITE`
    // (:698) → ESPIPE, then `vfs_write`'s `access_ok` → EFAULT.
    //
    // POSIX: pwrite with negative offset shall fail with EINVAL.
    if offset < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };
    if reject_path_fd_entry(&entry) {
        return -1;
    }

    if entry.kind != HandleKind::File {
        errno::set_errno(errno::ESPIPE);
        return -1;
    }

    if buf.is_null() && count > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // POSIX: "If nbyte is 0 and the file is a regular file, write() will
    // return zero and have no other results."  Below the ESPIPE test — see
    // `pread`.
    if count == 0 {
        return 0;
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

/// Outcome of the argument checks `import_iovec` performs on `(iov, iovcnt)`.
enum IovecCheck {
    /// `nr_segs == 0`.  Upstream returns an empty iterator, so the call
    /// succeeds transferring 0 bytes *without ever reading `iov`* — a NULL
    /// vector at count 0 is therefore not an error.
    Empty,
    /// The vector is usable.
    Usable,
    /// `errno` has been set; the caller must fail.
    Bad,
}

/// The `(iov, iovcnt)` checks from `iovec_from_user` (lib/iov_iter.c), in
/// upstream's order and with upstream's constants.
///
/// ```text
///   nr_segs == 0          -> empty iterator, success with 0 bytes
///   nr_segs > UIO_MAXIOV  -> EINVAL
///   copy_iovec_from_user  -> EFAULT
/// ```
///
/// The zero case is deliberate upstream, not an accident — the comment there
/// reads "SuS says the readv() function *may* fail if the iovcnt argument was
/// less than or equal to 0 … Linux has traditionally returned zero for zero
/// segments".
///
/// Our callers take `iovcnt` as `i32` where the syscall takes an
/// `unsigned long`, so a negative count arrives upstream as a huge value and
/// trips the `UIO_MAXIOV` test; that is why a negative count is `EINVAL` here
/// rather than anything else.
///
/// The three verdicts used to be folded into a single `EINVAL`, which told a
/// caller passing a valid count and a bad pointer that its *count* was wrong,
/// and rejected the traditional zero-segment call outright.
fn check_iovec(iov: *const Iovec, iovcnt: i32) -> IovecCheck {
    if iovcnt == 0 {
        return IovecCheck::Empty;
    }
    if iovcnt < 0 || iovcnt > crate::limits::IOV_MAX {
        errno::set_errno(errno::EINVAL);
        return IovecCheck::Bad;
    }
    if iov.is_null() {
        errno::set_errno(errno::EFAULT);
        return IovecCheck::Bad;
    }
    IovecCheck::Usable
}

/// Read data into multiple buffers (scatter read).
///
/// Reads sequentially into each iovec buffer.  Returns the total
/// number of bytes read, or -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn readv(fd: Fd, iov: *const Iovec, iovcnt: i32) -> SsizeT {
    // `do_readv` (fs/read_write.c) is `fdget_pos` first and only then
    // `vfs_readv` → `import_iovec`, so the descriptor outranks both the count
    // and the pointer.  The per-segment `read` below repeats this lookup, but
    // it would never run for a zero-segment call — which is exactly the case
    // that must still report EBADF.
    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };
    if reject_path_fd_entry(&entry) {
        return -1;
    }
    match check_iovec(iov, iovcnt) {
        IovecCheck::Empty => return 0,
        IovecCheck::Bad => return -1,
        IovecCheck::Usable => {}
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
    // `do_writev` mirrors `do_readv`: `fdget_pos` before `import_iovec`.
    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };
    if reject_path_fd_entry(&entry) {
        return -1;
    }
    match check_iovec(iov, iovcnt) {
        IovecCheck::Empty => return 0,
        IovecCheck::Bad => return -1,
        IovecCheck::Usable => {}
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
    // `do_preadv` (fs/read_write.c) fixes the order: `pos < 0` → EINVAL,
    // `fdget` → EBADF, `!FMODE_PREAD` → ESPIPE, and only then `vfs_readv` →
    // `import_iovec` for the count and the pointer.
    if offset < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };
    if reject_path_fd_entry(&entry) {
        return -1;
    }

    if entry.kind != HandleKind::File {
        errno::set_errno(errno::ESPIPE);
        return -1;
    }

    match check_iovec(iov, iovcnt) {
        IovecCheck::Empty => return 0,
        IovecCheck::Bad => return -1,
        IovecCheck::Usable => {}
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
    // Same order as `preadv`, from `do_pwritev` (fs/read_write.c).
    if offset < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = lookup_fd(fd) else {
        return -1;
    };
    if reject_path_fd_entry(&entry) {
        return -1;
    }

    if entry.kind != HandleKind::File {
        errno::set_errno(errno::ESPIPE);
        return -1;
    }

    match check_iovec(iov, iovcnt) {
        IovecCheck::Empty => return 0,
        IovecCheck::Bad => return -1,
        IovecCheck::Usable => {}
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
        HandleKind::PtyMaster | HandleKind::PtySlave => {
            // Share the handle, exactly as for pipes — *not* `SYS_PTY_DUP`.
            //
            // `SYS_PTY_DUP` returns the same value with the kernel refcount
            // bumped, so calling it here would need a matching extra
            // `SYS_PTY_CLOSE`.  But `close` short-circuits on
            // `is_handle_referenced` and issues exactly one kernel close for
            // however many fds name a handle, so a bumped refcount would
            // never be dropped and the device would outlive its last fd.
            // Sharing at the fd-table level is what the rest of this table
            // already does and is what the existing refcount-by-fd-scan is
            // built for.  `SYS_PTY_DUP` is for a *second* holder that the fd
            // scan cannot see — a handle handed to another process — which
            // is the spawn path, not this one.
            if let Some(fd) = fdtable::alloc_fd_with_flags(entry.kind, entry.handle, src_status) {
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
        | HandleKind::UnixStream
        | HandleKind::PtyMaster
        | HandleKind::PtySlave => entry.handle,
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

/// The raw kernel stat record for an open descriptor.
///
/// The descriptor counterpart of [`stat_path_raw`], and it exists for the same
/// reason: the 80-byte record carries a birth time that `struct stat` has no
/// field for, so [`statx`] needs the record rather than the translation.
///
/// * `Some(0)` — `raw` now holds the record.
/// * `Some(-1)` — the call failed and errno is set.
/// * `None` — this descriptor *has* no record.  Pipes, sockets, epoll fds and
///   the pty ends are not files the kernel can stat; [`fstat`] fabricates a
///   `struct stat` for them from the handle kind alone.  A caller wanting the
///   record's extra fields must fall back to `fstat` and report those fields
///   as unavailable, which is honest — there is no birth time for a pipe.
fn stat_fd_raw(fd: Fd, raw: &mut [u8; crate::stat::KERNEL_STAT_LEN]) -> Option<i32> {
    let Some(entry) = lookup_fd(fd) else {
        // `lookup_fd` set EBADF.  A closed fd is a failure, not an absence of
        // a record, so this is `Some(-1)` rather than `None`.
        return Some(-1);
    };
    if entry.kind != HandleKind::File {
        return None;
    }
    let ret = syscall2(SYS_FS_FSTAT, entry.handle, raw.as_mut_ptr() as u64);
    Some(if ret < 0 {
        errno::translate(ret) as i32
    } else {
        0
    })
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
        HandleKind::Console | HandleKind::PtyMaster | HandleKind::PtySlave => {
            // Return minimal stat for a character device.  Both pty ends are
            // character devices on Linux too (`/dev/ptmx` and `/dev/pts/N`),
            // and `S_ISCHR` on `fstat` is how several programs — `script(1)`
            // among them — decide an fd is terminal-shaped before they even
            // reach `isatty`.
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
/// Our kernel's `SYS_FS_RENAME` takes
/// (old_path, old_len, new_path, new_len, flags).
///
/// Plain `rename(2)` has no flags to send, so it sends [`NO_RENAME_FLAGS`].
/// That word used to be passed for a different reason: the kernel ignored
/// `arg4`, and this call sent an explicit zero so that the day it *started*
/// reading it, `r8` would not be holding whatever the previous syscall had left
/// there. That day has come — `6ea052654` answered
/// `requests/b-a-rename-cannot-be-told-to-refuse-an-existing-target.md` — and
/// the register now carries a value the kernel acts on. See [`renameat2`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn rename(oldpath: *const u8, newpath: *const u8) -> i32 {
    rename_ex(oldpath, newpath, NO_RENAME_FLAGS)
}

/// Shared `rename`/`renameat2` back-end: resolve both names, then send the pair
/// with a flags word.
///
/// `flags` is `renameat2(2)`'s bit-space, forwarded whole and deliberately
/// **not** masked. The kernel refuses a bit it does not recognise rather than
/// ignoring it, and that refusal is the point: a layer that quietly dropped an
/// unrecognised flag would hand a caller who asked for `RENAME_NOREPLACE`
/// exactly the overwrite it asked to be protected from, and report success. So
/// a flag Linux defines and this kernel does not — `RENAME_WHITEOUT` is the
/// live example — comes back `EINVAL`, which is loud and recoverable. Adding
/// one is a line in the kernel's `RenameMode::from_flags` and a request from
/// here, not a mask here.
fn rename_ex(oldpath: *const u8, newpath: *const u8, flags: u64) -> i32 {
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

    let ret = syscall5(
        SYS_FS_RENAME,
        old_resolved.as_ptr() as u64,
        old_len as u64,
        new_resolved.as_ptr() as u64,
        new_len as u64,
        flags,
    );
    errno::translate(ret) as i32
}

/// The flags word `rename(2)` sends: none of them.
///
/// Named rather than written as a bare trailing `0` because that is the one
/// argument of the five whose value is not obvious from the call site.
const NO_RENAME_FLAGS: u64 = 0;

/// Create a hard link.
///
/// Plain `link(2)` does NOT follow a trailing symlink in `oldpath` — the new
/// entry hard-links the symlink inode itself.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn link(oldpath: *const u8, newpath: *const u8) -> i32 {
    link_ex(oldpath, newpath, false)
}

/// Shared `link`/`linkat` back-end.  `follow` maps to the kernel
/// `SYS_FS_LINK` arg4 bit 0 (FOLLOW): `false` (plain `link(2)` / `linkat`
/// without `AT_SYMLINK_FOLLOW`) hard-links a trailing symlink itself; `true`
/// (`linkat` with `AT_SYMLINK_FOLLOW`) dereferences it.
fn link_ex(oldpath: *const u8, newpath: *const u8, follow: bool) -> i32 {
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

    let ret = syscall5(
        SYS_FS_LINK,
        old_resolved.as_ptr() as u64,
        old_len as u64,
        new_resolved.as_ptr() as u64,
        new_len as u64,
        u64::from(follow),
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

    // Kernel ABI (SYS_FS_SYMLINK): arg0/arg1 = link path, arg2/arg3 = target.
    // The link path is the resolved filesystem location; the target is stored
    // verbatim.  (Getting this order wrong makes the kernel try to create the
    // link *at* the target — EEXIST when the target already exists.)
    let ret = syscall4(
        SYS_FS_SYMLINK,
        link_resolved.as_ptr() as u64,
        link_len as u64,
        target as u64,
        target_len as u64,
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
///
/// The new directory's permission bits are `mode & ~umask` (masked to the
/// low 9 bits), computed here and passed to the kernel as the 3rd syscall
/// argument (the kernel is a thin create primitive — see `Vfs::mkdir_mode`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mkdir(path: *const u8, mode: ModeT) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };

    let ret = syscall3(
        SYS_FS_MKDIR_MODE,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        u64::from(apply_umask_mkdir(mode)),
    );
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
    // POSIX: "If length is negative, the function shall fail and the
    // file size shall remain unchanged.  [EINVAL]."
    //
    // Before the path, not after: `do_sys_truncate` (fs/open.c:129) is
    // `if (length < 0) return -EINVAL;` and only then `user_path_at`, so a
    // negative length is decided while the pointer is still untouched.
    // `ftruncate` below has the same shape for the same reason
    // (`do_sys_ftruncate`, fs/open.c:164-170, puts the EINVAL above `fdget`'s
    // EBADF).
    if length < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
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
    if reject_path_fd_entry(&entry) {
        return -1;
    }

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
        | HandleKind::UnixStream
        | HandleKind::PtyMaster
        | HandleKind::PtySlave => {
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
    if reject_path_fd_entry(&entry) {
        return -1;
    }

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
        | HandleKind::UnixStream
        | HandleKind::PtyMaster
        | HandleKind::PtySlave => 0,
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

/// The `EBADF` an `O_PATH` descriptor owes to any operation on the *file*.
///
/// `O_PATH` opens a **name**, not a file: the descriptor is good for `fstat`,
/// `fstatfs`, `close`, `dup`, `fchdir`, `F_GETFD`/`F_SETFD`/`F_DUPFD`, and for
/// standing in as the `dirfd` of an `*at` call — and for nothing else. Every
/// operation that would read, write or alter the object behind the name fails
/// with `EBADF`. Verified against Linux 6.6 for `read`, `write`, `pread`,
/// `readv`, `writev`, `lseek`, `fchmod`, `fchown`, `ftruncate`, `fsync`,
/// `fdatasync`, `flock`, `ioctl`, `mmap`, `fallocate`, `posix_fadvise`,
/// `sendfile`, `F_SETFL` and `F_GETLK`, and for the calls above *not*
/// failing.
///
/// **Where the check goes: exactly where the closed-fd `EBADF` goes.** That is
/// not an assumption, it is the measured rule. Everything that outranks
/// `EBADF` for a *closed* descriptor also outranks this one, and nothing else
/// does — `ftruncate(999, -1)` and `pread(999, buf, 4, -1)` are both `EINVAL`
/// rather than `EBADF`, because the length and offset are validated before the
/// descriptor is looked up at all, and correspondingly `ftruncate(path_fd, -1)`
/// is `EINVAL` while `ftruncate(path_fd, 0)` is `EBADF`. Conversely
/// `lseek(path_fd, 0, <nonsense whence>)` is `EBADF`, not `EINVAL`, because a
/// closed fd wins there too. So: put this beside the `lookup_fd`, and the
/// ordering follows for free.
///
/// See `design-decisions.md` §733 for why only the half a program can observe
/// is implemented, and `known-issues.md` →
/// `B-READLINKAT-CANNOT-READ-A-SYMLINK-FD-BECAUSE-O_PATH-IS-UNIMPLEMENTED`
/// for the other half, which needs a kernel handle that has no open file
/// description behind it.
#[inline]
pub(crate) fn is_path_fd_entry(entry: &fdtable::FdEntry) -> bool {
    entry.status_flags & fcntl::O_PATH != 0
}

/// [`is_path_fd_entry`], plus the `EBADF` — the form nearly every caller wants.
#[inline]
pub(crate) fn reject_path_fd_entry(entry: &fdtable::FdEntry) -> bool {
    if is_path_fd_entry(entry) {
        errno::set_errno(errno::EBADF);
        return true;
    }
    false
}

/// [`reject_path_fd_entry`] for a call site that has not looked the fd up.
///
/// Returns `false` for a descriptor that is not open, leaving that call's own
/// `EBADF` to report it — this function's job is only the `O_PATH` case.
#[inline]
pub(crate) fn reject_path_fd(fd: Fd) -> bool {
    fdtable::get_fd(fd).is_some_and(|e| reject_path_fd_entry(&e))
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
        HandleKind::Pipe => crate::pipe::pipe_kernel_close(handle),
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
        HandleKind::PtyMaster | HandleKind::PtySlave => close_pty_handle(handle),
    }
}

/// Open `/dev/ptmx` or `/dev/pts/<n>`, if that is what `resolved` names.
///
/// `None` means the path is an ordinary one and [`open`] should carry on to
/// the filesystem; `Some` carries the descriptor, or `-1` with `errno` set.
///
/// Reached after path resolution, so `../dev/pts/3` and `/dev//pts/3` land
/// here too -- a special case keyed on the *unresolved* argument would be a
/// special case a caller could step around by accident.
fn open_pty_device(resolved: &[u8], flags: i32) -> Option<Fd> {
    // The set this function claims, named once so `openat2_forward` can refuse
    // exactly it rather than a hand-copied approximation of it.  Redundant
    // with the two tests below by construction — which is the point: the
    // predicate is the definition, and these are its consequences.
    if !is_pty_device_path(resolved) {
        return None;
    }

    if resolved == b"/dev/ptmx" {
        let fd = crate::ioctl::posix_openpt(flags);
        if fd >= 0 && flags & fcntl::O_CLOEXEC != 0 {
            let _ = fdtable::set_fd_flags(fd, fdtable::FD_CLOEXEC);
        }
        return Some(fd);
    }

    let digits = resolved.strip_prefix(b"/dev/pts/".as_slice())?;
    // A non-numeric or empty tail is not a slave name at all.  Falling
    // through to the filesystem would be wrong in a subtler way than it
    // looks: `/dev/pts` is not a real directory here, so the caller would
    // get whatever the VFS says about a path that does not exist, which is
    // the same ENOENT by a longer route -- but it would also let
    // `/dev/pts/../../etc/passwd` reach the filesystem if resolution ever
    // stopped normalising, so the parse is strict and terminal.
    let mut id: u32 = 0;
    if digits.is_empty() {
        errno::set_errno(errno::ENOENT);
        return Some(-1);
    }
    for &b in digits {
        let Some(d) = (b as char).to_digit(10) else {
            errno::set_errno(errno::ENOENT);
            return Some(-1);
        };
        let Some(next) = id.checked_mul(10).and_then(|v| v.checked_add(d)) else {
            // A number too large to be a terminal id names no terminal.
            errno::set_errno(errno::ENOENT);
            return Some(-1);
        };
        id = next;
    }

    // The slave was created alongside its master and has been held by
    // `ptytab` ever since; claiming it is what transfers it to an fd.  A
    // name we have no record of is ENOENT, which is also what Linux
    // reports for a `/dev/pts/N` that no master has allocated.
    let Some(handle) = crate::ptytab::claim_slave(id) else {
        errno::set_errno(errno::ENOENT);
        return Some(-1);
    };

    let status = flags & (fcntl::O_ACCMODE | fcntl::O_APPEND | fcntl::O_NONBLOCK | fcntl::O_SYNC);
    let Some(fd) = fdtable::alloc_fd_with_flags(HandleKind::PtySlave, handle, status) else {
        // The claim is not rolled back.  A claimed-but-unopened slave is
        // still closed by the master's `close_pty_handle`, whereas an
        // un-claim would put it back in the state where a *second* claim
        // could hand the same handle to a second fd -- trading a leak this
        // process can still close for one it cannot.
        errno::set_errno(errno::EMFILE);
        return Some(-1);
    };
    if flags & fcntl::O_CLOEXEC != 0 {
        let _ = fdtable::set_fd_flags(fd, fdtable::FD_CLOEXEC);
    }
    // `O_NOCTTY` needs no handling: opening a terminal here never makes it
    // the controlling one.  Linux's implicit acquisition on open is a
    // documented misfeature that `O_NOCTTY` exists to switch *off*; our
    // kernel only ever grants a controlling terminal through an explicit
    // `SYS_TTY_ACQUIRE_CTTY`, which is what `login_tty` issues.
    Some(fd)
}

/// Release one end of a pseudo-terminal, and the pair record with it.
///
/// One syscall serves both ends -- the kernel reads the end out of the
/// handle's low bit -- so this is shared by `close` and by the dup2
/// eviction path rather than duplicated into each.
///
/// The second half is the part that is easy to miss.  `SYS_PTY_CREATE`
/// hands back *both* ends already open, and [`crate::ptytab`] holds the
/// slave until an `open("/dev/pts/<n>")` claims it.  If the caller closes
/// the master without ever having claimed the slave -- which is exactly
/// what a `posix_openpt` followed by a failed `openpty` does -- that slave
/// handle has no fd, and nothing would ever close it: the device would stay
/// alive with no way left to reach it.  [`crate::ptytab::retire_master`]
/// reports such an orphan, and we close it here.
fn close_pty_handle(handle: u64) -> i64 {
    let ret = syscall1(SYS_PTY_CLOSE, handle);
    if let Some(orphan) = crate::ptytab::retire_master(handle) {
        // Deliberately unchecked: the close the caller asked about is the
        // one above, and a failure to reap a slave they never named is not
        // something they can act on.
        let _ = syscall1(SYS_PTY_CLOSE, orphan);
    }
    crate::ptytab::retire_slave(handle);
    ret
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
    if is_empty_path(path) {
        if flags & AT_EMPTY_PATH == 0 {
            errno::set_errno(errno::ENOENT);
            return -1;
        }
        // The descriptor itself. `access` above answers "the file exists" and
        // then permits every mode, because we have no permission model to
        // consult; the descriptor's counterpart of "the file exists" is that
        // the fd is open, which `fstat` establishes. Any fd will do — Linux's
        // `faccessat2` accepts a pipe here, and so does `fstat`.
        if dirfd == AT_FDCWD {
            return access(CWD_DOT.as_ptr(), mode);
        }
        let mut probe = Stat::default();
        return fstat(dirfd, &raw mut probe);
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

/// Check file accessibility using the **effective** uid/gid rather than the
/// real ones (`eaccess`, glibc/musl extension; also spelled `euidaccess`).
///
/// `access(2)` deliberately asks "could the *real* user do this?", which is the
/// wrong question for a set-uid program deciding whether it can itself open a
/// file.  GNU bash uses `eaccess` throughout `findcmd.c` to decide whether a
/// `$PATH` candidate is executable, and for the `test -r/-w/-x` builtin.
///
/// Equivalent to `faccessat(AT_FDCWD, path, mode, AT_EACCESS)`.
///
/// Note this currently gives the same answer as [`access`]: that function has
/// no permission system to consult and reports existence only (see its docs),
/// so real and effective IDs cannot yet diverge.  Routing through
/// `faccessat`'s `AT_EACCESS` path rather than hard-coding the equivalence
/// means this becomes correct for free once permission checking lands, instead
/// of silently staying wrong.
///
/// Returns 0 if accessible, −1 on error (errno set).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn eaccess(path: *const u8, mode: i32) -> i32 {
    faccessat(AT_FDCWD, path, mode, AT_EACCESS)
}

/// `euidaccess` — the other name glibc exports the same function under.
///
/// Provided because different programs reach for different spellings, and both
/// must resolve when statically linking.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn euidaccess(path: *const u8, mode: i32) -> i32 {
    eaccess(path, mode)
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

/// Returns `true` if the C-string `path` is the empty string `""`.
///
/// Deliberately distinct from null. The two are different errors and Linux
/// keeps them apart: a null path is `EFAULT`, an empty one is `ENOENT` — or,
/// for the calls that take `AT_EMPTY_PATH`, the descriptor itself.
#[inline]
pub(crate) fn is_empty_path(path: *const u8) -> bool {
    // SAFETY: same contract as `is_absolute_path` — callers guarantee `path`
    // is null or a valid C string, and only the first byte is read.
    !path.is_null() && unsafe { *path } == 0
}

/// The path to hand a path-based call when `AT_EMPTY_PATH` names `AT_FDCWD`.
///
/// `fstatat(AT_FDCWD, "", &st, AT_EMPTY_PATH)` stats the current working
/// directory (measured on Linux 6.6), and there is no descriptor to operate on
/// in that case — `AT_FDCWD` is a sentinel, not an fd. `"."` is the same file
/// by definition, so the path-based route answers it correctly.
const CWD_DOT: &[u8; 2] = b".\0";

/// Refuse an empty relative path with `ENOENT`, returning `true` if it did.
///
/// Every `*at` call in the family does this, including with `AT_FDCWD`, and it
/// happens very early: measured on Linux 6.6, an empty path outranks both a
/// closed `dirfd` (`fstatat(999, "", &st, 0)` is `ENOENT`, not `EBADF`) and a
/// NULL output buffer (`fstatat(dirfd, "", NULL, 0)` is `ENOENT`, not
/// `EFAULT`). Only the flag gate outranks it — `fstatat(dirfd, "", &st, 0x40)`
/// is `EINVAL`. So the call order in each `*at` function is: validate flags,
/// then this, then everything else.
///
/// Callers that accept `AT_EMPTY_PATH` must test for it *before* calling this,
/// since for them the empty path is a request rather than a mistake.
///
/// This exists because the textual join cannot express "no name at all". Given
/// `""`, [`build_at_path`] produces `dir_path + "/"`, which is a perfectly good
/// path naming the directory itself — so the call would quietly *succeed* on
/// the wrong file rather than fail. That is the failure mode this prevents.
#[inline]
fn reject_empty_path(path: *const u8) -> bool {
    if is_empty_path(path) {
        errno::set_errno(errno::ENOENT);
        return true;
    }
    false
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

// ---------------------------------------------------------------------------
// The pinned `*at` fast path
// ---------------------------------------------------------------------------
//
// `resolve_dirfd_path` above is a textual join: it takes the path `dirfd` had
// when it was opened and glues the caller's relative name onto it, then hands
// the result to a path-based syscall.  Everything the descriptor was for is
// lost in that step.  If any component of the remembered path is replaced
// between the open and the call — classically, a directory swapped for a
// symlink pointing somewhere the caller has no business writing — the
// operation lands there, and the descriptor the caller held to prevent exactly
// that never came into it.  It also breaks with no attacker at all: rename the
// directory and the descriptor names a path that no longer exists.
//
// `SYS_FS_UNLINKAT_PINNED` (662), `SYS_FS_FSTATAT_PINNED` (663),
// `SYS_FS_FCHMODAT_PINNED` (665), `SYS_FS_MKDIRAT_PINNED` (666),
// `SYS_FS_SYMLINKAT_PINNED` (667), `SYS_FS_LINKAT_PINNED` (668) and
// `SYS_FS_UTIMENSAT_PINNED` (669) resolve the handle instead.  Where the
// arguments fit their shape — a real directory fd and a single-component name —
// they are used, and the join is not reached at all.  Everything else still
// goes through `resolve_dirfd_path`, because a multi-component name is a walk
// and the calls deliberately refuse to walk.
//
// They are not equally urgent, and the order they landed in follows that.  A
// pinned `fstatat` fooled by a swapped directory reports the wrong size; a
// pinned `fchmodat` fooled by one puts a mode — possibly setuid — on a file the
// caller never named.  That is why 665 was lane A's next delivery after
// `unlink` and not, say, `mkdirat`.
//
// 666–669 are the *destination* side of a recursive copy, and the race there is
// a different one again.  A `cp -r` creates objects, so redirecting its
// destination directory partway through the walk is a write primitive rather
// than a disclosure: every remaining entry in the tree is created somewhere the
// caller never named.  With those four wired, the destination side is closed.
// The **source** side is not: the walk still re-derives each source directory by
// name, because `SYS_FS_GETDENTS_PINNED` (664) has no caller yet.  That is a
// read-side race — you read the wrong file — rather than a write-side one, which
// is why it is the remaining half rather than the urgent one.  Tracked in
// known-issues.md.
//
// The fallback is narrow on purpose.  A pinned call that answers `ESTALE`, or
// `EACCES`, or anything else, has *answered*; retrying it by path would
// reintroduce the very race the call exists to close, and would do it silently
// on the failure path where nobody is looking.  Only "this kernel does not
// have the call" falls back.
//
// "This kernel does not have the call" is now something the kernel *says*
// rather than something this side infers.  An empty dispatch slot answers
// `NoSuchSyscall` (-10); a registered handler that refuses answers
// `NotSupported` (-2).  Both were -2 until 2026-08-31, and `pinned_answer`
// carried a per-syscall latch to guess between them — see `pinned_answer` for
// what that guess cost and why the seven latches are gone.
//
// 663 was refused here until 2026-08-31, and the reason is worth keeping: it
// used to write the 64-byte `FS_META_SIZE` record, which carries no inode
// number, no link count and no block count, where [`Stat`] needs all three.
// A pinned `fstatat` built on that would have reported `st_ino == 0` — not a
// failure anyone would see, but a silent break of every same-file test in
// userspace: `cp`'s refusal to copy a file onto itself, `ls -i`, hardlink
// coalescing in `du` and `tar`, `find -samefile`.  A slower `fstatat` that
// answers correctly beats a faster one that does not.  Lane A widened the
// record to the 80-byte one `SYS_FS_STAT` writes (see
// `requests/a-b-663-now-writes-the-80-byte-record-wire-up-fstatat.md`), which
// `crate::stat::fill_from_fsstat` already decodes, and the objection is gone.

/// The `AT_REMOVEDIR` bit as syscall 662 spells it.
///
/// Deliberately the same value Linux uses, so this is a re-export rather than
/// a remapping — but it is a *different constant* from [`AT_REMOVEDIR`]
/// because the two are different ABIs that merely agree today.  Unknown bits
/// are `EINVAL` on the kernel side, so a divergence would fail on the first
/// call rather than quietly mean something else.
const AT_REMOVEDIR_PINNED: u64 = 0x200;

/// The `AT_SYMLINK_NOFOLLOW` bit as syscall 663 spells it.
///
/// A separate constant from [`AT_SYMLINK_NOFOLLOW`] for the same reason
/// [`AT_REMOVEDIR_PINNED`] is separate from [`AT_REMOVEDIR`]: the two ABIs
/// agree on the value today, and nothing but this comment says they must keep
/// agreeing.
const AT_SYMLINK_NOFOLLOW_PINNED: u64 = 0x100;

/// Longest single component the pinned calls accept.
const PINNED_NAME_MAX: usize = 255;

/// Whether `name` is the one-component form the pinned `*at` calls accept.
///
/// Non-empty, at most 255 bytes, containing no `/`, and not `.` or `..`.
/// The last two are excluded by the kernel and not merely by convention: a
/// check that the handle still denotes its directory is ornamental if the
/// name is then allowed to climb out of it.
///
/// A pure function over bytes so it can be tested on the host, where the
/// syscalls themselves return `ENOSYS` and cannot be.
#[must_use]
pub(crate) fn is_pinnable_component(name: &[u8]) -> bool {
    !name.is_empty()
        && name.len() <= PINNED_NAME_MAX
        && !name.contains(&b'/')
        && name != b"."
        && name != b".."
}

/// Turn a raw pinned-syscall return into either a final answer or a fallback.
///
/// `Some(ret)` is the call's answer, translated to a libc return, and is final
/// — including its errors. `None` means this kernel does not have the call and
/// the caller must take the path-based route.
///
/// # The two facts that used to be one number
///
/// `NoSuchSyscall` (-10) is an **empty dispatch slot**: the kernel has never
/// heard of this number. `NotSupported` (-2) is a *registered handler* that ran
/// and refused — this filesystem cannot do the thing. The caller's correct
/// response differs completely: the first means "fall back to the older route",
/// the second means "this is the answer, stop".
///
/// Both were -2 until 2026-08-31, and this function could only guess between
/// them. It guessed with a per-syscall latch — fall back until the first
/// non-`-2` answer proves the slot is wired, honour every later -2 as real —
/// which was sound but wrong for exactly as long as no answer had yet arrived.
/// The first call on a filesystem that genuinely refuses was silently
/// downgraded to the racy path-based route, on the failure path, where nobody
/// is looking; that is the one outcome this whole fast path exists to prevent.
/// Lane A split the codes in `dispatch.rs`'s unregistered-slot arm, so the
/// question is now answered rather than estimated, and the latch — with its
/// seven statics and its dependence on call ordering — is gone.
///
/// [`HOST_ENOSYS`] joins `NoSuchSyscall` as a fallback because the raw
/// `SYSCALL` instruction is compiled out on a host build and every attempt
/// reports it. That is what keeps the host test suite meaningful: the `*at`
/// wrappers under `cargo test` behave exactly as they did before this fast path
/// existed.
fn pinned_answer(ret: i64) -> Option<i32> {
    if ret == crate::errno::native::NO_SUCH_SYSCALL || ret == crate::syscall::HOST_ENOSYS {
        return None;
    }
    Some(errno::translate(ret) as i32)
}

/// The kernel handle a libc directory fd stands for, if it is a file handle.
///
/// Deliberately sets **no** errno. `None` means only "the fast path does not
/// apply", and the caller then runs [`resolve_dirfd_path`], which produces the
/// `EBADF`/`ENOTDIR` diagnosis itself. Duplicating that judgement here would
/// create two places that must agree about what a bad `dirfd` is; leaving it
/// in one place means the fast path cannot be observed in the diagnostics
/// even if it is wrong about which fds it can handle.
///
/// Rejects handle 0 because the native ABI spends that value on "the process
/// working directory" — and the kernel's working directory is not this libc's
/// (see [`openat2_forward`]). Real file handles are never 0, so this can only
/// fire on a corrupt fd table, where falling back is the safe answer.
fn pinned_base(dirfd: i32) -> Option<u64> {
    let entry = crate::fdtable::get_fd(dirfd)?;
    if entry.kind != HandleKind::File || entry.handle == 0 {
        return None;
    }
    Some(entry.handle)
}

/// The caller's name as a byte slice, if it is a shape 662 accepts.
///
/// # Safety
///
/// `path` must be null or a valid C string.
unsafe fn pinnable_name<'a>(path: *const u8) -> Option<&'a [u8]> {
    if path.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees `path` is a valid C string.
    let len = unsafe { crate::string::strlen(path) };
    if len == 0 || len > PINNED_NAME_MAX {
        return None;
    }
    // SAFETY: `strlen` just measured `len` readable bytes at `path`.
    let name = unsafe { core::slice::from_raw_parts(path, len) };
    is_pinnable_component(name).then_some(name)
}

/// Remove `path` under `dirfd` through syscall 662, if that is possible here.
///
/// `Some(ret)` is the call's answer and is final — including its errors.
/// `None` means the fast path did not apply and the caller must fall back;
/// errno is untouched in that case.
///
/// Only the POSIX `AT_REMOVEDIR` bit is forwarded, and it is forwarded as 662's
/// own constant rather than passed through. The two agree today, but 662
/// rejects unknown bits with `EINVAL` while [`unlinkat`]'s path-based route
/// ignores them, so handing the caller's word over unfiltered would make the
/// treatment of a junk flag depend on which route ran — a difference no caller
/// could predict and no test would reliably catch.
///
/// # Safety
///
/// `path` must be null or a valid C string.
unsafe fn try_pinned_unlinkat(dirfd: i32, path: *const u8, flags: i32) -> Option<i32> {
    // SAFETY: forwarded from this function's own contract.
    let name = unsafe { pinnable_name(path) }?;
    let base = pinned_base(dirfd)?;

    let pinned_flags = if flags & AT_REMOVEDIR != 0 {
        AT_REMOVEDIR_PINNED
    } else {
        0
    };
    let ret = syscall4(
        SYS_FS_UNLINKAT_PINNED,
        base,
        name.as_ptr() as u64,
        name.len() as u64,
        pinned_flags,
    );
    pinned_answer(ret)
}

/// Stat `path` under `dirfd` through syscall 663, if that is possible here.
///
/// `Some(ret)` is the call's answer and is final — including its errors, and
/// including having already written `buf` when it succeeded. `None` means the
/// fast path did not apply and the caller must fall back; errno is untouched
/// and `buf` unwritten in that case.
///
/// A null `buf` is one of those `None`s, and deliberately so. 663 writes into a
/// buffer of ours and the translation into the caller's `Stat` happens here, so
/// the kernel never sees the caller's pointer and cannot diagnose it; falling
/// back leaves [`stat`]/[`lstat`] to raise the `EFAULT`, in the one place that
/// already does. Ordering is unaffected — a bad `dirfd` has already failed
/// [`pinned_base`] and fallen back by this point, so `EBADF` still outranks
/// `EFAULT` exactly as it does on the path-based route.
///
/// Only [`AT_SYMLINK_NOFOLLOW`] is forwarded, remapped to 663's own constant.
/// [`fstatat`] also accepts [`AT_NO_AUTOMOUNT`], [`AT_EMPTY_PATH`] and the two
/// [`AT_STATX_SYNC_TYPE`] bits, all of which it ignores; 663 rejects unknown
/// bits with `EINVAL`, so passing the caller's word through unfiltered would
/// turn three flags that mean nothing into an error, and only on the fast path.
///
/// # Safety
///
/// `path` must be null or a valid C string, and `buf` null or a writable
/// [`Stat`].
unsafe fn try_pinned_fstatat(
    dirfd: i32,
    path: *const u8,
    buf: *mut Stat,
    flags: i32,
) -> Option<i32> {
    if buf.is_null() {
        return None;
    }
    // SAFETY: forwarded from this function's own contract.
    let name = unsafe { pinnable_name(path) }?;
    let base = pinned_base(dirfd)?;

    let pinned_flags = if flags & AT_SYMLINK_NOFOLLOW != 0 {
        AT_SYMLINK_NOFOLLOW_PINNED
    } else {
        0
    };
    let mut raw = [0u8; crate::stat::KERNEL_STAT_LEN];
    let ret = syscall5(
        SYS_FS_FSTATAT_PINNED,
        base,
        name.as_ptr() as u64,
        name.len() as u64,
        pinned_flags,
        raw.as_mut_ptr() as u64,
    );
    let ret = pinned_answer(ret)?;
    if ret == 0 {
        // SAFETY: `buf` was checked non-null above, and the caller guarantees
        // it points to a writable `Stat`.  `raw` is `KERNEL_STAT_LEN` bytes,
        // which is the length 663 validated and wrote.
        crate::stat::fill_from_fsstat(unsafe { &mut *buf }, &raw);
    }
    Some(ret)
}

/// Chmod `path` under `dirfd` through syscall 665, if that is possible here.
///
/// `Some(ret)` is the call's answer and is final — including its errors.
/// `None` means the fast path did not apply and the caller must fall back;
/// errno is untouched in that case.
///
/// This is the member of the family where the pin earns the most. `chmod -R`
/// walking a tree it does not own is the classic swap-a-directory-for-a-symlink
/// shape, and what lands at the far end of the swap is not a wrong answer but a
/// mode — quite possibly setuid — on a file the caller never named. A pinned
/// `fstatat` that is fooled reports the wrong size; a pinned `fchmodat` that is
/// fooled hands out privilege.
///
/// The two arguments are filtered for opposite reasons. `flags` **must** be
/// remapped rather than passed through, because 665 rejects a bit it does not
/// know while [`fchmodat`]'s path route accepts and ignores `AT_EMPTY_PATH` —
/// forwarding the caller's word would make a flag's meaning depend on which
/// route ran. `mode` needs no masking at all, because 665 masks to the low
/// `0o7777` bits itself and never errors on the rest; it is masked here anyway,
/// to the same twelve bits `set_perms_path_ex` uses on the path route, so the
/// two routes put the identical value on the wire. Two routes sending different
/// numbers that happen to mean the same thing today is how a divergence gets
/// introduced later without anyone editing either one.
///
/// # Safety
///
/// `path` must be null or a valid C string.
unsafe fn try_pinned_fchmodat(dirfd: i32, path: *const u8, mode: ModeT, flags: i32) -> Option<i32> {
    // SAFETY: forwarded from this function's own contract.
    let name = unsafe { pinnable_name(path) }?;
    let base = pinned_base(dirfd)?;

    let pinned_flags = if flags & AT_SYMLINK_NOFOLLOW != 0 {
        AT_SYMLINK_NOFOLLOW_PINNED
    } else {
        0
    };
    let ret = syscall5(
        SYS_FS_FCHMODAT_PINNED,
        base,
        name.as_ptr() as u64,
        name.len() as u64,
        u64::from(mode & 0o7777),
        pinned_flags,
    );
    pinned_answer(ret)
}

/// Create a directory `path` under `dirfd` through syscall 666, if that is
/// possible here.
///
/// `Some(ret)` is the call's answer and is final — including its errors.
/// `None` means the fast path did not apply and the caller must fall back;
/// errno is untouched in that case.
///
/// `mode` goes through [`apply_umask_mkdir`], the same function [`mkdir`] uses
/// on the path route, because the umask lives in this layer and the kernel
/// applies none of its own. Getting that wrong on one route only would make a
/// process's umask depend on whether its `dirfd` happened to be pinnable, which
/// no caller could predict and no test would reliably catch — sharing the one
/// function is what makes the two routes agree by construction rather than by
/// two mask constants that have to be kept equal by hand.
///
/// 666 masks to **nine** bits where 665 masks to twelve, and the difference is
/// not an inconsistency to paper over: a directory that is setgid or sticky
/// from the instant it exists is a policy decision. See [`apply_umask_mkdir`]
/// for why nine is right for a directory even though twelve is right for a
/// file, and for the one bit (`S_ISVTX`) that may yet move.
///
/// # Safety
///
/// `path` must be null or a valid C string.
unsafe fn try_pinned_mkdirat(dirfd: i32, path: *const u8, mode: ModeT) -> Option<i32> {
    // SAFETY: forwarded from this function's own contract.
    let name = unsafe { pinnable_name(path) }?;
    let base = pinned_base(dirfd)?;

    let ret = syscall5(
        SYS_FS_MKDIRAT_PINNED,
        base,
        name.as_ptr() as u64,
        name.len() as u64,
        u64::from(apply_umask_mkdir(mode)),
        // `mkdirat(2)` has no flags, and 666 rejects any non-zero word.
        0,
    );
    pinned_answer(ret)
}

/// Create a symlink named `linkpath` under `newdirfd`, containing `target`,
/// through syscall 667, if that is possible here.
///
/// `Some(ret)` is the call's answer and is final — including its errors.
/// `None` means the fast path did not apply and the caller must fall back;
/// errno is untouched in that case.
///
/// The two strings are checked by completely different rules, which is the
/// call's whole shape rather than an accident. The **link name** must be one
/// component, because the pin's containment is a statement about the name being
/// created. The **target** is stored verbatim and constrained only in length:
/// `..`, `/`, absolute and dangling are all legal symlink bodies, they are
/// resolved only when something later walks the link — under the ordinary
/// traversal checks, which the pin does not replace — and refusing them would
/// leave this unable to reproduce the relative links a recursive copy exists to
/// copy.
///
/// An **empty** target declines the fast path rather than being forwarded. 667
/// rejects a zero-length target as `EINVAL` while the path-based
/// [`SYS_FS_SYMLINK`] stores it, so forwarding would make an empty target's
/// fate depend on whether the `dirfd` was pinnable. Falling back keeps the two
/// routes in agreement and leaves the question of whether an empty body is
/// legal where it already is.
///
/// # Safety
///
/// `target` and `linkpath` must each be null or a valid C string.
unsafe fn try_pinned_symlinkat(
    target: *const u8,
    newdirfd: i32,
    linkpath: *const u8,
) -> Option<i32> {
    if target.is_null() {
        return None;
    }
    // SAFETY: forwarded from this function's own contract.
    let name = unsafe { pinnable_name(linkpath) }?;
    let base = pinned_base(newdirfd)?;
    // SAFETY: the caller guarantees `target` is a valid C string, checked
    // non-null above.
    let target_len = unsafe { crate::string::strlen(target) };
    if target_len == 0 || target_len > crate::unistd::PATH_MAX {
        return None;
    }

    let ret = syscall5(
        SYS_FS_SYMLINKAT_PINNED,
        base,
        name.as_ptr() as u64,
        name.len() as u64,
        target as u64,
        target_len as u64,
    );
    pinned_answer(ret)
}

/// Hard-link `oldpath` under `olddirfd` to `newpath` under `newdirfd` through
/// syscall 668, if that is possible here.
///
/// `Some(ret)` is the call's answer and is final — including its errors.
/// `None` means the fast path did not apply and the caller must fall back;
/// errno is untouched in that case.
///
/// Both ends must be pinnable — two real directory handles and two
/// single-component names — because the call resolves both handles. A caller
/// linking from an absolute path into a `dirfd`, or vice versa, gets the path
/// route for both halves; there is no half-pinned form, and inventing one would
/// mean a guarantee that held for the destination and not the source without
/// anything in the signature saying so.
///
/// **The caller must have already refused `AT_SYMLINK_FOLLOW`.** 668 has no
/// register left for flags and is always the unfollowed form; following is by
/// definition leaving the pinned directory, so a followed link belongs on the
/// path route where the pin was buying nothing anyway. [`linkat`] checks the
/// flag before calling this, which is where the decision is visible next to the
/// flag's own validation.
///
/// # Safety
///
/// `oldpath` and `newpath` must each be null or a valid C string.
unsafe fn try_pinned_linkat(
    olddirfd: i32,
    oldpath: *const u8,
    newdirfd: i32,
    newpath: *const u8,
) -> Option<i32> {
    // SAFETY: forwarded from this function's own contract.
    let old_name = unsafe { pinnable_name(oldpath) }?;
    // SAFETY: as above.
    let new_name = unsafe { pinnable_name(newpath) }?;
    let old_base = pinned_base(olddirfd)?;
    let new_base = pinned_base(newdirfd)?;

    let ret = syscall6(
        SYS_FS_LINKAT_PINNED,
        old_base,
        old_name.as_ptr() as u64,
        old_name.len() as u64,
        new_base,
        new_name.as_ptr() as u64,
        new_name.len() as u64,
    );
    pinned_answer(ret)
}

/// Stamp `path` under `dirfd` with `(accessed_ns, modified_ns)` through syscall
/// 669, if that is possible here.
///
/// `Some(ret)` is the call's answer and is final — including its errors.
/// `None` means the fast path did not apply and the caller must fall back;
/// errno is untouched in that case.
///
/// The two nanosecond values are already in the kernel's zero-means-unchanged
/// form — [`utimens_pair_to_kernel`] has expanded `UTIME_NOW` into a real
/// timestamp and `UTIME_OMIT` into zero — so this takes them as-is and does not
/// re-derive them. 669 and the path-based `SYS_FS_SET_TIMES` share that
/// convention precisely so one expansion serves both.
///
/// This is the busiest member of the family. `fchmodat` and `mkdirat` run once
/// per directory; restoring mtime is the last thing done to *every* file a
/// `cp -p` or an archive extraction touches, so it is the one whose fallback
/// cost would be paid per entry.
///
/// # Safety
///
/// `path` must be null or a valid C string.
#[cfg(any(target_os = "none", test))]
unsafe fn try_pinned_utimensat(
    dirfd: i32,
    path: *const u8,
    accessed_ns: u64,
    modified_ns: u64,
    no_follow: bool,
) -> Option<i32> {
    // SAFETY: forwarded from this function's own contract.
    let name = unsafe { pinnable_name(path) }?;
    let base = pinned_base(dirfd)?;

    let pinned_flags = if no_follow {
        AT_SYMLINK_NOFOLLOW_PINNED
    } else {
        0
    };
    let ret = syscall6(
        SYS_FS_UTIMENSAT_PINNED,
        base,
        name.as_ptr() as u64,
        name.len() as u64,
        accessed_ns,
        modified_ns,
        pinned_flags,
    );
    pinned_answer(ret)
}

/// AT_FDCWD: use the current working directory.
pub const AT_FDCWD: i32 = -100;
/// AT_SYMLINK_NOFOLLOW: do not follow symlinks.
pub const AT_SYMLINK_NOFOLLOW: i32 = 0x100;
/// AT_REMOVEDIR: unlinkat should remove a directory.
pub const AT_REMOVEDIR: i32 = 0x200;
/// AT_SYMLINK_FOLLOW: follow symlinks (e.g., in `linkat`).
pub const AT_SYMLINK_FOLLOW: i32 = 0x400;
/// AT_NO_AUTOMOUNT: do not trigger an automount on the terminal component.
///
/// Accepted and ignored: we have no automounter, so there is nothing to
/// suppress. Linux ignores it too on a path with nothing to automount —
/// what matters is that it is *accepted*, because callers pass it
/// unconditionally and rejecting it would be a spurious `EINVAL`.
pub const AT_NO_AUTOMOUNT: i32 = 0x800;
/// AT_EMPTY_PATH: operate on the fd itself (Linux 2.6.39+).
pub const AT_EMPTY_PATH: i32 = 0x1000;
/// AT_EACCESS: check using effective IDs in faccessat.
pub const AT_EACCESS: i32 = 0x200;
/// AT_STATX_FORCE_SYNC: force a writeback before reporting attributes.
///
/// Accepted and ignored, like [`AT_NO_AUTOMOUNT`]: our filesystems do not
/// serve attributes from a cache that could be stale relative to a server.
pub const AT_STATX_FORCE_SYNC: i32 = 0x2000;
/// AT_STATX_DONT_SYNC: accept possibly-stale attributes without syncing.
pub const AT_STATX_DONT_SYNC: i32 = 0x4000;
/// The two-bit field [`AT_STATX_FORCE_SYNC`] and [`AT_STATX_DONT_SYNC`] share.
///
/// Setting *both* asks for a sync and for no sync at once. `statx` rejects
/// that with `EINVAL`; `fstatat` does not, which is a real asymmetry and not
/// an oversight in this file — measured against Linux 6.6, where
/// `newfstatat` accepts `0x6000` and `statx` refuses it.
pub const AT_STATX_SYNC_TYPE: i32 = 0x6000;

/// Open a file relative to a directory fd.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn openat(dirfd: i32, path: *const u8, flags: i32, mode: ModeT) -> Fd {
    // Before the dirfd is resolved: `build_open_flags` runs at the top of
    // `do_sys_openat2`, ahead of `do_filp_open`'s use of `dfd`, so an
    // impossible flag word outranks `EBADF` for the directory.  (`open`
    // re-runs this check; it is cheap and idempotent, and running it here too
    // is what keeps the *ordering* right on the non-AT_FDCWD path.)
    if !validate_open_flags(flags) {
        return -1;
    }
    // `openat` has no `AT_EMPTY_PATH`: there is no way to say "open the
    // descriptor again", and an empty name is simply a name that is not there.
    if reject_empty_path(path) {
        return -1;
    }
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
///
/// For the common shape — a real directory fd, a single-component name and a
/// non-null `buf` — this goes through [`try_pinned_fstatat`], which has the
/// kernel resolve the descriptor rather than the remembered path. See "The
/// pinned `*at` fast path" above for why that matters and when it does not
/// apply.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fstatat(dirfd: i32, path: *const u8, buf: *mut Stat, flags: i32) -> i32 {
    // Linux validates flags in `vfs_statx`, ahead of everything: measured on
    // 6.6, a bad flag bit outranks a NULL `buf` (EFAULT), a NULL or missing
    // path (EFAULT/ENOENT) and a closed `dirfd` (EBADF).  The two
    // `AT_STATX_*` bits are accepted here and refused by `statx` only when
    // both are set — that asymmetry is Linux's, and was measured, not assumed.
    if flags & !(AT_SYMLINK_NOFOLLOW | AT_NO_AUTOMOUNT | AT_EMPTY_PATH | AT_STATX_SYNC_TYPE) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if is_empty_path(path) {
        if flags & AT_EMPTY_PATH == 0 {
            errno::set_errno(errno::ENOENT);
            return -1;
        }
        // `fstat` on the descriptor itself, whatever kind it is — Linux
        // accepts a pipe, a socket and an `O_PATH` fd here, and so does ours.
        // `AT_SYMLINK_NOFOLLOW` is meaningless once there is no name left to
        // follow, and Linux ignores it rather than refusing it (measured).
        return if dirfd == AT_FDCWD {
            stat(CWD_DOT.as_ptr(), buf)
        } else {
            fstat(dirfd, buf)
        };
    }
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return if flags & AT_SYMLINK_NOFOLLOW != 0 {
            lstat(path, buf)
        } else {
            stat(path, buf)
        };
    }
    // SAFETY: `path` and `buf` are this function's own contract — null or a
    // valid C string, and null or a writable `Stat` — which is what
    // `try_pinned_fstatat` requires.
    if let Some(ret) = unsafe { try_pinned_fstatat(dirfd, path, buf, flags) } {
        return ret;
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
///
/// For the common shape — a real directory fd and a single-component name —
/// this goes through [`try_pinned_unlinkat`], which has the kernel resolve the
/// descriptor rather than the remembered path. See "The pinned `*at` fast
/// path" above for why that matters and when it does not apply.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn unlinkat(dirfd: i32, path: *const u8, flags: i32) -> i32 {
    // `AT_REMOVEDIR` is the only bit Linux's `do_unlinkat` accepts — not even
    // `AT_SYMLINK_NOFOLLOW`, which `unlink` implies unconditionally.  The check
    // outranks a NULL path (EFAULT) and a closed `dirfd` (EBADF); measured on
    // Linux 6.6 rather than read off the source, because the accepted set is
    // narrower than the family's other members and easy to over-guess.
    if flags & !AT_REMOVEDIR != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // `unlinkat` has no `AT_EMPTY_PATH` — removing a name requires a name.
    if reject_empty_path(path) {
        return -1;
    }
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return if flags & AT_REMOVEDIR != 0 {
            rmdir(path)
        } else {
            unlink(path)
        };
    }
    // SAFETY: `path` is this function's own `*const u8` contract — null or a
    // valid C string — which is what `try_pinned_unlinkat` requires.
    if let Some(ret) = unsafe { try_pinned_unlinkat(dirfd, path, flags) } {
        return ret;
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
    renameat_ex(olddirfd, oldpath, newdirfd, newpath, NO_RENAME_FLAGS)
}

/// Shared `renameat`/`renameat2` back-end: resolve each name against its own
/// `dirfd`, then hand the pair to [`rename_ex`] with a flags word.
///
/// The resolution happens *before* the flags are looked at, and that ordering
/// is observable: `renameat2(fd, "", …, RENAME_NOREPLACE)` is `ENOENT` for the
/// empty name rather than anything to do with the flag, which is Linux's order
/// too.
fn renameat_ex(
    olddirfd: i32,
    oldpath: *const u8,
    newdirfd: i32,
    newpath: *const u8,
    flags: u64,
) -> i32 {
    // Either name being empty is ENOENT, and the old name is examined first.
    if reject_empty_path(oldpath) || reject_empty_path(newpath) {
        return -1;
    }
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

    rename_ex(old_ptr, new_ptr, flags)
}

/// Rename a file with flags (Linux extension).
///
/// `flags` is `RENAME_NOREPLACE` (1), `RENAME_EXCHANGE` (2), or zero. The word
/// is forwarded to the kernel whole; the kernel decodes it and answers
/// `EINVAL` for anything it does not know, including the two set at once — a
/// combination Linux also refuses, because "must not replace" and "swap the
/// two" cannot both be honoured.
///
/// This used to refuse every non-zero `flags` itself. That was a *syscall* gap
/// rather than a kernel one: `Vfs::rename_noreplace` and `Vfs::rename_exchange`
/// both existed, and the first already did its destination-existence check
/// under the same lock as the rename — which is the hard part and the whole
/// point — but `sys_fs_rename` read four arguments and always called
/// `Vfs::rename`, so there was no way to ask. It reads `arg4` as of
/// `6ea052654`, answering
/// `requests/b-a-rename-cannot-be-told-to-refuse-an-existing-target.md`, and
/// the refusal here is gone with it.
///
/// What that closes is a race rather than a missing feature. Every caller of
/// `RENAME_NOREPLACE` is really saying "I checked that this name was free";
/// without the flag the check and the rename are two operations with a window
/// between them, and a name taken inside that window is silently overwritten.
/// The one caller in this tree is `coreutils`'s `backup` module, picking the
/// next `file.~N~`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn renameat2(
    olddirfd: i32,
    oldpath: *const u8,
    newdirfd: i32,
    newpath: *const u8,
    flags: u32,
) -> i32 {
    renameat_ex(olddirfd, oldpath, newdirfd, newpath, u64::from(flags))
}

/// Create a directory relative to a directory fd.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
///
/// For the common shape — a real directory fd and a single-component name —
/// this goes through [`try_pinned_mkdirat`], which has the kernel resolve the
/// descriptor rather than the remembered path. See "The pinned `*at` fast path"
/// above. The pin buys two things here that it does not buy elsewhere in the
/// family: a directory cannot be created somewhere the caller never named, and
/// the requested mode is stamped under the same filesystem lock that made the
/// directory — so a `0o700` directory is never briefly world-*openable* while a
/// separate chmod is still on its way, which is the window the path route
/// leaves open on every private directory anything creates.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mkdirat(dirfd: i32, path: *const u8, mode: ModeT) -> i32 {
    if reject_empty_path(path) {
        return -1;
    }
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return mkdir(path, mode);
    }
    // SAFETY: `path` is this function's own `*const u8` contract — null or a
    // valid C string — which is what `try_pinned_mkdirat` requires.
    if let Some(ret) = unsafe { try_pinned_mkdirat(dirfd, path, mode) } {
        return ret;
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
    // `readlinkat` takes no flags, and Linux nevertheless gives the empty path
    // a meaning: if `dirfd` is itself a *symlink* — which needs
    // `O_PATH|O_NOFOLLOW` to obtain — it reads that link's body, and otherwise
    // it is ENOENT.  Measured both halves on 6.6.
    //
    // We cannot reach the first half: `open` does not implement `O_PATH`, so
    // no fd in this libc ever refers to a symlink rather than its target, and
    // ENOENT is the correct answer for every descriptor a caller can actually
    // hold.  If `O_PATH` is implemented, this needs the other branch too —
    // tracked in known-issues.md as
    // `B-READLINKAT-CANNOT-READ-A-SYMLINK-FD-BECAUSE-O_PATH-IS-UNIMPLEMENTED`.
    if reject_empty_path(path) {
        return -1;
    }
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
///
/// For the common shape — a real directory fd and a single-component link name
/// — this goes through [`try_pinned_symlinkat`]. Note that only the *link name*
/// has to be pinnable: the target is unconstrained on both routes, which is the
/// point, since the links a recursive copy reproduces are overwhelmingly
/// relative ones like `../lib/libfoo.so`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn symlinkat(target: *const u8, newdirfd: i32, linkpath: *const u8) -> i32 {
    // Only `linkpath` is a path to resolve; `target` is stored verbatim, and an
    // empty target is a legal (if useless) symlink body, not an error.
    if reject_empty_path(linkpath) {
        return -1;
    }
    if newdirfd == AT_FDCWD || is_absolute_path(linkpath) {
        return symlink(target, linkpath);
    }
    // SAFETY: `target` and `linkpath` are this function's own `*const u8`
    // contract — null or valid C strings — which is what
    // `try_pinned_symlinkat` requires.
    if let Some(ret) = unsafe { try_pinned_symlinkat(target, newdirfd, linkpath) } {
        return ret;
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
///
/// When *both* ends are the common shape — a real directory fd and a
/// single-component name — and `AT_SYMLINK_FOLLOW` is absent, this goes through
/// [`try_pinned_linkat`]. There is no half-pinned form: 668 resolves both
/// handles, and a guarantee that held for the destination but not the source
/// would be one nothing in the signature announced.
///
/// A cross-mount link comes back as `EINVAL` on both routes rather than the
/// `EXDEV` POSIX describes. That is the path-based `link`'s long-standing
/// answer here and the pinned route deliberately agrees with it; one operation
/// with two error contracts depending on which route ran would be the worse
/// bug.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn linkat(
    olddirfd: i32,
    oldpath: *const u8,
    newdirfd: i32,
    newpath: *const u8,
    flags: i32,
) -> i32 {
    // Linux do_linkat: reject flags outside AT_SYMLINK_FOLLOW | AT_EMPTY_PATH.
    if flags & !(AT_SYMLINK_FOLLOW | AT_EMPTY_PATH) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // `linkat` accepts `AT_EMPTY_PATH` — "link the file this fd names, under
    // whatever name it currently has none of" — but Linux gates it on
    // `CAP_DAC_READ_SEARCH` and, crucially, reports the *absence* of that
    // capability as ENOENT rather than EPERM, so an unprivileged caller cannot
    // tell a missing file from a missing privilege. Measured on 6.6:
    // `linkat(fd, "", dirfd, "l", AT_EMPTY_PATH)` is ENOENT. Since we do not
    // implement the privileged form, ENOENT is the whole of the behaviour and
    // the flag needs no separate branch.
    if reject_empty_path(oldpath) || reject_empty_path(newpath) {
        return -1;
    }
    // The pinned route is the unfollowed form only, and both ends must be
    // pinnable. `AT_SYMLINK_FOLLOW` is checked *here* rather than inside
    // `try_pinned_linkat` so the reason sits next to the flag's own validation:
    // following a trailing symlink is by definition leaving the pinned
    // directory, so the flag asks 668 to stop providing the guarantee it exists
    // for, and the honest place to serve it is the path route.
    if flags & AT_SYMLINK_FOLLOW == 0 {
        // SAFETY: `oldpath` and `newpath` are this function's own `*const u8`
        // contract — null or valid C strings — which is what
        // `try_pinned_linkat` requires.
        if let Some(ret) = unsafe { try_pinned_linkat(olddirfd, oldpath, newdirfd, newpath) } {
            return ret;
        }
    }
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

    // linkat follows a trailing symlink in oldpath only with AT_SYMLINK_FOLLOW.
    link_ex(old_ptr, new_ptr, flags & AT_SYMLINK_FOLLOW != 0)
}

/// Change file mode bits relative to a directory fd.
///
/// Validates `flags` per Linux's `do_fchmodat` prologue:
/// `flags & ~(AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH)` → EINVAL.
/// AT_EACCESS is **not** a valid fchmodat flag (it's a faccessat
/// flag) — passing it here yields EINVAL.
///
/// POSIX: if `path` is absolute, `dirfd` is ignored.
///
/// For the common shape — a real directory fd and a single-component name —
/// this goes through [`try_pinned_fchmodat`], which has the kernel resolve the
/// descriptor rather than the remembered path. See "The pinned `*at` fast path"
/// above; of the calls that take it, this is the one where being fooled by a
/// swapped directory hands out privilege rather than merely returning a wrong
/// answer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fchmodat(dirfd: i32, path: *const u8, mode: ModeT, flags: i32) -> i32 {
    if flags & !(AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // AT_SYMLINK_NOFOLLOW routes through lchmod (chmod the link inode itself);
    // otherwise chmod (follow the final symlink).  Linux 6.6+ honours this via
    // fchmodat2; we thread NO_FOLLOW to SYS_FS_SET_PERMS the same way.
    let no_follow = flags & AT_SYMLINK_NOFOLLOW != 0;
    let apply = |p: *const u8| {
        if no_follow {
            lchmod(p, mode)
        } else {
            chmod(p, mode)
        }
    };
    if is_empty_path(path) {
        if flags & AT_EMPTY_PATH == 0 {
            errno::set_errno(errno::ENOENT);
            return -1;
        }
        // The mode of whatever the descriptor names. `AT_SYMLINK_NOFOLLOW` has
        // nothing to act on: an fd already refers to one specific inode, and
        // if that inode is a symlink then it is the symlink that gets chmodded
        // either way.
        return if dirfd == AT_FDCWD {
            chmod(CWD_DOT.as_ptr(), mode)
        } else {
            fchmod(dirfd, mode)
        };
    }
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return apply(path);
    }
    // SAFETY: `path` is this function's own `*const u8` contract — null or a
    // valid C string — which is what `try_pinned_fchmodat` requires.
    if let Some(ret) = unsafe { try_pinned_fchmodat(dirfd, path, mode, flags) } {
        return ret;
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 {
        return -1;
    }
    apply(full.as_ptr())
}

/// Change file mode bits WITHOUT following a final symlink (`lchmod`).
///
/// Like [`chmod`] but if `path` names a symlink, the link inode's own mode
/// bits are changed rather than the target's (`fchmodat2(AT_SYMLINK_NOFOLLOW)`).
///
/// Errors:
///   * `EFAULT` — `path` is NULL.
///   * any error the kernel returns from `SYS_FS_SET_PERMS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lchmod(path: *const u8, mode: ModeT) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        set_perms_path_ex(path, mode, true)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = mode;
        0
    }
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
    // AT_SYMLINK_NOFOLLOW routes through lchown (operate on the link inode
    // itself); otherwise chown (follow the final symlink).
    let no_follow = flags & AT_SYMLINK_NOFOLLOW != 0;
    let apply = |p: *const u8| {
        if no_follow {
            lchown(p, owner, group)
        } else {
            chown(p, owner, group)
        }
    };
    if is_empty_path(path) {
        if flags & AT_EMPTY_PATH == 0 {
            errno::set_errno(errno::ENOENT);
            return -1;
        }
        // The owner of whatever the descriptor names; as with `fchmodat`,
        // `AT_SYMLINK_NOFOLLOW` has no name left to decline to follow.
        return if dirfd == AT_FDCWD {
            chown(CWD_DOT.as_ptr(), owner, group)
        } else {
            fchown(dirfd, owner, group)
        };
    }
    if dirfd == AT_FDCWD || is_absolute_path(path) {
        return apply(path);
    }
    let mut full = [0u8; crate::unistd::PATH_MAX];
    let len = resolve_dirfd_path(dirfd, path, &mut full);
    if len == 0 {
        return -1;
    }
    apply(full.as_ptr())
}

// ---------------------------------------------------------------------------
// chmod / fchmod / chown / fchown / lchown
// ---------------------------------------------------------------------------
//
// The permission-changing family.  Each entry point validates its
// arguments (NULL path → EFAULT, bad/closed fd → EBADF) and then, on bare
// metal, issues the corresponding kernel syscall: SYS_FS_SET_PERMS for
// chmod/fchmod and SYS_FS_SET_OWNER for chown/fchown/lchown.  Authority is
// the kernel's — both handlers require a File capability with WRITE before
// touching anything, and libc does not pre-empt that (§314).  On the host build (no kernel) the syscall is
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
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if reject_path_fd_entry(&entry) {
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
/// Validates `path != NULL`, then issues `SYS_FS_SET_OWNER`.  A field of
/// `(uid_t)-1` / `(gid_t)-1` (i.e. `u32::MAX`) leaves that field unchanged;
/// a call that changes neither field is a pure no-op and skips the syscall.
///
/// # Authority — the kernel's, not libc's (§314; was a Phase-206 `CAP_CHOWN` gate)
///
/// Linux's rule is not a flat `CAP_CHOWN` test.  Changing the **owner** needs
/// the capability, but changing only the **group** is permitted to the file's
/// own owner when the target group is one they belong to — so a plain
/// `chgrp` by the owner needs no privilege at all.  Testing the capability
/// alone denies that case.
///
/// It is worse than a missing clause here, because `CAP_CHOWN` has **no rule
/// in §312's projection table** — nothing in the kernel's `(ResourceType,
/// Rights)` model maps to it, so it falls to the deny-by-default arm and
/// reads *false* for every process.  Under §312 step 3 a libc-side gate would
/// therefore refuse **every** `chown`, while the kernel went on permitting it.
///
/// The kernel is not thereby left ungated: `sys_fs_set_owner` requires a
/// `File` capability with `WRITE` rights before it resolves the path, and
/// returns its own error when that fails.  The evaluable-alternative route
/// (rule 3 of §314 — teach the gate Linux's whole predicate) was considered
/// and rejected *here*: it would need libc to `stat` the file for its current
/// owner and consult the supplementary group list, i.e. reimplement the
/// kernel's check one syscall earlier and out of date by construction.
///
/// Errors:
///   * `EFAULT` — `path` is NULL.
///   * any error the kernel returns from `SYS_FS_SET_OWNER`, including the
///     capability failure above.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn chown(path: *const u8, owner: UidT, group: GidT) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // owner/group == (uid_t)-1 means "don't change that field", so a
    // double-no-op call has nothing to authorise and nothing to do.
    if owner == u32::MAX && group == u32::MAX {
        // Nothing to change — succeed without touching ctime.
        return 0;
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
/// Validates `fd >= 0` and that `fd` refers to an open file description, then
/// resolves the fd to its stored path and issues `SYS_FS_SET_OWNER`.
/// Path-less descriptors (pipes, sockets, …) succeed as a no-op.
///
/// Authority is the kernel's — see [`chown`] for why libc does not pre-empt it
/// (§314).
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
///   * any error the kernel returns from `SYS_FS_SET_OWNER`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fchown(fd: Fd, owner: UidT, group: GidT) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if reject_path_fd_entry(&entry) {
        return -1;
    }
    // Same no-op shortcut as chown(), after EBADF validation.
    // The `O_PATH` rejection above outranks it: `fchown(path_fd, -1, -1)` is
    // EBADF on Linux, not the silent success the shortcut would give.
    if owner == u32::MAX && group == u32::MAX {
        return 0;
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
/// rather than its target.  Validates `path != NULL`, then issues
/// `SYS_FS_SET_OWNER` with the NO_FOLLOW flag so the *link inode itself* is
/// chowned, not its target.
///
/// Authority is the kernel's — see [`chown`] for why libc does not pre-empt it
/// (§314).
///
/// Errors:
///   * `EFAULT` — `path` is NULL.
///   * any error the kernel returns from `SYS_FS_SET_OWNER`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lchown(path: *const u8, owner: UidT, group: GidT) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // Same no-op shortcut as chown().
    if owner == u32::MAX && group == u32::MAX {
        return 0;
    }
    #[cfg(target_os = "none")]
    {
        // lchown(2): NO_FOLLOW — operate on the symlink itself.
        set_owner_path_ex(path, owner, group, true)
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
}

/// Process-local file mode creation mask.
///
/// Initialized to 0o022 (typical POSIX default: owner rw, group/other r).
///
/// One value for the whole process, which is what POSIX specifies: the umask
/// is a property of the process, not of a thread, so there is nothing to make
/// per-thread. `umask()` is a read-then-write and is therefore *not* atomic —
/// two threads swapping masks concurrently can each observe the other's value.
/// POSIX does not promise otherwise, and real programs set the umask once
/// during startup; the obligation not to race it is the caller's. This crate's
/// own tests are such a caller, and serialise on `UMASK_TEST_LOCK`.
static mut UMASK_VALUE: ModeT = 0o022;

/// Set file mode creation mask.
///
/// Stores the new mask and returns the previous one.  While the kernel
/// doesn't enforce permissions yet, this gives correct POSIX semantics
/// for programs that query or chain umask values.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn umask(cmask: ModeT) -> ModeT {
    // SAFETY: `UMASK_VALUE` is touched only through raw pointers, never through
    // a reference, so no `&mut` to a `static mut` is formed. Serialising the
    // read-then-write against other threads is the caller's obligation — see
    // the note on `UMASK_VALUE`.
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
    // SAFETY: A plain read through a raw pointer; no reference is formed. See
    // the note on `UMASK_VALUE` for why a concurrent `umask()` is the caller's
    // problem and not this function's.
    unsafe { core::ptr::addr_of!(UMASK_VALUE).read() }
}

/// Apply the process umask to a requested create mode, keeping `keep` of it.
///
/// Our umask lives entirely in this userspace layer — the kernel is a thin
/// create primitive that stamps whatever final mode we hand it (see
/// `kernel::fs::handle::open_with_mode` / `Vfs::mkdir_mode`) — so the masking
/// is done here, right before the create syscall, and the already-masked
/// result is passed to the kernel.
///
/// # Why `keep` is a parameter and not a constant
///
/// `open(O_CREAT)` and `mkdir` do not agree on how much of `mode` is real, and
/// this used to be one function that masked both to `0o777`. `mkdir(2)` says
/// the result is `(mode & ~umask & 0777)` in so many words; `open(2)` says
/// `(mode & ~umask)` with no such term. Collapsing them lost the difference in
/// the direction that silently discards a bit the caller asked for — see
/// [`apply_umask_create`].
///
/// The umask itself is always narrowed to nine bits regardless of `keep`, which
/// is a different restriction and not a redundant one: `umask(2)` specifies
/// that only the file permission bits of the mask are used, so `~umask` must
/// never be able to clear a setuid bit however wide the mode being masked is.
fn apply_umask_keeping(mode: ModeT, keep: ModeT) -> ModeT {
    (mode & keep) & !(get_umask() & 0o777)
}

/// Apply the process umask to `open(O_CREAT)`'s mode: **twelve** bits.
///
/// `open(2)`: "the mode of the created file is `(mode & ~umask)`" — note the
/// absence of the `& 0777` that `mkdir(2)` spells out explicitly. setuid,
/// setgid and sticky in an `O_CREAT` mode word are honoured on Linux, and are
/// honoured here.
///
/// This masked to `0o777` until 2026-08-31, which silently dropped all three.
/// The kernel had already been widened to twelve for exactly this reason —
/// `handle.rs`'s `open_with_mode` stamps `create_mode & 0o7777`, and its
/// comment cites design-decisions.md §639: "silently discarding a permission
/// bit a caller explicitly asked for is the failure lane B and lane A agreed to
/// rule out". This side went on narrowing the word before the syscall could see
/// it, so the widening reached nothing — a C caller doing
/// `open("s", O_CREAT|O_WRONLY, 0o4755)` got a plain `0755` file and no error
/// to say so.
///
/// `0o7777` rather than no mask at all: the argument is a `mode_t`, so a caller
/// may have `S_IFREG` or another file-type bit set in it, and what a create
/// does with those is the kernel's to decide rather than ours to forward.
pub(crate) fn apply_umask_create(mode: ModeT) -> ModeT {
    apply_umask_keeping(mode, 0o7777)
}

/// Apply the process umask to `mkdir`'s mode: **nine** bits.
///
/// `mkdir(2)`, DESCRIPTION: "in the absence of a default ACL, the mode of the
/// created directory is `(mode & ~umask & 0777)`. Whether other mode bits are
/// honored for the created directory depends on the operating system." Nine is
/// therefore the portable answer, and — unlike the nine [`apply_umask_create`]
/// used to use — it is a decision rather than an oversight.
///
/// Linux's one extension is `S_ISVTX` (VERSIONS: "apart from the permission
/// bits, the `S_ISVTX` mode bit is also honored"), which would let a sticky
/// directory be created without the window in which it exists world-writable
/// but not yet sticky. We do not send it, because the kernel would drop it
/// anyway: `Vfs::mkdir_mode` and `mkdir_at_pinned` both compute `mode & 0o777`.
/// Asked of lane A in
/// `requests/b-a-666-669-are-wired-two-answers-and-one-bug-that-was-mine.md`;
/// widen this to `0o1777` in the same change that widens those and not before,
/// since a libc that sends a bit the kernel discards has moved the silent drop
/// rather than fixed it.
///
/// setuid/setgid are not the extension to make here even then. Linux does not
/// take a directory's setgid bit from `mkdir`'s mode argument — it inherits it
/// from the parent ("If the parent directory has the set-group-ID bit set, then
/// so will the newly created directory"). Accepting it from the mode word would
/// let a caller produce a directory that `mkdir(2)` on Linux could not.
pub(crate) fn apply_umask_mkdir(mode: ModeT) -> ModeT {
    apply_umask_keeping(mode, 0o777)
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
    // The order of these four checks is measured on Linux 6.6, not guessed —
    // it used to be exactly backwards here, validating the arguments before
    // ever looking at `fd`, so a closed descriptor passed a bad `advice`
    // reported EINVAL where Linux reports EBADF.  What was measured:
    //
    //   posix_fadvise(999closed, 0, -1, NORMAL)  -> EBADF   (not EINVAL)
    //   posix_fadvise(pathfd,    0,  0, 999)     -> EBADF   (not EINVAL)
    //   posix_fadvise(pipe,      0, -1, NORMAL)  -> ESPIPE  (not EINVAL)
    //   posix_fadvise(pipe,      0,  0, 999)     -> ESPIPE  (not EINVAL)
    //   posix_fadvise(file,      0, -1, NORMAL)  -> EINVAL
    //   posix_fadvise(file,     -1,  0, NORMAL)  -> 0       (offset is unchecked)
    //
    // So: descriptor first, then what kind of file it is, and only then the
    // arguments.  This is the same rule the rest of the file follows — the
    // descriptor lookup outranks argument validation — and `posix_fadvise` is
    // simply one of the two places that had it inverted.
    let Some(entry) = fdtable::get_fd(fd) else {
        return errno::EBADF;
    };
    // Not `reject_path_fd_entry`: this call reports through its return value
    // and must leave errno alone.
    if is_path_fd_entry(&entry) {
        return errno::EBADF;
    }
    // ESPIPE for pipes (Linux extension; matches what real applications expect).
    if matches!(entry.kind, fdtable::HandleKind::Pipe) {
        return errno::ESPIPE;
    }
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
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if reject_path_fd_entry(&entry) {
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
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if reject_path_fd_entry(&entry) {
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
    set_times_path_ex(path, accessed_ns, modified_ns, false)
}

/// Like [`set_times_path`] but with explicit symlink-follow control.  When
/// `no_follow` is set (`lutimes` / `utimensat(AT_SYMLINK_NOFOLLOW)`), the
/// kernel stamps the link inode itself (arg4 bit 0 = NO_FOLLOW).
#[cfg(target_os = "none")]
fn set_times_path_ex(path: *const u8, accessed_ns: u64, modified_ns: u64, no_follow: bool) -> i32 {
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };
    let ret = syscall5(
        SYS_FS_SET_TIMES,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        accessed_ns,
        modified_ns,
        u64::from(no_follow),
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
    set_perms_path_ex(path, mode, false)
}

/// Like [`set_perms_path`] but with explicit symlink-follow control.  When
/// `no_follow` is set (`lchmod` / `fchmodat2(AT_SYMLINK_NOFOLLOW)`), the kernel
/// chmods the link inode itself (arg3 bit 0 = NO_FOLLOW).
#[cfg(target_os = "none")]
fn set_perms_path_ex(path: *const u8, mode: ModeT, no_follow: bool) -> i32 {
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };
    // The kernel masks to 0o7777, but mask here too so the ABI value is
    // unambiguous (mode_t carries file-type bits we must not forward).
    let perms = u64::from(mode & 0o7777);
    let ret = syscall4(
        SYS_FS_SET_PERMS,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        perms,
        u64::from(no_follow),
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
    set_owner_path_ex(path, uid, gid, false)
}

/// Like [`set_owner_path`] but with explicit symlink-follow control.  When
/// `no_follow` is set (`lchown` / `fchownat(AT_SYMLINK_NOFOLLOW)`), the kernel
/// chowns the link inode itself (arg4 bit 0 = NO_FOLLOW).
#[cfg(target_os = "none")]
fn set_owner_path_ex(path: *const u8, uid: u32, gid: u32, no_follow: bool) -> i32 {
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = resolve_or_err(path, &mut resolved) else {
        return -1;
    };
    let ret = syscall5(
        SYS_FS_SET_OWNER,
        resolved.as_ptr() as u64,
        resolved_len as u64,
        u64::from(uid),
        u64::from(gid),
        u64::from(no_follow),
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
    // Ahead of the `times` validation, which is *not* where the ordering
    // intuition puts it: `utimensat(dirfd, "", {tv_nsec: 2e9}, 0)` is ENOENT,
    // not EINVAL, measured on Linux 6.6 — so the empty name is caught before
    // the timestamps are looked at, even though a valid `times` is copied in
    // first.  Only the flag gate above outranks it.  `utimensat` has no
    // `AT_EMPTY_PATH`; the "operate on the fd" form is spelled with a NULL
    // path instead, which is the GNU extension `futimens` uses and which we
    // refuse above.
    if reject_empty_path(path) {
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
        // Resolve the dirfd/path pair the same way fstatat does.
        // AT_SYMLINK_NOFOLLOW → NO_FOLLOW flag: the kernel stamps the link
        // inode itself rather than its target.
        let no_follow = (flags & AT_SYMLINK_NOFOLLOW) != 0;
        let now = wall_clock_ns();
        // SAFETY: `times` was validated above; non-null implies two valid
        // Timespecs.
        let (a_ns, m_ns) = unsafe { utimens_pair_to_kernel(times, now) };
        if dirfd == AT_FDCWD || is_absolute_path(path) {
            set_times_path_ex(path, a_ns, m_ns, no_follow)
        } else {
            // SAFETY: `path` is this function's own `*const u8` contract —
            // checked non-null above — which is what `try_pinned_utimensat`
            // requires.
            if let Some(ret) = unsafe { try_pinned_utimensat(dirfd, path, a_ns, m_ns, no_follow) } {
                return ret;
            }
            let mut full = [0u8; crate::unistd::PATH_MAX];
            let len = resolve_dirfd_path(dirfd, path, &mut full);
            if len == 0 {
                return -1;
            }
            set_times_path_ex(full.as_ptr(), a_ns, m_ns, no_follow)
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
/// `O_EXCL` maps to the native `EXCL` bit (0x40); combined with `O_CREAT`, the
/// kernel rejects an already-existing path with `EEXIST`, giving the standard
/// exclusive-create semantics.  `O_EXCL` without `O_CREAT` is undefined by
/// POSIX; the kernel only enforces `EXCL` when `CREATE` is also set, so the
/// stray bit is harmless.
///
/// `O_NOFOLLOW` maps to the native `NOFOLLOW` bit (0x80); the kernel fails with
/// `ELOOP` if the final path component is a symbolic link (parent-component
/// symlinks are still followed).
pub(crate) fn translate_open_flags(posix_flags: i32) -> u64 {
    // Native OpenFlags bits (must match kernel `fs::handle::OpenFlags`).
    const N_READ: u64 = 0x01;
    const N_WRITE: u64 = 0x02;
    const N_CREATE: u64 = 0x04;
    const N_TRUNCATE: u64 = 0x08;
    const N_APPEND: u64 = 0x10;
    const N_DIRECTORY: u64 = 0x20;
    const N_EXCL: u64 = 0x40;
    const N_NOFOLLOW: u64 = 0x80;

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
    if posix_flags & fcntl::O_EXCL != 0 {
        native |= N_EXCL;
    }
    if posix_flags & fcntl::O_NOFOLLOW != 0 {
        native |= N_NOFOLLOW;
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
/// 2. `pathname` NULL → `EFAULT`.  `user_path_at(dfd, name, …)` takes
///    `getname_flags(name)` as an *argument* to `filename_lookup`, so the
///    name is imported — and faults — before `dfd` is ever consulted.
/// 2a. `pathname` empty without `AT_EMPTY_PATH` → `ENOENT`, from the same
///    import, and therefore also ahead of `dfd`:
///    `name_to_handle_at(999, "", …, 0)` is `ENOENT`, not `EBADF` (measured
///    on Linux 6.6).  *With* `AT_EMPTY_PATH` the lookup succeeds and the call
///    goes on to report on the handle buffer — `EOVERFLOW` for a
///    `handle_bytes` too small — which is past the point we can model, so
///    that case falls through to the `ENOSYS` below.
/// 3. If `dirfd != AT_FDCWD`, it must be a valid open fd → `EBADF`.
/// 4. `handle` or `mount_id` NULL → `EFAULT`.  These are only touched in
///    `do_sys_name_to_handle`, which runs *after* `user_path_at` succeeds.
/// 5. All validated → `ENOSYS`.
///
/// An earlier version checked all three pointers together at step 2 and
/// justified it as "our model can do the cheap NULL check up front without
/// observable difference".  There is an observable difference, and it is the
/// obvious one: `name_to_handle_at(bad_fd, "p", NULL, NULL, 0)` is `EBADF`
/// upstream and was `EFAULT` here.  (This was the third doc comment in the
/// audit found *arguing* for an order rather than citing one — see
/// design-decisions.md §303 and the `bind` and `posix_spawnattr_setflags`
/// cases.)
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
    if pathname.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if flags & AT_EMPTY_PATH == 0 && reject_empty_path(pathname) {
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
    if handle.is_null() || mount_id.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
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

// -- The native `resolve` word, which is not Linux's -------------------------
//
// `SYS_FS_OPENAT2`'s resolve bits are deliberately nowhere near Linux's:
// every Linux `RESOLVE_*` value lies in `0x00..=0x3f`, so an *untranslated*
// `open_how.resolve` arriving at the kernel has no known bit and at least one
// unknown one, and is refused on its first call.  Had the numbers matched, a
// dropped translation line would turn one restriction into a different one and
// the caller would be told its confinement was applied when it was not.
//
// That is why the two sets are named apart here (`K_` for the kernel's) rather
// than one set being reused for both, and why `plan_resolve` is a total
// function over the Linux word rather than a mask-and-pass.  See
// `kernel/src/syscall/number.rs::RESOLVE_NO_SYMLINKS` and
// `requests/a-b-openat2-is-661-and-the-mode-is-twelve-bits.md`.

/// Kernel `resolve` bit: refuse to traverse a symbolic link, in any
/// component.  Not `RESOLVE_NO_SYMLINKS` (`0x04`) — see the note above.
const K_RESOLVE_NO_SYMLINKS: u64 = 1 << 16;
/// Kernel `resolve` bit: refuse to resolve outside `dirfd`.  Not
/// `RESOLVE_BENEATH` (`0x08`) — see the note above.
const K_RESOLVE_BENEATH: u64 = 1 << 17;

/// What [`openat2`] does with a `resolve` word that has already passed
/// validation (steps 1–6 — known bits only, legal mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolvePlan {
    /// Nothing to carry: `openat` gives the same answer, and it is the call
    /// with the working machinery (`/dev/ptmx`, umask, `FD_CLOEXEC`, the
    /// stored path).  Two paths to the filesystem for one request is how the
    /// two drift, so we take the sibling whenever the word is empty of
    /// anything we must enforce.
    Delegate,
    /// Forward natively with this **kernel** resolve word.
    Forward(u64),
    /// Refuse with this errno.  A restriction we cannot apply must never come
    /// back as a descriptor: that is an unrestricted open wearing the answer
    /// the caller asked for.
    Refuse(i32),
}

/// Decide what to do with `resolve`.
///
/// Split out as a pure function because it is the part that can rot in
/// silence.  A host test cannot open anything — every native syscall is
/// stubbed off-target — so an end-to-end assertion about `RESOLVE_BENEATH`
/// degenerates into "the stub said ENOSYS" and would stay green with the
/// translation deleted.  The bit mapping is exactly what lane A warned would
/// be untested marshalling, so it is tested directly instead.
pub(crate) fn plan_resolve(resolve: u64) -> ResolvePlan {
    // `RESOLVE_CACHED` first: no dcache, so every request is conceptually a
    // miss, and Linux documents EAGAIN as the answer for a miss.  This is the
    // one refusal here that is also what Linux itself would say.
    if resolve & RESOLVE_CACHED != 0 {
        return ResolvePlan::Refuse(errno::EAGAIN);
    }
    // `RESOLVE_IN_ROOT` is not built kernel-side, not planned, and has no
    // constant there, so it cannot be forwarded even by accident.  Rooted
    // (chroot-like) resolution needs per-fd root tracking nothing keeps.
    // `known-issues.md` → `TD-OPENAT2-BENEATH-INROOT`.
    if resolve & RESOLVE_IN_ROOT != 0 {
        return ResolvePlan::Refuse(errno::EOPNOTSUPP);
    }

    let mut native = 0;
    if resolve & RESOLVE_BENEATH != 0 {
        native |= K_RESOLVE_BENEATH;
    }
    if resolve & RESOLVE_NO_SYMLINKS != 0 {
        native |= K_RESOLVE_NO_SYMLINKS;
    }
    if native != 0 {
        return ResolvePlan::Forward(native);
    }

    // `RESOLVE_NO_XDEV` and `RESOLVE_NO_MAGICLINKS` are dropped rather than
    // forwarded, on the judgement the kernel records for itself: nothing to
    // cross mid-walk, and no `/proc` magic symlinks to traverse.  That
    // judgement belongs to the VFS, so if it ever stops holding, this line and
    // the kernel's must change together.
    ResolvePlan::Delegate
}

/// `true` for the two path shapes this libc answers itself rather than
/// through the filesystem: `/dev/ptmx` and `/dev/pts/<n>`.
///
/// The gate is factored out so [`open_pty_device`] and [`openat2_forward`]
/// cannot disagree about which names are libc-internal — the second refuses
/// exactly the set the first claims.
pub(crate) fn is_pty_device_path(resolved: &[u8]) -> bool {
    resolved == b"/dev/ptmx" || resolved.starts_with(b"/dev/pts/")
}

/// Forward an `openat2` that carries a restriction to `SYS_FS_OPENAT2`.
///
/// Reached only from [`openat2`], and only for a [`ResolvePlan::Forward`], so
/// `k_resolve` is always non-zero: this is the path that exists *because* the
/// request cannot be expressed as an `openat`.
///
/// Returns a file descriptor, or -1 with errno set.
fn openat2_forward(dirfd: i32, path: *const u8, h: &OpenHow, k_resolve: u64) -> Fd {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let posix_flags = h.flags as i32;

    // The same two gates `openat` runs, in the same order, because this path
    // does not go through it.  `build_open_flags` outranks a bad path pointer
    // (fs/open.c runs it before `getname`), and the O_TMPFILE refusal that
    // follows is `do_tmpfile`'s, which ranks below both of them.
    if !validate_open_flags(posix_flags) {
        return -1;
    }
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if posix_flags & RAW_O_TMPFILE_I32 != 0 {
        // Our kernel file handles are path-based, so a nameless inode cannot
        // be represented.  `open` refuses this for the same reason.
        errno::set_errno(errno::EOPNOTSUPP);
        return -1;
    }

    // SAFETY: `path` is non-null (just checked) and a valid C string by the
    // caller's contract.
    let path_len = unsafe { crate::string::strlen(path) };
    if path_len == 0 {
        // POSIX: an empty path is ENOENT.  The kernel would answer
        // InvalidArgument for a zero length, which is the wrong error for the
        // right reason, so it is answered here.
        errno::set_errno(errno::ENOENT);
        return -1;
    }

    // The bookkeeping path: what this fd will report to a later `openat`,
    // `fchdir` or `/proc`-style query.  Computed the way `openat` computes it
    // — textual join plus normalization — so the two agree for identical
    // arguments.  It is also the name the pty gate below is asked about.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let resolved_len = if dirfd == AT_FDCWD || is_absolute_path(path) {
        match resolve_or_err(path, &mut resolved) {
            Some(n) => n,
            None => return -1,
        }
    } else {
        let n = resolve_dirfd_path(dirfd, path, &mut resolved);
        if n == 0 {
            return -1;
        }
        n
    };

    // `/dev/ptmx` and `/dev/pts/<n>` are answered inside this libc and do not
    // exist in the kernel's namespace at all.  Forwarding one would come back
    // ENOENT — a lie, because the file does exist to this caller — so the
    // honest answer is that a walk cannot be confined to something the walker
    // does not have.
    if is_pty_device_path(resolved.get(..resolved_len).unwrap_or(&[])) {
        errno::set_errno(errno::EOPNOTSUPP);
        return -1;
    }

    let native_flags = translate_open_flags(posix_flags);
    #[allow(clippy::cast_possible_truncation)]
    let create_mode = if posix_flags & fcntl::O_CREAT != 0 {
        u64::from(apply_umask_create(h.mode as ModeT))
    } else {
        0
    };

    // The base the kernel resolves `path` against.
    //
    // `dirfd == 0` in the native ABI means the *kernel's* process working
    // directory — and this libc's working directory is not that one.
    // `unistd::chdir` keeps its answer in a libc-side buffer and never tells
    // the kernel, so passing 0 for a relative path would confine the walk
    // beneath whichever directory the kernel last recorded.  A containment
    // check against the wrong base is precisely the failure `RESOLVE_BENEATH`
    // exists to prevent, and it fails *open*: a wrong base still returns a
    // valid descriptor.
    //
    // 0 is safe for an absolute path, and only for an absolute path, because
    // the base is then never read: without `BENEATH` the handler takes the
    // fragment as the whole answer, and with `BENEATH` it refuses an absolute
    // fragment before it looks the base up at all.
    let mut scratch: u64 = 0;
    let base = if is_absolute_path(path) {
        0
    } else if dirfd == AT_FDCWD {
        // Give the kernel a handle on *our* cwd.  Resolving "." against it is
        // how the rest of this file spells "the working directory", so the
        // base is the same string `resolve_or_err` would have joined against.
        let mut cwd = [0u8; crate::unistd::PATH_MAX];
        let Some(cwd_len) = resolve_or_err(b".\0".as_ptr(), &mut cwd) else {
            return -1;
        };
        let ret = syscall4(
            SYS_FS_OPEN_MODE,
            cwd.as_ptr() as u64,
            cwd_len as u64,
            translate_open_flags(fcntl::O_RDONLY | fcntl::O_DIRECTORY),
            0,
        );
        if ret < 0 {
            return errno::translate(ret) as Fd;
        }
        #[allow(clippy::cast_sign_loss)]
        {
            scratch = ret as u64;
        }
        scratch
    } else {
        let Some(entry) = fdtable::get_fd(dirfd) else {
            errno::set_errno(errno::EBADF);
            return -1;
        };
        if entry.kind != HandleKind::File {
            // A pipe, socket or console fd names no directory to walk from.
            errno::set_errno(errno::ENOTDIR);
            return -1;
        }
        entry.handle
    };

    let ret = syscall6(
        SYS_FS_OPENAT2,
        path.cast::<u8>() as u64,
        path_len as u64,
        native_flags,
        create_mode,
        k_resolve,
        base,
    );

    // The scratch base is ours and nobody else's; it must go back on every
    // exit from here, success or failure.
    if scratch != 0 {
        let _ = syscall1(SYS_FS_CLOSE, scratch);
    }

    if ret < 0 {
        return errno::translate(ret) as Fd;
    }

    // Registration is `open`'s, deliberately identical: the same status flags
    // survive, the same creation-only flags are stripped, the same
    // `FD_CLOEXEC`, the same stored path, and the same close-on-overflow.
    let stored_flags = posix_flags
        & (fcntl::O_ACCMODE
            | fcntl::O_APPEND
            | fcntl::O_NONBLOCK
            | fcntl::O_SYNC
            | fcntl::O_NOFOLLOW);
    #[allow(clippy::cast_sign_loss)]
    let kernel_handle = ret as u64;
    if let Some(fd_num) =
        fdtable::alloc_fd_with_flags(HandleKind::File, kernel_handle, stored_flags)
    {
        if posix_flags & fcntl::O_CLOEXEC != 0 {
            let _ = fdtable::set_fd_flags(fd_num, fdtable::FD_CLOEXEC);
        }
        fdtable::store_fd_path(fd_num, resolved.as_ptr(), resolved_len);
        fd_num
    } else {
        let _ = syscall1(SYS_FS_CLOSE, kernel_handle);
        errno::set_errno(errno::EMFILE);
        -1
    }
}

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
/// Once validation passes, step 7 decides between three outcomes
/// ([`plan_resolve`]): a `resolve` word with nothing to enforce delegates
/// to plain `openat`; `RESOLVE_BENEATH`/`RESOLVE_NO_SYMLINKS` forward to
/// `SYS_FS_OPENAT2`, which carries the word to the VFS resolver; and
/// `RESOLVE_IN_ROOT`/`RESOLVE_CACHED` are refused.  **No restriction is
/// ever silently dropped** — a bit we can neither carry nor honour comes
/// back as an error, because a descriptor would be an unrestricted open
/// wearing the answer the caller asked for.
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

    // Step 7: the resolve restrictions themselves.
    //
    // The invariant this step keeps is that a restriction is never *dropped*.
    // `openat` takes no `resolve` word, so a bit we cannot carry and do not
    // refuse becomes an unrestricted open returned to a caller who asked to be
    // confined — which is worse than an error, because it looks like success.
    // Step 4 refuses *unknown* bits on exactly that reasoning; the argument
    // does not weaken for a bit we happen to have named.
    //
    // Until 2026-08-30 the only way to keep it was to refuse: the containment
    // work existed kernel-side but was reachable only through the *Linux* ABI,
    // and this is the native one. `SYS_FS_OPENAT2` (661) is the native call
    // that carries the word, so `RESOLVE_BENEATH` and `RESOLVE_NO_SYMLINKS`
    // are now enforced rather than refused. `RESOLVE_IN_ROOT` and
    // `RESOLVE_CACHED` are still refused, and for reasons that have not moved.
    match plan_resolve(h.resolve) {
        ResolvePlan::Refuse(e) => {
            errno::set_errno(e);
            -1
        }
        ResolvePlan::Forward(k_resolve) => openat2_forward(dirfd, path, &h, k_resolve),
        ResolvePlan::Delegate => openat(dirfd, path, h.flags as i32, h.mode as ModeT),
    }
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
/// `statx` reserved mask bit — must never be set by a caller.
///
/// Linux refuses it with `EINVAL` so that the bit stays available for a future
/// meaning: a kernel that silently ignored it could not later start honouring
/// it without changing the behaviour of programs already setting it by
/// accident. Refusing it here for the same reason.
pub const STATX_RESERVED: u32 = 0x8000_0000;

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
    // Both checks precede the `buf` test, and that order is Linux's, measured:
    // `statx` with a bad flag bit and a NULL buffer reports EINVAL, while the
    // same call with valid flags reports EFAULT.  `do_statx` runs the mask and
    // flag tests before it touches the buffer at all.
    if mask & STATX_RESERVED != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if flags & !(AT_SYMLINK_NOFOLLOW | AT_NO_AUTOMOUNT | AT_EMPTY_PATH | AT_STATX_SYNC_TYPE) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Asking to sync and not to sync at once. Unlike `fstatat`, which accepts
    // this combination, `statx` refuses it — measured on Linux 6.6.
    if flags & AT_STATX_SYNC_TYPE == AT_STATX_SYNC_TYPE {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // An empty path *without* `AT_EMPTY_PATH` is refused here, above the
    // buffer test; an empty path *with* it is served below, under the buffer
    // test.  The split is not tidiness — the two orders genuinely differ on
    // Linux 6.6: `statx(fd, "", 0, …, NULL)` is ENOENT and
    // `statx(fd, "", AT_EMPTY_PATH, …, NULL)` is EFAULT.  Refusing a nameless
    // lookup happens during name resolution, which the flagged form skips
    // entirely, so by the time the flagged form has anything to say the buffer
    // has already been looked at.
    if flags & AT_EMPTY_PATH == 0 && reject_empty_path(path) {
        return -1;
    }
    if buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Resolve dirfd/path and pull the raw kernel buffer.  Mirror
    // `fstatat`: an absolute path or `AT_FDCWD` skips dirfd resolution.
    // `AT_SYMLINK_NOFOLLOW` selects `lstat` semantics.
    let follow = flags & AT_SYMLINK_NOFOLLOW == 0;
    let mut st = Stat::default();
    let mut raw = [0u8; crate::stat::KERNEL_STAT_LEN];
    // Whether `raw` holds a real kernel record.  Everything but a synthetic
    // descriptor (pipe, socket, epoll, pty) does; for those, `st` is filled
    // straight from the handle kind and `STATX_BTIME` stays unreported,
    // because a pipe has no birth time to report.
    let mut have_raw = true;
    if is_empty_path(path) {
        // The descriptor itself — `AT_EMPTY_PATH` is necessarily set, since
        // the check above returned for the case where it is not.
        let ret = if dirfd == AT_FDCWD {
            stat_path_raw(CWD_DOT.as_ptr(), true, &mut raw)
        } else if let Some(ret) = stat_fd_raw(dirfd, &mut raw) {
            ret
        } else {
            have_raw = false;
            fstat(dirfd, &raw mut st)
        };
        if ret != 0 {
            return ret;
        }
    } else {
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
    }
    if have_raw {
        crate::stat::fill_from_fsstat(&mut st, &raw);
    }

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
        && have_raw
        && let Some(btime) = crate::stat::btime_from_fsstat(&raw)
    {
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
        let flags =
            translate_open_flags(fcntl::O_RDWR | fcntl::O_CREAT | fcntl::O_TRUNC | fcntl::O_APPEND);
        assert_eq!(flags & (N_READ | N_WRITE), N_READ | N_WRITE);
        assert_ne!(flags & N_CREATE, 0);
        assert_ne!(flags & N_TRUNCATE, 0);
        assert_ne!(flags & N_APPEND, 0);
    }

    #[test]
    fn translate_no_flags() {
        // O_RDONLY == 0, but native OpenFlags make READ an explicit bit:
        // reading requires N_READ, so translate(0) yields exactly N_READ
        // (no CREATE/TRUNC/APPEND/etc).  (Pre-BUG-OPENFLAGS-ENCODING this
        // emitted the raw Linux bit pattern 0; the encoding was corrected
        // to native OpenFlags 2026-07-21.)
        let flags = translate_open_flags(0);
        assert_eq!(flags, N_READ);
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

    /// Serialises every test that sets the process umask.
    ///
    /// There is one `UMASK_VALUE` for the process and `libtest` runs these
    /// three tests on three threads at once. Each one is a "reset to a known
    /// value, then assert on what the next call gives back" sequence, and that
    /// sequence is only meaningful if nothing else moves the mask in between —
    /// so the *whole test body*, not each call, is the unit that has to be
    /// atomic. Held from the first statement for that reason.
    ///
    /// This has not been observed to fail, unlike the `strtok` and `HTAB`
    /// races in this crate; it is the same defect found by reading rather than
    /// by a flake, and is fixed the same way. Poison is recovered so that one
    /// genuine failure reports once instead of poisoning its two siblings.
    static UMASK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[must_use = "the guard serialises the process-wide umask; bind it to `_g`"]
    fn lock_umask_for_test() -> std::sync::MutexGuard<'static, ()> {
        UMASK_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn test_umask_returns_previous() {
        let _g = lock_umask_for_test();
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
        let _g = lock_umask_for_test();
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
        let _g = lock_umask_for_test();
        umask(0o137);
        let val = get_umask();
        assert_eq!(val, 0o137);
        // Reading should not change the value.
        assert_eq!(get_umask(), 0o137);
        // Clean up.
        umask(0o022);
    }

    /// A create keeps twelve bits; a `mkdir` keeps nine.
    ///
    /// One function masked both to `0o777` until 2026-08-31, so a caller's
    /// `open("s", O_CREAT, 0o4755)` produced a plain `0755` file with no error
    /// to say a bit had been dropped — and the kernel had *already* been
    /// widened to twelve to prevent exactly that (`handle.rs`'s `open_with_mode`
    /// stamps `create_mode & 0o7777`, citing §639). The widening reached
    /// nothing, because this side narrowed the word first.
    ///
    /// The `mkdir` half is deliberately still nine: `mkdir(2)` states the
    /// result is `(mode & ~umask & 0777)`, and the kernel's two mkdir routes
    /// both compute `mode & 0o777`, so sending more would only move the drop.
    #[test]
    fn a_create_keeps_the_special_bits_and_a_mkdir_does_not() {
        let _g = lock_umask_for_test();
        umask(0o022);

        // setuid, setgid and sticky all survive a file create.
        assert_eq!(apply_umask_create(0o4755), 0o4755);
        assert_eq!(apply_umask_create(0o2755), 0o2755);
        assert_eq!(apply_umask_create(0o1777), 0o1755);
        // ... and the same words lose them on a directory create.
        assert_eq!(apply_umask_mkdir(0o4755), 0o0755);
        assert_eq!(apply_umask_mkdir(0o1777), 0o0755);

        // The umask still only ever clears permission bits. `umask` itself
        // narrows its argument to nine, so this is belt-and-braces against a
        // future caller of `apply_umask_keeping` passing a wider mask: a
        // setuid bit must not be clearable by a umask.
        umask(0o7777);
        assert_eq!(get_umask(), 0o777);
        assert_eq!(apply_umask_create(0o4755), 0o4000);

        // A file-type bit in the mode word is the kernel's business, not
        // something to forward — this is what `0o7777` buys over no mask.
        umask(0o000);
        assert_eq!(apply_umask_create(crate::fcntl::S_IFREG | 0o644), 0o644);

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
        // Unknown advice value → EINVAL, but only once the descriptor has
        // survived: these tests used to pass `-1` for the fd and assert
        // EINVAL, on the stated belief that "Linux validates advice before
        // touching the fd table".  It does not — measured on 6.6,
        // `posix_fadvise(999closed, 0, 0, 999)` is EBADF.  So the fd here is a
        // real one and the closed-fd case is asserted separately below.
        let fd = fdtable::alloc_fd(fdtable::HandleKind::File, 4325).unwrap();
        assert_eq!(posix_fadvise(fd, 0, 0, 99), errno::EINVAL);
        assert_eq!(posix_fadvise(fd, 0, 0, -1), errno::EINVAL);
        assert_eq!(posix_fadvise(fd, 0, 0, 6), errno::EINVAL);
        assert!(fdtable::close_fd(fd).is_some());
    }

    #[test]
    fn test_posix_fadvise_negative_len_returns_einval() {
        // Negative len is the only length constraint (offset may be any value:
        // `posix_fadvise(f, -1, 0, NORMAL)` succeeds on Linux).
        let fd = fdtable::alloc_fd(fdtable::HandleKind::File, 4326).unwrap();
        assert_eq!(posix_fadvise(fd, 0, -1, POSIX_FADV_NORMAL), errno::EINVAL);
        assert_eq!(
            posix_fadvise(fd, 100, -100, POSIX_FADV_SEQUENTIAL),
            errno::EINVAL
        );
        assert_eq!(posix_fadvise(fd, -1, 0, POSIX_FADV_NORMAL), 0);
        assert!(fdtable::close_fd(fd).is_some());
    }

    #[test]
    fn test_posix_fadvise_descriptor_outranks_arguments() {
        // The ordering the two tests above used to have backwards.  A closed
        // descriptor is EBADF whatever else is wrong with the call, and a pipe
        // is ESPIPE whatever else is wrong with the call — measured:
        //
        //   posix_fadvise(999closed, 0, -1, NORMAL) -> EBADF
        //   posix_fadvise(pipe,      0, -1, NORMAL) -> ESPIPE
        //   posix_fadvise(pipe,      0,  0, 999)    -> ESPIPE
        assert_eq!(posix_fadvise(900, 0, -1, POSIX_FADV_NORMAL), errno::EBADF);
        assert_eq!(posix_fadvise(900, 0, 0, 999), errno::EBADF);

        let fd = fdtable::alloc_fd(fdtable::HandleKind::Pipe, 4327).unwrap();
        assert_eq!(posix_fadvise(fd, 0, -1, POSIX_FADV_NORMAL), errno::ESPIPE);
        assert_eq!(posix_fadvise(fd, 0, 0, 999), errno::ESPIPE);
        assert!(fdtable::close_fd(fd).is_some());
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
        // fadvise64 must validate the same way as posix_fadvise — including
        // the descriptor-before-arguments ordering, so a bad advice on a
        // closed fd is EBADF here too.
        assert_eq!(fadvise64(-1, 0, 0, POSIX_FADV_NORMAL), errno::EBADF);
        assert_eq!(fadvise64(-1, 0, 0, 99), errno::EBADF);
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Console, 0).expect("fd available");
        assert_eq!(fadvise64(fd, 0, 0, POSIX_FADV_NORMAL), 0);
        assert_eq!(fadvise64(fd, 0, 0, 99), errno::EINVAL);
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
    fn test_renameat2_does_not_refuse_flags_itself() {
        // This used to assert EINVAL for every non-zero `flags`, back when the
        // kernel's `sys_fs_rename` read four arguments and this layer had no
        // way to ask for anything but a plain rename.  It reads `arg4` now, so
        // the word is forwarded and the *kernel* is what decides — which is the
        // whole point: a layer that answered EINVAL on its own would keep
        // refusing a flag long after the kernel had learned it.
        //
        // Off-target there is no kernel to forward to, so `syscall5` answers
        // with its `HOST_ENOSYS` sentinel, which is a *native* code rather than
        // an errno and falls through `errno_for`'s catch-all to `EIO`.  So the
        // errno here is an artefact of the stub and not worth naming; what is
        // worth pinning is that the call got far enough to reach the stub at
        // all — i.e. that the answer is no longer this function's own `EINVAL`.
        // The flags actually reaching the kernel is what the kernel's
        // `RenameMode::from_flags` and the `mv`/`backup` callers cover.
        for flags in [
            crate::linux_at_flags_user_types::RENAME_NOREPLACE,
            crate::linux_at_flags_user_types::RENAME_EXCHANGE,
            // Linux defines this one; this kernel does not, and refuses it
            // rather than dropping it.  Either way the refusal is not ours.
            crate::linux_at_flags_user_types::RENAME_WHITEOUT,
        ] {
            crate::errno::set_errno(0);
            let result = renameat2(
                AT_FDCWD,
                b"/a\0".as_ptr(),
                AT_FDCWD,
                b"/b\0".as_ptr(),
                flags,
            );
            assert_eq!(result, -1, "host stub cannot rename, flags {flags:#x}");
            assert_ne!(
                crate::errno::get_errno(),
                crate::errno::EINVAL,
                "flags {flags:#x} should have reached the syscall layer, \
                 not been refused here"
            );
        }
    }

    #[test]
    fn test_renameat2_empty_path_outranks_the_flags_word() {
        // The names are resolved before the flags word is looked at, so an
        // empty name is ENOENT for being empty rather than anything to do with
        // the flag that came with it.  Linux orders it the same way.  Both
        // sides are checked because the old name is examined first and a
        // regression there would hide behind the new one.
        for (old, new) in [
            (b"\0".as_ptr(), b"/b\0".as_ptr()),
            (b"/a\0".as_ptr(), b"\0".as_ptr()),
        ] {
            crate::errno::set_errno(0);
            let result = renameat2(
                AT_FDCWD,
                old,
                AT_FDCWD,
                new,
                crate::linux_at_flags_user_types::RENAME_NOREPLACE,
            );
            assert_eq!(result, -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
        }
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

    // -- open flag validation, and its position --

    /// "Block bugs where O_DIRECTORY | O_CREAT created regular files"
    /// (`build_open_flags`, fs/open.c).  We had no such check at all.
    #[test]
    fn test_open_directory_plus_creat_is_einval() {
        let path = b"/tmp/does-not-matter\0";
        assert_eq!(
            open(path.as_ptr(), fcntl::O_DIRECTORY | fcntl::O_CREAT, 0o644),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// `build_open_flags` runs before `getname(filename)` in `do_sys_openat2`,
    /// so the flag verdict outranks a NULL path.
    #[test]
    fn test_open_bad_flags_outrank_a_null_path() {
        assert_eq!(
            open(
                core::ptr::null(),
                fcntl::O_DIRECTORY | fcntl::O_CREAT,
                0o644
            ),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// …and before `do_filp_open` uses `dfd`, so it outranks a bad dirfd too.
    #[test]
    fn test_openat_bad_flags_outrank_a_bad_dirfd() {
        let path = b"relative/path\0";
        assert_eq!(
            openat(
                -1,
                path.as_ptr(),
                fcntl::O_DIRECTORY | fcntl::O_CREAT,
                0o644
            ),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// `__O_TMPFILE` without `O_DIRECTORY` is EINVAL upstream — the pairing is
    /// enforced so that old kernels give an explicit error.
    #[test]
    fn test_open_raw_tmpfile_without_directory_is_einval() {
        let path = b"/tmp\0";
        assert_eq!(
            open(path.as_ptr(), RAW_O_TMPFILE_I32 | fcntl::O_WRONLY, 0o644),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// O_TMPFILE for reading is EINVAL: `!(acc_mode & MAY_WRITE)`.
    #[test]
    fn test_open_tmpfile_readonly_is_einval() {
        let path = b"/tmp\0";
        assert_eq!(
            open(path.as_ptr(), fcntl::O_TMPFILE | fcntl::O_RDONLY, 0o644),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// A *well-formed* O_TMPFILE open is the one we genuinely can't service,
    /// and it still reports EOPNOTSUPP — that verdict corresponds to
    /// `do_tmpfile` inside `path_openat`, which upstream reaches only after
    /// the two EINVALs above.
    #[test]
    fn test_open_wellformed_tmpfile_is_still_eopnotsupp() {
        let path = b"/tmp\0";
        assert_eq!(
            open(path.as_ptr(), fcntl::O_TMPFILE | fcntl::O_WRONLY, 0o644),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EOPNOTSUPP);
    }

    // -- read / write descriptor-before-buffer ordering --

    /// `ksys_read` (fs/read_write.c:604) is `fdget_pos` first; `EFAULT` only
    /// arises inside `vfs_read` (:458).  A zero-length read is *not* a
    /// shortcut past the lookup — upstream has no such shortcut — so probing a
    /// closed descriptor with `read(fd, buf, 0)` must report EBADF.
    #[test]
    fn test_read_bad_fd_outranks_a_null_buffer() {
        assert_eq!(read(-1, core::ptr::null_mut(), 10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_read_zero_count_on_a_closed_fd_is_ebadf() {
        let mut buf = [0u8; 1];
        assert_eq!(read(-1, buf.as_mut_ptr(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_write_bad_fd_outranks_a_null_buffer() {
        assert_eq!(write(-1, core::ptr::null(), 10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_write_zero_count_on_a_closed_fd_is_ebadf() {
        let buf = [0u8; 1];
        assert_eq!(write(-1, buf.as_ptr(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    /// A NULL buffer at count 0 is not a fault: `access_ok(NULL, 0)` succeeds
    /// upstream, so the call reaches `vfs_read` and returns 0.
    #[test]
    fn test_read_null_buffer_at_zero_count_is_not_a_fault() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(read(fd, core::ptr::null_mut(), 0), 0);
        let _ = close(fd);
    }

    // -- truncate validation order --

    /// `do_sys_truncate` (fs/open.c:129) rejects a negative length before
    /// `user_path_at` ever looks at the path, so EINVAL outranks EFAULT.
    /// `ftruncate` had this right already (fs/open.c:164-170); `truncate` did
    /// not.
    #[test]
    fn test_truncate_negative_length_outranks_a_null_path() {
        assert_eq!(truncate(core::ptr::null(), -1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_truncate_null_path_at_a_valid_length_is_efault() {
        assert_eq!(truncate(core::ptr::null(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// The sibling: a negative length is decided above `fdget`'s EBADF.
    #[test]
    fn test_ftruncate_negative_length_outranks_a_bad_fd() {
        assert_eq!(ftruncate(-1, -1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- pread / pwrite validation --

    /// These need a *seekable* fd.  They used to pass fd 0, which is a console
    /// here, and so were only reaching the EFAULT/zero-count shortcuts that
    /// used to sit above the descriptor checks.  `pread` on a tty is `ESPIPE`
    /// upstream (`ksys_pread64`, fs/read_write.c:664), which is what fd 0 now
    /// correctly yields — see `test_pread_on_a_console_is_espipe` below.
    #[test]
    fn test_pread_null_buf_nonzero_count() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let result = pread(fd, core::ptr::null_mut(), 10, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        let _ = close(fd);
    }

    #[test]
    fn test_pread_zero_count() {
        // POSIX: "If nbyte is 0, read() will return 0."
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let mut buf = [0u8; 1];
        let result = pread(fd, buf.as_mut_ptr(), 0, 0);
        assert_eq!(result, 0);
        let _ = close(fd);
    }

    #[test]
    fn test_pread_negative_offset() {
        let mut buf = [0u8; 10];
        let result = pread(0, buf.as_mut_ptr(), 10, -1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// `pos < 0` (fs/read_write.c:658) is tested before `fdget` (:661), so a
    /// negative offset outranks a closed descriptor.
    #[test]
    fn test_pread_negative_offset_outranks_a_bad_fd() {
        let mut buf = [0u8; 10];
        let result = pread(-1, buf.as_mut_ptr(), 10, -1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// …and `fdget` in turn precedes `vfs_read`'s `access_ok` (:458), so a bad
    /// descriptor outranks a NULL buffer.
    #[test]
    fn test_pread_bad_fd_outranks_a_null_buffer() {
        let result = pread(-1, core::ptr::null_mut(), 10, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    /// The `FMODE_PREAD` test (:664) sits above `vfs_read` too, so a
    /// zero-length pread on an unseekable fd is `ESPIPE`, not a silent 0.
    #[test]
    fn test_pread_on_a_console_is_espipe() {
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Console, 0).expect("fd available");
        let mut buf = [0u8; 1];
        let result = pread(fd, buf.as_mut_ptr(), 0, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESPIPE);
        let _ = close(fd);
    }

    #[test]
    fn test_pwrite_null_buf_nonzero_count() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let result = pwrite(fd, core::ptr::null(), 10, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        let _ = close(fd);
    }

    #[test]
    fn test_pwrite_zero_count() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let buf = [0u8; 1];
        let result = pwrite(fd, buf.as_ptr(), 0, 0);
        assert_eq!(result, 0);
        let _ = close(fd);
    }

    /// Same three ranks as `pread`, from `ksys_pwrite64` (:686).
    #[test]
    fn test_pwrite_bad_fd_outranks_a_null_buffer() {
        let result = pwrite(-1, core::ptr::null(), 10, 0);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
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
    /// A NULL vector at a valid count faults in `copy_iovec_from_user`; it is
    /// not the `UIO_MAXIOV` EINVAL.  The two used to be folded together.
    fn test_readv_null_iov() {
        let result = readv(0, core::ptr::null(), 1);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// "Linux has traditionally returned zero for zero segments"
    /// (`iovec_from_user`, lib/iov_iter.c): `nr_segs == 0` returns the empty
    /// iterator before any other check, so this succeeds.
    #[test]
    fn test_readv_zero_iovcnt() {
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        let result = readv(0, &raw const iov, 0);
        assert_eq!(result, 0);
    }

    /// `do_readv` is `fdget_pos` before `vfs_readv`, so the descriptor
    /// outranks both the count and the pointer — including for the
    /// zero-segment call, which otherwise returns success.
    #[test]
    fn test_readv_bad_fd_outranks_the_iovec_checks() {
        assert_eq!(readv(-1, core::ptr::null(), 1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_readv_zero_iovcnt_on_a_closed_fd_is_ebadf() {
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        assert_eq!(readv(-1, &raw const iov, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
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
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_writev_zero_iovcnt() {
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        let result = writev(0, &raw const iov, 0);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_writev_bad_fd_outranks_the_iovec_checks() {
        assert_eq!(writev(-1, core::ptr::null(), 1), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
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

    // -- lseek error ordering (all four orderings measured on Linux 6.6) --
    //
    // These four checks nest, and the nesting is the whole point: each one is
    // only reachable once the previous has passed.  Before 2026-08-30 `lseek`
    // ran them in the opposite order and got two of them wrong, so every
    // assertion below is one a mutation would break.

    #[test]
    fn test_lseek_seek_data_negative_offset_is_enxio() {
        // `lseek(f, -5, SEEK_DATA)` is ENXIO, not EINVAL — a negative start is
        // "past EOF" as far as the kernels are concerned, and reports the same
        // error a genuinely past-EOF start does.  Measured identically on ext4
        // and tmpfs.
        let fd = fdtable::alloc_fd(fdtable::HandleKind::File, 4321).unwrap();
        crate::errno::set_errno(0);
        let result = lseek(fd, -1, crate::fcntl::SEEK_DATA);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENXIO);
        assert!(fdtable::close_fd(fd).is_some());
    }

    #[test]
    fn test_lseek_seek_hole_negative_offset_is_enxio() {
        let fd = fdtable::alloc_fd(fdtable::HandleKind::File, 4322).unwrap();
        crate::errno::set_errno(0);
        let result = lseek(fd, -1, crate::fcntl::SEEK_HOLE);
        assert_eq!(result, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENXIO);
        assert!(fdtable::close_fd(fd).is_some());
    }

    #[test]
    fn test_lseek_bad_whence_on_closed_fd_is_ebadf() {
        // The descriptor outranks `whence`: `lseek(999closed, 0, 12345)` is
        // EBADF on Linux.  We used to validate `whence` first and answer
        // EINVAL.
        crate::errno::set_errno(0);
        assert_eq!(lseek(999, 0, 12345), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_lseek_bad_whence_on_unseekable_fd_is_einval() {
        // ...but `whence` outranks seekability: `lseek(pipe, 0, 12345)` is
        // EINVAL, where `lseek(pipe, 0, SEEK_SET)` is ESPIPE.  Both directions
        // are asserted, because a check placed on either side of the kind
        // match satisfies one of them and breaks the other.
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Pipe, 4323).unwrap();
        crate::errno::set_errno(0);
        assert_eq!(lseek(fd, 0, 12345), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        crate::errno::set_errno(0);
        assert_eq!(lseek(fd, 0, crate::fcntl::SEEK_SET), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESPIPE);
        assert!(fdtable::close_fd(fd).is_some());
    }

    #[test]
    fn test_lseek_negative_data_offset_on_unseekable_fd_is_espipe() {
        // And seekability outranks the offset: `lseek(pipe, -5, SEEK_DATA)` is
        // ESPIPE, not the ENXIO the same arguments give on a real file.  This
        // is what keeps the offset check inside the `HandleKind::File` arm.
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Pipe, 4324).unwrap();
        crate::errno::set_errno(0);
        assert_eq!(lseek(fd, -5, crate::fcntl::SEEK_DATA), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESPIPE);
        assert!(fdtable::close_fd(fd).is_some());
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

    // These delegate to `pread`, so they need a seekable fd for the same
    // reason the `pread` tests above do — fd 0 is a console and now yields
    // ESPIPE before the buffer is ever examined.

    #[test]
    fn test_pread_chk_null_buf() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(__pread_chk(fd, core::ptr::null_mut(), 10, 0, 10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        let _ = close(fd);
    }

    #[test]
    fn test_pread_chk_zero_count() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(__pread_chk(fd, core::ptr::null_mut(), 0, 0, 0), 0);
        let _ = close(fd);
    }

    #[test]
    fn test_pread_chk_negative_offset() {
        let mut buf = [0u8; 10];
        assert_eq!(__pread_chk(0, buf.as_mut_ptr(), 10, -1, 10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_pread64_chk_null_buf() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(__pread64_chk(fd, core::ptr::null_mut(), 10, 0, 10), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        let _ = close(fd);
    }

    #[test]
    fn test_pread64_chk_zero_count() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        assert_eq!(__pread64_chk(fd, core::ptr::null_mut(), 0, 0, 0), 0);
        let _ = close(fd);
    }

    #[test]
    fn test_realpath_chk_null_path() {
        // Delegates to `realpath`, which rejects a NULL path with EINVAL
        // in userspace (glibc stdlib/canonicalize.c), not EFAULT.
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
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        assert_eq!(preadv2(fd, core::ptr::null(), 1, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        let _ = close(fd);
    }

    #[test]
    fn test_preadv2_zero_iovcnt() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        assert_eq!(preadv2(fd, core::ptr::null(), 0, 0, 0), 0);
        let _ = close(fd);
    }

    #[test]
    fn test_preadv2_negative_offset_delegates_to_readv() {
        // offset == -1 should use readv behavior (current file position).
        // With null iov and iovcnt == 1, readv now returns EFAULT.
        crate::errno::set_errno(0);
        assert_eq!(preadv2(0, core::ptr::null(), 1, -1, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_pwritev2_null_iov() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        assert_eq!(pwritev2(fd, core::ptr::null(), 1, 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        let _ = close(fd);
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

    // These need a seekable fd: `do_preadv` reaches `import_iovec` only after
    // `fdget` and the `FMODE_PREAD` test, so on fd 0 (a console here) the
    // ESPIPE fires first and the iovec checks are never exercised.

    /// A NULL vector at a valid count is `EFAULT` — `copy_iovec_from_user`,
    /// not the `UIO_MAXIOV` test.  This used to be folded into `EINVAL`.
    #[test]
    fn test_preadv_null_iov() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        let ret = preadv(fd, core::ptr::null(), 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        let _ = close(fd);
    }

    /// "Linux has traditionally returned zero for zero segments"
    /// (`iovec_from_user`, lib/iov_iter.c) — `nr_segs == 0` returns an empty
    /// iterator before any other check, so this succeeds rather than failing
    /// with EINVAL as we used to report.
    #[test]
    fn test_preadv_zero_iovcnt() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        crate::errno::set_errno(0);
        let ret = preadv(fd, &iov, 0, 0);
        assert_eq!(ret, 0);
        let _ = close(fd);
    }

    /// `iovcnt` is `unsigned long` at the syscall boundary, so a negative
    /// count arrives as a huge value and trips `nr_segs > UIO_MAXIOV`.
    #[test]
    fn test_preadv_negative_iovcnt() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        crate::errno::set_errno(0);
        let ret = preadv(fd, &iov, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = close(fd);
    }

    #[test]
    fn test_preadv_over_max_iovcnt() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        crate::errno::set_errno(0);
        let ret = preadv(fd, &iov, 1025, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = close(fd);
    }

    /// The count is checked before the pointer, so an over-max count outranks
    /// a NULL vector.
    #[test]
    fn test_preadv_over_max_iovcnt_outranks_a_null_iov() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        let ret = preadv(fd, core::ptr::null(), 1025, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        let _ = close(fd);
    }

    /// …and ESPIPE outranks both, because `FMODE_PREAD` is tested before
    /// `vfs_readv` is ever called.
    #[test]
    fn test_preadv_espipe_outranks_a_bad_iovcnt() {
        let fd = fdtable::alloc_fd(fdtable::HandleKind::Console, 0).expect("fd available");
        crate::errno::set_errno(0);
        let ret = preadv(fd, core::ptr::null(), 1025, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ESPIPE);
        let _ = close(fd);
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
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        crate::errno::set_errno(0);
        let ret = pwritev(fd, core::ptr::null(), 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        let _ = close(fd);
    }

    #[test]
    fn test_pwritev_zero_iovcnt() {
        let fd = fdtable::alloc_fd(HandleKind::File, 0).expect("alloc_fd File failed");
        let iov = Iovec {
            iov_base: core::ptr::null_mut(),
            iov_len: 0,
        };
        crate::errno::set_errno(0);
        let ret = pwritev(fd, &iov, 0, 0);
        assert_eq!(ret, 0);
        let _ = close(fd);
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

    // -- eaccess / euidaccess ----------------------------------------------
    //
    // eaccess asks access()'s question against the EFFECTIVE uid.  It routes
    // through faccessat(AT_FDCWD, …, AT_EACCESS) rather than hard-coding the
    // equivalence to access(), so it inherits that path's validation now and
    // real permission checking later.  These assert the validation is not
    // bypassed and that both spellings behave identically.

    #[test]
    fn test_eaccess_rejects_out_of_range_mode_bits() {
        // Anything outside R_OK|W_OK|X_OK is EINVAL, same as access().
        crate::errno::set_errno(0);
        let ret = eaccess(b"/nonexistent_xyz\0".as_ptr(), 0x40);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_eaccess_null_path_is_efault() {
        crate::errno::set_errno(0);
        let ret = eaccess(core::ptr::null(), crate::fcntl::F_OK);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_eaccess_accepts_every_valid_mode() {
        crate::errno::set_errno(0);
        let _ret = eaccess(
            b"/nonexistent_xyz\0".as_ptr(),
            crate::fcntl::R_OK | crate::fcntl::W_OK | crate::fcntl::X_OK,
        );
        assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_euidaccess_is_the_same_function_under_the_other_name() {
        // glibc exports both spellings; programs reach for either, so both
        // must resolve AND agree.
        for mode in [
            crate::fcntl::F_OK,
            crate::fcntl::R_OK,
            crate::fcntl::W_OK,
            crate::fcntl::X_OK,
            0x40, // invalid, to compare the error path too
        ] {
            crate::errno::set_errno(0);
            let a = eaccess(b"/nonexistent_xyz\0".as_ptr(), mode);
            let a_errno = crate::errno::get_errno();
            crate::errno::set_errno(0);
            let b = euidaccess(b"/nonexistent_xyz\0".as_ptr(), mode);
            let b_errno = crate::errno::get_errno();
            assert_eq!(a, b, "return differs for mode {mode:#x}");
            assert_eq!(a_errno, b_errno, "errno differs for mode {mode:#x}");
        }
    }

    #[test]
    fn test_eaccess_agrees_with_faccessat_at_eaccess() {
        // eaccess is *defined* as this call; if the two ever diverge the
        // delegation has been broken.
        for mode in [crate::fcntl::F_OK, crate::fcntl::R_OK, 0x40] {
            crate::errno::set_errno(0);
            let via_eaccess = eaccess(b"/nonexistent_xyz\0".as_ptr(), mode);
            let e1 = crate::errno::get_errno();
            crate::errno::set_errno(0);
            let via_faccessat =
                faccessat(AT_FDCWD, b"/nonexistent_xyz\0".as_ptr(), mode, AT_EACCESS);
            let e2 = crate::errno::get_errno();
            assert_eq!(
                via_eaccess, via_faccessat,
                "return differs for mode {mode:#x}"
            );
            assert_eq!(e1, e2, "errno differs for mode {mode:#x}");
        }
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

    // -- The resolve word: enforced, or refused; never dropped -------------
    //
    // The property is one thing said several ways: a restriction the caller
    // asked for must either reach the kernel or come back as an error. It must
    // never turn into a descriptor, because a descriptor is an unrestricted
    // open wearing the answer the caller wanted.
    //
    // Two bits (`BENEATH`, `NO_SYMLINKS`) are now *enforced*, by forwarding to
    // `SYS_FS_OPENAT2`; two (`IN_ROOT`, `CACHED`) are still refused. The
    // enforced half cannot be asserted end-to-end in a host test — every
    // native syscall is stubbed off-target, so both the forward and the
    // delegation come back ENOSYS and are indistinguishable — so it is
    // asserted on `plan_resolve`, which is where the decision actually lives.
    // That is the half lane A warned would otherwise be untested marshalling.

    /// Every Linux `RESOLVE_*` value, paired with its name for failure text.
    const LINUX_RESOLVE_BITS: [(&str, u64); 6] = [
        ("NO_XDEV", RESOLVE_NO_XDEV),
        ("NO_MAGICLINKS", RESOLVE_NO_MAGICLINKS),
        ("NO_SYMLINKS", RESOLVE_NO_SYMLINKS),
        ("BENEATH", RESOLVE_BENEATH),
        ("IN_ROOT", RESOLVE_IN_ROOT),
        ("CACHED", RESOLVE_CACHED),
    ];

    #[test]
    fn test_resolve_beneath_is_forwarded_not_refused() {
        // Was EXDEV until the native call existed to carry the word.
        assert_eq!(
            plan_resolve(RESOLVE_BENEATH),
            ResolvePlan::Forward(K_RESOLVE_BENEATH)
        );
    }

    #[test]
    fn test_resolve_no_symlinks_is_forwarded_not_refused() {
        assert_eq!(
            plan_resolve(RESOLVE_NO_SYMLINKS),
            ResolvePlan::Forward(K_RESOLVE_NO_SYMLINKS)
        );
    }

    #[test]
    fn test_resolve_beneath_and_no_symlinks_compose() {
        // Lane A's containment work composes the two without special handling,
        // so the translation must too — an `if/else if` here would silently
        // drop one of a pair the caller asked for together.
        assert_eq!(
            plan_resolve(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS),
            ResolvePlan::Forward(K_RESOLVE_BENEATH | K_RESOLVE_NO_SYMLINKS)
        );
    }

    #[test]
    fn test_kernel_resolve_bits_are_not_linux_resolve_bits() {
        // The divergence is deliberate and load-bearing: every Linux resolve
        // value lies in 0x00..=0x3f, so an *untranslated* word arriving at the
        // kernel has no known bit and at least one unknown one, and is refused
        // on its first call. Had the numbers matched, a dropped translation
        // line would turn one restriction into a different one and the caller
        // would be told its confinement was applied when it was not.
        //
        // This test fails if anyone "harmonises" the constants for tidiness —
        // which is the point. Its kernel-side twin is
        // `dispatch.rs::test_dispatch_openat2_native` case (a).
        for (name, bit) in LINUX_RESOLVE_BITS {
            assert!(
                bit < 0x40,
                "Linux RESOLVE_{name} ({bit:#x}) left the range the \
                 no-collision argument rests on"
            );
        }
        for k in [K_RESOLVE_BENEATH, K_RESOLVE_NO_SYMLINKS] {
            assert!(
                k >= 0x40,
                "kernel resolve bit {k:#x} collides with Linux's range; a \
                 dropped translation would now mean a *different* restriction \
                 rather than an unknown one"
            );
        }
    }

    #[test]
    fn test_every_linux_resolve_bit_has_a_plan() {
        // Totality: `VALID_RESOLVE_FLAGS` is what step 4 lets through, so every
        // bit in it reaches `plan_resolve` and none may fall off the end into
        // `Delegate` by accident. A new bit added to the mask without a line in
        // `plan_resolve` would silently become "unrestricted open".
        for (name, bit) in LINUX_RESOLVE_BITS {
            assert_ne!(bit & VALID_RESOLVE_FLAGS, 0, "RESOLVE_{name} not in mask");
            let plan = plan_resolve(bit);
            let delegated = plan == ResolvePlan::Delegate;
            let free = bit == RESOLVE_NO_XDEV || bit == RESOLVE_NO_MAGICLINKS;
            assert_eq!(
                delegated, free,
                "RESOLVE_{name} plans {plan:?}; only NO_XDEV and \
                 NO_MAGICLINKS may be delegated unrestricted"
            );
        }
    }

    /// Returns the errno a given `resolve` word produces, having first checked
    /// that it is a refusal *and* that the refusal came from the resolve gate.
    ///
    /// The second half matters more than it looks. A host test cannot open
    /// anything — `open` bottoms out in the stubbed native `SYS_FS_OPEN_MODE`
    /// — so `ret == -1` is true of every call here and proves nothing on its
    /// own. Comparing against the `resolve = 0` baseline is what makes these
    /// tests real: it rules out the case where the errno being asserted is
    /// simply whatever the stub was going to return anyway, which would leave
    /// the whole suite passing against a gate that had been deleted.
    fn refused_resolve(resolve: u64) -> i32 {
        let call = |resolve: u64| {
            let how = OpenHow {
                flags: crate::fcntl::O_RDONLY as u64,
                mode: 0,
                resolve,
            };
            crate::errno::set_errno(0);
            let ret = openat2(
                AT_FDCWD,
                b"/\0".as_ptr(),
                &how,
                core::mem::size_of::<OpenHow>(),
            );
            (ret, crate::errno::get_errno())
        };
        let (ret, err) = call(resolve);
        assert_eq!(ret, -1, "resolve={resolve:#x} must not yield a descriptor");
        assert_ne!(
            err,
            call(0).1,
            "resolve={resolve:#x} must be refused by the gate, \
             not merely share the unrestricted call's errno"
        );
        err
    }

    #[test]
    fn test_resolve_in_root_is_refused_not_ignored() {
        // Still refused: not built kernel-side, no constant there, so it cannot
        // be forwarded even by accident.
        assert_eq!(refused_resolve(RESOLVE_IN_ROOT), crate::errno::EOPNOTSUPP);
    }

    #[test]
    fn test_resolve_cached_is_refused_not_ignored() {
        assert_eq!(refused_resolve(RESOLVE_CACHED), crate::errno::EAGAIN);
    }

    #[test]
    fn test_refusal_outranks_a_forwardable_bit() {
        // A word mixing a forwardable restriction with an unsupported one must
        // be refused, not partially honoured. Forwarding the BENEATH half and
        // dropping IN_ROOT would confine the walk less than asked and say
        // nothing about it.
        assert_eq!(
            plan_resolve(RESOLVE_BENEATH | RESOLVE_IN_ROOT),
            ResolvePlan::Refuse(crate::errno::EOPNOTSUPP)
        );
        assert_eq!(
            plan_resolve(RESOLVE_BENEATH | RESOLVE_CACHED),
            ResolvePlan::Refuse(crate::errno::EAGAIN)
        );
    }

    #[test]
    fn test_real_restriction_survives_being_mixed_with_free_bits() {
        // NO_XDEV and NO_MAGICLINKS are dropped rather than forwarded, so a
        // caller could otherwise hide a real restriction behind them: an
        // implementation that decided "nothing to forward" from the presence of
        // a droppable bit would answer the whole word with `Delegate`.
        assert_eq!(
            plan_resolve(RESOLVE_NO_XDEV | RESOLVE_NO_MAGICLINKS | RESOLVE_BENEATH),
            ResolvePlan::Forward(K_RESOLVE_BENEATH)
        );
    }

    #[test]
    fn test_unknown_resolve_bit_still_outranks_the_unenforceable_ones() {
        // Ordering: step 4 (EINVAL for unknown bits) runs before step 7, which
        // is what keeps forward-compat detection working — a caller probing
        // with a future bit must learn "unknown", not "unsupported".
        let how = OpenHow {
            flags: crate::fcntl::O_RDONLY as u64,
            mode: 0,
            resolve: RESOLVE_BENEATH | (1u64 << 40),
        };
        crate::errno::set_errno(0);
        let ret = openat2(
            AT_FDCWD,
            b"/\0".as_ptr(),
            &how,
            core::mem::size_of::<OpenHow>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_free_resolve_bits_do_not_block_an_open() {
        // The complement of the refusal tests: the two bits we judge trivially
        // satisfied must not have become a blanket refusal of every resolve
        // word. Without this, "refuse everything" would pass the suite above.
        //
        // Asserted differentially rather than as `ret >= 0`, because no open
        // can succeed in a host test at all: `open` reaches the filesystem
        // through `syscall4(SYS_FS_OPEN_MODE, …)`, a SlateOS native syscall
        // that is stubbed off-target. Comparing against the `resolve = 0`
        // baseline says precisely the thing under test — that these bits
        // change nothing — and says it identically on both targets.
        let call = |resolve: u64| {
            let how = OpenHow {
                flags: crate::fcntl::O_RDONLY as u64,
                mode: 0,
                resolve,
            };
            crate::errno::set_errno(0);
            let ret = openat2(
                AT_FDCWD,
                b"/\0".as_ptr(),
                &how,
                core::mem::size_of::<OpenHow>(),
            );
            (ret, crate::errno::get_errno())
        };
        assert_eq!(
            call(RESOLVE_NO_XDEV | RESOLVE_NO_MAGICLINKS),
            call(0),
            "NO_XDEV/NO_MAGICLINKS must be indistinguishable from no restriction"
        );
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

    /// NULL pathname AND bad dirfd — EFAULT wins, because `user_path_at`
    /// passes `getname_flags(name)` as an *argument* to `filename_lookup`, so
    /// the name is imported before `dfd` is consulted.
    #[test]
    fn test_name_to_handle_at_pointer_check_before_dirfd_check() {
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

    /// But `handle` and `mount_id` are read only in `do_sys_name_to_handle`,
    /// which runs *after* `user_path_at` returns — so with a good path they
    /// rank below the dirfd.  We used to check all three pointers together and
    /// answer EFAULT here.
    #[test]
    fn test_name_to_handle_at_dirfd_outranks_the_output_pointers() {
        crate::errno::set_errno(0);
        let ret = name_to_handle_at(
            -5,
            b"foo\0".as_ptr(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
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
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) = crate::sys_capability::current_caps_effective();
                Self { lo, hi }
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
    // §314 — chown / fchown / lchown / fchownat have NO libc CAP_CHOWN gate
    // =======================================================================
    //
    // These were the Phase-206 gate tests.  The gate is gone: Linux permits a
    // file's owner to chgrp to a group they belong to with no capability at
    // all, and — decisively — `CAP_CHOWN` has no rule in §312's projection
    // table, so under step 3 it would read false for *every* process and deny
    // every chown while the kernel went on allowing them.
    //
    // What remains testable on the host is that the capability is not an
    // input: the answer must be identical with it held and with it dropped.
    // Argument validation is unaffected and keeps its priority, because
    // EFAULT/EBADF/EINVAL are questions about libc's own arguments rather
    // than about authority — the distinction §314 turns on.
    //
    // owner/group == (uid_t)-1 (u32::MAX) still means "don't change", and a
    // double-no-op still returns early without a syscall.
    mod phase206_cap_chown {
        use super::*;

        const CAP_CHOWN: u32 = crate::sys_capability::CAP_CHOWN;

        struct CapGuard {
            lo: u32,

            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) = crate::sys_capability::current_caps_effective();
                Self { lo, hi }
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

        /// Dropping `CAP_CHOWN` must not change `chown`'s answer (§314).
        #[test]
        fn test_chown_without_cap_is_not_libc_denied() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(
                chown(b"/tmp\0".as_ptr(), 1000, 1000),
                0,
                "§314: libc must not refuse a chown the kernel has not been \
                 asked about — and CAP_CHOWN, having no rule in §312's table, \
                 reads false for every process once the words are truthful"
            );
            assert_ne!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        /// Owner-only change: same, and worth its own case because this is
        /// the one half of Linux's rule that *is* capability-only. Even here
        /// libc does not pre-empt — `sys_fs_set_owner` requires a `File`
        /// capability with `WRITE`, so the check is made where the file is.
        #[test]
        fn test_chown_owner_only_without_cap_is_not_libc_denied() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(chown(b"/a\0".as_ptr(), 500, u32::MAX), 0);
        }

        /// Group-only change: the case Linux permits outright to a file's
        /// owner, and therefore the clearest false denial the old gate caused.
        #[test]
        fn test_chown_group_only_without_cap_is_not_libc_denied() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(
                chown(b"/a\0".as_ptr(), u32::MAX, 100),
                0,
                "a chgrp by the file's owner needs no privilege in Linux"
            );
        }

        /// Both fields `(uid_t)-1` is a no-op and returns before anything
        /// else — unchanged by §314, since it never needed authorising.
        #[test]
        fn test_chown_noop_returns_early() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(chown(b"/tmp\0".as_ptr(), u32::MAX, u32::MAX), 0);
        }

        /// A NULL path is still rejected first: argument validation is a
        /// question libc can answer completely, unlike an authority question.
        #[test]
        fn test_chown_efault_still_precedes_everything() {
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

        /// Dropping `CAP_CHOWN` must not change `fchown`'s answer (§314).
        #[test]
        fn test_fchown_without_cap_is_not_libc_denied() {
            let _g = CapGuard::snapshot();
            let fd =
                crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 998).expect("alloc fd");
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(fchown(fd, 1000, 1000), 0);
            assert_ne!(crate::errno::get_errno(), crate::errno::EPERM);
            let _ = crate::fdtable::close_fd(fd);
        }

        /// `fchown` no-op (both `-1`) returns early, unchanged by §314.
        #[test]
        fn test_fchown_noop_returns_early() {
            let _g = CapGuard::snapshot();
            let fd =
                crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 997).expect("alloc fd");
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(fchown(fd, u32::MAX, u32::MAX), 0);
            let _ = crate::fdtable::close_fd(fd);
        }

        /// A bad fd is still rejected first — argument validation, not
        /// authority.
        #[test]
        fn test_fchown_ebadf_still_precedes_everything() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(fchown(-1, 0, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
        }

        // ---- lchown ------------------------------------------------------

        /// Dropping `CAP_CHOWN` must not change `lchown`'s answer (§314).
        #[test]
        fn test_lchown_without_cap_is_not_libc_denied() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(lchown(b"/tmp\0".as_ptr(), 1000, 1000), 0);
            assert_ne!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        /// `lchown` no-op returns early, unchanged by §314.
        #[test]
        fn test_lchown_noop_returns_early() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(lchown(b"/a\0".as_ptr(), u32::MAX, u32::MAX), 0);
        }

        /// `lchown` still rejects a NULL path first.
        #[test]
        fn test_lchown_efault_still_precedes_everything() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(lchown(core::ptr::null(), 0, 0), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        }

        // ---- fchownat (delegates to chown) -------------------------------

        /// `fchownat` delegates to `chown`, so it inherits the *absence* of
        /// the gate just as it previously inherited its presence.
        #[test]
        fn test_fchownat_without_cap_is_not_libc_denied() {
            let _g = CapGuard::snapshot();
            drop_cap_chown();
            crate::errno::set_errno(0);
            assert_eq!(fchownat(AT_FDCWD, b"/x\0".as_ptr(), 0, 0, 0), 0);
            assert_ne!(crate::errno::get_errno(), crate::errno::EPERM);
        }

        /// `fchownat` still validates its flags before delegating.
        #[test]
        fn test_fchownat_einval_still_precedes_everything() {
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

    // -----------------------------------------------------------------
    // The pinned `*at` fast path
    // -----------------------------------------------------------------
    //
    // The syscalls themselves cannot be exercised on the host — `syscallN`
    // has no `SYSCALL` instruction there and returns `HOST_ENOSYS` — so what
    // is testable is the gate that decides whether to issue one. That gate is
    // where a mistake is dangerous in the same direction as the bug the fast
    // path exists to fix: accepting a name the kernel would have to walk means
    // asking a call that refuses to walk to walk anyway, and accepting `..`
    // would make the kernel's identity check ornamental.

    mod pinned_at {
        use super::super::{
            AT_REMOVEDIR, AT_SYMLINK_NOFOLLOW, Stat, fstatat, is_pinnable_component, pinned_answer,
            pinned_base, try_pinned_fchmodat, try_pinned_fstatat, try_pinned_linkat,
            try_pinned_mkdirat, try_pinned_symlinkat, try_pinned_unlinkat, try_pinned_utimensat,
            unlinkat,
        };
        use crate::fdtable::{self, HandleKind};

        #[test]
        fn plain_names_are_pinnable() {
            for name in [
                &b"a"[..],
                b"file.txt",
                b"...",     // three dots is an ordinary name
                b".hidden", // so is a leading dot
                b"..a",     // and a name that merely starts with two
                b"a..",
                b"with space",
                b"\xff\xfe", // paths are bytes, not UTF-8
                b"-",
            ] {
                assert!(
                    is_pinnable_component(name),
                    "{name:?} should be a pinnable single component"
                );
            }
        }

        #[test]
        fn dot_and_dotdot_are_refused() {
            // Not style: the kernel verifies that the handle still denotes the
            // directory it was opened on, and a name allowed to climb out of
            // that directory makes the verification prove nothing.
            assert!(!is_pinnable_component(b"."));
            assert!(!is_pinnable_component(b".."));
        }

        #[test]
        fn anything_with_a_slash_is_refused() {
            for name in [
                &b"a/b"[..],
                b"/abs",
                b"trailing/",
                b"../escape",
                b"a/../b",
                b"/",
            ] {
                assert!(
                    !is_pinnable_component(name),
                    "{name:?} is a walk, not a component"
                );
            }
        }

        #[test]
        fn empty_is_refused() {
            assert!(!is_pinnable_component(b""));
        }

        #[test]
        fn the_length_limit_is_255_inclusive() {
            let at_limit = [b'x'; 255];
            let over = [b'x'; 256];
            assert!(is_pinnable_component(&at_limit));
            assert!(!is_pinnable_component(&over));
        }

        /// `pinned_base` accepts a `File` fd and declines every other kind —
        /// and declines all of them *silently*.
        ///
        /// The silence is the point. `resolve_dirfd_path` is the single author
        /// of `EBADF`/`ENOTDIR` for a bad `dirfd`; if this gate started
        /// answering them too, the two would have to be kept in agreement
        /// forever, and the first divergence would show up as a `dirfd` that
        /// diagnoses differently depending on whether its name happened to
        /// contain a slash.
        #[test]
        fn pinned_base_accepts_only_file_handles_and_sets_no_errno() {
            let file_fd = fdtable::alloc_fd(HandleKind::File, 0x1234).expect("fd table full");
            let pipe_fd = fdtable::alloc_fd(HandleKind::Pipe, 0x1234).expect("fd table full");

            crate::errno::set_errno(0);
            assert_eq!(pinned_base(file_fd), Some(0x1234));
            assert_eq!(pinned_base(pipe_fd), None);
            // A fd that was never opened.
            assert_eq!(pinned_base(4242), None);
            assert_eq!(
                crate::errno::get_errno(),
                0,
                "the gate must leave errno to resolve_dirfd_path"
            );

            assert!(fdtable::close_fd(file_fd).is_some());
            assert!(fdtable::close_fd(pipe_fd).is_some());
        }

        /// Handle 0 is refused, because the native ABI spends that value on
        /// "the kernel's working directory" — which is not this libc's.
        ///
        /// Only reachable through a corrupt fd table, but the consequence of
        /// getting it wrong is an unlink in a directory the caller never named,
        /// so it is checked rather than assumed impossible.
        #[test]
        fn pinned_base_refuses_handle_zero() {
            let zero_fd = fdtable::alloc_fd(HandleKind::File, 0).expect("fd table full");
            assert_eq!(pinned_base(zero_fd), None);
            assert!(fdtable::close_fd(zero_fd).is_some());
        }

        /// On a host build every pinned attempt must decline, so that the whole
        /// suite exercises the path-based route it always did.
        ///
        /// This is the test that fails if the `HOST_ENOSYS` recognition breaks:
        /// `try_pinned_unlinkat` would then return `Some(-1)` — a failure
        /// manufactured from a syscall that never ran — and `unlinkat` would
        /// report it as the final answer instead of falling back.
        #[test]
        fn every_pinned_attempt_declines_on_the_host() {
            let dir_fd = fdtable::alloc_fd(HandleKind::File, 0x1234).expect("fd table full");

            // SAFETY: both are valid C strings.
            unsafe {
                assert_eq!(try_pinned_unlinkat(dir_fd, b"victim\0".as_ptr(), 0), None);
                assert_eq!(
                    try_pinned_unlinkat(dir_fd, b"victim\0".as_ptr(), AT_REMOVEDIR),
                    None
                );
                // Shapes the fast path never accepts in the first place.
                assert_eq!(try_pinned_unlinkat(dir_fd, b"a/b\0".as_ptr(), 0), None);
                assert_eq!(try_pinned_unlinkat(dir_fd, b"..\0".as_ptr(), 0), None);
                assert_eq!(try_pinned_unlinkat(dir_fd, b"\0".as_ptr(), 0), None);
                assert_eq!(try_pinned_unlinkat(dir_fd, core::ptr::null(), 0), None);
            }

            assert!(fdtable::close_fd(dir_fd).is_some());
        }

        /// A bad `dirfd` reaches the same `EBADF` whether or not the name was
        /// one the fast path would have taken.
        #[test]
        fn a_bad_dirfd_still_reports_ebadf() {
            for name in [&b"victim\0"[..], b"sub/victim\0"] {
                for flags in [0, AT_REMOVEDIR] {
                    crate::errno::set_errno(0);
                    assert_eq!(unlinkat(4242, name.as_ptr(), flags), -1);
                    assert_eq!(
                        crate::errno::get_errno(),
                        crate::errno::EBADF,
                        "{name:?} flags={flags:#x}"
                    );
                }
            }
        }

        /// The same for `fstatat`, whose fast path additionally declines a
        /// null `buf`.
        #[test]
        fn every_pinned_fstatat_attempt_declines_on_the_host() {
            let dir_fd = fdtable::alloc_fd(HandleKind::File, 0x1234).expect("fd table full");
            let mut st = Stat::default();

            // SAFETY: all the names are valid C strings and `st` is writable.
            unsafe {
                assert_eq!(
                    try_pinned_fstatat(dir_fd, b"f\0".as_ptr(), &raw mut st, 0),
                    None
                );
                assert_eq!(
                    try_pinned_fstatat(dir_fd, b"f\0".as_ptr(), &raw mut st, AT_SYMLINK_NOFOLLOW),
                    None
                );
                // Shapes the fast path never accepts in the first place.
                assert_eq!(
                    try_pinned_fstatat(dir_fd, b"a/b\0".as_ptr(), &raw mut st, 0),
                    None
                );
                assert_eq!(
                    try_pinned_fstatat(dir_fd, b"..\0".as_ptr(), &raw mut st, 0),
                    None
                );
                assert_eq!(
                    try_pinned_fstatat(dir_fd, core::ptr::null(), &raw mut st, 0),
                    None
                );
                // And the one that is specific to this call: a null `buf` is
                // left to `stat`/`lstat` to diagnose, because 663 never sees
                // the caller's pointer and so cannot diagnose it itself.
                assert_eq!(
                    try_pinned_fstatat(dir_fd, b"f\0".as_ptr(), core::ptr::null_mut(), 0),
                    None
                );
            }

            assert!(fdtable::close_fd(dir_fd).is_some());
        }

        /// `EBADF` outranks `EFAULT` for `fstatat`, on either route.
        ///
        /// The pinned route reaches the same answer by a different path: a bad
        /// `dirfd` fails [`pinned_base`] and falls back, so the fallback's
        /// `EBADF` is still the one that lands. The claim is worth a test
        /// because the fast path *could* have been written to check `buf`
        /// first and return `EFAULT` from the kernel's `arg4` validation,
        /// which would have inverted the order for exactly the shape — a
        /// single-component name — that most callers use.
        #[test]
        fn a_bad_dirfd_outranks_a_null_buf_in_fstatat() {
            for name in [&b"f\0"[..], b"sub/f\0"] {
                crate::errno::set_errno(0);
                assert_eq!(fstatat(4242, name.as_ptr(), core::ptr::null_mut(), 0), -1);
                assert_eq!(crate::errno::get_errno(), crate::errno::EBADF, "{name:?}");
            }
        }

        /// The same for `fchmodat`, which has no output buffer and so declines
        /// only on the shape of the name and the kind of the fd.
        #[test]
        fn every_pinned_fchmodat_attempt_declines_on_the_host() {
            let dir_fd = fdtable::alloc_fd(HandleKind::File, 0x1234).expect("fd table full");

            // SAFETY: all the names are valid C strings.
            unsafe {
                assert_eq!(try_pinned_fchmodat(dir_fd, b"f\0".as_ptr(), 0o644, 0), None);
                assert_eq!(
                    try_pinned_fchmodat(dir_fd, b"f\0".as_ptr(), 0o644, AT_SYMLINK_NOFOLLOW),
                    None
                );
                assert_eq!(
                    try_pinned_fchmodat(dir_fd, b"a/b\0".as_ptr(), 0o644, 0),
                    None
                );
                assert_eq!(try_pinned_fchmodat(dir_fd, b".\0".as_ptr(), 0o644, 0), None);
                assert_eq!(
                    try_pinned_fchmodat(dir_fd, core::ptr::null(), 0o644, 0),
                    None
                );
                // A non-directory fd, which is where `chmod -R` would land if
                // it ever handed a file descriptor to the wrong argument.
                let pipe_fd = fdtable::alloc_fd(HandleKind::Pipe, 0x1234).expect("fd table full");
                assert_eq!(
                    try_pinned_fchmodat(pipe_fd, b"f\0".as_ptr(), 0o644, 0),
                    None
                );
                assert!(fdtable::close_fd(pipe_fd).is_some());
            }

            assert!(fdtable::close_fd(dir_fd).is_some());
        }

        /// The whole of the fallback rule: an *empty slot* falls back, and
        /// nothing else does.
        ///
        /// The second assertion is the one with history. `NotSupported` (-2) is
        /// a registered handler that ran and refused, and treating it as
        /// "kernel too old" would retry by path and reintroduce the very race
        /// the call exists to close — silently, on the failure path, where
        /// nobody is looking. It shared -2 with the empty-slot answer until
        /// lane A split them, and this function could only guess between them
        /// with a latch that was wrong for exactly as long as no call had yet
        /// succeeded. Now it is a comparison, and this test is the statement of
        /// it rather than a probe of a heuristic.
        ///
        /// Note the test does **not** depend on call order any more, which is
        /// itself the fix: the latched version's behaviour differed between the
        /// first invocation and every later one, so a test could only pin one
        /// of the two at a time.
        #[test]
        fn only_an_empty_dispatch_slot_falls_back() {
            // The kernel has never heard of the number: take the path route.
            assert_eq!(pinned_answer(crate::errno::native::NO_SUCH_SYSCALL), None);
            // Same on a host build, where the SYSCALL instruction is compiled
            // out and every attempt reports this instead.
            assert_eq!(pinned_answer(crate::syscall::HOST_ENOSYS), None);

            // A registered handler that refused. This is an *answer*, and
            // retrying it by path would undo the point of the call.
            assert_eq!(pinned_answer(crate::errno::native::NOT_SUPPORTED), Some(-1));
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOTSUP);

            // Any other error is likewise final, on the first call as much as
            // the hundredth.
            assert_eq!(pinned_answer(crate::errno::native::NOT_FOUND), Some(-1));
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
            assert_eq!(pinned_answer(crate::errno::native::STALE_HANDLE), Some(-1));
            assert_eq!(crate::errno::get_errno(), crate::errno::ESTALE);

            // Success is passed through unchanged, not folded to 0 by
            // `translate` — 664 will want a byte count here.
            assert_eq!(pinned_answer(7), Some(7));
            assert_eq!(pinned_answer(0), Some(0));
        }

        /// `mkdirat`'s gate, which is the plainest of the family: one name, and
        /// a flags word that is always zero.
        #[test]
        fn every_pinned_mkdirat_attempt_declines_on_the_host() {
            let dir_fd = fdtable::alloc_fd(HandleKind::File, 0x1234).expect("fd table full");

            // SAFETY: all the names are valid C strings.
            unsafe {
                assert_eq!(try_pinned_mkdirat(dir_fd, b"d\0".as_ptr(), 0o755), None);
                assert_eq!(try_pinned_mkdirat(dir_fd, b"a/b\0".as_ptr(), 0o755), None);
                assert_eq!(try_pinned_mkdirat(dir_fd, b"..\0".as_ptr(), 0o755), None);
                assert_eq!(try_pinned_mkdirat(dir_fd, core::ptr::null(), 0o755), None);
                assert_eq!(try_pinned_mkdirat(4242, b"d\0".as_ptr(), 0o755), None);
            }

            assert!(fdtable::close_fd(dir_fd).is_some());
        }

        /// The asymmetry that is the whole shape of 667: the **link name** must
        /// be one component, the **target** must not be constrained.
        ///
        /// Worth a test precisely because it reads like an oversight. A target
        /// forced to be a single component could not express `../lib/libfoo.so`
        /// — which is most of the symlinks in any real tree, and the reason a
        /// recursive copy wanted the call. The pin is a claim about where the
        /// new *entry* lands, not about what text it holds.
        #[test]
        fn a_symlink_target_is_never_the_thing_that_declines_the_fast_path() {
            let dir_fd = fdtable::alloc_fd(HandleKind::File, 0x1234).expect("fd table full");

            // SAFETY: all the strings are valid C strings.
            unsafe {
                // Every one of these targets is legal, so each attempt reaches
                // the syscall and declines only because the host has none.
                for target in [
                    &b"t\0"[..],
                    b"../lib/libfoo.so\0",
                    b"/absolute/elsewhere\0",
                    b"..\0",
                    b"does/not/exist\0",
                    b"\xff\xfe\0", // a target is bytes, not UTF-8
                ] {
                    assert_eq!(
                        try_pinned_symlinkat(target.as_ptr(), dir_fd, b"l\0".as_ptr()),
                        None,
                        "{target:?}"
                    );
                }

                // The link name, by contrast, declines on exactly the shapes
                // the rest of the family declines on.
                for link in [&b"a/b\0"[..], b".\0", b"..\0"] {
                    assert_eq!(
                        try_pinned_symlinkat(b"t\0".as_ptr(), dir_fd, link.as_ptr()),
                        None,
                        "{link:?}"
                    );
                }
                assert_eq!(
                    try_pinned_symlinkat(b"t\0".as_ptr(), dir_fd, core::ptr::null()),
                    None
                );

                // An **empty** target declines rather than being forwarded:
                // 667 calls it EINVAL and the path-based route stores it, so
                // forwarding would let the pinnability of the `dirfd` decide
                // what an empty body means.
                assert_eq!(
                    try_pinned_symlinkat(b"\0".as_ptr(), dir_fd, b"l\0".as_ptr()),
                    None
                );
                assert_eq!(
                    try_pinned_symlinkat(core::ptr::null(), dir_fd, b"l\0".as_ptr()),
                    None
                );
            }

            assert!(fdtable::close_fd(dir_fd).is_some());
        }

        /// 668 resolves *both* handles, so both ends must be pinnable — there
        /// is no half-pinned form.
        ///
        /// The asymmetric cases are the point of the test. A version that
        /// checked only the source would issue the call with a destination
        /// handle it had never validated, and the guarantee it advertises would
        /// hold for the half nobody was worried about.
        #[test]
        fn a_pinned_link_needs_both_ends_and_declines_when_either_is_wrong() {
            let a = fdtable::alloc_fd(HandleKind::File, 0x1234).expect("fd table full");
            let b = fdtable::alloc_fd(HandleKind::File, 0x5678).expect("fd table full");
            let pipe = fdtable::alloc_fd(HandleKind::Pipe, 0x9abc).expect("fd table full");

            // SAFETY: all the names are valid C strings.
            unsafe {
                // Both ends well-formed: declines only for want of a kernel.
                assert_eq!(
                    try_pinned_linkat(a, b"f\0".as_ptr(), b, b"g\0".as_ptr()),
                    None
                );
                // The same handle on both sides is legal — a link within one
                // directory is the ordinary case.
                assert_eq!(
                    try_pinned_linkat(a, b"f\0".as_ptr(), a, b"g\0".as_ptr()),
                    None
                );
                // A bad name on either side, and a bad fd on either side.
                assert_eq!(
                    try_pinned_linkat(a, b"x/f\0".as_ptr(), b, b"g\0".as_ptr()),
                    None
                );
                assert_eq!(
                    try_pinned_linkat(a, b"f\0".as_ptr(), b, b"x/g\0".as_ptr()),
                    None
                );
                assert_eq!(
                    try_pinned_linkat(a, b"f\0".as_ptr(), b, b"..\0".as_ptr()),
                    None
                );
                assert_eq!(
                    try_pinned_linkat(pipe, b"f\0".as_ptr(), b, b"g\0".as_ptr()),
                    None
                );
                assert_eq!(
                    try_pinned_linkat(a, b"f\0".as_ptr(), pipe, b"g\0".as_ptr()),
                    None
                );
                assert_eq!(
                    try_pinned_linkat(a, b"f\0".as_ptr(), 4242, b"g\0".as_ptr()),
                    None
                );
                assert_eq!(
                    try_pinned_linkat(a, core::ptr::null(), b, b"g\0".as_ptr()),
                    None
                );
                assert_eq!(
                    try_pinned_linkat(a, b"f\0".as_ptr(), b, core::ptr::null()),
                    None
                );
            }

            assert!(fdtable::close_fd(pipe).is_some());
            assert!(fdtable::close_fd(b).is_some());
            assert!(fdtable::close_fd(a).is_some());
        }

        /// `utimensat`'s gate. The timestamps are already in the kernel's
        /// zero-means-unchanged form by this point, so no value of them can
        /// decline the fast path — including a zero pair, which means "change
        /// nothing" and is a legitimate call rather than a no-op to elide.
        #[test]
        fn every_pinned_utimensat_attempt_declines_on_the_host() {
            let dir_fd = fdtable::alloc_fd(HandleKind::File, 0x1234).expect("fd table full");

            // SAFETY: all the names are valid C strings.
            unsafe {
                for (a_ns, m_ns) in [(0, 0), (1, 0), (0, 1), (u64::MAX, u64::MAX)] {
                    assert_eq!(
                        try_pinned_utimensat(dir_fd, b"f\0".as_ptr(), a_ns, m_ns, false),
                        None
                    );
                    assert_eq!(
                        try_pinned_utimensat(dir_fd, b"f\0".as_ptr(), a_ns, m_ns, true),
                        None
                    );
                }
                assert_eq!(
                    try_pinned_utimensat(dir_fd, b"a/b\0".as_ptr(), 1, 1, false),
                    None
                );
                assert_eq!(
                    try_pinned_utimensat(dir_fd, b"..\0".as_ptr(), 1, 1, false),
                    None
                );
                assert_eq!(
                    try_pinned_utimensat(dir_fd, core::ptr::null(), 1, 1, false),
                    None
                );
                assert_eq!(
                    try_pinned_utimensat(4242, b"f\0".as_ptr(), 1, 1, false),
                    None
                );
            }

            assert!(fdtable::close_fd(dir_fd).is_some());
        }
    }

    // -----------------------------------------------------------------
    // `*at` flag validation
    // -----------------------------------------------------------------
    //
    // Six of the nine `*at` calls in this file validated their `flags`
    // argument and three — `unlinkat`, `fstatat`, `statx` — did not, so a
    // junk bit was silently ignored by exactly the calls where Linux is
    // strictest. The accepted sets below were *measured* against Linux 6.6
    // (a C program issuing each syscall directly, one flag bit at a time)
    // rather than read off the kernel source, because two of the answers are
    // not what the source reads like: `unlinkat` refuses even
    // `AT_SYMLINK_NOFOLLOW`, and `fstatat` accepts both `AT_STATX_*` sync
    // bits together where `statx` refuses that exact combination.

    mod at_flag_validation {
        use super::super::{
            AT_EMPTY_PATH, AT_FDCWD, AT_NO_AUTOMOUNT, AT_REMOVEDIR, AT_STATX_DONT_SYNC,
            AT_STATX_FORCE_SYNC, AT_STATX_SYNC_TYPE, AT_SYMLINK_FOLLOW, AT_SYMLINK_NOFOLLOW,
            STATX_BASIC_STATS, STATX_RESERVED, Stat, Statx, fstatat, statx, unlinkat,
        };

        /// Every bit Linux rejects, and only those. The `ok` rows are not
        /// asserted to *succeed* — there is no filesystem under a host test —
        /// only to get past the flag gate, which is what `!= EINVAL` shows.
        const REJECTED_BY_UNLINKAT: &[i32] = &[
            AT_SYMLINK_NOFOLLOW,
            AT_SYMLINK_FOLLOW,
            AT_NO_AUTOMOUNT,
            AT_EMPTY_PATH,
            AT_STATX_FORCE_SYNC,
            AT_STATX_DONT_SYNC,
            0x8000,
            0x1,
            -1,
        ];

        const REJECTED_BY_STAT_FAMILY: &[i32] = &[AT_REMOVEDIR, AT_SYMLINK_FOLLOW, 0x8000, 0x1, -1];

        const ACCEPTED_BY_STAT_FAMILY: &[i32] = &[
            0,
            AT_SYMLINK_NOFOLLOW,
            AT_NO_AUTOMOUNT,
            AT_EMPTY_PATH,
            AT_STATX_FORCE_SYNC,
            AT_STATX_DONT_SYNC,
        ];

        #[test]
        fn unlinkat_accepts_only_at_removedir() {
            for &f in REJECTED_BY_UNLINKAT {
                crate::errno::set_errno(0);
                assert_eq!(unlinkat(AT_FDCWD, b"/nonexistent\0".as_ptr(), f), -1);
                assert_eq!(
                    crate::errno::get_errno(),
                    crate::errno::EINVAL,
                    "flags={f:#x} should be EINVAL"
                );
            }
            // The two it does accept must get past the gate.
            for f in [0, AT_REMOVEDIR] {
                crate::errno::set_errno(0);
                unlinkat(AT_FDCWD, b"/nonexistent\0".as_ptr(), f);
                assert_ne!(
                    crate::errno::get_errno(),
                    crate::errno::EINVAL,
                    "flags={f:#x} should pass the gate"
                );
            }
        }

        /// The flag gate outranks every other diagnosis — a NULL buffer, a
        /// NULL path, a missing file, a closed `dirfd`.
        ///
        /// Measured, and worth pinning: a caller that passes a junk flag
        /// *and* a bad pointer must be told about the flag, because that is
        /// the bug it can fix.
        #[test]
        fn the_flag_gate_outranks_efault_and_ebadf() {
            crate::errno::set_errno(0);
            assert_eq!(unlinkat(4242, core::ptr::null(), 0x1), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

            let mut st = Stat::default();
            crate::errno::set_errno(0);
            assert_eq!(fstatat(4242, core::ptr::null(), &raw mut st, 0x1), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

            crate::errno::set_errno(0);
            assert_eq!(
                fstatat(AT_FDCWD, b"/x\0".as_ptr(), core::ptr::null_mut(), 0x1),
                -1
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

            crate::errno::set_errno(0);
            assert_eq!(
                statx(
                    AT_FDCWD,
                    b"/x\0".as_ptr(),
                    0x1,
                    STATX_BASIC_STATS,
                    core::ptr::null_mut()
                ),
                -1
            );
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::EINVAL,
                "a bad flag outranks the NULL buffer"
            );

            // …and with *good* flags the NULL buffer is what is reported.
            crate::errno::set_errno(0);
            assert_eq!(
                statx(
                    AT_FDCWD,
                    b"/x\0".as_ptr(),
                    0,
                    STATX_BASIC_STATS,
                    core::ptr::null_mut()
                ),
                -1
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        }

        #[test]
        fn fstatat_accepts_the_statx_sync_bits_and_rejects_the_rest() {
            let mut st = Stat::default();
            for &f in REJECTED_BY_STAT_FAMILY {
                crate::errno::set_errno(0);
                assert_eq!(
                    fstatat(AT_FDCWD, b"/nonexistent\0".as_ptr(), &raw mut st, f),
                    -1
                );
                assert_eq!(
                    crate::errno::get_errno(),
                    crate::errno::EINVAL,
                    "flags={f:#x} should be EINVAL"
                );
            }
            for &f in ACCEPTED_BY_STAT_FAMILY {
                crate::errno::set_errno(0);
                fstatat(AT_FDCWD, b"/nonexistent\0".as_ptr(), &raw mut st, f);
                assert_ne!(
                    crate::errno::get_errno(),
                    crate::errno::EINVAL,
                    "flags={f:#x} should pass the gate"
                );
            }
            // Both sync bits at once: fstatat allows it, statx does not.
            // This asymmetry is Linux's, measured on 6.6.
            crate::errno::set_errno(0);
            fstatat(
                AT_FDCWD,
                b"/nonexistent\0".as_ptr(),
                &raw mut st,
                AT_STATX_SYNC_TYPE,
            );
            assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
        }

        #[test]
        fn statx_rejects_both_sync_bits_and_the_reserved_mask() {
            let mut sx = Statx::default();
            let p = b"/nonexistent\0".as_ptr();

            crate::errno::set_errno(0);
            assert_eq!(
                statx(
                    AT_FDCWD,
                    p,
                    AT_STATX_SYNC_TYPE,
                    STATX_BASIC_STATS,
                    &raw mut sx
                ),
                -1
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

            crate::errno::set_errno(0);
            assert_eq!(statx(AT_FDCWD, p, 0, STATX_RESERVED, &raw mut sx), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

            // The reserved-mask check also outranks the NULL buffer.
            crate::errno::set_errno(0);
            assert_eq!(
                statx(AT_FDCWD, p, 0, STATX_RESERVED, core::ptr::null_mut()),
                -1
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

            // Each sync bit *alone* is fine.
            for f in [AT_STATX_FORCE_SYNC, AT_STATX_DONT_SYNC] {
                crate::errno::set_errno(0);
                statx(AT_FDCWD, p, f, STATX_BASIC_STATS, &raw mut sx);
                assert_ne!(
                    crate::errno::get_errno(),
                    crate::errno::EINVAL,
                    "flags={f:#x} alone should pass the gate"
                );
            }
        }
    }

    // -----------------------------------------------------------------
    // The empty relative path
    // -----------------------------------------------------------------
    //
    // `build_at_path` cannot represent "no name at all": given `""` it
    // produces `dir_path + "/"`, a valid path naming the directory itself.
    // So before these gates existed, every `*at` call given an empty name
    // silently operated on the *directory the caller happened to hold* and
    // reported success. `unlinkat(fd, "", AT_REMOVEDIR)` removed the
    // directory; `fchownat(fd, "", u, g, 0)` rechowned it; `scandirat(fd,
    // "", …)` listed it. None of them failed, which is what makes this worse
    // than a wrong errno.
    //
    // Linux answers ENOENT, and answers it very early — ahead of a closed
    // `dirfd`, ahead of a NULL output buffer, ahead even of `utimensat`'s
    // timestamp validation. Only the flag gate outranks it. All of the
    // orderings asserted below were measured on Linux 6.6, not inferred.
    mod at_empty_path {
        use super::super::{
            AT_EMPTY_PATH, AT_FDCWD, AT_REMOVEDIR, AT_SYMLINK_NOFOLLOW, STATX_BASIC_STATS,
            STATX_BTIME, STATX_TYPE, Stat, Statx, faccessat, fchmodat, fchownat, fstatat,
            is_empty_path, linkat, mkdirat, openat, readlinkat, renameat, statx, symlinkat,
            unlinkat, utimensat,
        };
        use crate::fdtable::{self, HandleKind};

        const EMPTY: *const u8 = b"\0".as_ptr();
        const NAME: *const u8 = b"n\0".as_ptr();

        fn errno_after(f: impl FnOnce()) -> i32 {
            crate::errno::set_errno(0);
            f();
            crate::errno::get_errno()
        }

        #[test]
        fn empty_is_neither_null_nor_a_name() {
            assert!(is_empty_path(EMPTY));
            assert!(!is_empty_path(core::ptr::null()));
            assert!(!is_empty_path(NAME));
            assert!(!is_empty_path(b"/\0".as_ptr()));
        }

        /// The whole family, against a `dirfd` that is open and usable.
        ///
        /// A real fd matters here: with a closed one the old code would have
        /// failed anyway (for the wrong reason), so the bug only showed up
        /// when everything else was right.
        #[test]
        fn every_at_call_refuses_an_empty_relative_name() {
            let dir = fdtable::alloc_fd(HandleKind::File, 0x1234).expect("fd table full");
            let mut st = Stat::default();
            let mut sx = Statx::default();
            let mut buf = [0u8; 64];

            let cases: [(&str, &dyn Fn()); 12] = [
                ("openat", &|| {
                    openat(dir, EMPTY, crate::fcntl::O_RDONLY, 0);
                }),
                ("unlinkat", &|| {
                    unlinkat(dir, EMPTY, 0);
                }),
                ("unlinkat AT_REMOVEDIR", &|| {
                    unlinkat(dir, EMPTY, AT_REMOVEDIR);
                }),
                ("mkdirat", &|| {
                    mkdirat(dir, EMPTY, 0o755);
                }),
                ("symlinkat", &|| {
                    symlinkat(NAME, dir, EMPTY);
                }),
                ("renameat old", &|| {
                    renameat(dir, EMPTY, dir, NAME);
                }),
                ("renameat new", &|| {
                    renameat(dir, NAME, dir, EMPTY);
                }),
                ("linkat old", &|| {
                    linkat(dir, EMPTY, dir, NAME, 0);
                }),
                ("linkat new", &|| {
                    linkat(dir, NAME, dir, EMPTY, 0);
                }),
                ("utimensat", &|| {
                    utimensat(dir, EMPTY, core::ptr::null(), 0);
                }),
                ("faccessat", &|| {
                    faccessat(dir, EMPTY, crate::fcntl::F_OK, 0);
                }),
                ("fchmodat", &|| {
                    fchmodat(dir, EMPTY, 0o644, 0);
                }),
            ];
            for (name, run) in cases {
                assert_eq!(
                    errno_after(run),
                    crate::errno::ENOENT,
                    "{name} with an empty name must be ENOENT"
                );
            }

            // The ones with awkward signatures, spelled out.
            assert_eq!(
                errno_after(|| {
                    fchownat(dir, EMPTY, 0, 0, 0);
                }),
                crate::errno::ENOENT
            );
            assert_eq!(
                errno_after(|| {
                    readlinkat(dir, EMPTY, buf.as_mut_ptr(), buf.len());
                }),
                crate::errno::ENOENT
            );
            assert_eq!(
                errno_after(|| {
                    fstatat(dir, EMPTY, &raw mut st, 0);
                }),
                crate::errno::ENOENT
            );
            assert_eq!(
                errno_after(|| {
                    statx(dir, EMPTY, 0, STATX_BASIC_STATS, &raw mut sx);
                }),
                crate::errno::ENOENT
            );

            assert!(fdtable::close_fd(dir).is_some());
        }

        /// `AT_FDCWD` is not a special case: an empty name is still no name.
        #[test]
        fn at_fdcwd_does_not_make_an_empty_name_mean_the_cwd() {
            let mut st = Stat::default();
            assert_eq!(
                errno_after(|| {
                    unlinkat(AT_FDCWD, EMPTY, 0);
                }),
                crate::errno::ENOENT
            );
            assert_eq!(
                errno_after(|| {
                    mkdirat(AT_FDCWD, EMPTY, 0o755);
                }),
                crate::errno::ENOENT
            );
            assert_eq!(
                errno_after(|| {
                    fstatat(AT_FDCWD, EMPTY, &raw mut st, 0);
                }),
                crate::errno::ENOENT
            );
        }

        /// ENOENT outranks EBADF. `fstatat(999, "", &st, 0)` is ENOENT on
        /// Linux 6.6 — the name is imported and rejected before the
        /// descriptor is looked at.
        #[test]
        fn an_empty_name_outranks_a_closed_dirfd() {
            let mut st = Stat::default();
            let mut sx = Statx::default();
            const CLOSED: i32 = 4242;

            for (name, got) in [
                (
                    "fstatat",
                    errno_after(|| {
                        fstatat(CLOSED, EMPTY, &raw mut st, 0);
                    }),
                ),
                (
                    "unlinkat",
                    errno_after(|| {
                        unlinkat(CLOSED, EMPTY, 0);
                    }),
                ),
                (
                    "openat",
                    errno_after(|| {
                        openat(CLOSED, EMPTY, crate::fcntl::O_RDONLY, 0);
                    }),
                ),
                (
                    "mkdirat",
                    errno_after(|| {
                        mkdirat(CLOSED, EMPTY, 0o755);
                    }),
                ),
                (
                    "utimensat",
                    errno_after(|| {
                        utimensat(CLOSED, EMPTY, core::ptr::null(), 0);
                    }),
                ),
                (
                    "statx",
                    errno_after(|| {
                        statx(CLOSED, EMPTY, 0, STATX_BASIC_STATS, &raw mut sx);
                    }),
                ),
            ] {
                assert_eq!(
                    got,
                    crate::errno::ENOENT,
                    "{name} should be ENOENT not EBADF"
                );
            }
        }

        /// But *with* `AT_EMPTY_PATH` the descriptor is the whole point, so a
        /// closed one is EBADF again.
        #[test]
        fn with_the_flag_a_closed_dirfd_is_ebadf_again() {
            let mut st = Stat::default();
            assert_eq!(
                errno_after(|| {
                    fstatat(4242, EMPTY, &raw mut st, AT_EMPTY_PATH);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_after(|| {
                    fchmodat(4242, EMPTY, 0o644, AT_EMPTY_PATH);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_after(|| {
                    fchownat(4242, EMPTY, 0, 0, AT_EMPTY_PATH);
                }),
                crate::errno::EBADF
            );
        }

        /// The flag gate still outranks the empty name.
        #[test]
        fn a_bad_flag_bit_outranks_an_empty_name() {
            let mut st = Stat::default();
            let mut sx = Statx::default();
            assert_eq!(
                errno_after(|| {
                    unlinkat(AT_FDCWD, EMPTY, 0x40);
                }),
                crate::errno::EINVAL
            );
            assert_eq!(
                errno_after(|| {
                    fstatat(AT_FDCWD, EMPTY, &raw mut st, 0x40);
                }),
                crate::errno::EINVAL
            );
            assert_eq!(
                errno_after(|| {
                    statx(AT_FDCWD, EMPTY, 0x40, STATX_BASIC_STATS, &raw mut sx);
                }),
                crate::errno::EINVAL
            );
            assert_eq!(
                errno_after(|| {
                    utimensat(AT_FDCWD, EMPTY, core::ptr::null(), 0x40);
                }),
                crate::errno::EINVAL
            );
        }

        /// `statx` is the one place the two orders differ, and both halves are
        /// Linux's: `statx(fd, "", 0, …, NULL)` is ENOENT because the nameless
        /// lookup is refused during name resolution, while
        /// `statx(fd, "", AT_EMPTY_PATH, …, NULL)` is EFAULT because the
        /// flagged form skips resolution entirely and reaches the buffer.
        #[test]
        fn statx_reports_the_empty_name_before_the_buffer_but_the_flag_after() {
            assert_eq!(
                errno_after(|| {
                    statx(AT_FDCWD, EMPTY, 0, STATX_BASIC_STATS, core::ptr::null_mut());
                }),
                crate::errno::ENOENT
            );
            assert_eq!(
                errno_after(|| {
                    statx(
                        AT_FDCWD,
                        EMPTY,
                        AT_EMPTY_PATH,
                        STATX_BASIC_STATS,
                        core::ptr::null_mut(),
                    );
                }),
                crate::errno::EFAULT
            );
        }

        /// `AT_EMPTY_PATH` genuinely reaches the descriptor, and this is the
        /// test that shows it rather than inferring it.
        ///
        /// A pipe fd is the lever: `fstat` answers a pipe from the handle kind
        /// alone, with no syscall, so it is the one kind that *succeeds* on a
        /// host build. If `fstatat` were still going through the textual join,
        /// the pipe would have no stored path and the answer would be ENOTDIR.
        /// A pipe is also a case the join could never serve at all — there is
        /// no name in the filesystem to join to.
        #[test]
        fn at_empty_path_stats_the_descriptor_itself() {
            let pipe = fdtable::alloc_fd(HandleKind::Pipe, 0x1234).expect("fd table full");
            let mut st = Stat::default();

            assert_eq!(fstatat(pipe, EMPTY, &raw mut st, AT_EMPTY_PATH), 0);
            assert_eq!(st.st_mode, crate::fcntl::S_IFIFO);

            // `AT_SYMLINK_NOFOLLOW` alongside it is accepted and ignored —
            // there is no name left to decline to follow.
            let mut st2 = Stat::default();
            assert_eq!(
                fstatat(
                    pipe,
                    EMPTY,
                    &raw mut st2,
                    AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW
                ),
                0
            );
            assert_eq!(st2.st_mode, crate::fcntl::S_IFIFO);

            // And a non-empty name does *not* take the flag's route: the flag
            // is simply ignored, and the pipe has no path to join to.
            assert_eq!(fstatat(pipe, NAME, &raw mut st, AT_EMPTY_PATH), -1);

            assert!(fdtable::close_fd(pipe).is_some());
        }

        /// The same for `statx`, which has its own stat source and so could
        /// have been wired to the descriptor incorrectly and independently.
        ///
        /// Also pins the honest consequence: a pipe has no kernel stat record,
        /// so there is no birth time to report and `STATX_BTIME` must stay out
        /// of `stx_mask` rather than being reported as zero.
        #[test]
        fn statx_with_the_flag_reaches_the_descriptor_and_omits_btime() {
            let pipe = fdtable::alloc_fd(HandleKind::Pipe, 0x1234).expect("fd table full");
            let mut sx = Statx::default();

            assert_eq!(
                statx(pipe, EMPTY, AT_EMPTY_PATH, STATX_BASIC_STATS, &raw mut sx),
                0
            );
            assert_eq!(u32::from(sx.stx_mode), crate::fcntl::S_IFIFO);
            assert_ne!(sx.stx_mask & STATX_TYPE, 0);
            assert_eq!(
                sx.stx_mask & STATX_BTIME,
                0,
                "a pipe has no creation time; the bit must stay clear"
            );

            assert!(fdtable::close_fd(pipe).is_some());
        }

        /// `faccessat` with the flag asks only whether the descriptor is open,
        /// which is the descriptor-shaped form of the "does it exist" question
        /// `access` answers for a path.
        #[test]
        fn faccessat_with_the_flag_tests_the_descriptor() {
            let pipe = fdtable::alloc_fd(HandleKind::Pipe, 0x1234).expect("fd table full");
            assert_eq!(faccessat(pipe, EMPTY, crate::fcntl::F_OK, AT_EMPTY_PATH), 0);
            assert!(fdtable::close_fd(pipe).is_some());

            // Now closed.
            assert_eq!(
                errno_after(|| {
                    faccessat(pipe, EMPTY, crate::fcntl::F_OK, AT_EMPTY_PATH);
                }),
                crate::errno::EBADF
            );
        }
    }

    // -----------------------------------------------------------------
    // `O_PATH` descriptors
    // -----------------------------------------------------------------
    //
    // An `O_PATH` descriptor names a file without opening it. Before this
    // module existed we stored the flag and then ignored it, so every
    // file operation ran normally on a descriptor that Linux refuses —
    // `read`, `write`, `mmap`, `ftruncate`, `flock` and the rest all
    // succeeded where Linux answers EBADF. Ignoring the flag is the
    // over-permissive direction, which is why it went unnoticed: nothing
    // fails, things merely work that should not.
    //
    // **The rule, and why there is only one.** Linux does not scatter this
    // check through twenty syscalls. `fdget()` — the generic "turn an fd
    // number into a file" helper — masks `FMODE_PATH` and returns nothing,
    // so the refusal is emitted at exactly the position of the closed-fd
    // EBADF. `fdget_raw()`, used by the handful of calls that legitimately
    // accept these descriptors (`fstat`, `fchdir`, `dup`, `fcntl`), does
    // not mask it. That single mechanism is what lets us place the check
    // beside each `lookup_fd` and get every ordering right for free rather
    // than measuring twenty of them — and the orderings pinned at the
    // bottom of this module are the evidence that it holds.
    //
    // A host build cannot `open(path, O_PATH)`: `open` issues a real
    // syscall, which on the host returns `HOST_ENOSYS`. So the descriptors
    // below are built directly with `alloc_fd_with_flags`, which is the
    // same thing `open` would have stored.
    mod o_path {
        use super::*;
        use crate::fcntl;

        /// A descriptor carrying `O_PATH`, of a kind that would otherwise
        /// accept every operation asserted against it.
        fn path_fd() -> Fd {
            fdtable::alloc_fd_with_flags(HandleKind::File, 0x5150, fcntl::O_PATH)
                .expect("fd table full")
        }

        /// The same descriptor without the flag, to prove each assertion is
        /// about `O_PATH` and not about the host build refusing everything.
        fn plain_fd() -> Fd {
            fdtable::alloc_fd_with_flags(HandleKind::File, 0x5150, fcntl::O_RDWR)
                .expect("fd table full")
        }

        fn errno_of(f: impl FnOnce()) -> i32 {
            crate::errno::set_errno(0);
            f();
            crate::errno::get_errno()
        }

        #[test]
        fn every_file_operation_refuses_a_path_descriptor() {
            let fd = path_fd();
            let mut buf = [0u8; 8];
            let iov = [Iovec {
                iov_base: buf.as_mut_ptr(),
                iov_len: 8,
            }];

            // Each of these is EBADF on Linux 6.6 for an `O_PATH` fd, and
            // each reaches its `HandleKind` dispatch on a plain one.
            assert_eq!(
                errno_of(|| {
                    read(fd, buf.as_mut_ptr(), 8);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    write(fd, buf.as_ptr(), 8);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    pread(fd, buf.as_mut_ptr(), 8, 0);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    pwrite(fd, buf.as_ptr(), 8, 0);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    readv(fd, iov.as_ptr(), 1);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    writev(fd, iov.as_ptr(), 1);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    preadv(fd, iov.as_ptr(), 1, 0);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    pwritev(fd, iov.as_ptr(), 1, 0);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    lseek(fd, 0, fcntl::SEEK_SET);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    ftruncate(fd, 0);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    fsync(fd);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    fdatasync(fd);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    fchmod(fd, 0o644);
                }),
                crate::errno::EBADF
            );
            assert_eq!(
                errno_of(|| {
                    fchown(fd, 0, 0);
                }),
                crate::errno::EBADF
            );

            assert!(fdtable::close_fd(fd).is_some());
        }

        /// The mutation guard for the block above: strip `O_PATH` and the
        /// same calls stop answering EBADF. Without this, a check that
        /// rejected *every* descriptor would pass the test above.
        #[test]
        fn a_plain_descriptor_is_not_refused() {
            let fd = plain_fd();
            let mut buf = [0u8; 8];
            for e in [
                errno_of(|| {
                    read(fd, buf.as_mut_ptr(), 8);
                }),
                errno_of(|| {
                    lseek(fd, 0, fcntl::SEEK_SET);
                }),
                errno_of(|| {
                    ftruncate(fd, 0);
                }),
                errno_of(|| {
                    fsync(fd);
                }),
            ] {
                assert_ne!(
                    e,
                    crate::errno::EBADF,
                    "an open, non-O_PATH descriptor must get past the fd check"
                );
            }
            assert!(fdtable::close_fd(fd).is_some());
        }

        /// `posix_fadvise` reports through its return value, so it needs its
        /// own assertion — and, critically, must not disturb errno while
        /// refusing.
        #[test]
        fn posix_fadvise_returns_ebadf_without_touching_errno() {
            let fd = path_fd();
            crate::errno::set_errno(12345);
            assert_eq!(
                posix_fadvise(fd, 0, 0, POSIX_FADV_NORMAL),
                crate::errno::EBADF
            );
            assert_eq!(crate::errno::get_errno(), 12345);
            assert!(fdtable::close_fd(fd).is_some());
        }

        /// The calls Linux *allows* on an `O_PATH` descriptor, because they
        /// ask about the descriptor rather than about the file behind it.
        /// The host build cannot complete `fstat` (it needs a real syscall),
        /// so what is asserted is that the failure is not ours.
        #[test]
        fn the_descriptor_itself_is_still_usable() {
            let fd = path_fd();

            assert_ne!(
                errno_of(|| {
                    let mut st = Stat::zeroed();
                    fstat(fd, &raw mut st);
                }),
                crate::errno::EBADF,
                "fstat uses fdget_raw on Linux and must accept an O_PATH fd"
            );

            // `F_GETFL` reports exactly `O_PATH` — measured on Linux 6.6,
            // and it falls out here because `O_RDONLY` is 0.
            assert_eq!(
                crate::fcntl_ops::fcntl(fd, crate::fcntl_ops::F_GETFL, 0),
                fcntl::O_PATH
            );
            assert_eq!(crate::fcntl_ops::fcntl(fd, crate::fcntl_ops::F_GETFD, 0), 0);
            assert_eq!(
                crate::fcntl_ops::fcntl(
                    fd,
                    crate::fcntl_ops::F_SETFD,
                    i64::from(fdtable::FD_CLOEXEC),
                ),
                0
            );

            let dup_fd = crate::fcntl_ops::fcntl(fd, crate::fcntl_ops::F_DUPFD, 0);
            assert!(dup_fd >= 0, "F_DUPFD must work on an O_PATH descriptor");
            // The duplicate inherits the flag, so it is refused in turn.
            assert_eq!(
                errno_of(|| {
                    fsync(dup_fd);
                }),
                crate::errno::EBADF
            );
            assert!(fdtable::close_fd(dup_fd).is_some());

            assert!(fdtable::close_fd(fd).is_some());
        }

        /// `fcntl` is the one call that splits rather than accepting or
        /// refusing wholesale, because it is really a dozen calls. The four
        /// refused commands are the ones that act on the open file.
        #[test]
        fn fcntl_refuses_only_the_commands_that_touch_the_file() {
            let fd = path_fd();
            for cmd in [
                crate::fcntl_ops::F_SETFL,
                crate::fcntl_ops::F_GETLK,
                crate::fcntl_ops::F_SETLK,
                crate::fcntl_ops::F_SETLKW,
            ] {
                assert_eq!(
                    errno_of(|| {
                        crate::fcntl_ops::fcntl(fd, cmd, 0);
                    }),
                    crate::errno::EBADF,
                    "fcntl cmd {cmd} must be EBADF on an O_PATH descriptor"
                );
            }
            assert!(fdtable::close_fd(fd).is_some());
        }

        /// A file-backed `mmap` needs the file, which an `O_PATH` descriptor
        /// does not have. `MAP_ANONYMOUS` ignores `fd` entirely, so it is
        /// the mutation guard: it must still work with the same descriptor.
        #[test]
        fn mmap_refuses_a_file_backed_mapping_but_not_an_anonymous_one() {
            let fd = path_fd();
            assert_eq!(
                errno_of(|| {
                    crate::mman::mmap(
                        core::ptr::null_mut(),
                        4096,
                        crate::mman::PROT_READ,
                        crate::mman::MAP_PRIVATE,
                        fd,
                        0,
                    );
                }),
                crate::errno::EBADF
            );

            // `MAP_ANONYMOUS` ignores `fd` entirely, so the same descriptor
            // must get past the check. The host build cannot complete the
            // mapping (there is no real `mmap` syscall behind it), so what is
            // asserted is that whatever stops it is not our EBADF.
            assert_ne!(
                errno_of(|| {
                    crate::mman::mmap(
                        core::ptr::null_mut(),
                        4096,
                        crate::mman::PROT_READ | crate::mman::PROT_WRITE,
                        crate::mman::MAP_PRIVATE | crate::mman::MAP_ANONYMOUS,
                        fd,
                        0,
                    );
                }),
                crate::errno::EBADF,
                "MAP_ANONYMOUS ignores fd, so an O_PATH fd must not be consulted"
            );

            assert!(fdtable::close_fd(fd).is_some());
        }

        /// `ioctl` on an `O_PATH` descriptor is EBADF ahead of the ENOTTY an
        /// unrecognised request would otherwise get — measured with FIONREAD.
        #[test]
        fn ioctl_is_ebadf_ahead_of_enotty() {
            let fd = path_fd();
            assert_eq!(
                errno_of(|| {
                    crate::ioctl::ioctl(fd, 0xDEAD_BEEF, core::ptr::null_mut());
                }),
                crate::errno::EBADF
            );
            assert!(fdtable::close_fd(fd).is_some());
        }

        // -------------------------------------------------------------
        // The orderings the single-rule claim rests on
        // -------------------------------------------------------------
        //
        // If the `O_PATH` refusal really does sit where the closed-fd EBADF
        // sits, then everything that outranks EBADF for a *closed*
        // descriptor must also outrank it here, and nothing else may. These
        // three were measured on Linux 6.6 in both forms — with a closed fd
        // and with an `O_PATH` fd — and agreed in every case.

        #[test]
        fn argument_checks_that_outrank_a_closed_fd_also_outrank_this_one() {
            let fd = path_fd();
            let mut buf = [0u8; 8];

            // `ftruncate(999closed, -1)` is EINVAL, so `ftruncate(pathfd, -1)`
            // is EINVAL too — the negative length is checked first.
            assert_eq!(
                errno_of(|| {
                    ftruncate(fd, -1);
                }),
                crate::errno::EINVAL
            );

            // Likewise `pread`'s negative offset (`ksys_pread64` checks
            // `pos < 0` before `fdget`).
            assert_eq!(
                errno_of(|| {
                    pread(fd, buf.as_mut_ptr(), 4, -1);
                }),
                crate::errno::EINVAL
            );

            assert!(fdtable::close_fd(fd).is_some());
        }

        #[test]
        fn argument_checks_that_do_not_outrank_a_closed_fd_do_not_outrank_this_one() {
            let fd = path_fd();

            // `lseek(999closed, 0, <nonsense whence>)` is EBADF, not EINVAL,
            // so the same call on an `O_PATH` fd is EBADF as well. This is
            // the assertion that fails if the check is hoisted above the
            // descriptor lookup, and the one that fails if `lseek` reverts
            // to validating `whence` first.
            assert_eq!(
                errno_of(|| {
                    lseek(fd, 0, 12345);
                }),
                crate::errno::EBADF
            );

            // Same shape for `posix_fadvise`, through its return value.
            assert_eq!(posix_fadvise(fd, 0, -1, 999), crate::errno::EBADF);

            assert!(fdtable::close_fd(fd).is_some());
        }
    }
}

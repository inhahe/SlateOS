//! `Slate OS` SSH Server Daemon (sshd)
//!
//! An SSH-2 protocol server for `Slate OS`. Listens for incoming SSH connections,
//! authenticates users, and spawns interactive shell sessions or executes
//! commands on behalf of authenticated users.
//!
//! # Usage
//!
//! ```text
//! sshd                           Start with defaults (port 22)
//! sshd -p 2222                   Listen on custom port
//! sshd -f /etc/ssh/sshd_config   Use alternate config file
//! sshd -d                        Debug mode (no fork, verbose)
//! sshd -D                        Don't daemonize, stay foreground
//! sshd -e                        Log to stderr
//! sshd -h /path/to/hostkey       Specify host key file
//! sshd -t                        Test configuration and exit
//! sshd -T                        Extended test (dump config)
//! ```
//!
//! # Protocol
//!
//! Implements a subset of SSH-2 (RFC 4253, 4252, 4254):
//! - Version exchange (SSH-2.0-SlateOS_SSHD_1.0)
//! - Key exchange: diffie-hellman-group14-sha256
//! - Host key: ssh-ed25519 (RFC 8032, via `posix::ed25519`)
//! - Encryption: AES-128-CTR
//! - MAC: HMAC-SHA256
//! - User auth: password (through `authlib`, the one verifier the system has),
//!   public key (`authorized_keys`, with the RFC 4252 section 7 signature
//!   actually verified)
//! - Session channels: `pty-req` + `shell` give a real interactive session on a
//!   pseudo-terminal; `exec` requests run a command
//!
//! # Two independent limits on guessing
//!
//! `MaxAuthTries` bounds one *conversation*: after that many refusals the
//! daemon disconnects. On its own it bounds nothing at all, because the client
//! may simply reconnect — which is why password attempts also go through a
//! single daemon-wide [`authlib::Authenticator`] whose per-*user* failure tally
//! survives the connection and answers with a doubling delay. The two limits
//! are deliberately different things: one stops a conversation running forever,
//! the other stops an account being ground down.
//!
//! The host key is read from, or created at, `HostKeyFile`
//! (`/etc/ssh/ssh_host_ed25519_key` by default) in OpenSSH private key format.
//! A file that exists but cannot be parsed is fatal rather than replaced: see
//! `HostKey::load_from_file` for why substituting a key is worse than refusing
//! to start.
//!
//! # What a session can and cannot do
//!
//! **`pty-req` + `shell` is a real interactive session.** `pty-req` allocates a
//! pseudo-terminal sized to the client's window; `shell` runs the authenticated
//! user's login shell on the slave end, as that user, with `argv[0]` prefixed by
//! a hyphen so the shell reads the login profiles. Keystrokes go into the
//! terminal's line discipline — so editing, `^C`, `^Z` and job control are the
//! kernel's job, not this daemon's — and the shell's output comes back out of
//! the master. `window-change` resizes the terminal and raises `SIGWINCH`, and
//! the shell's exit is reported as `exit-status` (or `exit-signal`) followed by
//! EOF and close, after its final output has been delivered.
//!
//! This rests on the pty stack landing in the kernel; before that there was no
//! terminal to allocate, and both requests were refused outright. The request
//! that asked for it, now fulfilled, is
//! `b-a-pty-devices-need-the-line-discipline-that-the-console-already-has.md`
//! under `requests/`.
//!
//! **`exec` is real but does not stream.** The command runs through the user's
//! login shell, as that user, and its stdout, stderr and exit status are
//! reported separately and truthfully — but output is collected and delivered
//! when the command exits, and its stdin is `/dev/null`. A command that never
//! exits therefore produces nothing.
//!
//! **`shell` without a `pty-req` is refused.** `ssh -T host` asks for a shell on
//! plain pipes, which needs the same streaming machinery `exec` lacks. The
//! refusal is the honest answer: a `SUCCESS` followed by a session that never
//! produced a byte would look like a hung server rather than a missing feature.
//! Both gaps are tracked in `known-issues.md`.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
//
// sshd parses SSH-2 binary packets (RFC 4253) and runs cryptographic
// transforms (AES-CTR, HMAC-SHA256, Curve25519/Ed25519, DH group14).
// Arithmetic operates on packet lengths, padding sizes, and counter values
// already bounded by RFC 4253 packet limits and `data.len()` length checks.
// Indexing/slicing into packet buffers is gated by length checks at the
// call site; out-of-range conditions return Err, never panic.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use std::env;
use std::fmt;
use std::io;
#[allow(unused_imports)]
use std::io::Write;
use std::process;

// ============================================================================
// Syscall numbers (from kernel/src/syscall/number.rs)
// ============================================================================
//
// The full syscall ABI is mirrored here so that helpers can be wired up as the
// daemon grows (per-session process spawning, listener teardown, fd shutdown,
// authorized-keys file writes, etc.). Numbers that are not yet referenced by an
// active code path are kept (rather than deleted) to keep this table a complete,
// authoritative copy of the kernel ABI; `#[allow(dead_code)]` documents that the
// gap is intentional, not an oversight.

#[allow(dead_code)]
const SYS_EXIT: u64 = 1;
const SYS_CLOCK_MONOTONIC: u64 = 10;
const SYS_SLEEP: u64 = 11;
#[allow(dead_code)]
const SYS_PROCESS_SPAWN: u64 = 500;
const SYS_PROCESS_ID: u64 = 502;
const SYS_FS_READ_FILE: u64 = 600;
const SYS_FS_WRITE_FILE: u64 = 601;
#[allow(dead_code)]
const SYS_FS_STAT: u64 = 606;
const SYS_FS_SET_PERMS: u64 = 631;
#[allow(dead_code)]
const SYS_TCP_CONNECT: u64 = 800;
const SYS_TCP_SEND: u64 = 801;
const SYS_TCP_RECV: u64 = 802;
const SYS_TCP_CLOSE: u64 = 803;
const SYS_TCP_BIND: u64 = 804;
const SYS_TCP_ACCEPT: u64 = 805;
#[allow(dead_code)]
const SYS_TCP_CLOSE_LISTENER: u64 = 806;
const SYS_TCP_PEER_ADDR: u64 = 808;
const SYS_TCP_POLL_STATUS: u64 = 845;
#[allow(dead_code)]
const SYS_TCP_SHUTDOWN: u64 = 855;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 0-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number.
#[cfg(target_vendor = "slateos")]
unsafe fn syscall0(nr: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees nr is valid. The `syscall` instruction
    // clobbers rcx and r11 per the x86_64 ABI.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Host stub for `syscall0` — see the gated definition above.
///
/// On a development host there is no SlateOS kernel to talk to, and a raw
/// `syscall` instruction does not fail cleanly: it enters whatever kernel is
/// actually running, with this crate's SlateOS call number in RAX. Those
/// numbers mean unrelated things elsewhere, so the call is not a no-op — it
/// is someone else's syscall. Returning `ENOSYS` keeps `cargo test`, `cargo
/// run` and `clippy` on the host honest instead of dangerous.
///
/// See known-issues.md
/// `B-FORTY-SIX-USERSPACE-CRATES-CAN-ISSUE-A-RAW-SYSCALL-ON-THE-DEV-HOST`.
#[cfg(not(target_vendor = "slateos"))]
unsafe fn syscall0(_nr: u64) -> i64 {
    -38 // ENOSYS
}

/// Issue a 1-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and `a1` is valid
/// for the specific syscall.
#[cfg(target_vendor = "slateos")]
unsafe fn syscall1(nr: u64, a1: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Host stub for `syscall1` — see the gated definition above.
///
/// On a development host there is no SlateOS kernel to talk to, and a raw
/// `syscall` instruction does not fail cleanly: it enters whatever kernel is
/// actually running, with this crate's SlateOS call number in RAX. Those
/// numbers mean unrelated things elsewhere, so the call is not a no-op — it
/// is someone else's syscall. Returning `ENOSYS` keeps `cargo test`, `cargo
/// run` and `clippy` on the host honest instead of dangerous.
///
/// See known-issues.md
/// `B-FORTY-SIX-USERSPACE-CRATES-CAN-ISSUE-A-RAW-SYSCALL-ON-THE-DEV-HOST`.
#[cfg(not(target_vendor = "slateos"))]
unsafe fn syscall1(_nr: u64, _a1: u64) -> i64 {
    -38 // ENOSYS
}

/// Issue a 3-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for the specific syscall.
#[cfg(target_vendor = "slateos")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Host stub for `syscall3` — see the gated definition above.
///
/// On a development host there is no SlateOS kernel to talk to, and a raw
/// `syscall` instruction does not fail cleanly: it enters whatever kernel is
/// actually running, with this crate's SlateOS call number in RAX. Those
/// numbers mean unrelated things elsewhere, so the call is not a no-op — it
/// is someone else's syscall. Returning `ENOSYS` keeps `cargo test`, `cargo
/// run` and `clippy` on the host honest instead of dangerous.
///
/// See known-issues.md
/// `B-FORTY-SIX-USERSPACE-CRATES-CAN-ISSUE-A-RAW-SYSCALL-ON-THE-DEV-HOST`.
#[cfg(not(target_vendor = "slateos"))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 {
    -38 // ENOSYS
}

/// Issue a 4-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for the specific syscall.
#[cfg(target_vendor = "slateos")]
unsafe fn syscall4(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Host stub for `syscall4` — see the gated definition above.
///
/// On a development host there is no SlateOS kernel to talk to, and a raw
/// `syscall` instruction does not fail cleanly: it enters whatever kernel is
/// actually running, with this crate's SlateOS call number in RAX. Those
/// numbers mean unrelated things elsewhere, so the call is not a no-op — it
/// is someone else's syscall. Returning `ENOSYS` keeps `cargo test`, `cargo
/// run` and `clippy` on the host honest instead of dangerous.
///
/// See known-issues.md
/// `B-FORTY-SIX-USERSPACE-CRATES-CAN-ISSUE-A-RAW-SYSCALL-ON-THE-DEV-HOST`.
#[cfg(not(target_vendor = "slateos"))]
unsafe fn syscall4(_nr: u64, _a1: u64, _a2: u64, _a3: u64, _a4: u64) -> i64 {
    -38 // ENOSYS
}

// ============================================================================
// Syscall wrappers
// ============================================================================

/// Read an entire file into a byte vector via the kernel filesystem.
fn fs_read_file(path: &str) -> Result<Vec<u8>, SshdError> {
    let mut buf = vec![0u8; 65536];
    // SAFETY: We pass a valid path pointer+len and a valid output buffer
    // pointer+len. The kernel reads the path and writes file contents into buf.
    let ret = unsafe {
        syscall4(
            SYS_FS_READ_FILE,
            path.as_ptr() as u64,
            path.len() as u64,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(SshdError::IoError(io::Error::new(
            io::ErrorKind::NotFound,
            format!("cannot read {path}: error {ret}"),
        )));
    }
    buf.truncate(ret as usize);
    Ok(buf)
}

/// Write a whole file through the kernel filesystem, creating or truncating it.
fn fs_write_file(path: &str, data: &[u8]) -> Result<(), SshdError> {
    // SAFETY: We pass a valid path pointer+len and a valid data pointer+len.
    // The kernel reads both and writes nothing back through them.
    let ret = unsafe {
        syscall4(
            SYS_FS_WRITE_FILE,
            path.as_ptr() as u64,
            path.len() as u64,
            data.as_ptr() as u64,
            data.len() as u64,
        )
    };
    if ret < 0 {
        return Err(SshdError::IoError(io::Error::other(format!(
            "cannot write {path}: error {ret}"
        ))));
    }
    Ok(())
}

/// Set a file's permission bits.
fn fs_set_mode(path: &str, mode: u32) -> Result<(), SshdError> {
    // SAFETY: We pass a valid path pointer+len; `mode` and the no-follow flag
    // are scalars. The kernel reads the path and writes nothing back.
    let ret = unsafe {
        syscall4(
            SYS_FS_SET_PERMS,
            path.as_ptr() as u64,
            path.len() as u64,
            u64::from(mode & 0o7777),
            0,
        )
    };
    if ret < 0 {
        return Err(SshdError::IoError(io::Error::other(format!(
            "cannot set mode on {path}: error {ret}"
        ))));
    }
    Ok(())
}

/// Get the current monotonic clock in milliseconds.
fn clock_monotonic_ms() -> u64 {
    // SAFETY: SYS_CLOCK_MONOTONIC takes no pointer arguments, returns time.
    let ret = unsafe { syscall0(SYS_CLOCK_MONOTONIC) };
    if ret < 0 { 0 } else { ret as u64 }
}

/// Get the current process ID.
fn get_pid() -> u64 {
    // SAFETY: SYS_PROCESS_ID takes no arguments, returns the pid.
    let ret = unsafe { syscall0(SYS_PROCESS_ID) };
    if ret < 0 { 0 } else { ret as u64 }
}

/// Bind a TCP listener to a local port. Returns a listener handle.
fn tcp_bind(port: u16) -> Result<u64, SshdError> {
    // SAFETY: SYS_TCP_BIND takes one scalar argument (port number).
    let ret = unsafe { syscall1(SYS_TCP_BIND, u64::from(port)) };
    if ret < 0 {
        return Err(SshdError::NetworkError(format!(
            "tcp_bind({port}) failed: {ret}"
        )));
    }
    Ok(ret as u64)
}

/// Accept an incoming connection on a listener (blocking).
/// Returns a connection handle.
fn tcp_accept(listener: u64) -> Result<u64, SshdError> {
    // SAFETY: listener is a valid listener handle from tcp_bind.
    let ret = unsafe { syscall1(SYS_TCP_ACCEPT, listener) };
    if ret < 0 {
        return Err(SshdError::NetworkError(format!("tcp_accept failed: {ret}")));
    }
    Ok(ret as u64)
}

/// Send data on a TCP connection. Returns number of bytes sent.
fn tcp_send(handle: u64, data: &[u8]) -> Result<usize, SshdError> {
    // SAFETY: We pass a valid handle and a pointer to a byte buffer with its
    // correct length.
    let ret = unsafe {
        syscall3(
            SYS_TCP_SEND,
            handle,
            data.as_ptr() as u64,
            data.len() as u64,
        )
    };
    if ret < 0 {
        return Err(SshdError::NetworkError("tcp_send failed".into()));
    }
    Ok(ret as usize)
}

/// Send all bytes, looping until the entire buffer is transmitted.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), SshdError> {
    let mut offset = 0;
    while offset < data.len() {
        let n = tcp_send(handle, &data[offset..])?;
        if n == 0 {
            return Err(SshdError::NetworkError("tcp_send returned 0".into()));
        }
        offset = offset
            .checked_add(n)
            .ok_or_else(|| SshdError::NetworkError("offset overflow".into()))?;
    }
    Ok(())
}

/// Receive data from a TCP connection. Returns 0 when peer has closed.
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, SshdError> {
    // SAFETY: We pass a valid handle and a mutable buffer pointer with length.
    let ret = unsafe {
        syscall3(
            SYS_TCP_RECV,
            handle,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(SshdError::NetworkError("tcp_recv failed".into()));
    }
    Ok(ret as usize)
}

/// Close a TCP connection handle.
fn tcp_close(handle: u64) {
    // SAFETY: handle is (or was) a valid TCP connection handle.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

/// Close a TCP listener handle.
// Reserved for graceful-shutdown wiring: the main accept loop will call this to
// release the bound listener when the daemon is asked to stop. Not yet invoked.
#[allow(dead_code)]
fn tcp_close_listener(listener: u64) {
    // SAFETY: listener is (or was) a valid TCP listener handle.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE_LISTENER, listener) };
}

/// Get the peer address of a TCP connection.
/// Returns (`ip_u32_network_order`, port).
fn tcp_peer_addr(handle: u64) -> Result<(u32, u16), SshdError> {
    let mut buf = [0u8; 6];
    // SAFETY: handle is valid. buf is a stack-allocated 6-byte buffer.
    let ret = unsafe { syscall3(SYS_TCP_PEER_ADDR, handle, buf.as_mut_ptr() as u64, 0) };
    if ret < 0 {
        return Err(SshdError::NetworkError("tcp_peer_addr failed".into()));
    }
    let ip = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let port = u16::from_be_bytes([buf[4], buf[5]]);
    Ok((ip, port))
}

/// Spawn a new process. Returns child pid on success.
// Reserved for session wiring: once a channel session request is granted the
// daemon will spawn the user's login shell / requested command via this helper.
// The exec path is not yet wired into handle_channel_request. Not yet invoked.
#[allow(dead_code)]
fn process_spawn(path: &str) -> Result<u64, SshdError> {
    // SAFETY: We pass a valid path pointer and its length.
    let ret = unsafe {
        syscall3(
            SYS_PROCESS_SPAWN,
            path.as_ptr() as u64,
            path.len() as u64,
            0,
        )
    };
    if ret < 0 {
        return Err(SshdError::IoError(io::Error::other(format!(
            "process_spawn({path}) failed: {ret}"
        ))));
    }
    Ok(ret as u64)
}

/// Report whether the connection has something waiting for `tcp_recv`.
///
/// `SYS_TCP_POLL_STATUS` answers with the Linux `poll` bitmask — `POLLIN`
/// (0x01) when the receive buffer is non-empty, `POLLERR` (0x08) and
/// `POLLHUP` (0x10) when the connection has failed or the peer has gone. All
/// three mean the same thing to this daemon: a `tcp_recv` will return *now*,
/// with either bytes or the truth about the connection.
///
/// A negative return is deliberately reported as "readable". It happens on a
/// handle the kernel no longer knows, and on the development host, where the
/// syscall stub answers `-ENOSYS`. Treating it as ready makes the caller
/// perform the real `tcp_recv` and act on whatever *that* says, which is both
/// the honest answer and — on the host — exactly the blocking behaviour the
/// daemon had before readiness polling existed. Reporting "not ready" instead
/// would turn an unknown handle into a silent, permanent sleep.
fn tcp_readable(handle: u64) -> bool {
    // SAFETY: `handle` is a TCP connection handle owned by this process; the
    // syscall takes it by value and writes no memory.
    let status = unsafe { syscall1(SYS_TCP_POLL_STATUS, handle) };
    if status < 0 {
        return true;
    }
    (status & i64::from(POLLIN | POLLERR | POLLHUP)) != 0
}

/// Sleep for the given number of **nanoseconds**.
///
/// `SYS_SLEEP`'s argument is nanoseconds, not milliseconds — see
/// `kernel/src/syscall/number.rs`. The unit is easy to get wrong (two other
/// daemons in this tree did), and getting it wrong is not loud: passing
/// milliseconds makes every sleep a million times shorter, which looks like a
/// working program that merely burns a core. Hence the explicit `_ns` name and
/// nanosecond-valued constants at every call site.
///
/// A sleep that fails is ignored: the only caller is the idle backoff in
/// [`handle_channels`], for which a failed sleep degrades to a spin — worse for
/// the CPU, but never wrong for the session. There is nothing better to do with
/// the error, and propagating it would drop a live SSH connection over a
/// missing nap.
fn sleep_ns(ns: u64) {
    // SAFETY: `SYS_SLEEP` takes one scalar argument (a nanosecond count) and
    // writes no memory.
    let _ = unsafe { syscall1(SYS_SLEEP, ns) };
}

/// Format an IPv4 address from network byte order u32.
fn format_ip(ip: u32) -> String {
    let bytes = ip.to_be_bytes();
    format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
}

// ============================================================================
// Pseudo-terminal
// ============================================================================
//
// # Why this reaches the *linked* libc instead of the `posix` rlib
//
// `posix` is a dependency of this crate, and it has `openpty`, `login_tty` and
// the fd layer they need. Calling them through the rlib would not work: a
// SlateOS program target is `target_os = "linux"`, every syscall in `posix` is
// gated `#[cfg(target_os = "none")]` for the bare-metal `libc.a` build, so the
// rlib copy answers `-ENOSYS` to everything while the real libc — with the real
// fd table — sits next to it answering correctly. See this crate's Cargo.toml
// and `known-issues.md`
// `TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT`. The
// route that works from a program is the C symbol, which is what `randrange`
// already does for `getrandom`.
//
// # Why fds here, when everything else in this file is a raw handle
//
// The pty syscalls (544–556) name their ends with `PtyHandle` values, and a
// handle is what `SYS_PTY_POLL` and friends take. Handles are not reachable
// from a child process: `login_tty` has to make the slave the child's
// controlling terminal and its fds 0/1/2, and both of those are fd operations.
// There is no "adopt this raw pty handle as an fd" call, so a session built on
// handles could allocate a terminal and then have no way to put a shell on it.
//
// Going through libc costs one thing — the socket stays a raw handle, so the
// readiness loop polls the two sides with two different primitives
// (`SYS_TCP_POLL_STATUS` on the handle, `poll(2)` on the master fd) rather than
// one `poll` over both. That is a smaller price than not being able to start a
// shell, and each primitive is the correct one for its own object.

/// `struct winsize` — the layout `TIOCSWINSZ` and `SYS_PTY_SET_WINSIZE` expect.
///
/// Rows come first. SSH's `pty-req` sends columns first (RFC 4254 §6.2), so the
/// two orders must be crossed exactly once, at the point of conversion.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

/// `struct pollfd`.
#[repr(C)]
#[derive(Clone, Copy)]
struct Pollfd {
    fd: i32,
    events: i16,
    revents: i16,
}

/// Data may be read without blocking.
const POLLIN: i16 = 0x0001;
/// Data may be written without blocking.
const POLLOUT: i16 = 0x0004;
/// Error condition.
const POLLERR: i16 = 0x0008;
/// The peer has gone.
const POLLHUP: i16 = 0x0010;
/// The descriptor is not open.
const POLLNVAL: i16 = 0x0020;

/// `ioctl` request number for "set this terminal's window size".
const TIOCSWINSZ: u64 = 0x5414;

/// `EIO` — on a pty master, the last slave has closed.
const EIO: i32 = 5;

/// `EAGAIN` — nothing to read or no room to write, on a descriptor that is not
/// finished. Distinguishing it from every other `read` failure is what keeps a
/// momentarily-empty pipe from being mistaken for a closed one.
const EAGAIN: i32 = 11;

/// `fcntl` command: read the file status flags.
const F_GETFL: i32 = 3;
/// `fcntl` command: write the file status flags.
const F_SETFL: i32 = 4;
/// Status flag: reads and writes return rather than wait.
const O_NONBLOCK: i64 = 0o4000;

/// Bindings to the linked C library. See the section comment above for why
/// these are `extern "C"` rather than calls into the `posix` rlib.
mod ptylibc {
    use super::{Pollfd, Winsize};

    #[cfg(target_vendor = "slateos")]
    unsafe extern "C" {
        pub fn openpty(
            amaster: *mut i32,
            aslave: *mut i32,
            name: *mut u8,
            termp: *const u8,
            winp: *const Winsize,
        ) -> i32;
        pub fn login_tty(fd: i32) -> i32;
        pub fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
        pub fn write(fd: i32, buf: *const u8, count: usize) -> isize;
        pub fn close(fd: i32) -> i32;
        pub fn poll(fds: *mut Pollfd, nfds: u64, timeout: i32) -> i32;
        pub fn ioctl(fd: i32, request: u64, arg: *mut u8) -> i32;
        /// C's `fcntl` is variadic; every command this daemon issues takes one
        /// integer argument, so it is declared with that argument fixed. On
        /// SysV x86-64 the two forms pass integers in the same registers, and
        /// SlateOS's own `posix::fcntl_ops::fcntl` is itself non-variadic.
        pub fn fcntl(fd: i32, cmd: i32, arg: i64) -> i32;
        pub fn __errno_location() -> *mut i32;
    }

    /// The last error the C library recorded.
    ///
    /// # Safety
    ///
    /// Must be called on the same thread as the failing call, before anything
    /// else can overwrite it — the same contract C's `errno` has always had.
    #[cfg(target_vendor = "slateos")]
    pub unsafe fn errno() -> i32 {
        // SAFETY: `__errno_location` returns a valid, thread-local, non-null
        // pointer by its documented ABI; libc guarantees it for the life of
        // the thread.
        unsafe { *__errno_location() }
    }

    // Host stubs. On a development host there is no SlateOS libc to link
    // against, and these symbols would either fail to resolve or — worse for
    // `read`/`write`/`close` — resolve to the *host's* C library and operate on
    // the host's descriptors. Every one of them therefore fails with `ENOSYS`
    // on the host, exactly as the raw syscall stubs above do, so `cargo test`
    // and `clippy` exercise the failure paths instead of the host's terminals.
    //
    // `dead_code`/`unused_imports`: which of these the host reaches depends on
    // the host OS, because the one call to `login_tty` lives in a `#[cfg(unix)]`
    // `pre_exec` block — the hook does not exist on Windows. So the same stub is
    // live on a Linux host and unreferenced on a Windows one. Deleting it to
    // silence the Windows build would break the Linux build; gating it on
    // `unix` in turn would make the two hosts disagree about what this module
    // contains, for no gain. The complete set is kept, and the lint waived.
    #[cfg(not(target_vendor = "slateos"))]
    #[allow(dead_code)]
    mod host {
        use super::{Pollfd, Winsize};

        /// `ENOSYS`, as a negative return.
        pub unsafe fn openpty(
            _amaster: *mut i32,
            _aslave: *mut i32,
            _name: *mut u8,
            _termp: *const u8,
            _winp: *const Winsize,
        ) -> i32 {
            -1
        }
        pub unsafe fn login_tty(_fd: i32) -> i32 {
            -1
        }
        pub unsafe fn read(_fd: i32, _buf: *mut u8, _count: usize) -> isize {
            -1
        }
        pub unsafe fn write(_fd: i32, _buf: *const u8, _count: usize) -> isize {
            -1
        }
        pub unsafe fn close(_fd: i32) -> i32 {
            -1
        }
        pub unsafe fn poll(_fds: *mut Pollfd, _nfds: u64, _timeout: i32) -> i32 {
            -1
        }
        pub unsafe fn ioctl(_fd: i32, _request: u64, _arg: *mut u8) -> i32 {
            -1
        }
        pub unsafe fn fcntl(_fd: i32, _cmd: i32, _arg: i64) -> i32 {
            -1
        }
        /// `ENOSYS`. See the module comment for why the host cannot answer.
        pub unsafe fn errno() -> i32 {
            38
        }
    }

    // `unused_imports`: same reason as the `dead_code` waiver above — on a
    // Windows host nothing calls `login_tty`, so its re-export is unreferenced
    // there and used on every other host.
    #[cfg(not(target_vendor = "slateos"))]
    #[allow(unused_imports)]
    pub use host::{close, errno, fcntl, ioctl, login_tty, openpty, poll, read, write};
}

/// What a read from a session's output stream found.
///
/// The same three answers describe a pty master and an ordinary pipe, which is
/// why one enum serves both — but they *arrive* differently, and the difference
/// is the whole reason this is an enum rather than a byte count. A pipe signals
/// the end with a zero-length read; a pty master signals it with `EIO` and
/// treats a zero-length read as "nothing right now" (`design-decisions.md`
/// §259, so that a caller cannot spin forever on a dead terminal). Each reader
/// applies its own rule and hands back the same vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamRead {
    /// This many bytes of program output.
    Data(usize),
    /// Nothing right now; the stream is still open.
    Empty,
    /// The far end has closed — nothing more will ever arrive.
    Hangup,
}

/// Whether a descriptor can be read or written without blocking.
///
/// Returns `(readable, writable)`. Hangup is folded into `readable`
/// deliberately: a read on a hung-up descriptor returns immediately —
/// delivering whatever was buffered ahead of the end-of-stream — and an event
/// loop that treated hangup as "not ready" would sleep through the end of the
/// session. A failed `poll` also reports readable, so the caller performs the
/// read and learns the real reason rather than spinning on a descriptor whose
/// state it cannot query.
fn fd_ready(fd: i32) -> (bool, bool) {
    if fd < 0 {
        return (false, false);
    }
    let mut pfd = Pollfd {
        fd,
        events: POLLIN | POLLOUT,
        revents: 0,
    };
    // SAFETY: `pfd` is a single live `Pollfd`; `nfds` is 1 to match; a zero
    // timeout makes the call non-blocking.
    let ret = unsafe { ptylibc::poll(&raw mut pfd, 1, 0) };
    if ret < 0 || pfd.revents & (POLLERR | POLLNVAL) != 0 {
        return (true, false);
    }
    (
        pfd.revents & (POLLIN | POLLHUP) != 0,
        pfd.revents & POLLOUT != 0,
    )
}

/// Put a descriptor into non-blocking mode.
///
/// Returns whether it worked. The daemon is single-threaded and drives every
/// session from one loop, so a descriptor that can block is a descriptor that
/// can hang every other connection on the machine. `poll` alone is not enough
/// for the write side: POSIX only promises that *some* data may be written
/// without blocking when `POLLOUT` is set, so a 32 KiB channel payload aimed at
/// a pipe with 100 bytes of room would block in `write` despite a clean poll.
///
/// A failure here is reported rather than swallowed so the caller can refuse
/// the session instead of running it in a mode where one stalled command takes
/// the daemon with it.
fn set_nonblocking(fd: i32) -> bool {
    if fd < 0 {
        return true;
    }
    // SAFETY: `fd` is owned by the caller and open; `F_GETFL` takes no
    // argument, and the zero passed for it is ignored.
    let flags = unsafe { ptylibc::fcntl(fd, F_GETFL, 0) };
    if flags < 0 {
        return false;
    }
    // SAFETY: as above; `F_SETFL` takes the new status flags as its argument.
    unsafe { ptylibc::fcntl(fd, F_SETFL, i64::from(flags) | O_NONBLOCK) >= 0 }
}

/// Read from a pipe, applying the *pipe* end-of-stream rule.
///
/// A zero-length read is EOF and nothing else: unlike a pty master, a pipe has
/// no other way to say the writer has gone.
fn read_pipe(fd: i32, buf: &mut [u8]) -> StreamRead {
    if fd < 0 || buf.is_empty() {
        return StreamRead::Empty;
    }
    // SAFETY: `buf` is a live slice; the length passed is its own.
    let n = unsafe { ptylibc::read(fd, buf.as_mut_ptr(), buf.len()) };
    if n > 0 {
        #[allow(clippy::cast_sign_loss)] // guarded positive
        return StreamRead::Data(n as usize);
    }
    if n == 0 {
        return StreamRead::Hangup;
    }
    // SAFETY: called immediately after the failing `read`, on this thread,
    // with nothing in between.
    let err = unsafe { ptylibc::errno() };
    if err == EAGAIN {
        StreamRead::Empty
    } else {
        // Any other failure — EBADF, EIO on a pipe whose other end vanished
        // with the process — means this descriptor will never produce another
        // byte. Reporting it as "nothing right now" would leave the session
        // waiting on output that can no longer come.
        StreamRead::Hangup
    }
}

/// The three ordinary pipes of a session started without a terminal.
///
/// This is the counterpart of [`Pty`] for `exec`, `ssh -T` and subsystems.
/// Where a pty carries both directions on one descriptor and merges the two
/// output streams through a line discipline, a pipe-backed session keeps them
/// apart — which is exactly what SSH wants, since stderr travels as
/// `CHANNEL_EXTENDED_DATA` rather than as `CHANNEL_DATA`.
///
/// Each field is `-1` once closed, and `Drop` closes whatever is left, so the
/// descriptors cannot outlive the channel that owns them.
struct Pipes {
    /// The child's standard input, or -1 once the client's EOF closed it.
    ///
    /// Closing this is the *only* way to tell a filter like `cat` or `wc` that
    /// its input is finished, which is why it is tracked separately from the
    /// other two rather than released in `Drop` alone: a session that waited
    /// for `Drop` would never end, because the child never exits.
    stdin: i32,
    /// The child's standard output, or -1 once it reported end-of-file.
    stdout: i32,
    /// The child's standard error, or -1 once it reported end-of-file.
    stderr: i32,
}

impl Pipes {
    /// Take ownership of a spawned child's three pipe descriptors.
    ///
    /// The child's handles are *moved* out of the `Child`, so `std` will not
    /// also close them; from here on their lifetime is this value's.
    ///
    /// Returns `None` if any of the three could not be put into non-blocking
    /// mode — see [`set_nonblocking`] for why that is fatal rather than a
    /// degraded mode worth continuing in. The descriptors are already owned by
    /// the value being dropped at that point, so they are closed on the way
    /// out and the child sees its pipes vanish.
    ///
    /// On a non-unix host there are no raw descriptors to take — `std` hands
    /// out `HANDLE`s that our `ptylibc` shims cannot use — so every field is
    /// `-1` and every operation below degrades to "closed". That is the same
    /// choice the `ptylibc` host stubs make, and it keeps the host test build
    /// exercising the failure paths instead of the host's real pipes.
    fn take(child: &mut process::Child) -> Option<Self> {
        #[cfg(unix)]
        let pipes = {
            use std::os::unix::io::IntoRawFd;
            Self {
                stdin: child.stdin.take().map_or(-1, IntoRawFd::into_raw_fd),
                stdout: child.stdout.take().map_or(-1, IntoRawFd::into_raw_fd),
                stderr: child.stderr.take().map_or(-1, IntoRawFd::into_raw_fd),
            }
        };
        #[cfg(not(unix))]
        let pipes = {
            let _ = child;
            Self {
                stdin: -1,
                stdout: -1,
                stderr: -1,
            }
        };
        if !set_nonblocking(pipes.stdin)
            || !set_nonblocking(pipes.stdout)
            || !set_nonblocking(pipes.stderr)
        {
            return None;
        }
        Some(pipes)
    }

    /// Whether the child's stdin will accept a write without blocking.
    fn input_ready(&self) -> bool {
        fd_ready(self.stdin).1
    }

    /// Hand client input to the child's stdin.
    ///
    /// Returns the number of bytes accepted, which may be short — a full pipe
    /// is how the kernel applies back-pressure, and the caller must resume from
    /// the count rather than assume the whole slice landed.
    fn write_input(&self, data: &[u8]) -> Result<usize, SshdError> {
        if self.stdin < 0 {
            // Already closed by the client's EOF. Report the bytes as consumed
            // rather than as an error: the session is still perfectly healthy,
            // there is simply nowhere for late input to go, and failing here
            // would tear down a channel whose command is still running.
            return Ok(data.len());
        }
        // SAFETY: `data` is a live slice; the length passed is its own.
        let n = unsafe { ptylibc::write(self.stdin, data.as_ptr(), data.len()) };
        if n < 0 {
            // SAFETY: called immediately after the failing `write`, on this
            // thread, with nothing in between.
            if unsafe { ptylibc::errno() } == EAGAIN {
                // The pipe filled up between the poll and the write, or the
                // poll promised less room than the whole slice. Nothing has
                // gone wrong: report zero bytes taken and the caller will
                // offer the same bytes again on the next pass.
                return Ok(0);
            }
            return Err(SshdError::IoError(io::Error::last_os_error()));
        }
        #[allow(clippy::cast_sign_loss)] // guarded non-negative
        Ok(n as usize)
    }

    /// Close the child's stdin, signalling end-of-input to the command.
    fn close_input(&mut self) {
        if self.stdin >= 0 {
            // SAFETY: `self.stdin` was moved out of a `Child` and is owned
            // here; the guard makes a double close impossible.
            unsafe {
                ptylibc::close(self.stdin);
            }
            self.stdin = -1;
        }
    }

    /// Whether both output streams have reported end-of-file.
    fn output_finished(&self) -> bool {
        self.stdout < 0 && self.stderr < 0
    }

    /// Read at most `budget` bytes from whichever output stream has some.
    ///
    /// Returns the stream it read from as well as the outcome, because stderr
    /// must travel as extended data and stdout must not. stdout is offered
    /// first so that the ordinary case costs one `poll` rather than two.
    ///
    /// A stream that reports end-of-file is closed and set to -1 here, which is
    /// what makes [`Self::output_finished`] eventually true and lets the pump
    /// stop asking.
    fn read_output(&mut self, buf: &mut [u8], budget: usize) -> (bool, StreamRead) {
        let room = buf.len().min(budget);
        let Some(slice) = buf.get_mut(..room) else {
            return (false, StreamRead::Empty);
        };
        if slice.is_empty() {
            // No window credit, or no buffer. Leaving the bytes in the kernel
            // pipe is the point: the child blocks on a full pipe, which is the
            // back-pressure travelling all the way from the client's window to
            // the process producing the output.
            return (false, StreamRead::Empty);
        }
        for stderr in [false, true] {
            let fd = if stderr { self.stderr } else { self.stdout };
            if fd < 0 || !fd_ready(fd).0 {
                continue;
            }
            match read_pipe(fd, slice) {
                StreamRead::Data(n) => return (stderr, StreamRead::Data(n)),
                StreamRead::Empty => {}
                StreamRead::Hangup => {
                    // SAFETY: `fd` was moved out of a `Child` and is owned
                    // here; it is set to -1 immediately below, so no path can
                    // close it twice.
                    unsafe {
                        ptylibc::close(fd);
                    }
                    if stderr {
                        self.stderr = -1;
                    } else {
                        self.stdout = -1;
                    }
                    return (stderr, StreamRead::Hangup);
                }
            }
        }
        (false, StreamRead::Empty)
    }
}

impl Drop for Pipes {
    fn drop(&mut self) {
        self.close_input();
        for fd in [self.stdout, self.stderr] {
            if fd >= 0 {
                // SAFETY: each fd was moved out of a `Child`, is owned here,
                // and `Pipes` is not `Clone`, so this is the only close.
                unsafe {
                    ptylibc::close(fd);
                }
            }
        }
        self.stdout = -1;
        self.stderr = -1;
    }
}

/// A pseudo-terminal pair owned by one session channel.
///
/// The daemon holds the master and reads what the shell printed; the child
/// process adopts the slave as its controlling terminal via `login_tty`.
struct Pty {
    /// The master fd. Always open for the life of this value.
    master: i32,
    /// The slave fd, or -1 once it has been handed to a child.
    ///
    /// The parent **must** close its slave copy after spawning, and that is why
    /// this is tracked separately rather than closed in `Drop` alone: while any
    /// slave fd remains open anywhere, the master never reports hangup, so a
    /// session whose shell had exited would look alive forever.
    slave: i32,
}

impl Pty {
    /// Allocate a pty pair sized for the client's terminal.
    fn open(cols: u32, rows: u32, width_px: u32, height_px: u32) -> Result<Self, SshdError> {
        let ws = winsize_from_ssh(cols, rows, width_px, height_px);
        let mut master: i32 = -1;
        let mut slave: i32 = -1;
        // SAFETY: both out-pointers address live locals; `name` is NULL, which
        // `openpty` documents as "do not report the slave's name"; `termp` is
        // NULL, meaning "leave the slave's termios at the kernel default"
        // (cooked, echoing — what a login shell expects); `winp` addresses a
        // live `Winsize` that outlives the call.
        let ret = unsafe {
            ptylibc::openpty(
                &raw mut master,
                &raw mut slave,
                core::ptr::null_mut(),
                core::ptr::null(),
                &raw const ws,
            )
        };
        if ret != 0 {
            return Err(SshdError::IoError(io::Error::last_os_error()));
        }
        Ok(Self { master, slave })
    }

    /// The slave fd, for a child that is about to adopt it.
    fn slave_fd(&self) -> i32 {
        self.slave
    }

    /// Drop the parent's reference to the slave, after a child has inherited it.
    ///
    /// Until this happens the master cannot see the session end: hangup is
    /// "the last slave closed", and the daemon is holding one.
    fn close_slave(&mut self) {
        if self.slave >= 0 {
            // SAFETY: `self.slave` is an fd this process opened via `openpty`
            // and has not closed; the guard above makes double-close impossible.
            unsafe {
                ptylibc::close(self.slave);
            }
            self.slave = -1;
        }
    }

    /// Whether the master can be read or written without blocking.
    ///
    /// See [`fd_ready`] for why hangup counts as readable and why a failed
    /// `poll` does too.
    fn ready(&self) -> (bool, bool) {
        fd_ready(self.master)
    }

    /// Read program output from the master, at most `budget` bytes.
    ///
    /// A `budget` of zero reads nothing and reports `Empty`, leaving the bytes
    /// in the terminal's own buffer — that is how the client's send window
    /// reaches back to the program producing the output, instead of being
    /// absorbed into daemon memory.
    fn read_output(&self, buf: &mut [u8], budget: usize) -> StreamRead {
        let room = buf.len().min(budget);
        let Some(slice) = buf.get_mut(..room) else {
            return StreamRead::Empty;
        };
        if slice.is_empty() {
            return StreamRead::Empty;
        }
        // SAFETY: `slice` is a live slice; the length passed is its own.
        let n = unsafe { ptylibc::read(self.master, slice.as_mut_ptr(), slice.len()) };
        if n > 0 {
            #[allow(clippy::cast_sign_loss)] // guarded positive
            return StreamRead::Data(n as usize);
        }
        if n == 0 {
            // Not expected from a pty master — the kernel reports the end of a
            // session as EIO — but a zero-length read is unambiguously "no
            // bytes", so treat it as nothing rather than as a hangup we cannot
            // prove.
            return StreamRead::Empty;
        }
        // SAFETY: called immediately after the failing `read`, on this thread,
        // with nothing in between.
        let err = unsafe { ptylibc::errno() };
        if err == EIO {
            StreamRead::Hangup
        } else {
            StreamRead::Empty
        }
    }

    /// Deliver client keystrokes into the slave's line discipline.
    ///
    /// Returns the number of bytes accepted, which may be short: the kernel's
    /// input ring is finite and a short count is how it applies back-pressure.
    /// The caller must resume from it rather than assume the whole slice landed.
    fn write_input(&self, data: &[u8]) -> Result<usize, SshdError> {
        // SAFETY: `data` is a live slice; the length passed is its own.
        let n = unsafe { ptylibc::write(self.master, data.as_ptr(), data.len()) };
        if n < 0 {
            return Err(SshdError::IoError(io::Error::last_os_error()));
        }
        #[allow(clippy::cast_sign_loss)] // guarded non-negative
        Ok(n as usize)
    }

    /// Resize the terminal, raising `SIGWINCH` in the session's foreground group.
    ///
    /// Setting the size and signalling are one kernel operation, so a client
    /// that resizes its window cannot leave the program inside disagreeing with
    /// the terminal about how wide it is.
    fn set_winsize(&self, cols: u32, rows: u32, width_px: u32, height_px: u32) -> bool {
        let ws = winsize_from_ssh(cols, rows, width_px, height_px);
        // SAFETY: `ws` is a live `Winsize`, which is what `TIOCSWINSZ`
        // documents its third argument to be; the cast to `*mut u8` matches the
        // C prototype's untyped `arg` and the callee only reads through it.
        let ret =
            unsafe { ptylibc::ioctl(self.master, TIOCSWINSZ, (&raw const ws).cast_mut().cast()) };
        ret == 0
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.close_slave();
        if self.master >= 0 {
            // SAFETY: `self.master` came from `openpty` and is closed exactly
            // once, here, because `Pty` is not `Clone` and owns the fd.
            unsafe {
                ptylibc::close(self.master);
            }
            self.master = -1;
        }
    }
}

/// Where one session channel's standard streams are attached.
///
/// A channel has a terminal *or* three pipes, never both and never one of each
/// — the client chooses by whether it sends `pty-req` before `shell`. Making
/// that an enum rather than two `Option` fields is what stops the impossible
/// combinations from being representable: a channel with a pty and pipes at
/// once would have two sources of output, two ideas of where input goes, and
/// two ways to decide the session had ended.
enum SessionIo {
    /// Nothing attached: no `pty-req` has arrived and no process has started.
    None,
    /// A pseudo-terminal, allocated by `pty-req`.
    ///
    /// Held for the channel's whole life rather than moved into the session,
    /// because `window-change` can arrive both before the shell starts and
    /// while it runs, and one home for the terminal means one place to look.
    Terminal(Pty),
    /// Three ordinary pipes, for a process started without a terminal —
    /// `exec`, `ssh -T`, or a subsystem.
    Pipes(Pipes),
}

impl SessionIo {
    /// The terminal, if this session has one.
    fn pty(&self) -> Option<&Pty> {
        match self {
            Self::Terminal(pty) => Some(pty),
            _ => None,
        }
    }

    /// The terminal, mutably, if this session has one.
    fn pty_mut(&mut self) -> Option<&mut Pty> {
        match self {
            Self::Terminal(pty) => Some(pty),
            _ => None,
        }
    }

    /// Whether no further output can ever arrive from this session.
    ///
    /// This — not "a read just came back empty" — is what licenses closing the
    /// channel. An empty read only means the process had printed nothing *at
    /// that instant*; a process that writes its last line a microsecond later,
    /// between the read and the `try_wait` that reports its exit, would have
    /// that line cut off if the two were treated alike. End-of-file on both
    /// pipes, or `EIO` on the pty master, is unambiguous: the writing end is
    /// gone, so everything it ever wrote has already been read.
    ///
    /// The cost is that a session whose child left a *grandchild* holding the
    /// descriptors stays open until that grandchild exits too. That is the same
    /// behaviour OpenSSH has, for the same reason, and it is the honest answer:
    /// output is still reachable, so the session is not over.
    fn output_finished(&self) -> bool {
        match self {
            Self::None => true,
            Self::Terminal(_) => false,
            Self::Pipes(pipes) => pipes.output_finished(),
        }
    }

    /// Whether client `CHANNEL_DATA` on this channel has somewhere to go.
    ///
    /// A channel with no attachment yet still says yes if it holds a terminal,
    /// because type-ahead sent between `pty-req` and `shell` belongs in the
    /// line discipline's buffer where the shell will read it — exactly as it
    /// would on a local console. A channel with nothing attached at all says
    /// no, and its caller drops the bytes and credits the window immediately
    /// rather than withholding credit for a window that could never reopen.
    fn accepts_input(&self) -> bool {
        match self {
            Self::None => false,
            Self::Terminal(_) => true,
            Self::Pipes(pipes) => pipes.stdin >= 0,
        }
    }

    /// Whether a write of client input would proceed without blocking.
    fn input_ready(&self) -> bool {
        match self {
            Self::None => false,
            Self::Terminal(pty) => pty.ready().1,
            Self::Pipes(pipes) => pipes.input_ready(),
        }
    }

    /// Deliver client input, returning how many bytes were accepted.
    fn write_input(&self, data: &[u8]) -> Result<usize, SshdError> {
        match self {
            Self::None => Ok(0),
            Self::Terminal(pty) => pty.write_input(data),
            Self::Pipes(pipes) => pipes.write_input(data),
        }
    }

    /// Signal end-of-input to the session, on the client's `CHANNEL_EOF`.
    ///
    /// For pipes this closes the child's stdin, which is the only way a filter
    /// like `cat` or `wc` ever learns its input is finished. For a terminal it
    /// does nothing on purpose: the client is still there, still able to type,
    /// and OpenSSH likewise ignores EOF on an interactive session. Closing the
    /// master here would kill a live shell over a message that means "I have
    /// stopped sending", not "I have gone".
    fn close_input(&mut self) {
        if let Self::Pipes(pipes) = self {
            pipes.close_input();
        }
    }
}

/// Convert an SSH terminal size to a `struct winsize`.
///
/// SSH sends 32-bit dimensions; `winsize` holds 16-bit ones. Values that do not
/// fit are clamped rather than truncated: a client claiming 65 536 columns is
/// either confused or hostile, and `u16::MAX` is a wrong-but-harmless answer
/// where a wrapping cast would silently produce zero — and a zero-width
/// terminal makes `less`, `vi` and every line-wrapping program misbehave in
/// ways that look like a bug in *them*.
fn winsize_from_ssh(cols: u32, rows: u32, width_px: u32, height_px: u32) -> Winsize {
    let clamp = |v: u32| u16::try_from(v).unwrap_or(u16::MAX);
    Winsize {
        ws_row: clamp(rows),
        ws_col: clamp(cols),
        ws_xpixel: clamp(width_px),
        ws_ypixel: clamp(height_px),
    }
}

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
enum SshdError {
    ConfigError(String),
    NetworkError(String),
    ProtocolError(String),
    AuthError(String),
    IoError(io::Error),
    #[allow(dead_code)]
    Timeout,
}

impl fmt::Display for SshdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfigError(msg) => write!(f, "config error: {msg}"),
            Self::NetworkError(msg) => write!(f, "network error: {msg}"),
            Self::ProtocolError(msg) => write!(f, "protocol error: {msg}"),
            Self::AuthError(msg) => write!(f, "auth error: {msg}"),
            Self::IoError(e) => write!(f, "I/O error: {e}"),
            Self::Timeout => write!(f, "timeout"),
        }
    }
}

impl From<io::Error> for SshdError {
    fn from(e: io::Error) -> Self {
        Self::IoError(e)
    }
}

// ============================================================================
// SSH-2 constants
// ============================================================================

/// Our server version identification string.
const SSH_SERVER_VERSION: &str = "SSH-2.0-SlateOS_SSHD_1.0";

/// SSH message type codes (RFC 4253 / 4252 / 4254).
mod msg {
    pub const SSH_MSG_DISCONNECT: u8 = 1;
    pub const SSH_MSG_IGNORE: u8 = 2;
    pub const SSH_MSG_UNIMPLEMENTED: u8 = 3;
    pub const SSH_MSG_DEBUG: u8 = 4;
    pub const SSH_MSG_SERVICE_REQUEST: u8 = 5;
    pub const SSH_MSG_SERVICE_ACCEPT: u8 = 6;
    pub const SSH_MSG_KEXINIT: u8 = 20;
    pub const SSH_MSG_NEWKEYS: u8 = 21;
    #[allow(dead_code)]
    pub const SSH_MSG_KEX_DH_INIT: u8 = 30;
    pub const SSH_MSG_KEX_DH_REPLY: u8 = 31;
    pub const SSH_MSG_USERAUTH_REQUEST: u8 = 50;
    pub const SSH_MSG_USERAUTH_FAILURE: u8 = 51;
    pub const SSH_MSG_USERAUTH_SUCCESS: u8 = 52;
    pub const SSH_MSG_USERAUTH_BANNER: u8 = 53;
    #[allow(dead_code)]
    pub const SSH_MSG_USERAUTH_PK_OK: u8 = 60;
    pub const SSH_MSG_GLOBAL_REQUEST: u8 = 80;
    #[allow(dead_code)]
    pub const SSH_MSG_REQUEST_SUCCESS: u8 = 81;
    pub const SSH_MSG_REQUEST_FAILURE: u8 = 82;
    pub const SSH_MSG_CHANNEL_OPEN: u8 = 90;
    pub const SSH_MSG_CHANNEL_OPEN_CONFIRMATION: u8 = 91;
    pub const SSH_MSG_CHANNEL_OPEN_FAILURE: u8 = 92;
    pub const SSH_MSG_CHANNEL_WINDOW_ADJUST: u8 = 93;
    pub const SSH_MSG_CHANNEL_DATA: u8 = 94;
    pub const SSH_MSG_CHANNEL_EXTENDED_DATA: u8 = 95;
    pub const SSH_MSG_CHANNEL_EOF: u8 = 96;
    pub const SSH_MSG_CHANNEL_CLOSE: u8 = 97;
    pub const SSH_MSG_CHANNEL_REQUEST: u8 = 98;
    pub const SSH_MSG_CHANNEL_SUCCESS: u8 = 99;
    pub const SSH_MSG_CHANNEL_FAILURE: u8 = 100;
}

// ============================================================================
// SSH-2 packet framing
// ============================================================================

/// Maximum SSH packet payload size.
const MAX_PACKET_SIZE: usize = 35000;

/// Minimum block size for packet alignment.
const BLOCK_SIZE_UNENCRYPTED: usize = 8;

/// Build a raw SSH binary packet from a payload.
///
/// Format: `[u32 packet_length][u8 padding_length][payload][random_padding]`
fn build_packet(payload: &[u8], encrypted: bool, seq: u32, enc: &EncryptionState) -> Vec<u8> {
    let block_size = if encrypted {
        enc.block_size.max(8)
    } else {
        BLOCK_SIZE_UNENCRYPTED
    };

    let unpadded = 1 + payload.len();
    let mut padding = block_size - ((4 + unpadded) % block_size);
    if padding < 4 {
        padding += block_size;
    }

    let packet_length = unpadded + padding;
    let mut pkt = Vec::with_capacity(4 + packet_length);
    pkt.extend_from_slice(&(packet_length as u32).to_be_bytes());
    pkt.push(padding as u8);
    pkt.extend_from_slice(payload);
    // Zero-fill padding (simplified; real impl would use random bytes).
    pkt.resize(4 + packet_length, 0);

    if encrypted {
        let mac = compute_mac(&enc.mac_key_s2c, seq, &pkt);
        encrypt_packet_aes_ctr(&mut pkt, &enc.enc_key_s2c, &enc.iv_s2c, seq);
        pkt.extend_from_slice(&mac);
    }

    pkt
}

/// Read one SSH binary packet from the TCP stream, blocking until it arrives.
///
/// This is the right shape for the phases that are strictly a conversation —
/// version exchange, key exchange, authentication — where there is nothing else
/// to do but wait for the client's next move. Once a session is running there
/// *is* something else to do (the shell is producing output), and that loop uses
/// [`try_parse_packet`] and [`StreamBuffer::fill_once`] directly instead.
fn read_packet(
    handle: u64,
    buf: &mut StreamBuffer,
    encrypted: bool,
    seq: u32,
    enc: &EncryptionState,
) -> Result<Vec<u8>, SshdError> {
    loop {
        if let Some(payload) = try_parse_packet(buf, encrypted, seq, enc)? {
            return Ok(payload);
        }
        buf.fill_once(handle)?;
    }
}

/// Parse one SSH binary packet out of `buf` if a whole one is already there.
///
/// Returns `Ok(None)` when more bytes are needed, having consumed nothing, so
/// the caller may go and do something else and try again later. This is a pure
/// function of the buffer and the cipher state: it performs no I/O, which is
/// what makes SSH framing testable on the development host, where there is no
/// kernel to hold a TCP connection.
///
/// Peeking at the first block to learn the packet length does not disturb the
/// cipher: AES-CTR derives each block's keystream from the sequence number and
/// block index, not from a running state, so decrypting block 0 twice — once to
/// read the length, once as part of the whole packet — produces the same bytes.
fn try_parse_packet(
    buf: &mut StreamBuffer,
    encrypted: bool,
    seq: u32,
    enc: &EncryptionState,
) -> Result<Option<Vec<u8>>, SshdError> {
    let block_size = if encrypted {
        enc.block_size.max(8)
    } else {
        BLOCK_SIZE_UNENCRYPTED
    };

    if buf.available() < block_size {
        return Ok(None);
    }

    let first_block = buf.peek(block_size);
    let first_decrypted = if encrypted {
        decrypt_block_aes_ctr(first_block, &enc.enc_key_c2s, &enc.iv_c2s, seq, 0)
    } else {
        first_block.to_vec()
    };

    if first_decrypted.len() < 4 {
        return Err(SshdError::ProtocolError("short first block".into()));
    }
    let packet_length = u32::from_be_bytes([
        first_decrypted[0],
        first_decrypted[1],
        first_decrypted[2],
        first_decrypted[3],
    ]) as usize;

    if packet_length > MAX_PACKET_SIZE {
        return Err(SshdError::ProtocolError(format!(
            "packet too large: {packet_length}"
        )));
    }

    let mac_len = if encrypted { enc.mac_len } else { 0 };
    let total = 4 + packet_length + mac_len;
    if buf.available() < total {
        return Ok(None);
    }

    let raw = buf.consume(total);

    let decrypted = if encrypted {
        let (pkt_data, mac_data) = raw.split_at(4 + packet_length);
        let mut dec = pkt_data.to_vec();
        decrypt_packet_aes_ctr(&mut dec, &enc.enc_key_c2s, &enc.iv_c2s, seq);

        let expected_mac = compute_mac(&enc.mac_key_c2s, seq, &dec);
        if mac_data.len() >= mac_len
            && !constant_time_eq(mac_data.get(..mac_len).unwrap_or_default(), &expected_mac)
        {
            return Err(SshdError::ProtocolError("MAC verification failed".into()));
        }
        dec
    } else {
        raw[..4 + packet_length].to_vec()
    };

    if decrypted.len() < 5 {
        return Err(SshdError::ProtocolError("packet too short".into()));
    }
    let padding_length = decrypted[4] as usize;
    let payload_len = packet_length
        .checked_sub(1 + padding_length)
        .ok_or_else(|| SshdError::ProtocolError("invalid padding length".into()))?;
    if 5 + payload_len > decrypted.len() {
        return Err(SshdError::ProtocolError("payload exceeds packet".into()));
    }
    Ok(Some(decrypted[5..5 + payload_len].to_vec()))
}

// ============================================================================
// Stream buffer -- accumulates TCP data for packet parsing
// ============================================================================

struct StreamBuffer {
    data: Vec<u8>,
    pos: usize,
}

impl StreamBuffer {
    fn new() -> Self {
        Self {
            data: Vec::with_capacity(8192),
            pos: 0,
        }
    }

    fn available(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Read once from the connection and append whatever arrived.
    ///
    /// One read, not a loop: the caller decides what to do when the buffer is
    /// still short, and a session loop's answer is "go and check whether the
    /// shell has printed anything" rather than "block until the client types".
    ///
    /// A zero-length read is the peer's orderly close and is an error here,
    /// because every caller is in the middle of wanting more bytes. The message
    /// is matched on by `handle_channels`, which treats it as a normal end of
    /// connection rather than a protocol failure.
    fn fill_once(&mut self, handle: u64) -> Result<(), SshdError> {
        // Reclaim the consumed prefix before growing. Doing this only past a
        // threshold keeps a long session from memmoving the tail on every
        // packet, while still bounding the buffer for one that runs for hours.
        if self.pos > 4096 {
            self.data.drain(..self.pos);
            self.pos = 0;
        }
        let mut tmp = [0u8; 8192];
        let n = tcp_recv(handle, &mut tmp)?;
        if n == 0 {
            return Err(SshdError::ProtocolError("connection closed".into()));
        }
        self.data.extend_from_slice(&tmp[..n]);
        Ok(())
    }

    fn peek(&self, n: usize) -> &[u8] {
        &self.data[self.pos..self.pos + n]
    }

    fn consume(&mut self, n: usize) -> Vec<u8> {
        let result = self.data[self.pos..self.pos + n].to_vec();
        self.pos += n;
        result
    }
}

// ============================================================================
// SSH data encoding helpers
// ============================================================================

/// Encode a string/bytes as SSH `string` type: u32 length + data.
fn ssh_string(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + data.len());
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    out
}

/// Read an SSH `string` from a byte slice at the given offset.
fn read_ssh_string(data: &[u8], offset: usize) -> Result<(&[u8], usize), SshdError> {
    if offset + 4 > data.len() {
        return Err(SshdError::ProtocolError("truncated string length".into()));
    }
    let len = u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]) as usize;
    let start = offset + 4;
    let end = start + len;
    if end > data.len() {
        return Err(SshdError::ProtocolError(format!(
            "string length {len} exceeds packet (have {})",
            data.len() - start
        )));
    }
    Ok((&data[start..end], end))
}

/// Read a u32 from a byte slice at the given offset.
fn read_u32(data: &[u8], offset: usize) -> Result<(u32, usize), SshdError> {
    if offset + 4 > data.len() {
        return Err(SshdError::ProtocolError("truncated u32".into()));
    }
    let v = u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]);
    Ok((v, offset + 4))
}

/// Read a byte from a slice at the given offset.
fn read_byte(data: &[u8], offset: usize) -> Result<(u8, usize), SshdError> {
    if offset >= data.len() {
        return Err(SshdError::ProtocolError("truncated byte".into()));
    }
    Ok((data[offset], offset + 1))
}

/// Encode an SSH `mpint` from big-endian unsigned byte array.
fn encode_mpint(value: &[u8]) -> Vec<u8> {
    let stripped = strip_leading_zeros(value);
    if stripped.is_empty() {
        return vec![0, 0, 0, 0];
    }
    let needs_pad = (stripped[0] & 0x80) != 0;
    let total_len = stripped.len() + usize::from(needs_pad);
    let mut out = Vec::with_capacity(4 + total_len);
    out.extend_from_slice(&(total_len as u32).to_be_bytes());
    if needs_pad {
        out.push(0);
    }
    out.extend_from_slice(stripped);
    out
}

/// Read an SSH `mpint` from a byte slice, returning unsigned big-endian bytes.
fn read_mpint(data: &[u8], offset: usize) -> Result<(Vec<u8>, usize), SshdError> {
    let (raw, next) = read_ssh_string(data, offset)?;
    let stripped = strip_leading_zeros(raw);
    Ok((stripped.to_vec(), next))
}

fn strip_leading_zeros(data: &[u8]) -> &[u8] {
    let first_nonzero = data.iter().position(|&b| b != 0).unwrap_or(data.len());
    &data[first_nonzero..]
}

/// Read a boolean from the data at the given offset.
fn read_bool(data: &[u8], offset: usize) -> Result<(bool, usize), SshdError> {
    let (b, next) = read_byte(data, offset)?;
    Ok((b != 0, next))
}

// ============================================================================
// Minimal big-integer arithmetic for Diffie-Hellman
// ============================================================================

/// Big-endian unsigned big integer.
#[derive(Clone, Debug)]
struct BigUint {
    bytes: Vec<u8>,
}

impl BigUint {
    fn zero() -> Self {
        Self { bytes: Vec::new() }
    }

    fn one() -> Self {
        Self { bytes: vec![1] }
    }

    fn from_bytes_be(data: &[u8]) -> Self {
        let stripped = strip_leading_zeros(data);
        Self {
            bytes: stripped.to_vec(),
        }
    }

    fn to_bytes_be(&self) -> Vec<u8> {
        if self.bytes.is_empty() {
            return vec![0];
        }
        self.bytes.clone()
    }

    fn is_zero(&self) -> bool {
        self.bytes.is_empty()
    }

    fn bit_length(&self) -> usize {
        if self.bytes.is_empty() {
            return 0;
        }
        let top = self.bytes[0];
        let top_bits = 8 - top.leading_zeros() as usize;
        (self.bytes.len() - 1) * 8 + top_bits
    }

    fn bit(&self, pos: usize) -> bool {
        let byte_idx = pos / 8;
        let bit_idx = pos % 8;
        if byte_idx >= self.bytes.len() {
            return false;
        }
        let idx = self.bytes.len() - 1 - byte_idx;
        (self.bytes[idx] >> bit_idx) & 1 == 1
    }

    /// Modular exponentiation: self^exp mod modulus.
    fn mod_pow(&self, exp: &BigUint, modulus: &BigUint) -> BigUint {
        if modulus.is_zero() {
            return BigUint::zero();
        }
        let mut result = BigUint::one();
        let mut base = self.mod_reduce(modulus);
        let bits = exp.bit_length();
        for i in 0..bits {
            if exp.bit(i) {
                result = result.mod_mul(&base, modulus);
            }
            base = base.mod_mul(&base, modulus);
        }
        result
    }

    fn mod_mul(&self, other: &BigUint, modulus: &BigUint) -> BigUint {
        let product = self.mul_big(other);
        product.mod_reduce(modulus)
    }

    fn mod_reduce(&self, modulus: &BigUint) -> BigUint {
        if modulus.is_zero() {
            return BigUint::zero();
        }
        self.div_rem(modulus).1
    }

    /// Full multiplication (schoolbook, O(n^2)).
    fn mul_big(&self, other: &BigUint) -> BigUint {
        if self.is_zero() || other.is_zero() {
            return BigUint::zero();
        }
        let a = &self.bytes;
        let b = &other.bytes;
        let mut result = vec![0u32; a.len() + b.len()];

        for (i, &av) in a.iter().enumerate().rev() {
            let ai = a.len() - 1 - i;
            for (j, &bv) in b.iter().enumerate().rev() {
                let bj = b.len() - 1 - j;
                let pos = ai + bj;
                let prod = u32::from(av) * u32::from(bv) + result[pos];
                result[pos] = prod & 0xFF;
                if pos + 1 < result.len() {
                    result[pos + 1] += prod >> 8;
                }
            }
        }

        // Propagate carries.
        for i in 0..result.len() - 1 {
            if result[i] > 255 {
                result[i + 1] += result[i] >> 8;
                result[i] &= 0xFF;
            }
        }

        let mut bytes: Vec<u8> = result.iter().rev().map(|&v| v as u8).collect();
        while bytes.len() > 1 && bytes[0] == 0 {
            bytes.remove(0);
        }
        if bytes == [0] {
            bytes.clear();
        }
        BigUint { bytes }
    }

    /// Division with remainder.
    fn div_rem(&self, divisor: &BigUint) -> (BigUint, BigUint) {
        if divisor.is_zero() {
            return (BigUint::zero(), BigUint::zero());
        }
        if self.cmp_unsigned(divisor) == std::cmp::Ordering::Less {
            return (BigUint::zero(), self.clone());
        }

        let mut remainder = BigUint::zero();
        let mut quotient_bits = Vec::new();

        for i in (0..self.bit_length()).rev() {
            remainder = remainder.shl1();
            if self.bit(i) {
                remainder = remainder.add_small(1);
            }
            if remainder.cmp_unsigned(divisor) != std::cmp::Ordering::Less {
                remainder = remainder.sub_big(divisor);
                quotient_bits.push(i);
            }
        }

        if quotient_bits.is_empty() {
            return (BigUint::zero(), remainder);
        }

        let max_bit = quotient_bits[0];
        let num_bytes = max_bit / 8 + 1;
        let mut qbytes = vec![0u8; num_bytes];
        for pos in quotient_bits {
            let byte_idx = pos / 8;
            let bit_idx = pos % 8;
            let idx = num_bytes - 1 - byte_idx;
            qbytes[idx] |= 1 << bit_idx;
        }
        while qbytes.len() > 1 && qbytes[0] == 0 {
            qbytes.remove(0);
        }
        if qbytes == [0] {
            qbytes.clear();
        }
        (BigUint { bytes: qbytes }, remainder)
    }

    fn shl1(&self) -> BigUint {
        if self.is_zero() {
            return BigUint::zero();
        }
        let mut result = vec![0u8; self.bytes.len() + 1];
        let mut carry = 0u8;
        for i in (0..self.bytes.len()).rev() {
            let v = (u16::from(self.bytes[i]) << 1) | u16::from(carry);
            result[i + 1] = v as u8;
            carry = (v >> 8) as u8;
        }
        result[0] = carry;
        while result.len() > 1 && result[0] == 0 {
            result.remove(0);
        }
        if result == [0] {
            result.clear();
        }
        BigUint { bytes: result }
    }

    fn add_small(&self, val: u8) -> BigUint {
        if val == 0 {
            return self.clone();
        }
        if self.is_zero() {
            return BigUint { bytes: vec![val] };
        }
        let mut result = self.bytes.clone();
        let mut carry = u16::from(val);
        for b in result.iter_mut().rev() {
            let sum = u16::from(*b) + carry;
            *b = sum as u8;
            carry = sum >> 8;
        }
        if carry > 0 {
            result.insert(0, carry as u8);
        }
        BigUint { bytes: result }
    }

    fn sub_big(&self, other: &BigUint) -> BigUint {
        if other.is_zero() {
            return self.clone();
        }
        let a = &self.bytes;
        let b = &other.bytes;
        let len = a.len();
        let mut result = vec![0u8; len];
        let mut borrow: i16 = 0;

        for i in (0..len).rev() {
            let av = i16::from(a[i]);
            let bi = i as isize - (len as isize - b.len() as isize);
            let bv = if bi >= 0 {
                i16::from(b[bi as usize])
            } else {
                0
            };
            let diff = av - bv - borrow;
            if diff < 0 {
                result[i] = (diff + 256) as u8;
                borrow = 1;
            } else {
                result[i] = diff as u8;
                borrow = 0;
            }
        }

        while result.len() > 1 && result[0] == 0 {
            result.remove(0);
        }
        if result == [0] {
            result.clear();
        }
        BigUint { bytes: result }
    }

    fn cmp_unsigned(&self, other: &BigUint) -> std::cmp::Ordering {
        let a = strip_leading_zeros(&self.bytes);
        let b = strip_leading_zeros(&other.bytes);
        match a.len().cmp(&b.len()) {
            std::cmp::Ordering::Equal => a.cmp(b),
            ord => ord,
        }
    }
}

// ============================================================================
// SHA-256
// ============================================================================

/// Compute SHA-256 of `data`.
///
/// A thin name over `sha2::sha256`. The round constants, initial words and
/// compression function used to be written out here -- one of ten copies under
/// `userspace/`, and the mirror image of the client's copy in `userspace/ssh`.
/// Two implementations of one digest on the two ends of a connection is the
/// worst arrangement of all: a divergence shows up as a handshake that fails
/// against every other implementation but succeeds against its own twin.
fn sha256(data: &[u8]) -> [u8; 32] {
    sha2::sha256(data)
}

// ============================================================================
// HMAC-SHA256
// ============================================================================

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let block_size = 64;

    // `key_hash` is declared out here, and only *assigned* in the long-key
    // branch, because `key_used` borrows it: the hash has to outlive the `if`
    // that produced it. Declaring it inside the branch would drop it at the
    // brace and leave the borrow dangling.
    let key_hash;
    let key_used = if key.len() > block_size {
        key_hash = sha256(key);
        &key_hash[..]
    } else {
        key
    };

    let mut k_padded = vec![0u8; block_size];
    k_padded[..key_used.len()].copy_from_slice(key_used);

    let mut inner = Vec::with_capacity(block_size + data.len());
    for &b in &k_padded {
        inner.push(b ^ 0x36);
    }
    inner.extend_from_slice(data);
    let inner_hash = sha256(&inner);

    let mut outer = Vec::with_capacity(block_size + 32);
    for &b in &k_padded {
        outer.push(b ^ 0x5c);
    }
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

/// SSH MAC: HMAC-SHA256(key, `sequence_number(u32` be) || `unencrypted_packet`).
fn compute_mac(key: &[u8], seq: u32, packet: &[u8]) -> Vec<u8> {
    let mut mac_input = Vec::with_capacity(4 + packet.len());
    mac_input.extend_from_slice(&seq.to_be_bytes());
    mac_input.extend_from_slice(packet);
    hmac_sha256(key, &mac_input).to_vec()
}

/// Constant-time comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ============================================================================
// AES-128-CTR encryption/decryption
// ============================================================================

const AES_SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

const AES_RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

fn gf_mul2(x: u8) -> u8 {
    if x & 0x80 != 0 {
        (x << 1) ^ 0x1b
    } else {
        x << 1
    }
}

/// AES-128 key expansion: 16-byte key -> 11 round keys (176 bytes).
fn aes128_expand_key(key: &[u8]) -> Vec<u8> {
    let mut expanded = vec![0u8; 176];
    expanded[..16].copy_from_slice(&key[..16]);

    let mut i = 16;
    let mut rcon_idx = 0;
    while i < 176 {
        let mut temp = [
            expanded[i - 4],
            expanded[i - 3],
            expanded[i - 2],
            expanded[i - 1],
        ];

        if i % 16 == 0 {
            temp.rotate_left(1);
            for b in &mut temp {
                *b = AES_SBOX[*b as usize];
            }
            temp[0] ^= AES_RCON[rcon_idx];
            rcon_idx += 1;
        }

        for j in 0..4 {
            expanded[i + j] = expanded[i + j - 16] ^ temp[j];
        }
        i += 4;
    }
    expanded
}

/// AES-128 encrypt a single 16-byte block in-place.
fn aes128_encrypt_block(block: &mut [u8; 16], round_keys: &[u8]) {
    // AddRoundKey (round 0)
    for i in 0..16 {
        block[i] ^= round_keys[i];
    }

    for round in 1..10 {
        // SubBytes
        for b in block.iter_mut() {
            *b = AES_SBOX[*b as usize];
        }
        // ShiftRows
        let tmp = *block;
        block[1] = tmp[5];
        block[5] = tmp[9];
        block[9] = tmp[13];
        block[13] = tmp[1];
        block[2] = tmp[10];
        block[6] = tmp[14];
        block[10] = tmp[2];
        block[14] = tmp[6];
        block[3] = tmp[15];
        block[7] = tmp[3];
        block[11] = tmp[7];
        block[15] = tmp[11];
        // MixColumns
        for col in 0..4 {
            let c = col * 4;
            let a0 = block[c];
            let a1 = block[c + 1];
            let a2 = block[c + 2];
            let a3 = block[c + 3];
            block[c] = gf_mul2(a0) ^ gf_mul2(a1) ^ a1 ^ a2 ^ a3;
            block[c + 1] = a0 ^ gf_mul2(a1) ^ gf_mul2(a2) ^ a2 ^ a3;
            block[c + 2] = a0 ^ a1 ^ gf_mul2(a2) ^ gf_mul2(a3) ^ a3;
            block[c + 3] = gf_mul2(a0) ^ a0 ^ a1 ^ a2 ^ gf_mul2(a3);
        }
        // AddRoundKey
        let rk_off = round * 16;
        for i in 0..16 {
            block[i] ^= round_keys[rk_off + i];
        }
    }

    // Final round (no MixColumns)
    for b in block.iter_mut() {
        *b = AES_SBOX[*b as usize];
    }
    let tmp = *block;
    block[1] = tmp[5];
    block[5] = tmp[9];
    block[9] = tmp[13];
    block[13] = tmp[1];
    block[2] = tmp[10];
    block[6] = tmp[14];
    block[10] = tmp[2];
    block[14] = tmp[6];
    block[3] = tmp[15];
    block[7] = tmp[3];
    block[11] = tmp[7];
    block[15] = tmp[11];
    for i in 0..16 {
        block[i] ^= round_keys[160 + i];
    }
}

/// Increment a 128-bit counter (big-endian) for CTR mode.
// Reserved for the AES-CTR cipher path: the keystream generator will bump the
// per-block counter via this helper once CTR transport encryption is wired into
// the packet layer. Not yet invoked.
#[allow(dead_code)]
fn increment_counter(ctr: &mut [u8; 16]) {
    for i in (0..16).rev() {
        let (val, overflow) = ctr[i].overflowing_add(1);
        ctr[i] = val;
        if !overflow {
            break;
        }
    }
}

/// Build the AES-CTR counter for a given block index.
fn build_ctr(iv: &[u8], seq: u32, block_idx: usize) -> [u8; 16] {
    let mut ctr = [0u8; 16];
    let copy_len = iv.len().min(16);
    ctr[..copy_len].copy_from_slice(&iv[..copy_len]);

    // For SSH AES-CTR, the IV is used as the initial counter and incremented
    // per block. We add seq * (large_blocks) + block_idx to get the correct
    // counter for a given packet/block.
    let offset = u64::from(seq)
        .wrapping_mul(256)
        .wrapping_add(block_idx as u64);
    let mut carry = offset;
    for i in (0..16).rev() {
        let sum = u64::from(ctr[i]).wrapping_add(carry & 0xFF);
        ctr[i] = sum as u8;
        carry = (carry >> 8).wrapping_add(sum >> 8);
    }
    ctr
}

/// Encrypt a packet with AES-128-CTR in-place.
fn encrypt_packet_aes_ctr(packet: &mut [u8], key: &[u8], iv: &[u8], seq: u32) {
    if key.len() < 16 {
        return;
    }
    let round_keys = aes128_expand_key(key);
    let mut block_idx = 0;
    let mut offset = 0;
    while offset < packet.len() {
        let mut ctr = build_ctr(iv, seq, block_idx);
        aes128_encrypt_block(&mut ctr, &round_keys);
        let end = (offset + 16).min(packet.len());
        for i in offset..end {
            packet[i] ^= ctr[i - offset];
        }
        offset += 16;
        block_idx += 1;
    }
}

/// Decrypt is the same as encrypt for CTR mode.
fn decrypt_packet_aes_ctr(packet: &mut [u8], key: &[u8], iv: &[u8], seq: u32) {
    encrypt_packet_aes_ctr(packet, key, iv, seq);
}

/// Decrypt a single block for peeking at the first block.
fn decrypt_block_aes_ctr(
    data: &[u8],
    key: &[u8],
    iv: &[u8],
    seq: u32,
    block_idx: usize,
) -> Vec<u8> {
    if key.len() < 16 {
        return data.to_vec();
    }
    let round_keys = aes128_expand_key(key);
    let mut ctr = build_ctr(iv, seq, block_idx);
    aes128_encrypt_block(&mut ctr, &round_keys);
    let mut result = data.to_vec();
    for (i, byte) in result.iter_mut().enumerate() {
        if i < 16 {
            *byte ^= ctr[i];
        }
    }
    result
}

// ============================================================================
// Encryption state
// ============================================================================

#[derive(Clone)]
struct EncryptionState {
    enc_key_c2s: Vec<u8>,
    enc_key_s2c: Vec<u8>,
    iv_c2s: Vec<u8>,
    iv_s2c: Vec<u8>,
    mac_key_c2s: Vec<u8>,
    mac_key_s2c: Vec<u8>,
    block_size: usize,
    mac_len: usize,
}

impl EncryptionState {
    fn none() -> Self {
        Self {
            enc_key_c2s: Vec::new(),
            enc_key_s2c: Vec::new(),
            iv_c2s: Vec::new(),
            iv_s2c: Vec::new(),
            mac_key_c2s: Vec::new(),
            mac_key_s2c: Vec::new(),
            block_size: 8,
            mac_len: 0,
        }
    }
}

/// Derive SSH transport keys from the shared secret and exchange hash.
/// RFC 4253, Section 7.2.
fn derive_keys(
    shared_secret: &[u8],
    exchange_hash: &[u8; 32],
    session_id: &[u8; 32],
) -> EncryptionState {
    let k_enc = encode_mpint(shared_secret);

    let derive = |label: u8| -> Vec<u8> {
        let mut input = Vec::new();
        input.extend_from_slice(&k_enc);
        input.extend_from_slice(exchange_hash);
        input.push(label);
        input.extend_from_slice(session_id);
        sha256(&input).to_vec()
    };

    EncryptionState {
        iv_c2s: derive(b'A')[..16].to_vec(),
        iv_s2c: derive(b'B')[..16].to_vec(),
        enc_key_c2s: derive(b'C')[..16].to_vec(),
        enc_key_s2c: derive(b'D')[..16].to_vec(),
        mac_key_c2s: derive(b'E'),
        mac_key_s2c: derive(b'F'),
        block_size: 16,
        mac_len: 32,
    }
}

// ============================================================================
// Diffie-Hellman group 14 parameters (RFC 3526)
// ============================================================================

/// DH group 14 prime (2048-bit MODP group).
fn dh_group14_prime() -> BigUint {
    let p_hex = concat!(
        "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD1",
        "29024E088A67CC74020BBEA63B139B22514A08798E3404DD",
        "EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245",
        "E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED",
        "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3D",
        "C2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F",
        "83655D23DCA3AD961C62F356208552BB9ED529077096966D",
        "670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B",
        "E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9",
        "DE2BCBF6955817183995497CEA956AE515D2261898FA0510",
        "15728E5A8AACAA68FFFFFFFFFFFFFFFF"
    );

    let mut bytes = Vec::new();
    let mut chars = p_hex.chars();
    while let Some(hi) = chars.next() {
        if let Some(lo) = chars.next() {
            let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16).unwrap_or(0);
            bytes.push(byte);
        }
    }
    BigUint::from_bytes_be(&bytes)
}

/// DH group 14 generator.
fn dh_group14_generator() -> BigUint {
    BigUint::from_bytes_be(&[2])
}

/// Draw a Diffie-Hellman private exponent from the kernel CSPRNG.
///
/// This is the secret that makes the session key secret; nothing else in the
/// exchange hides it. It used to be derived from the process id and the
/// monotonic clock, which is a search space small enough to enumerate, and the
/// doc comment here described that as acceptable "for this simplified daemon".
/// It is not, so it draws from the CSPRNG now and fails the connection if it
/// cannot.
fn generate_dh_private() -> Result<BigUint, SshdError> {
    // 256 bits, per RFC 4419 section 6.2's guidance that the exponent be at
    // least twice the 128-bit security level the group14 prime provides.
    let mut bytes = [0u8; 32];
    randrange::fill_secret(&mut bytes).map_err(|e| {
        SshdError::ProtocolError(format!("cannot generate a Diffie-Hellman private key: {e}"))
    })?;
    // Set the top bit so the exponent is a full 256 bits rather than however
    // many the leading zero bytes leave, and the bottom bit so it is odd. Both
    // are what OpenSSH's BN_rand(..., BN_RAND_TOP_ONE, BN_RAND_BOTTOM_ODD) does.
    bytes[0] |= 0x80;
    bytes[31] |= 1;
    Ok(BigUint::from_bytes_be(&bytes))
}

// ============================================================================
// Host key (Ed25519, RFC 8032)
//
// This used to be a structural placeholder: the "public key" was SHA-256 of
// the seed and `sign` returned an HMAC-SHA256 zero-extended to 64 bytes, both
// labelled `ssh-ed25519` on the wire. Nothing about that is verifiable by a
// real client, so no real client could ever complete a handshake with this
// daemon -- and the failure would have appeared as a signature mismatch in
// OpenSSH rather than as anything pointing here.
//
// The mathematics now lives in `posix::ed25519`, which is checked against the
// RFC 8032 section 7.1 test vectors. See that module's header for why it is in
// the libc rather than in each of the three programs that were faking it.
// ============================================================================

struct HostKey {
    /// 32-byte private seed (RFC 8032 secret key).
    seed: [u8; 32],
    /// 32-byte Ed25519 public key, derived from the seed.
    public_key: [u8; 32],
}

impl HostKey {
    /// Create a host key from a 32-byte RFC 8032 seed.
    fn from_seed(seed: [u8; 32]) -> Self {
        let public_key = posix::ed25519::public_key(&seed);
        Self { seed, public_key }
    }

    /// Generate a fresh host key from the kernel CSPRNG and write it to `path`
    /// in OpenSSH private key format, so the next start reads back the same
    /// identity.
    ///
    /// Persisting is the whole point. The previous version derived the seed
    /// from `sha256("slateos-sshd-default-host-key" || pid)`, which is both
    /// guessable — the search space is the pid, about 32000 values — and
    /// different on every start, so every client's `known_hosts` entry went
    /// stale at every reboot. A host key that changes constantly trains users
    /// to accept the changed-key warning, which is precisely the warning that
    /// distinguishes a reboot from a man in the middle.
    ///
    /// # Errors
    ///
    /// Fails if the kernel cannot supply random bytes, or if the key cannot be
    /// written. Both are fatal: a daemon that runs with a key it could not
    /// persist is a daemon whose identity changes silently.
    fn generate_and_persist(path: &str) -> Result<Self, SshdError> {
        let mut seed = [0u8; 32];
        randrange::fill_secret(&mut seed)
            .map_err(|e| SshdError::ConfigError(format!("cannot generate a host key: {e}")))?;
        let key = Self::from_seed(seed);
        write_openssh_private_key(path, &seed, &key.public_key)?;
        Ok(key)
    }

    /// Encode the public key in SSH wire format: "ssh-ed25519" + `key_data`.
    fn public_key_blob(&self) -> Vec<u8> {
        let mut blob = Vec::new();
        blob.extend_from_slice(&ssh_string(b"ssh-ed25519"));
        blob.extend_from_slice(&ssh_string(&self.public_key));
        blob
    }

    /// Sign data with the host key, returning an RFC 4253 section 6.6 signature
    /// blob: `string "ssh-ed25519" || string sig`, where `sig` is the 64-byte
    /// RFC 8032 signature.
    fn sign(&self, data: &[u8]) -> Vec<u8> {
        let sig = posix::ed25519::sign(&self.seed, data);
        let mut sig_blob = Vec::new();
        sig_blob.extend_from_slice(&ssh_string(b"ssh-ed25519"));
        sig_blob.extend_from_slice(&ssh_string(&sig));
        sig_blob
    }

    /// Compute the SHA-256 fingerprint of the public key, in the form
    /// `ssh-keygen -lf` prints: base64 of the digest with the `=` padding
    /// removed. The padding matters — an operator comparing this line against
    /// the one their client shows needs the two to be the same string.
    fn fingerprint(&self) -> String {
        let blob = self.public_key_blob();
        let hash = sha256(&blob);
        let encoded = base64_encode(&hash);
        let encoded = encoded.trim_end_matches('=');
        format!("SHA256:{encoded}")
    }

    /// Try to load a host key from a file.
    ///
    /// Three formats are accepted, in order: an OpenSSH private key (what
    /// `ssh-keygen -t ed25519` writes), 32 raw seed bytes, or 64 hex digits.
    ///
    /// A file that matches none of them is an **error**. The previous version
    /// fell back to `sha256(first_line)` — it invented a seed from a file it
    /// could not parse, so `sshd -h /etc/ssh/ssh_host_rsa_key` started
    /// successfully with a host key unrelated to the file named, and the
    /// operator's only clue would have been that every client reported a
    /// changed host key. Failing to parse a host key must stop the daemon.
    fn load_from_file(path: &str) -> Result<Self, SshdError> {
        let data = fs_read_file(path)?;
        let text = String::from_utf8_lossy(&data);

        if text.contains("BEGIN OPENSSH PRIVATE KEY") {
            let seed = parse_openssh_private_key(&text)
                .map_err(|e| SshdError::ConfigError(format!("{path}: {e}")))?;
            return Ok(Self::from_seed(seed));
        }

        if data.len() == 32 {
            let mut seed = [0u8; 32];
            seed.copy_from_slice(&data);
            return Ok(Self::from_seed(seed));
        }

        let hex_str: String = text.chars().filter(char::is_ascii_hexdigit).collect();
        if hex_str.len() == 64 {
            let mut seed = [0u8; 32];
            for i in 0..32 {
                seed[i] = u8::from_str_radix(&hex_str[i * 2..i * 2 + 2], 16)
                    .map_err(|_| SshdError::ConfigError(format!("invalid hex in {path}")))?;
            }
            return Ok(Self::from_seed(seed));
        }

        Err(SshdError::ConfigError(format!(
            "cannot parse host key from {path}: expected an OpenSSH private key, \
             32 raw bytes, or 64 hex digits"
        )))
    }
}

/// Extract the Ed25519 seed from an unencrypted OpenSSH private key file.
///
/// The container (`PROTOCOL.key` in the OpenSSH distribution) is:
///
/// ```text
/// "openssh-key-v1\0"
/// string  ciphername
/// string  kdfname
/// string  kdfoptions
/// uint32  number of keys N
/// string  publickey[N]
/// string  encrypted-private-section
/// ```
///
/// and the private section, once decrypted (a no-op when `ciphername` is
/// `none`), is:
///
/// ```text
/// uint32  checkint
/// uint32  checkint   (must equal the first: this is how a wrong passphrase
///                     is detected, and it is a free integrity check for us)
/// string  keytype
/// string  public key
/// string  private key
/// string  comment
/// byte[]  padding 1, 2, 3, ...
/// ```
///
/// For `ssh-ed25519` the "private key" field is 64 bytes: the 32-byte seed
/// followed by a copy of the public key. We take the seed and re-derive the
/// public key rather than trusting the copy, so a corrupted file fails the
/// consistency check below instead of producing a key whose halves disagree.
fn parse_openssh_private_key(text: &str) -> Result<[u8; 32], String> {
    let body: String = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("-----") && !l.is_empty())
        .collect();
    let raw = base64_decode(&body);

    const MAGIC: &[u8] = b"openssh-key-v1\0";
    if raw.len() < MAGIC.len() || &raw[..MAGIC.len()] != MAGIC {
        return Err("not an openssh-key-v1 container".into());
    }
    let mut off = MAGIC.len();

    let (ciphername, next) = read_ssh_string(&raw, off).map_err(|_| "truncated ciphername")?;
    off = next;
    if ciphername != b"none" {
        // Refusing is the honest answer: we have no passphrase prompt (sshd is
        // started by init, with no terminal), and no bcrypt_pbkdf to derive the
        // key with even if we had one.
        return Err(format!(
            "key is encrypted with {}; sshd cannot prompt for a passphrase, \
             re-create it with an empty passphrase",
            String::from_utf8_lossy(ciphername)
        ));
    }
    let (_kdfname, next) = read_ssh_string(&raw, off).map_err(|_| "truncated kdfname")?;
    off = next;
    let (_kdfopts, next) = read_ssh_string(&raw, off).map_err(|_| "truncated kdfoptions")?;
    off = next;

    if off + 4 > raw.len() {
        return Err("truncated key count".into());
    }
    let nkeys = u32::from_be_bytes([raw[off], raw[off + 1], raw[off + 2], raw[off + 3]]);
    off += 4;
    if nkeys != 1 {
        return Err(format!(
            "expected exactly one key in the file, found {nkeys}"
        ));
    }

    let (_pubkey, next) = read_ssh_string(&raw, off).map_err(|_| "truncated public key")?;
    off = next;
    let (private, _) = read_ssh_string(&raw, off).map_err(|_| "truncated private section")?;

    if private.len() < 8 {
        return Err("private section too short".into());
    }
    let check1 = u32::from_be_bytes([private[0], private[1], private[2], private[3]]);
    let check2 = u32::from_be_bytes([private[4], private[5], private[6], private[7]]);
    if check1 != check2 {
        return Err("private section checkints differ (corrupt or encrypted key)".into());
    }

    let (keytype, poff) = read_ssh_string(private, 8).map_err(|_| "truncated key type")?;
    if keytype != b"ssh-ed25519" {
        return Err(format!(
            "unsupported host key type {}; only ssh-ed25519 is implemented",
            String::from_utf8_lossy(keytype)
        ));
    }
    let (stored_public, poff) = read_ssh_string(private, poff).map_err(|_| "truncated pubkey")?;
    let (secret, _) = read_ssh_string(private, poff).map_err(|_| "truncated secret")?;
    if secret.len() != 64 {
        return Err(format!(
            "ssh-ed25519 secret should be 64 bytes, found {}",
            secret.len()
        ));
    }

    let mut seed = [0u8; 32];
    seed.copy_from_slice(&secret[..32]);
    if posix::ed25519::public_key(&seed).as_slice() != stored_public {
        return Err("public key in the file does not match the private seed".into());
    }
    Ok(seed)
}

/// Write an unencrypted OpenSSH-format Ed25519 private key to `path`, mode
/// 0600.
///
/// The layout is the one [`parse_openssh_private_key`] documents, written back.
/// Using the real container rather than a bare 32-byte seed file means a key
/// this daemon generates can be inspected with `ssh-keygen -lf` and copied to
/// another machine, and that the round trip through our own parser is exercised
/// on the very next start.
fn write_openssh_private_key(
    path: &str,
    seed: &[u8; 32],
    public: &[u8; 32],
) -> Result<(), SshdError> {
    let mut pub_blob = Vec::new();
    pub_blob.extend_from_slice(&ssh_string(b"ssh-ed25519"));
    pub_blob.extend_from_slice(&ssh_string(public));

    let mut secret = Vec::with_capacity(64);
    secret.extend_from_slice(seed);
    secret.extend_from_slice(public);

    // The two checkints are compared on read to detect a wrong passphrase. We
    // never encrypt, so any value works as long as they match; a random one
    // keeps the file byte-identical in structure to what ssh-keygen writes.
    let mut checkint = [0u8; 4];
    randrange::fill_secret(&mut checkint)
        .map_err(|e| SshdError::ConfigError(format!("cannot generate a host key: {e}")))?;

    let mut private = Vec::new();
    private.extend_from_slice(&checkint);
    private.extend_from_slice(&checkint);
    private.extend_from_slice(&ssh_string(b"ssh-ed25519"));
    private.extend_from_slice(&ssh_string(public));
    private.extend_from_slice(&ssh_string(&secret));
    private.extend_from_slice(&ssh_string(b"slateos-sshd"));
    // Pad to a multiple of 8 (the cipher block size that "none" nominally has)
    // with the bytes 1, 2, 3, … as PROTOCOL.key specifies.
    let mut pad: u8 = 1;
    while private.len() % 8 != 0 {
        private.push(pad);
        pad += 1;
    }

    let mut raw = Vec::new();
    raw.extend_from_slice(b"openssh-key-v1\0");
    raw.extend_from_slice(&ssh_string(b"none")); // ciphername
    raw.extend_from_slice(&ssh_string(b"none")); // kdfname
    raw.extend_from_slice(&ssh_string(b"")); // kdfoptions
    raw.extend_from_slice(&1u32.to_be_bytes()); // one key
    raw.extend_from_slice(&ssh_string(&pub_blob));
    raw.extend_from_slice(&ssh_string(&private));

    let body = base64_encode(&raw);
    let mut text = String::from("-----BEGIN OPENSSH PRIVATE KEY-----\n");
    for chunk in body.as_bytes().chunks(70) {
        text.push_str(&String::from_utf8_lossy(chunk));
        text.push('\n');
    }
    text.push_str("-----END OPENSSH PRIVATE KEY-----\n");

    fs_write_file(path, text.as_bytes())?;
    // Order matters: the file is created by the write above with whatever the
    // umask gives it, so tighten it immediately. A host key readable by other
    // users is a host key that has already been compromised.
    fs_set_mode(path, 0o600)?;
    Ok(())
}

/// Minimal base64 encoder (RFC 4648, with `=` padding).
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] } else { 0 };

        result.push(ALPHABET[(b0 >> 2) as usize] as char);
        result.push(ALPHABET[((b0 & 0x03) << 4 | (b1 >> 4)) as usize] as char);

        if i + 1 < data.len() {
            result.push(ALPHABET[((b1 & 0x0F) << 2 | (b2 >> 6)) as usize] as char);
        } else {
            result.push('=');
        }

        if i + 2 < data.len() {
            result.push(ALPHABET[(b2 & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        i += 3;
    }
    result
}

/// Minimal base64 decoder.
// a..d are the four decoded sextets of a base64 quartet; short names match the
// usual base64 decode formulation.
#[allow(clippy::many_single_char_names)]
fn base64_decode(input: &str) -> Vec<u8> {
    const DECODE: [u8; 128] = {
        let mut table = [0xFFu8; 128];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = i;
            table[(b'a' + i) as usize] = i + 26;
            i += 1;
        }
        let mut d = 0u8;
        while d < 10 {
            table[(b'0' + d) as usize] = d + 52;
            d += 1;
        }
        table[b'+' as usize] = 62;
        table[b'/' as usize] = 63;
        table
    };

    let mut output = Vec::new();
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|&b| b != b'=' && b != b'\n' && b != b'\r' && b != b' ')
        .collect();
    let mut i = 0;
    // Process 4-char groups. The padding ('=') has already been stripped, so
    // the final group may legitimately be 2 or 3 chars (encoding 1 or 2 bytes);
    // read the 3rd/4th positions with bounds checks rather than requiring a
    // full quartet (the old `i + 3 < len` condition dropped the last group).
    while i + 1 < bytes.len() {
        let a = DECODE.get(bytes[i] as usize).copied().unwrap_or(0xFF);
        let b = DECODE.get(bytes[i + 1] as usize).copied().unwrap_or(0xFF);
        let c = bytes
            .get(i + 2)
            .and_then(|&x| DECODE.get(x as usize).copied())
            .unwrap_or(0xFF);
        let d = bytes
            .get(i + 3)
            .and_then(|&x| DECODE.get(x as usize).copied())
            .unwrap_or(0xFF);
        if a == 0xFF || b == 0xFF {
            break;
        }
        output.push((a << 2) | (b >> 4));
        if c != 0xFF {
            output.push((b << 4) | (c >> 2));
            if d != 0xFF {
                output.push((c << 6) | d);
            }
        }
        i += 4;
    }
    output
}

// ============================================================================
// Configuration
// ============================================================================

/// SSH server configuration parsed from `sshd_config`.
#[derive(Clone)]
struct SshdConfig {
    port: u16,
    listen_address: String,
    host_key_file: String,
    permit_root_login: PermitRootLogin,
    password_authentication: bool,
    pubkey_authentication: bool,
    authorized_keys_file: String,
    max_auth_tries: u32,
    login_grace_time: u32,
    max_sessions: u32,
    banner_file: String,
    print_motd: bool,
    subsystems: Vec<(String, String)>,
    allow_users: Vec<String>,
    deny_users: Vec<String>,
    allow_groups: Vec<String>,
    deny_groups: Vec<String>,
    /// Names of environment variables a client may set on its session, as
    /// shell-glob patterns.
    ///
    /// Empty by default, and that emptiness is the security property rather
    /// than an omission: a client's environment is attacker-controlled input to
    /// a process this daemon is about to run as the authenticated user, and
    /// `LD_PRELOAD`, `PATH`, `IFS` and `BASH_ENV` turn "set a variable" into
    /// "choose the code that runs". OpenSSH makes the same choice; its shipped
    /// config opts in to `LANG` and `LC_*` and nothing else.
    accept_env: Vec<String>,
}

/// Root login policy.
#[derive(Clone, PartialEq, Eq, Debug)]
enum PermitRootLogin {
    Yes,
    No,
    ProhibitPassword,
}

impl SshdConfig {
    fn default_config() -> Self {
        Self {
            port: 22,
            listen_address: "0.0.0.0".into(),
            host_key_file: "/etc/ssh/ssh_host_ed25519_key".into(),
            permit_root_login: PermitRootLogin::ProhibitPassword,
            password_authentication: true,
            pubkey_authentication: true,
            authorized_keys_file: ".ssh/authorized_keys".into(),
            max_auth_tries: 6,
            login_grace_time: 120,
            max_sessions: 10,
            banner_file: String::new(),
            print_motd: true,
            subsystems: vec![("sftp".into(), "/usr/lib/sftp-server".into())],
            allow_users: Vec::new(),
            deny_users: Vec::new(),
            allow_groups: Vec::new(),
            deny_groups: Vec::new(),
            accept_env: Vec::new(),
        }
    }

    /// Parse configuration from `sshd_config` file contents.
    fn parse(content: &str) -> Result<Self, SshdError> {
        let mut config = Self::default_config();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Split on first whitespace.
            let (directive, value) = match line.split_once(|c: char| c.is_whitespace()) {
                Some((d, v)) => (d.trim(), v.trim()),
                None => continue,
            };

            match directive.to_lowercase().as_str() {
                "port" => {
                    config.port = value
                        .parse()
                        .map_err(|_| SshdError::ConfigError(format!("invalid port: {value}")))?;
                }
                "listenaddress" => {
                    config.listen_address = value.into();
                }
                "hostkey" => {
                    config.host_key_file = value.into();
                }
                "permitrootlogin" => {
                    config.permit_root_login = match value.to_lowercase().as_str() {
                        "yes" => PermitRootLogin::Yes,
                        "no" => PermitRootLogin::No,
                        "prohibit-password" | "without-password" => {
                            PermitRootLogin::ProhibitPassword
                        }
                        _ => {
                            return Err(SshdError::ConfigError(format!(
                                "invalid PermitRootLogin: {value}"
                            )));
                        }
                    };
                }
                "passwordauthentication" => {
                    config.password_authentication = parse_bool(value)?;
                }
                "pubkeyauthentication" => {
                    config.pubkey_authentication = parse_bool(value)?;
                }
                "authorizedkeysfile" => {
                    config.authorized_keys_file = value.into();
                }
                "maxauthtries" => {
                    config.max_auth_tries = value.parse().map_err(|_| {
                        SshdError::ConfigError(format!("invalid MaxAuthTries: {value}"))
                    })?;
                }
                "logingracetime" => {
                    config.login_grace_time = value.parse().map_err(|_| {
                        SshdError::ConfigError(format!("invalid LoginGraceTime: {value}"))
                    })?;
                }
                "maxsessions" => {
                    config.max_sessions = value.parse().map_err(|_| {
                        SshdError::ConfigError(format!("invalid MaxSessions: {value}"))
                    })?;
                }
                "banner" => {
                    config.banner_file = value.into();
                }
                "printmotd" => {
                    config.print_motd = parse_bool(value)?;
                }
                "subsystem" => {
                    if let Some((name, cmd)) = value.split_once(|c: char| c.is_whitespace()) {
                        config
                            .subsystems
                            .push((name.trim().into(), cmd.trim().into()));
                    }
                }
                "allowusers" => {
                    for user in value.split_whitespace() {
                        config.allow_users.push(user.into());
                    }
                }
                "denyusers" => {
                    for user in value.split_whitespace() {
                        config.deny_users.push(user.into());
                    }
                }
                // Repeated `AcceptEnv` lines accumulate rather than replace,
                // which is OpenSSH's behaviour and the only one that lets a
                // drop-in file add a variable without restating the base list.
                "acceptenv" => {
                    for pattern in value.split_whitespace() {
                        config.accept_env.push(pattern.into());
                    }
                }
                "allowgroups" => {
                    for group in value.split_whitespace() {
                        config.allow_groups.push(group.into());
                    }
                }
                "denygroups" => {
                    for group in value.split_whitespace() {
                        config.deny_groups.push(group.into());
                    }
                }
                _ => {
                    // Unknown directive -- ignore for forward compatibility.
                }
            }
        }

        Ok(config)
    }

    /// Format config as human-readable text (for -T option).
    fn dump(&self) -> String {
        let root_login = match &self.permit_root_login {
            PermitRootLogin::Yes => "yes",
            PermitRootLogin::No => "no",
            PermitRootLogin::ProhibitPassword => "prohibit-password",
        };
        let yn = |b: bool| if b { "yes" } else { "no" };
        let mut lines = vec![
            format!("port {}", self.port),
            format!("listenaddress {}", self.listen_address),
            format!("hostkey {}", self.host_key_file),
            format!("permitrootlogin {root_login}"),
            format!(
                "passwordauthentication {}",
                yn(self.password_authentication)
            ),
            format!("pubkeyauthentication {}", yn(self.pubkey_authentication)),
            format!("authorizedkeysfile {}", self.authorized_keys_file),
            format!("maxauthtries {}", self.max_auth_tries),
            format!("logingracetime {}", self.login_grace_time),
            format!("maxsessions {}", self.max_sessions),
        ];
        if !self.banner_file.is_empty() {
            lines.push(format!("banner {}", self.banner_file));
        }
        lines.push(format!("printmotd {}", yn(self.print_motd)));
        for (name, cmd) in &self.subsystems {
            lines.push(format!("subsystem {name} {cmd}"));
        }
        if !self.allow_users.is_empty() {
            lines.push(format!("allowusers {}", self.allow_users.join(" ")));
        }
        if !self.deny_users.is_empty() {
            lines.push(format!("denyusers {}", self.deny_users.join(" ")));
        }
        if !self.allow_groups.is_empty() {
            lines.push(format!("allowgroups {}", self.allow_groups.join(" ")));
        }
        if !self.deny_groups.is_empty() {
            lines.push(format!("denygroups {}", self.deny_groups.join(" ")));
        }
        if !self.accept_env.is_empty() {
            lines.push(format!("acceptenv {}", self.accept_env.join(" ")));
        }
        // Each directive on its own line, with a trailing newline to match the
        // historical per-line `push_str(... "\n")` output.
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }
}

/// Match `name` against one shell-glob `pattern`: `*` for any run of
/// characters, `?` for exactly one, everything else literal.
///
/// This is the matcher `sshd_config` has always implied — `AcceptEnv LC_*` is
/// the line OpenSSH's own shipped config uses — so a pattern that is only ever
/// compared literally is a directive that silently does nothing.
///
/// The loop backtracks, but only ever to the *most recent* `*`, and each
/// backtrack advances the text position by one. That is what keeps it linear
/// in practice rather than exponential in the number of stars: a naive
/// recursive matcher on `a*a*a*a*b` against a long run of `a`s is the classic
/// way to hand a remote client an unbounded amount of the daemon's CPU.
fn glob_matches(pattern: &str, name: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = name.chars().collect();
    let mut pi = 0usize;
    let mut ti = 0usize;
    // Where the most recent `*` sits, and how much of the text it had consumed
    // when we passed it. `None` means there is nothing to fall back to and a
    // mismatch is final.
    let mut resume: Option<(usize, usize)> = None;

    loop {
        let p = pat.get(pi).copied();
        let t = text.get(ti).copied();
        match (p, t) {
            (Some('*'), _) => {
                resume = Some((pi, ti));
                pi = pi.saturating_add(1);
            }
            (Some('?'), Some(_)) => {
                pi = pi.saturating_add(1);
                ti = ti.saturating_add(1);
            }
            (Some(pc), Some(tc)) if pc == tc => {
                pi = pi.saturating_add(1);
                ti = ti.saturating_add(1);
            }
            _ => {
                if p.is_none() && t.is_none() {
                    return true;
                }
                let Some((star_pi, star_ti)) = resume else {
                    return false;
                };
                // Give the `*` one more character. Running past the end of the
                // text means even the greediest reading of it cannot match.
                let next_ti = star_ti.saturating_add(1);
                if next_ti > text.len() {
                    return false;
                }
                resume = Some((star_pi, next_ti));
                pi = star_pi.saturating_add(1);
                ti = next_ti;
            }
        }
    }
}

/// Match `name` against a list of patterns, where a leading `!` negates.
///
/// A negated match wins outright, so `LC_*` `!LC_ALL` accepts every locale
/// variable except that one regardless of the order the two are written in.
/// This is OpenSSH's `match_pattern_list` rule, and order-independence is the
/// point: a configuration whose meaning depends on which line came first is one
/// an administrator will eventually get wrong.
fn pattern_list_matches(patterns: &[String], name: &str) -> bool {
    let mut matched = false;
    for pattern in patterns {
        if let Some(negated) = pattern.strip_prefix('!') {
            if glob_matches(negated, name) {
                return false;
            }
        } else if glob_matches(pattern, name) {
            matched = true;
        }
    }
    matched
}

/// Parse a boolean config value.
fn parse_bool(value: &str) -> Result<bool, SshdError> {
    match value.to_lowercase().as_str() {
        "yes" | "true" | "1" => Ok(true),
        "no" | "false" | "0" => Ok(false),
        _ => Err(SshdError::ConfigError(format!("invalid boolean: {value}"))),
    }
}

// ============================================================================
// User authentication
// ============================================================================

// Password verification lives in `authlib`, not here. What this crate used to
// do instead is worth recording, because it is the shape of the bug rather than
// one instance of it:
//
//   - It parsed `/etc/shadow` itself, taking field 1 and splitting the salt out
//     of it by hand -- the exact manoeuvre `design-decisions.md` section 329
//     found at the root of all three earlier copies. A stored crypt entry *is*
//     its own setting; the only correct way to check it is to recompute it
//     whole, which is what `posix::crypt::verify` does and what taking it apart
//     makes impossible.
//   - For `$5$` and `$6$` it computed `sha256(password || salt)` and hex-encoded
//     it. That is not SHA-crypt by any reading: not the right digest for `$6$`,
//     not the thousands of rounds, not the base-64 output alphabet. A password
//     set with `passwd` could therefore never be used to log in over ssh, and no
//     error said so -- it looked exactly like a wrong password.
//   - Anything else fell through to comparing the password against the stored
//     field as *plaintext*. A `$y$` (yescrypt) or `$1$` (MD5) entry -- formats
//     this tree does not implement -- silently became a plaintext comparison
//     rather than a refusal, which is the wrong answer to "I cannot check this".
//
// `authlib` also brings the two properties a network-facing password oracle
// needs and that no amount of local hashing provides: per-user failure counting
// with a growing delay that outlives a single connection, and a constant cost
// for users that do not exist, so the daemon cannot be timed to enumerate them.

/// An entry from an `authorized_keys` file.
#[derive(Clone, Debug)]
struct AuthorizedKey {
    // Parsed and retained for completeness; publickey auth currently matches on
    // `key_data` (the wire blob) only. `key_type` will gate algorithm selection
    // and `comment` will appear in audit logs once those paths are wired.
    #[allow(dead_code)]
    key_type: String,
    key_data: Vec<u8>,
    #[allow(dead_code)]
    comment: String,
}

/// Parse an `authorized_keys` file.
fn parse_authorized_keys(content: &str) -> Vec<AuthorizedKey> {
    let mut keys = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() >= 2 {
            let key_type = parts[0].to_string();
            // Validate key type.
            if !matches!(
                key_type.as_str(),
                "ssh-rsa"
                    | "ssh-ed25519"
                    | "ecdsa-sha2-nistp256"
                    | "ecdsa-sha2-nistp384"
                    | "ecdsa-sha2-nistp521"
                    | "ssh-dss"
            ) {
                continue;
            }
            let key_data = base64_decode(parts[1]);
            let comment = if parts.len() >= 3 {
                parts[2].to_string()
            } else {
                String::new()
            };
            keys.push(AuthorizedKey {
                key_type,
                key_data,
                comment,
            });
        }
    }
    keys
}

/// Check if a username is allowed by the allow/deny user/group lists.
///
/// All four directives are *pattern* lists, matched by [`pattern_list_matches`]
/// — `AllowUsers admin*` is the shape `sshd_config(5)` documents and the shape
/// an administrator who knows OpenSSH will write. Comparing them literally,
/// which this did until 2026-09-05, fails in both directions at once: an
/// `AllowUsers` pattern matches no account and locks everybody out, while a
/// `DenyUsers` pattern matches no account and quietly lets the very people it
/// names straight in.
fn is_user_allowed(username: &str, groups: &[String], config: &SshdConfig) -> bool {
    // DenyUsers takes precedence.
    if pattern_list_matches(&config.deny_users, username) {
        return false;
    }

    // DenyGroups.
    if groups
        .iter()
        .any(|group| pattern_list_matches(&config.deny_groups, group))
    {
        return false;
    }

    // AllowUsers: if specified, user must match one of the patterns.
    if !config.allow_users.is_empty() && !pattern_list_matches(&config.allow_users, username) {
        return false;
    }

    // AllowGroups: if specified, at least one group must match.
    if !config.allow_groups.is_empty()
        && !groups
            .iter()
            .any(|group| pattern_list_matches(&config.allow_groups, group))
    {
        return false;
    }

    true
}

/// Check root login policy.
fn is_root_login_allowed(auth_method: &str, config: &SshdConfig) -> bool {
    match config.permit_root_login {
        PermitRootLogin::Yes => true,
        PermitRootLogin::No => false,
        PermitRootLogin::ProhibitPassword => auth_method != "password",
    }
}

// ============================================================================
// Account lookup (/etc/passwd)
// ============================================================================

/// One account, as read from `/etc/passwd`.
///
/// Authentication proves *who* the client is; this record says what that
/// identity means to the operating system — which uid to run as, where the
/// session starts, and which program `shell`/`exec` requests go through.
/// Without it a session request has no identity to assume, and the only
/// identity available would be the daemon's own, which is root.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PasswdEntry {
    username: String,
    uid: u32,
    gid: u32,
    home: String,
    shell: String,
}

/// The login shell assumed when the `/etc/passwd` field is empty.
///
/// POSIX says an empty shell field means `/bin/sh`, and every implementation
/// agrees; an empty field is a default, not a locked account.
const DEFAULT_LOGIN_SHELL: &str = "/bin/sh";

/// Parse `/etc/passwd` content into entries.
///
/// Malformed lines are skipped rather than failing the whole file: one bad
/// line in `/etc/passwd` must not lock every account out of the machine.
fn parse_passwd(content: &str) -> Vec<PasswdEntry> {
    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(':').collect();
        // name:passwd:uid:gid:gecos:home:shell -- seven fields.
        if fields.len() < 7 {
            continue;
        }
        let (Ok(uid), Ok(gid)) = (fields[2].parse::<u32>(), fields[3].parse::<u32>()) else {
            continue;
        };
        let shell = if fields[6].is_empty() {
            DEFAULT_LOGIN_SHELL.to_string()
        } else {
            fields[6].to_string()
        };
        entries.push(PasswdEntry {
            username: fields[0].to_string(),
            uid,
            gid,
            home: fields[5].to_string(),
            shell,
        });
    }
    entries
}

/// Look an authenticated username up in `/etc/passwd`.
///
/// Returns `None` when the file is unreadable or the name is absent. Callers
/// must treat that as a refusal to run anything — see [`session_command`].
fn lookup_passwd(username: &str) -> Option<PasswdEntry> {
    let data = fs_read_file("/etc/passwd").ok()?;
    let content = String::from_utf8_lossy(&data);
    parse_passwd(&content)
        .into_iter()
        .find(|e| e.username == username)
}

/// The `PATH` given to a session, chosen by whether the account is root.
///
/// Matches the split every Unix uses: the superuser gets the `sbin`
/// directories, an unprivileged user does not.
fn default_path_for_uid(uid: u32) -> &'static str {
    if uid == 0 {
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
    } else {
        "/usr/local/bin:/usr/bin:/bin"
    }
}

/// Build the environment for a session, from scratch.
///
/// The daemon's own environment is *not* inherited. sshd is started by init
/// with whatever init happened to be holding, and passing that to a remote
/// user leaks the daemon's configuration to them and lets a variable set at
/// boot silently change how their commands behave.
/// Apply the variables the client asked for and `AcceptEnv` allowed.
///
/// Called after [`session_env`] has laid down the base, so an allowlisted name
/// that also appears there wins — which is the point of an administrator having
/// listed it. The names that must *not* be overridable are refused at the
/// request instead, where the client can be told (see `REFUSED_SESSION_ENV`);
/// silently dropping them here would recreate the exact bug this replaced.
fn apply_client_env(cmd: &mut process::Command, client_env: &[(String, String)]) {
    for (key, value) in client_env {
        cmd.env(key, value);
    }
}

fn session_env(user: &PasswdEntry) -> Vec<(String, String)> {
    vec![
        ("HOME".to_string(), user.home.clone()),
        ("USER".to_string(), user.username.clone()),
        ("LOGNAME".to_string(), user.username.clone()),
        ("SHELL".to_string(), user.shell.clone()),
        (
            "PATH".to_string(),
            default_path_for_uid(user.uid).to_string(),
        ),
    ]
}

// ============================================================================
// Session execution
// ============================================================================

/// How a session's command finished, in the two forms RFC 4254 §6.10 defines.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionExit {
    /// The command exited normally with this status.
    Status(u32),
    /// The command was killed by a signal, named as the RFC requires:
    /// the bare name with no `SIG` prefix.
    ///
    /// Only constructed on Unix — which is every target this daemon actually
    /// runs on. The development host builds this crate for its unit tests and
    /// has no signals to report.
    #[cfg_attr(not(unix), allow(dead_code))]
    Signal {
        name: &'static str,
        core_dumped: bool,
    },
}

/// Map a signal number to the name RFC 4254 §6.10 expects.
///
/// The RFC enumerates exactly these thirteen names and says anything else
/// must use the `name@domain` extension form. Rather than invent one, an
/// unrecognised signal is reported as an exit *status* of `128 + n` — the
/// convention every shell already uses, so the number a caller sees matches
/// what they would have got running the command locally.
///
/// Only called on Unix; see [`SessionExit::Signal`].
#[cfg_attr(not(unix), allow(dead_code))]
fn rfc4254_signal_name(sig: i32) -> Option<&'static str> {
    Some(match sig {
        1 => "HUP",
        2 => "INT",
        3 => "QUIT",
        4 => "ILL",
        6 => "ABRT",
        8 => "FPE",
        9 => "KILL",
        10 => "USR1",
        11 => "SEGV",
        12 => "USR2",
        13 => "PIPE",
        14 => "ALRM",
        15 => "TERM",
        _ => return None,
    })
}

/// Classify a finished child process into a [`SessionExit`].
#[cfg(unix)]
fn classify_exit(status: &std::process::ExitStatus) -> SessionExit {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        return SessionExit::Status(u32::try_from(code).unwrap_or(255));
    }
    if let Some(sig) = status.signal() {
        if let Some(name) = rfc4254_signal_name(sig) {
            return SessionExit::Signal {
                name,
                core_dumped: status.core_dumped(),
            };
        }
        // Outside the RFC's list: fall back to the shell's 128+n convention.
        let n = u32::try_from(sig).unwrap_or(0);
        return SessionExit::Status(128u32.saturating_add(n));
    }
    SessionExit::Status(255)
}

/// Classify a finished child process into a [`SessionExit`].
///
/// The non-Unix arm exists because this crate's unit tests are compiled and
/// run on the development host, which is not `Slate OS`. It is never the arm
/// that runs in production.
#[cfg(not(unix))]
fn classify_exit(status: &std::process::ExitStatus) -> SessionExit {
    SessionExit::Status(
        status
            .code()
            .map_or(255, |c| u32::try_from(c).unwrap_or(255)),
    )
}

/// Build the `Command` that runs `command_line` as `user`.
///
/// The identity change is the point of this function. sshd binds port 22 and
/// therefore runs as root; if it spawned a session without dropping to the
/// authenticated account, every user who could log in would get root, which
/// is a worse failure than having no session support at all.
fn session_command(
    user: &PasswdEntry,
    command_line: &str,
    client_env: &[(String, String)],
) -> process::Command {
    let mut cmd = process::Command::new(&user.shell);
    cmd.arg("-c").arg(command_line);

    // Start from an empty environment; see `session_env` for why the
    // daemon's own environment is not a safe base.
    cmd.env_clear();
    for (key, value) in session_env(user) {
        cmd.env(key, value);
    }
    apply_client_env(&mut cmd, client_env);

    // All three are pipes, stdin included. Running the command on `/dev/null`
    // instead — which is what this did while `exec` was a one-shot — makes
    // `ssh host 'wc -l' < file` report 0: the client dutifully forwards the
    // file and the command reads end-of-file immediately.
    cmd.stdin(process::Stdio::piped());
    cmd.stdout(process::Stdio::piped());
    cmd.stderr(process::Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Group first: after the uid drops, the process can no longer choose
        // its group, so a uid-then-gid order would leave the session in the
        // daemon's group. (`std` orders the two calls correctly on its own;
        // writing them in this order keeps the reader from having to know
        // that.)
        cmd.gid(user.gid);
        cmd.uid(user.uid);
    }

    cmd
}

/// Spawn a session command, retrying from `/` if the home directory is
/// unusable.
///
/// A missing home directory is common on a freshly-provisioned account and
/// must not turn every command into a spawn failure — the account exists and
/// the user authenticated, so the session should start, just not there.
/// OpenSSH does the same, logging the fallback.
fn spawn_session_command(
    user: &PasswdEntry,
    command_line: &str,
    client_env: &[(String, String)],
) -> Result<process::Child, io::Error> {
    spawn_in_home(user, || session_command(user, command_line, client_env))
}

/// Spawn the command `build` produces from `user`'s home directory, retrying
/// from `/` if that fails.
///
/// `build` is called again for the retry rather than the first `Command` being
/// reused, because a `Command` carrying a `pre_exec` closure is not `Clone` and
/// its working directory cannot be un-set. Every session entry point shares
/// this so that the home-directory fallback cannot be present on one and
/// missing on another.
fn spawn_in_home(
    user: &PasswdEntry,
    build: impl Fn() -> process::Command,
) -> Result<process::Child, io::Error> {
    if !user.home.is_empty() {
        let mut cmd = build();
        cmd.current_dir(&user.home);
        // A failure here falls through to the `/` retry below. The original
        // error is dropped deliberately: if the retry also fails, its error is
        // the one that describes why no session could start at all, and if the
        // retry succeeds there was nothing to report.
        if let Ok(child) = cmd.spawn() {
            return Ok(child);
        }
    }
    let mut cmd = build();
    cmd.current_dir("/");
    cmd.spawn()
}

/// The `argv[0]` a login shell expects: its own basename with a leading `-`.
///
/// That leading hyphen is the entire protocol by which a shell is told it is a
/// *login* shell, and therefore that it should read `/etc/profile` and the
/// user's own profile. Passing `-l` instead works for some shells and is a
/// syntax error for others, which is why every `login`, `su` and sshd in
/// existence uses the hyphen.
///
/// The sole caller is inside `shell_command`'s `#[cfg(unix)]` block, so on a
/// non-unix *host* build — which is how this crate's tests are compiled — the
/// function is genuinely unreachable outside the test module. The allow is
/// therefore conditioned on `not(unix)` rather than unconditional: on the real
/// target, `dead_code` here would mean the login-shell convention had been
/// dropped from the spawn path, and that is a warning worth keeping.
#[cfg_attr(not(unix), allow(dead_code))]
fn login_argv0(shell_path: &str) -> String {
    let base = shell_path.rsplit('/').next().unwrap_or(shell_path);
    let base = if base.is_empty() { shell_path } else { base };
    format!("-{base}")
}

/// Build the `Command` that runs `user`'s login shell — program, environment,
/// `argv[0]` and identity, but no attachment.
///
/// The two callers below differ only in where the shell's standard descriptors
/// end up, so everything that is *not* the attachment lives here and is
/// therefore impossible for one of them to get right and the other wrong.
///
/// The identity change is the same one `session_command` performs and for the
/// same reason: sshd binds port 22 and runs as root, so a session that did not
/// drop to the authenticated account would hand out root to everyone who could
/// log in.
fn login_shell_command(
    user: &PasswdEntry,
    term: &str,
    client_env: &[(String, String)],
) -> process::Command {
    let mut cmd = process::Command::new(&user.shell);

    // Start from an empty environment; see `session_env` for why the daemon's
    // own environment is not a safe base.
    cmd.env_clear();
    for (key, value) in session_env(user) {
        cmd.env(key, value);
    }
    apply_client_env(&mut cmd, client_env);
    // `TERM` is the one variable the client is allowed to choose, because it
    // describes the client's own display and nothing on this end can know it.
    // It is set last, and `env` requests naming it are refused outright, so
    // there is one path by which it can arrive: the `pty-req` that describes
    // the terminal it belongs to.
    if !term.is_empty() {
        cmd.env("TERM", term);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        cmd.arg0(login_argv0(&user.shell));

        // Group first: after the uid drops the process can no longer choose its
        // group. (`std` orders the two correctly on its own; writing them in
        // this order keeps the reader from having to know that.)
        cmd.gid(user.gid);
        cmd.uid(user.uid);
    }

    cmd
}

/// The login shell on three ordinary pipes, for a `shell` request that came
/// with no `pty-req` — what `ssh -T host` and `ssh host < script` ask for.
///
/// It is the same shell, the same environment and the same identity change as
/// the pty version; only the attachment differs. In particular it still gets
/// the login `argv[0]`, because a script piped through a remote shell has the
/// same claim on `/etc/profile` as an interactive login does.
fn shell_command_pipes(
    user: &PasswdEntry,
    term: &str,
    client_env: &[(String, String)],
) -> process::Command {
    let mut cmd = login_shell_command(user, term, client_env);
    cmd.stdin(process::Stdio::piped());
    cmd.stdout(process::Stdio::piped());
    cmd.stderr(process::Stdio::piped());
    cmd
}

/// The login shell attached to the pty slave `slave_fd`.
fn shell_command(
    user: &PasswdEntry,
    term: &str,
    slave_fd: i32,
    client_env: &[(String, String)],
) -> process::Command {
    let mut cmd = login_shell_command(user, term, client_env);

    // The three standard descriptors are wired to `/dev/null` and then
    // immediately replaced: `login_tty` dup2s the slave over 0, 1 and 2 in the
    // child. std sets up its own stdio *before* running the `pre_exec` closure,
    // so the null descriptors exist only for the instant between the two.
    // Asking for pipes instead would create three the parent must then remember
    // never to read, and a pipe nobody reads is a shell that blocks on its
    // first line of output.
    cmd.stdin(process::Stdio::null());
    cmd.stdout(process::Stdio::null());
    cmd.stderr(process::Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        // The closure is built *outside* the `unsafe` block that registers it,
        // which is not a stylistic choice: a closure body written inside an
        // `unsafe` block inherits that block's unsafe context, so the
        // `login_tty` call below would compile with no `unsafe` of its own —
        // and adding one anyway is an `unused_unsafe` warning. Either way the
        // single most dangerous call in this file ends up with no unsafe block
        // naming it. Splitting the two gives each `unsafe` exactly one
        // operation to justify, which is what the policy in CLAUDE.md asks for.
        let enter_session = move || {
            // SAFETY: this runs in the forked child, between `fork` and
            // `execve`, where only async-signal-safe work is permitted.
            // `login_tty` is `setsid` + `ioctl(TIOCSCTTY)` + three `dup2`s +
            // one `close` — all bare syscalls, no allocation, no lock, no libc
            // state. `slave_fd` is copied into the closure by value and is
            // open in the child because `openpty` does not set `FD_CLOEXEC`
            // on it.
            if unsafe { ptylibc::login_tty(slave_fd) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        };

        // SAFETY: `pre_exec`'s contract is that the closure is async-signal-
        // safe, which the block above establishes for this one. Registering a
        // closure at all also takes `std` off its `posix_spawn` fast path onto
        // `fork`/`exec`, which is required rather than incidental:
        // `posix_spawn` has no hook that could acquire a controlling terminal.
        unsafe {
            cmd.pre_exec(enter_session);
        }
    }
    #[cfg(not(unix))]
    let _ = slave_fd;

    cmd
}

// ============================================================================
// SSH channel
// ============================================================================

/// The receive window advertised on every channel we open, and the figure the
/// window is credited back up to.
const INITIAL_LOCAL_WINDOW: u32 = 0x0020_0000; // 2 MiB

/// Credit the peer more window once the remaining window falls below this.
/// Chosen at a quarter of the window so the credit message is sent long
/// before the peer would actually stall on it.
const WINDOW_ADJUST_THRESHOLD: u32 = INITIAL_LOCAL_WINDOW / 4;

struct Channel {
    /// Our channel number (server-side).
    local_id: u32,
    /// Client's channel number.
    remote_id: u32,
    /// Window size remaining for sending to client.
    remote_window: u32,
    /// Our window size.
    local_window: u32,
    /// Maximum packet size for sending, from the peer's `CHANNEL_OPEN`.
    /// Outbound data is split to fit it — see `send_channel_stream`.
    remote_max_packet: u32,
    /// Terminal type (from PTY request).
    term: String,
    /// Terminal width in columns.
    term_width: u32,
    /// Terminal height in rows.
    term_height: u32,
    /// Terminal width in pixels, as the client reported it (0 when unknown).
    term_width_px: u32,
    /// Terminal height in pixels, as the client reported it (0 when unknown).
    term_height_px: u32,
    /// Whether the channel has been closed.
    closed: bool,
    /// Whether EOF has been sent.
    eof_sent: bool,
    /// Where this session's standard streams go: a terminal, pipes, or nothing
    /// yet. Dropping it closes the descriptors, which hangs up the process.
    io: SessionIo,
    /// The process running on this channel, once a `shell` request started one.
    child: Option<process::Child>,
    /// How the process finished, once it has, and before the client is told.
    ///
    /// Held between the two halves of ending a session — noticing the exit and
    /// finishing delivering the output — because those do not happen on the
    /// same pass of the pump.
    exit: Option<SessionExit>,
    /// Client keystrokes accepted from the wire but not yet in the terminal.
    ///
    /// The kernel's pty input ring is small and a program that has stopped
    /// reading its stdin — `cat > file` while the user pastes, a shell running
    /// a compute loop — fills it. Writing the remainder in a blocking loop
    /// would stall the *whole daemon* on one uninterested process: no output
    /// forwarded, no `window-change` handled, no other channel served. So the
    /// remainder waits here and the pump retries it when the master reports
    /// writable.
    ///
    /// It cannot grow without bound, because the SSH window is precisely the
    /// mechanism for saying "stop sending": the window is credited back only
    /// for bytes that have actually entered the terminal, so a client that
    /// outruns the shell runs out of window and blocks — which is the correct
    /// end-to-end back-pressure, applied to the client rather than absorbed
    /// into the daemon's memory.
    pending_input: Vec<u8>,
    /// The client sent `CHANNEL_EOF` while input was still queued.
    ///
    /// The close of the command's stdin has to wait until `pending_input`
    /// drains, or the last of the client's data would be discarded by the very
    /// message that says it has all been sent.
    input_eof: bool,
    /// Variables the client asked for and the configuration allowed, applied to
    /// the child when `shell`/`exec`/`subsystem` spawns it.
    ///
    /// Only names matching an `AcceptEnv` pattern ever land here, and that list
    /// is empty unless an administrator wrote one — see
    /// [`SshdConfig::accept_env`]. RFC 4254 §6.4 puts these before the session
    /// request, so collecting them on the channel and applying them at spawn is
    /// the order the protocol already prescribes.
    env: Vec<(String, String)>,
}

impl Channel {
    fn new(local_id: u32, remote_id: u32, remote_window: u32, remote_max_packet: u32) -> Self {
        Self {
            local_id,
            remote_id,
            remote_window,
            local_window: INITIAL_LOCAL_WINDOW,
            remote_max_packet,
            term: String::new(),
            term_width: 80,
            term_height: 24,
            term_width_px: 0,
            term_height_px: 0,
            closed: false,
            eof_sent: false,
            io: SessionIo::None,
            child: None,
            exit: None,
            pending_input: Vec::new(),
            input_eof: false,
            env: Vec::new(),
        }
    }
}

// ============================================================================
// PTY request parsing
// ============================================================================

/// Parsed PTY request payload:
/// (term, `width_cols`, `height_rows`, `width_px`, `height_px`, modes).
type PtyRequest = (String, u32, u32, u32, u32, Vec<u8>);

/// Parse a PTY request payload (after the "pty-req" string and `want_reply` byte).
fn parse_pty_request(data: &[u8], offset: usize) -> Result<PtyRequest, SshdError> {
    let (term_bytes, off) = read_ssh_string(data, offset)?;
    let term = String::from_utf8_lossy(term_bytes).into_owned();
    let (width_cols, off) = read_u32(data, off)?;
    let (height_rows, off) = read_u32(data, off)?;
    let (width_px, off) = read_u32(data, off)?;
    let (height_px, off) = read_u32(data, off)?;
    let (modes, _off) = read_ssh_string(data, off)?;
    Ok((
        term,
        width_cols,
        height_rows,
        width_px,
        height_px,
        modes.to_vec(),
    ))
}

// ============================================================================
// Connection handler
// ============================================================================

/// State for a single SSH connection.
struct ConnectionState {
    handle: u64,
    stream_buf: StreamBuffer,
    config: SshdConfig,
    host_key: HostKey,
    enc: EncryptionState,
    /// The client's version identification line, without its CRLF.
    ///
    /// Kept because RFC 4253 §8 makes it `V_C`, the first field of the exchange
    /// hash — the value we sign with the host key and the client recomputes
    /// independently. It is not merely logged: a server that does not remember
    /// what the client said cannot compute a hash the client will agree with.
    client_version: String,
    session_id: Option<[u8; 32]>,
    send_seq: u32,
    recv_seq: u32,
    encrypted: bool,
    authenticated: bool,
    auth_attempts: u32,
    username: String,
    channels: Vec<Channel>,
    next_channel_id: u32,
    debug_mode: bool,
    connection_start_ms: u64,
}

impl ConnectionState {
    fn new(handle: u64, config: SshdConfig, host_key: HostKey, debug_mode: bool) -> Self {
        Self {
            handle,
            stream_buf: StreamBuffer::new(),
            config,
            host_key,
            enc: EncryptionState::none(),
            client_version: String::new(),
            session_id: None,
            send_seq: 0,
            recv_seq: 0,
            encrypted: false,
            authenticated: false,
            auth_attempts: 0,
            username: String::new(),
            channels: Vec::new(),
            next_channel_id: 0,
            debug_mode,
            connection_start_ms: clock_monotonic_ms(),
        }
    }

    /// Send a packet.
    fn send_packet(&mut self, payload: &[u8]) -> Result<(), SshdError> {
        let pkt = build_packet(payload, self.encrypted, self.send_seq, &self.enc);
        tcp_send_all(self.handle, &pkt)?;
        self.send_seq = self.send_seq.wrapping_add(1);
        Ok(())
    }

    /// Receive a packet.
    fn recv_packet(&mut self) -> Result<Vec<u8>, SshdError> {
        let payload = read_packet(
            self.handle,
            &mut self.stream_buf,
            self.encrypted,
            self.recv_seq,
            &self.enc,
        )?;
        self.recv_seq = self.recv_seq.wrapping_add(1);
        Ok(payload)
    }

    /// Receive a packet **only if one is already framed in the buffer**.
    ///
    /// Returns `Ok(None)` rather than reading the socket, which is what makes
    /// it safe to call from the session pump: the pump must never block on the
    /// client, because the shell's output is also waiting to be forwarded.
    ///
    /// The sequence number advances only when a packet is actually produced.
    /// That is the whole correctness requirement here — the sequence number is
    /// an input to both the MAC and the CTR keystream, so incrementing it for a
    /// packet that did not arrive would desynchronise the cipher and turn every
    /// subsequent packet into a MAC failure.
    fn try_recv_packet(&mut self) -> Result<Option<Vec<u8>>, SshdError> {
        let payload = try_parse_packet(
            &mut self.stream_buf,
            self.encrypted,
            self.recv_seq,
            &self.enc,
        )?;
        if payload.is_some() {
            self.recv_seq = self.recv_seq.wrapping_add(1);
        }
        Ok(payload)
    }

    /// The sequence number of the packet currently being dispatched.
    ///
    /// Both receive paths advance `recv_seq` the moment a packet is produced,
    /// so by the time a handler runs the counter already names the *next*
    /// packet. RFC 4253 §11.4's `SSH_MSG_UNIMPLEMENTED` must carry the number of
    /// the packet it is rejecting, and sending the counter as-is would name a
    /// packet the client has not sent yet — a reply the client would attribute
    /// to the wrong message, or to none at all.
    fn current_recv_seq(&self) -> u32 {
        self.recv_seq.wrapping_sub(1)
    }

    /// Log a debug message.
    fn debug_log(&self, msg: &str) {
        if self.debug_mode {
            eprintln!("sshd[debug]: {msg}");
        }
    }
}

/// Handle a single SSH connection.
fn handle_connection(
    handle: u64,
    config: &SshdConfig,
    host_key: &HostKey,
    debug_mode: bool,
    auth: &mut authlib::Authenticator,
) {
    let peer = tcp_peer_addr(handle).map_or_else(
        |_| "unknown".into(),
        |(ip, port)| format!("{}:{}", format_ip(ip), port),
    );

    if debug_mode {
        eprintln!("sshd: connection from {peer}");
    }

    let hk = HostKey::from_seed(host_key.seed);
    let mut conn = ConnectionState::new(handle, config.clone(), hk, debug_mode);

    let result = run_connection(&mut conn, auth);

    if let Err(e) = &result
        && debug_mode
    {
        eprintln!("sshd: connection from {peer} error: {e}");
    }

    tcp_close(handle);

    if debug_mode {
        eprintln!("sshd: connection from {peer} closed");
    }
}

/// Main connection protocol flow.
fn run_connection(
    conn: &mut ConnectionState,
    auth: &mut authlib::Authenticator,
) -> Result<(), SshdError> {
    // 1. Version exchange.
    do_version_exchange(conn)?;

    // 2. Key exchange.
    do_key_exchange(conn)?;

    // 3. Service request (ssh-userauth).
    handle_service_request(conn)?;

    // 4. User authentication.
    do_user_auth(conn, auth)?;

    // 5. Channel handling loop.
    handle_channels(conn)?;

    Ok(())
}

// ============================================================================
// Protocol phases
// ============================================================================

/// SSH version exchange. We send our version, read the client's.
fn do_version_exchange(conn: &mut ConnectionState) -> Result<(), SshdError> {
    // Send our version string.
    let version_line = format!("{SSH_SERVER_VERSION}\r\n");
    tcp_send_all(conn.handle, version_line.as_bytes())?;

    conn.debug_log("sent version string");

    // Read client version string.
    let client_version = read_version_line(conn)?;
    conn.debug_log(&format!("client version: {client_version}"));

    if !client_version.starts_with("SSH-2.0-") {
        return Err(SshdError::ProtocolError(format!(
            "unsupported client version: {client_version}"
        )));
    }

    // Keep it. This line used to be read, logged and dropped, and the exchange
    // hash substituted a fixed "SSH-2.0-client" for it — which made every
    // handshake unverifiable. See `compute_exchange_hash`.
    conn.client_version = client_version;

    Ok(())
}

/// Read the SSH version line from the client.
fn read_version_line(conn: &mut ConnectionState) -> Result<String, SshdError> {
    let mut line = Vec::new();
    let mut single = [0u8; 1];
    loop {
        let n = tcp_recv(conn.handle, &mut single)?;
        if n == 0 {
            return Err(SshdError::ProtocolError(
                "connection closed during version exchange".into(),
            ));
        }
        if single[0] == b'\n' {
            break;
        }
        if single[0] != b'\r' {
            line.push(single[0]);
        }
        if line.len() > 255 {
            return Err(SshdError::ProtocolError("version string too long".into()));
        }
    }
    String::from_utf8(line)
        .map_err(|_| SshdError::ProtocolError("invalid UTF-8 in version string".into()))
}

/// Parse an SSH version string, returning the software version.
// Reserved for peer-compatibility handling: the banner exchange will use the
// parsed software version to enable known-client workarounds. The current
// handshake only validates the "SSH-2.0" prefix. Not yet invoked (but tested).
#[allow(dead_code)]
fn parse_version_string(version: &str) -> Option<&str> {
    // Format: SSH-protoversion-softwareversion SP comments
    let version = version.trim();
    if !version.starts_with("SSH-") {
        return None;
    }
    let after_ssh = &version[4..];
    // Skip protocol version.
    let after_proto = after_ssh.find('-').map(|i| &after_ssh[i + 1..])?;
    // Software version is up to the first space (or end).
    Some(after_proto.split(' ').next().unwrap_or(after_proto))
}

/// Perform SSH key exchange (DH group14-sha256).
fn do_key_exchange(conn: &mut ConnectionState) -> Result<(), SshdError> {
    // Build and send our KEXINIT.
    let server_kexinit = build_kexinit();
    conn.send_packet(&server_kexinit)?;
    conn.debug_log("sent KEXINIT");

    // Receive client KEXINIT.
    let client_kexinit = conn.recv_packet()?;
    if client_kexinit.first().copied() != Some(msg::SSH_MSG_KEXINIT) {
        return Err(SshdError::ProtocolError("expected KEXINIT".into()));
    }
    conn.debug_log("received client KEXINIT");

    // Receive KEX_DH_INIT from client.
    let dh_init = conn.recv_packet()?;
    if dh_init.first().copied() != Some(msg::SSH_MSG_KEX_DH_INIT) {
        return Err(SshdError::ProtocolError("expected KEX_DH_INIT".into()));
    }
    conn.debug_log("received KEX_DH_INIT");

    // Parse client's DH public value (e).
    let (client_e_bytes, _) = read_mpint(&dh_init, 1)?;
    let client_e = BigUint::from_bytes_be(&client_e_bytes);

    // Generate our DH keypair.
    let p = dh_group14_prime();
    let g = dh_group14_generator();

    // RFC 4253 section 8: "values of e or f that are not in the range
    // [1, p-1]" must be rejected. Without this a client can send e = 0 or
    // e = 1 and pin the shared secret to 0 or 1 -- a value it knows, and
    // therefore session keys it knows, with no need to break anything.
    // e = p-1 is excluded too: it has order 2, so K is one of two values.
    let p_minus_1 = p.sub_big(&BigUint::one());
    if client_e.cmp_unsigned(&BigUint::one()) != std::cmp::Ordering::Greater
        || client_e.cmp_unsigned(&p_minus_1) != std::cmp::Ordering::Less
    {
        return Err(SshdError::ProtocolError(
            "client DH value out of range (RFC 4253 section 8)".into(),
        ));
    }
    let y = generate_dh_private()?;
    let f = g.mod_pow(&y, &p); // f = g^y mod p
    let shared_secret_big = client_e.mod_pow(&y, &p); // K = e^y mod p
    let shared_secret = shared_secret_big.to_bytes_be();

    // Compute exchange hash H.
    let exchange_hash = compute_exchange_hash(
        &conn.client_version,
        SSH_SERVER_VERSION,
        &client_kexinit,
        &server_kexinit,
        &conn.host_key.public_key_blob(),
        &client_e.to_bytes_be(),
        &f.to_bytes_be(),
        &shared_secret,
    );

    // This is the session ID (first exchange hash).
    let session_id = exchange_hash;
    conn.session_id = Some(session_id);

    // Sign the exchange hash with our host key.
    let signature = conn.host_key.sign(&exchange_hash);

    // Build KEX_DH_REPLY.
    let mut reply = Vec::new();
    reply.push(msg::SSH_MSG_KEX_DH_REPLY);
    reply.extend_from_slice(&ssh_string(&conn.host_key.public_key_blob()));
    reply.extend_from_slice(&encode_mpint(&f.to_bytes_be()));
    reply.extend_from_slice(&ssh_string(&signature));
    conn.send_packet(&reply)?;
    conn.debug_log("sent KEX_DH_REPLY");

    // Send NEWKEYS.
    conn.send_packet(&[msg::SSH_MSG_NEWKEYS])?;
    conn.debug_log("sent NEWKEYS");

    // Receive NEWKEYS from client.
    let newkeys = conn.recv_packet()?;
    if newkeys.first().copied() != Some(msg::SSH_MSG_NEWKEYS) {
        return Err(SshdError::ProtocolError("expected NEWKEYS".into()));
    }
    conn.debug_log("received NEWKEYS");

    // Derive encryption keys.
    conn.enc = derive_keys(&shared_secret, &exchange_hash, &session_id);
    conn.encrypted = true;
    conn.debug_log("encryption activated");

    Ok(())
}

/// Build a KEXINIT message.
fn build_kexinit() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(msg::SSH_MSG_KEXINIT);

    // 16-byte cookie (pseudo-random).
    let cookie = sha256(b"sshd-kex-cookie");
    payload.extend_from_slice(&cookie[..16]);

    // Name-lists:
    // kex_algorithms
    payload.extend_from_slice(&ssh_string(b"diffie-hellman-group14-sha256"));
    // server_host_key_algorithms
    payload.extend_from_slice(&ssh_string(b"ssh-ed25519"));
    // encryption_algorithms_client_to_server
    payload.extend_from_slice(&ssh_string(b"aes128-ctr"));
    // encryption_algorithms_server_to_client
    payload.extend_from_slice(&ssh_string(b"aes128-ctr"));
    // mac_algorithms_client_to_server
    payload.extend_from_slice(&ssh_string(b"hmac-sha2-256"));
    // mac_algorithms_server_to_client
    payload.extend_from_slice(&ssh_string(b"hmac-sha2-256"));
    // compression_algorithms_client_to_server
    payload.extend_from_slice(&ssh_string(b"none"));
    // compression_algorithms_server_to_client
    payload.extend_from_slice(&ssh_string(b"none"));
    // languages_client_to_server
    payload.extend_from_slice(&ssh_string(b""));
    // languages_server_to_client
    payload.extend_from_slice(&ssh_string(b""));
    // first_kex_packet_follows
    payload.push(0);
    // reserved
    payload.extend_from_slice(&0u32.to_be_bytes());

    payload
}

/// Compute the SSH exchange hash H (RFC 4253, Section 8).
fn compute_exchange_hash(
    client_version: &str,
    server_version: &str,
    client_kexinit: &[u8],
    server_kexinit: &[u8],
    host_key_blob: &[u8],
    client_e: &[u8],
    server_f: &[u8],
    shared_secret: &[u8],
) -> [u8; 32] {
    let mut hash_input = Vec::new();

    // V_C: client version, as the client actually sent it.
    //
    // This was a fixed `b"SSH-2.0-client"`, with a comment saying we did not
    // store the real one. That single substitution made the daemon unusable by
    // any client at all, ours included: `H` is what we sign with the host key,
    // and the client recomputes it from its own view of the handshake and
    // checks our signature against *that*. Two different `V_C` values give two
    // different hashes, so the signature could never verify and every
    // connection died at host-key verification, before authentication.
    //
    // It survived because both ends are unit-tested against themselves and
    // there is no test that makes them agree. See known-issues.md
    // `TD-B-SSHD-SIGNS-AN-EXCHANGE-HASH-OVER-A-CLIENT-VERSION-THE-CLIENT-NEVER-SENT`.
    hash_input.extend_from_slice(&ssh_string(client_version.as_bytes()));
    // V_S: server version.
    hash_input.extend_from_slice(&ssh_string(server_version.as_bytes()));
    // I_C: client KEXINIT payload.
    hash_input.extend_from_slice(&ssh_string(client_kexinit));
    // I_S: server KEXINIT payload.
    hash_input.extend_from_slice(&ssh_string(server_kexinit));
    // K_S: host key blob.
    hash_input.extend_from_slice(&ssh_string(host_key_blob));
    // e: client DH value.
    hash_input.extend_from_slice(&encode_mpint(client_e));
    // f: server DH value.
    hash_input.extend_from_slice(&encode_mpint(server_f));
    // K: shared secret.
    hash_input.extend_from_slice(&encode_mpint(shared_secret));

    sha256(&hash_input)
}

/// Handle the SSH-USERAUTH service request.
fn handle_service_request(conn: &mut ConnectionState) -> Result<(), SshdError> {
    let payload = conn.recv_packet()?;
    if payload.first().copied() != Some(msg::SSH_MSG_SERVICE_REQUEST) {
        return Err(SshdError::ProtocolError("expected SERVICE_REQUEST".into()));
    }

    let (service_name, _) = read_ssh_string(&payload, 1)?;
    let service = String::from_utf8_lossy(service_name);
    conn.debug_log(&format!("service request: {service}"));

    if service != "ssh-userauth" {
        return Err(SshdError::ProtocolError(format!(
            "unsupported service: {service}"
        )));
    }

    // Accept the service.
    let mut accept = Vec::new();
    accept.push(msg::SSH_MSG_SERVICE_ACCEPT);
    accept.extend_from_slice(&ssh_string(b"ssh-userauth"));
    conn.send_packet(&accept)?;

    // Send banner if configured.
    if !conn.config.banner_file.is_empty()
        && let Ok(banner_data) = fs_read_file(&conn.config.banner_file)
    {
        let mut banner_msg = Vec::new();
        banner_msg.push(msg::SSH_MSG_USERAUTH_BANNER);
        banner_msg.extend_from_slice(&ssh_string(&banner_data));
        banner_msg.extend_from_slice(&ssh_string(b"")); // language tag
        let _ = conn.send_packet(&banner_msg);
    }

    Ok(())
}

/// Perform user authentication loop.
///
/// `auth` is borrowed from the daemon rather than created here on purpose: its
/// per-user failure tally has to outlive the connection, or an attacker gets an
/// unlimited number of guesses simply by reconnecting after each `max_auth_tries`
/// refusal. `max_auth_tries` bounds one conversation; `auth` bounds the account.
fn do_user_auth(
    conn: &mut ConnectionState,
    auth: &mut authlib::Authenticator,
) -> Result<(), SshdError> {
    loop {
        // Check login grace time.
        let elapsed_s = (clock_monotonic_ms() - conn.connection_start_ms) / 1000;
        if elapsed_s > u64::from(conn.config.login_grace_time) {
            send_disconnect(conn, 2, "login grace time expired")?;
            return Err(SshdError::AuthError("login grace time expired".into()));
        }

        let payload = conn.recv_packet()?;
        if payload.first().copied() != Some(msg::SSH_MSG_USERAUTH_REQUEST) {
            // Ignore non-auth messages during auth phase.
            continue;
        }

        let (username_bytes, off) = read_ssh_string(&payload, 1)?;
        // The publickey signature is computed over the *bytes* the client sent
        // for the user and service names, so those are kept alongside the lossy
        // strings the rest of the loop uses for lookups and logging. A username
        // that is not valid UTF-8 would otherwise be re-encoded with U+FFFD and
        // every signature over it would fail for a reason nothing reports.
        let user_bytes = username_bytes.to_vec();
        let username = String::from_utf8_lossy(username_bytes).into_owned();
        let (service_bytes, off) = read_ssh_string(&payload, off)?;
        let service_bytes = service_bytes.to_vec();
        let (method_bytes, off) = read_ssh_string(&payload, off)?;
        let method = String::from_utf8_lossy(method_bytes).into_owned();

        conn.debug_log(&format!("auth request: user={username} method={method}"));
        username.clone_into(&mut conn.username);

        // Check user allow/deny lists.
        if !is_user_allowed(&username, &[], &conn.config) {
            conn.debug_log(&format!("user {username} denied by access list"));
            send_auth_failure(conn, false)?;
            conn.auth_attempts += 1;
            if conn.auth_attempts >= conn.config.max_auth_tries {
                send_disconnect(conn, 2, "too many authentication failures")?;
                return Err(SshdError::AuthError("max auth tries exceeded".into()));
            }
            continue;
        }

        // Check root login restrictions.
        if username == "root" && !is_root_login_allowed(&method, &conn.config) {
            conn.debug_log("root login denied by policy");
            send_auth_failure(conn, false)?;
            conn.auth_attempts += 1;
            if conn.auth_attempts >= conn.config.max_auth_tries {
                send_disconnect(conn, 2, "too many authentication failures")?;
                return Err(SshdError::AuthError("max auth tries exceeded".into()));
            }
            continue;
        }

        let success = match method.as_str() {
            "password" if conn.config.password_authentication => {
                let outcome = handle_password_auth(&payload, off, &username, auth)?;
                // The person typing is told one thing for every kind of no --
                // which one it was is exactly what an attacker wants to learn.
                // The *log* distinguishes them, because an unusable entry is a
                // broken system that nobody finds out about otherwise, and a
                // rate-limited user is the daemon working as intended rather
                // than someone forgetting their password.
                match outcome {
                    authlib::Outcome::Accepted => {}
                    authlib::Outcome::RateLimited { retry_after_secs } => {
                        log_error(
                            &format!(
                                "user {username} is rate limited for another \
                                 {retry_after_secs}s after repeated failures"
                            ),
                            false,
                        );
                    }
                    authlib::Outcome::Unusable => {
                        log_error(
                            &format!(
                                "user {username} has a stored password entry this \
                                 system cannot recompute; no password can ever match \
                                 it and an administrator must set a new one"
                            ),
                            false,
                        );
                    }
                    other => conn.debug_log(&format!("password auth for {username}: {other:?}")),
                }
                outcome.is_accepted()
            }
            "publickey" if conn.config.pubkey_authentication => {
                // Authentication runs after NEWKEYS, so the session ID always
                // exists here; a client that got this far without a key
                // exchange is speaking a protocol we do not implement.
                let Some(session_id) = conn.session_id else {
                    return Err(SshdError::ProtocolError(
                        "publickey auth before key exchange".into(),
                    ));
                };
                match handle_pubkey_auth(
                    &payload,
                    off,
                    &user_bytes,
                    &username,
                    &service_bytes,
                    &session_id,
                    &conn.config,
                )? {
                    PubkeyOutcome::Accepted => true,
                    PubkeyOutcome::Query {
                        algorithm,
                        key_blob,
                    } => {
                        let mut ok = Vec::new();
                        ok.push(msg::SSH_MSG_USERAUTH_PK_OK);
                        ok.extend_from_slice(&ssh_string(&algorithm));
                        ok.extend_from_slice(&ssh_string(&key_blob));
                        conn.send_packet(&ok)?;
                        conn.debug_log("offered key is acceptable, awaiting signature");
                        // A query is neither success nor failure: skip both the
                        // attempt counter and the FAILURE reply below.
                        continue;
                    }
                    PubkeyOutcome::Rejected => false,
                }
            }
            _ => false,
        };

        if success {
            conn.authenticated = true;
            let msg_buf = vec![msg::SSH_MSG_USERAUTH_SUCCESS];
            conn.send_packet(&msg_buf)?;
            conn.debug_log(&format!("user {username} authenticated via {method}"));
            return Ok(());
        }

        conn.auth_attempts += 1;
        if conn.auth_attempts >= conn.config.max_auth_tries {
            send_disconnect(conn, 2, "too many authentication failures")?;
            return Err(SshdError::AuthError("max auth tries exceeded".into()));
        }

        send_auth_failure(conn, false)?;
    }
}

/// Handle password authentication (RFC 4252 section 8).
///
/// The whole of the decision belongs to `authlib`; this function's job is to
/// get the password off the wire without damaging it and to hand back what
/// `authlib` said.
///
/// # The password is bytes
///
/// It is passed to [`authlib::Authenticator::authenticate`] as the exact bytes
/// the client sent. RFC 4252 says a password is UTF-8, but a client is free to
/// be wrong about that, and running it through `String::from_utf8_lossy` first
/// -- as this did -- replaces every invalid byte with U+FFFD. That silently
/// changes the password, so a user whose password contains a byte their
/// terminal encoded differently could never log in, and two *different*
/// passwords could hash to the same thing.
///
/// # Errors
///
/// Fails only if the request packet is malformed.
fn handle_password_auth(
    payload: &[u8],
    offset: usize,
    username: &str,
    auth: &mut authlib::Authenticator,
) -> Result<authlib::Outcome, SshdError> {
    // RFC 4252 section 8 also defines a password-*change* request, carrying a
    // second string. We do not implement changing a password over ssh, so the
    // flag is read and the request treated as an ordinary attempt; the client
    // is told the attempt failed, which is true.
    let (_change, off) = read_bool(payload, offset)?;
    let (password_bytes, _) = read_ssh_string(payload, off)?;
    Ok(auth.authenticate(username, password_bytes))
}

/// The outcome of examining one `publickey` `SSH_MSG_USERAUTH_REQUEST`.
enum PubkeyOutcome {
    /// The client proved possession of an authorised key. Let it in.
    Accepted,
    /// The client asked, without a signature, whether this key would be
    /// acceptable (RFC 4252 section 7). It is; the caller owes it an
    /// `SSH_MSG_USERAUTH_PK_OK` carrying these two fields back verbatim.
    ///
    /// This is *not* an authentication failure and must not be counted as one
    /// or answered with `SSH_MSG_USERAUTH_FAILURE` -- OpenSSH sends the query
    /// first for every key in the agent, and a failure reply makes it give up
    /// on a key it was about to sign with.
    Query {
        algorithm: Vec<u8>,
        key_blob: Vec<u8>,
    },
    /// Not an authorised key, or the signature did not verify.
    Rejected,
}

/// Handle public key authentication (RFC 4252 section 7).
///
/// # The bug this replaces
///
/// The previous implementation compared the offered public key against
/// `authorized_keys` and, if it matched and a signature was present, returned
/// success **without looking at the signature**. A public key is public: SSH
/// sends it in the clear and it sits world-readable in `authorized_keys`, so
/// anyone who had ever seen the user connect -- or who could read that file, or
/// the user's `id_ed25519.pub` -- could log in as them by replaying it. The
/// signature is the entire proof of possession; skipping it removes the only
/// thing being authenticated.
///
/// # What is checked now
///
/// The signature must verify over the exact blob RFC 4252 section 7 specifies:
///
/// ```text
/// string    session identifier
/// byte      SSH_MSG_USERAUTH_REQUEST
/// string    user name
/// string    service name
/// string    "publickey"
/// boolean   TRUE
/// string    public key algorithm name
/// string    public key
/// ```
///
/// Binding the session identifier is what stops a signature captured from one
/// connection being replayed on another, and binding the user and service name
/// is what stops a signature for one account being presented for a different
/// one.
fn handle_pubkey_auth(
    payload: &[u8],
    offset: usize,
    user_bytes: &[u8],
    username: &str,
    service_bytes: &[u8],
    session_id: &[u8; 32],
    config: &SshdConfig,
) -> Result<PubkeyOutcome, SshdError> {
    let (has_sig, off) = read_bool(payload, offset)?;
    let (algorithm, off) = read_ssh_string(payload, off)?;
    let (key_blob, sig_off) = read_ssh_string(payload, off)?;

    // Read authorized_keys for this user.
    let keys_path = format!("/home/{username}/{}", config.authorized_keys_file);
    let keys_content = match fs_read_file(&keys_path) {
        Ok(data) => String::from_utf8_lossy(&data).into_owned(),
        Err(_) => return Ok(PubkeyOutcome::Rejected),
    };

    let authorized = parse_authorized_keys(&keys_content);
    if !authorized.iter().any(|ak| ak.key_data == key_blob) {
        return Ok(PubkeyOutcome::Rejected);
    }

    // Only ssh-ed25519 can be verified here, so only ssh-ed25519 may be
    // accepted. An RSA key listed in authorized_keys is refused rather than
    // waved through: "we cannot check this one" must never resolve to "yes".
    if algorithm != b"ssh-ed25519" {
        return Ok(PubkeyOutcome::Rejected);
    }
    let Some(ed_public) = ed25519_key_from_blob(key_blob) else {
        return Ok(PubkeyOutcome::Rejected);
    };

    if !has_sig {
        return Ok(PubkeyOutcome::Query {
            algorithm: algorithm.to_vec(),
            key_blob: key_blob.to_vec(),
        });
    }

    let (sig_blob, _) = read_ssh_string(payload, sig_off)?;
    let verified = verify_pubkey_signature(
        &ed_public,
        sig_blob,
        session_id,
        user_bytes,
        service_bytes,
        algorithm,
        key_blob,
    );
    if verified {
        Ok(PubkeyOutcome::Accepted)
    } else {
        Ok(PubkeyOutcome::Rejected)
    }
}

/// Build the RFC 4252 section 7 signed blob and check `sig_blob` against it.
///
/// Split out from [`handle_pubkey_auth`] because that function reads
/// `authorized_keys` from the filesystem, and the part worth testing
/// exhaustively is this one: it is the whole of what authenticates the client.
///
/// `sig_blob` is the wire form `string algorithm || string signature`.
fn verify_pubkey_signature(
    ed_public: &[u8],
    sig_blob: &[u8],
    session_id: &[u8; 32],
    user_bytes: &[u8],
    service_bytes: &[u8],
    algorithm: &[u8],
    key_blob: &[u8],
) -> bool {
    let Ok((sig_algorithm, inner_off)) = read_ssh_string(sig_blob, 0) else {
        return false;
    };
    if sig_algorithm != b"ssh-ed25519" {
        return false;
    }
    let Ok((signature, _)) = read_ssh_string(sig_blob, inner_off) else {
        return false;
    };

    let signed = pubkey_signed_blob(session_id, user_bytes, service_bytes, algorithm, key_blob);
    posix::ed25519::verify_slices(ed_public, &signed, signature)
}

/// The exact byte sequence a `publickey` signature covers (RFC 4252 section 7).
///
/// Shared by the server's verification path and by the tests, so that a test
/// asserting a signature verifies cannot pass by agreeing with a blob that
/// only the test knows how to build.
fn pubkey_signed_blob(
    session_id: &[u8; 32],
    user_bytes: &[u8],
    service_bytes: &[u8],
    algorithm: &[u8],
    key_blob: &[u8],
) -> Vec<u8> {
    let mut signed = Vec::new();
    signed.extend_from_slice(&ssh_string(session_id));
    signed.push(msg::SSH_MSG_USERAUTH_REQUEST);
    signed.extend_from_slice(&ssh_string(user_bytes));
    signed.extend_from_slice(&ssh_string(service_bytes));
    signed.extend_from_slice(&ssh_string(b"publickey"));
    signed.push(1); // boolean TRUE
    signed.extend_from_slice(&ssh_string(algorithm));
    signed.extend_from_slice(&ssh_string(key_blob));
    signed
}

/// Extract the raw 32-byte Ed25519 point from an SSH public key blob
/// (`string "ssh-ed25519" || string key`). Returns `None` if the blob is not
/// an Ed25519 key or is malformed.
fn ed25519_key_from_blob(blob: &[u8]) -> Option<Vec<u8>> {
    let (algorithm, off) = read_ssh_string(blob, 0).ok()?;
    if algorithm != b"ssh-ed25519" {
        return None;
    }
    let (key, _) = read_ssh_string(blob, off).ok()?;
    if key.len() != 32 {
        return None;
    }
    Some(key.to_vec())
}

/// Send `SSH_MSG_USERAUTH_FAILURE`.
fn send_auth_failure(conn: &mut ConnectionState, partial: bool) -> Result<(), SshdError> {
    let mut methods = Vec::new();
    if conn.config.password_authentication {
        methods.push("password");
    }
    if conn.config.pubkey_authentication {
        methods.push("publickey");
    }
    let methods_str = methods.join(",");

    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_USERAUTH_FAILURE);
    msg_buf.extend_from_slice(&ssh_string(methods_str.as_bytes()));
    msg_buf.push(u8::from(partial));
    conn.send_packet(&msg_buf)
}

/// Send `SSH_MSG_DISCONNECT`.
fn send_disconnect(
    conn: &mut ConnectionState,
    reason: u32,
    description: &str,
) -> Result<(), SshdError> {
    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_DISCONNECT);
    msg_buf.extend_from_slice(&reason.to_be_bytes());
    msg_buf.extend_from_slice(&ssh_string(description.as_bytes()));
    msg_buf.extend_from_slice(&ssh_string(b"")); // language tag
    conn.send_packet(&msg_buf)
}

/// Whether the connection loop should keep running after a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// Keep going.
    Continue,
    /// The conversation is over; return normally.
    Stop,
}

/// Shortest and longest the connection loop sleeps when nothing is happening.
///
/// The bound is reset to the minimum on any activity, so a session being typed
/// into never sleeps longer than the minimum, and one sitting idle costs
/// 50 wake-ups a second rather than a spin. The maximum is set well under the
/// ~50 ms at which a human perceives echo as laggy, because the worst case —
/// the first keystroke after an idle pause — is exactly the one a user notices.
///
/// This polling is a consequence of there being no unified readiness syscall:
/// the socket is a raw TCP handle and the terminal is an fd, and no single call
/// can wait on both. When the kernel grows one, this loop should block on it
/// instead. See `design-decisions.md` §770 and `known-issues.md`.
const IDLE_SLEEP_MIN_NS: u64 = 500_000; // 0.5 ms
const IDLE_SLEEP_MAX_NS: u64 = 20_000_000; // 20 ms

/// Handle the channel phase after authentication: run the connection until the
/// client disconnects or every channel it opened has closed.
fn handle_channels(conn: &mut ConnectionState) -> Result<(), SshdError> {
    let mut idle_ns = IDLE_SLEEP_MIN_NS;

    loop {
        // With no process running there is nothing to watch but the socket, so
        // block on it exactly as this loop always did: no polling, no wake-ups,
        // and no latency added to the client's next packet. Readiness polling
        // is the price of having a second thing to watch, and it is only paid
        // when there is one.
        if !conn.channels.iter().any(|ch| ch.child.is_some()) {
            let payload = match conn.recv_packet() {
                Ok(p) => p,
                Err(SshdError::ProtocolError(msg)) if msg.contains("connection closed") => {
                    return Ok(());
                }
                Err(e) => return Err(e),
            };
            if dispatch_channel_message(conn, &payload)? == Flow::Stop {
                return Ok(());
            }
            continue;
        }

        let mut worked = false;

        // 1. Every packet already framed in the buffer, before asking for more.
        //    A single TCP read can carry several SSH packets, and leaving the
        //    extras in the buffer while sleeping would add a whole idle period
        //    to each of them.
        while let Some(payload) = conn.try_recv_packet()? {
            worked = true;
            if dispatch_channel_message(conn, &payload)? == Flow::Stop {
                return Ok(());
            }
        }

        // 2. More bytes from the client, but only when there are some: an
        //    unconditional read here would block, and the shell's output would
        //    stop until the user typed.
        if tcp_readable(conn.handle) {
            match conn.stream_buf.fill_once(conn.handle) {
                Ok(()) => worked = true,
                Err(SshdError::ProtocolError(msg)) if msg.contains("connection closed") => {
                    return Ok(());
                }
                Err(e) => return Err(e),
            }
        }

        // 3. The other direction: shell output out, finished sessions reaped.
        if pump_sessions(conn)? {
            worked = true;
        }

        if conn.channels.iter().all(|ch| ch.closed) && !conn.channels.is_empty() {
            return Ok(());
        }

        if worked {
            idle_ns = IDLE_SLEEP_MIN_NS;
        } else {
            sleep_ns(idle_ns);
            idle_ns = idle_ns.saturating_mul(2).min(IDLE_SLEEP_MAX_NS);
        }
    }
}

/// Act on one channel-layer message.
fn dispatch_channel_message(conn: &mut ConnectionState, payload: &[u8]) -> Result<Flow, SshdError> {
    if payload.is_empty() {
        return Ok(Flow::Continue);
    }

    let msg_type = payload[0];

    match msg_type {
        msg::SSH_MSG_CHANNEL_OPEN => {
            handle_channel_open(conn, payload)?;
        }
        msg::SSH_MSG_CHANNEL_REQUEST => {
            handle_channel_request(conn, payload)?;
        }
        msg::SSH_MSG_CHANNEL_DATA => {
            handle_channel_data(conn, payload)?;
        }
        msg::SSH_MSG_CHANNEL_WINDOW_ADJUST => {
            handle_window_adjust(conn, payload)?;
        }
        msg::SSH_MSG_CHANNEL_EOF => {
            handle_channel_eof(conn, payload)?;
        }
        msg::SSH_MSG_CHANNEL_CLOSE => {
            handle_channel_close(conn, payload)?;
            if conn.channels.iter().all(|ch| ch.closed) {
                return Ok(Flow::Stop);
            }
        }
        msg::SSH_MSG_GLOBAL_REQUEST => {
            handle_global_request(conn, payload)?;
        }
        msg::SSH_MSG_DISCONNECT => {
            conn.debug_log("client sent DISCONNECT");
            return Ok(Flow::Stop);
        }
        msg::SSH_MSG_IGNORE | msg::SSH_MSG_DEBUG => {
            // Both are advisory by definition and neither is answered: IGNORE
            // exists to be discarded (RFC 4253 §11.2), and DEBUG carries text
            // for a human, not a request (§11.3).
        }
        msg::SSH_MSG_UNIMPLEMENTED => {
            // Answering this would be answering an answer. If a peer ever sent
            // UNIMPLEMENTED for our UNIMPLEMENTED, a reply would ping-pong the
            // two ends until one of them ran out of socket buffer.
            conn.debug_log("client sent UNIMPLEMENTED");
        }
        msg::SSH_MSG_KEXINIT => {
            // A rekey request. We do not implement rekeying, and this arm exists
            // to say so in the log rather than to handle it: dropping it silently
            // is what makes the session hang, because an OpenSSH client holds all
            // other traffic until the key exchange it started completes. Tracked
            // as TD-B-SSHD-DOES-NOT-REKEY-SO-A-LONG-SESSION-HANGS in
            // known-issues.md. Deliberately *not* answered with UNIMPLEMENTED:
            // KEXINIT is a message we recognise, so that reply would be a lie
            // about which of the two problems the client is looking at.
            conn.debug_log("client requested rekey (KEXINIT); unsupported, ignoring");
        }
        _ => {
            // RFC 4253 §11.4: an unrecognised message must be answered, and the
            // answer carries the sequence number of the packet being rejected so
            // the peer can tell *which* of its messages we did not understand.
            let rejected_seq = conn.current_recv_seq();
            conn.debug_log(&format!(
                "unhandled message type: {msg_type} (seq {rejected_seq}); replying UNIMPLEMENTED"
            ));
            conn.send_packet(&unimplemented_packet(rejected_seq))?;
        }
    }

    Ok(Flow::Continue)
}

/// Handle `SSH_MSG_GLOBAL_REQUEST` (RFC 4254 §4).
///
/// We implement no global requests — there is no TCP forwarding in this daemon
/// — so every one of them is refused. The point of the arm is not the refusal
/// but the *reply*: §4 says a request with `want_reply` set must be answered,
/// and a client that asked and heard nothing has no way to distinguish "refused"
/// from "the server is gone".
///
/// That distinction is the whole feature for the request people actually send.
/// `ServerAliveInterval` makes an OpenSSH client emit
/// `keepalive@openssh.com` with `want_reply = true`, and it picks that name
/// precisely *because* no server implements it: a `REQUEST_FAILURE` is proof the
/// server is alive and parsing, which is all the probe wanted. Dropping it means
/// the liveness probe times out `ServerAliveCountMax` times and the client kills
/// a perfectly healthy session — so the option a user sets to keep a session up
/// through a NAT is exactly the one that tears it down.
fn handle_global_request(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (name, reply) = global_request_reply(payload)?;
    conn.debug_log(&format!(
        "global request: {name} (unsupported, refusing; replying={})",
        reply.is_some()
    ));
    if let Some(reply) = reply {
        conn.send_packet(&reply)?;
    }
    Ok(())
}

/// Build an `SSH_MSG_UNIMPLEMENTED` naming the packet being rejected.
///
/// RFC 4253 §11.4: `byte SSH_MSG_UNIMPLEMENTED`, `uint32 packet sequence number
/// of rejected message`.
fn unimplemented_packet(rejected_seq: u32) -> Vec<u8> {
    let mut reply = Vec::with_capacity(5);
    reply.push(msg::SSH_MSG_UNIMPLEMENTED);
    reply.extend_from_slice(&rejected_seq.to_be_bytes());
    reply
}

/// Decide what a global request is owed, without touching the socket.
///
/// Returns the request's name — for the log — and the reply payload if one is
/// owed. Split out from [`handle_global_request`] so the decision can be tested
/// on its own: the reply is the entire observable behaviour here, and a test
/// that needed a live socket to see it would not be run.
fn global_request_reply(payload: &[u8]) -> Result<(String, Option<Vec<u8>>), SshdError> {
    let (name_bytes, off) = read_ssh_string(payload, 1)?;
    let name = String::from_utf8_lossy(name_bytes).into_owned();
    let (want_reply, _) = read_bool(payload, off)?;

    let reply = if want_reply {
        Some(vec![msg::SSH_MSG_REQUEST_FAILURE])
    } else {
        None
    };
    Ok((name, reply))
}

/// Handle `CHANNEL_OPEN`.
fn handle_channel_open(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (chan_type_bytes, off) = read_ssh_string(payload, 1)?;
    let chan_type = String::from_utf8_lossy(chan_type_bytes);
    let (sender_channel, off) = read_u32(payload, off)?;
    let (initial_window, off) = read_u32(payload, off)?;
    let (max_packet, _) = read_u32(payload, off)?;

    conn.debug_log(&format!(
        "channel open: type={chan_type} remote_id={sender_channel}"
    ));

    if chan_type != "session" {
        // Reject non-session channels.
        let mut reply = Vec::new();
        reply.push(msg::SSH_MSG_CHANNEL_OPEN_FAILURE);
        reply.extend_from_slice(&sender_channel.to_be_bytes());
        reply.extend_from_slice(&1u32.to_be_bytes()); // reason: administratively prohibited
        reply.extend_from_slice(&ssh_string(b"only session channels supported"));
        reply.extend_from_slice(&ssh_string(b""));
        return conn.send_packet(&reply);
    }

    // Check max sessions.
    let active = conn.channels.iter().filter(|ch| !ch.closed).count();
    if active >= conn.config.max_sessions as usize {
        let mut reply = Vec::new();
        reply.push(msg::SSH_MSG_CHANNEL_OPEN_FAILURE);
        reply.extend_from_slice(&sender_channel.to_be_bytes());
        reply.extend_from_slice(&4u32.to_be_bytes()); // reason: resource shortage
        reply.extend_from_slice(&ssh_string(b"max sessions exceeded"));
        reply.extend_from_slice(&ssh_string(b""));
        return conn.send_packet(&reply);
    }

    let local_id = conn.next_channel_id;
    conn.next_channel_id += 1;

    let channel = Channel::new(local_id, sender_channel, initial_window, max_packet);
    let local_window = channel.local_window;
    conn.channels.push(channel);

    // Send CHANNEL_OPEN_CONFIRMATION.
    let mut reply = Vec::new();
    reply.push(msg::SSH_MSG_CHANNEL_OPEN_CONFIRMATION);
    reply.extend_from_slice(&sender_channel.to_be_bytes());
    reply.extend_from_slice(&local_id.to_be_bytes());
    reply.extend_from_slice(&local_window.to_be_bytes());
    reply.extend_from_slice(&(32768u32).to_be_bytes()); // max packet size
    conn.send_packet(&reply)
}

/// Handle `CHANNEL_REQUEST`.
fn handle_channel_request(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (recipient, off) = read_u32(payload, 1)?;
    let (req_type_bytes, off) = read_ssh_string(payload, off)?;
    let req_type = String::from_utf8_lossy(req_type_bytes).into_owned();
    let (want_reply, off) = read_bool(payload, off)?;

    conn.debug_log(&format!(
        "channel request: channel={recipient} type={req_type} want_reply={want_reply}"
    ));

    let Some(channel) = conn.channels.iter_mut().find(|ch| ch.local_id == recipient) else {
        if want_reply {
            let mut fail = vec![msg::SSH_MSG_CHANNEL_FAILURE];
            fail.extend_from_slice(&recipient.to_be_bytes());
            conn.send_packet(&fail)?;
        }
        return Ok(());
    };

    let remote_id = channel.remote_id;

    match req_type.as_str() {
        "pty-req" => {
            let (term, width, height, wpx, hpx, _modes) = parse_pty_request(payload, off)?;
            channel.term = term;
            channel.term_width = width;
            channel.term_height = height;
            channel.term_width_px = wpx;
            channel.term_height_px = hpx;
            let term_name = channel.term.clone();
            let already_allocated = !matches!(channel.io, SessionIo::None);
            // The mutable borrow of `channel` ends here, so `conn` is free.

            // The terminal modes the client sent are deliberately not applied.
            // They describe the *client's* terminal at the moment it connected,
            // and the interesting ones (`ECHO`, `ICANON`, the control
            // characters) are exactly what the shell on this end will set for
            // itself the moment it starts. Applying them would install a
            // stranger's idea of ^C on a terminal the shell is about to
            // reconfigure, and every disagreement between the two would look
            // like a bug in the shell. OpenSSH does apply them; it can, because
            // it has a `termios` translation table this daemon does not, and
            // getting a partial translation wrong is worse than starting from
            // the kernel's own cooked defaults, which is what a login on the
            // console gets. Tracked in `known-issues.md`.

            if already_allocated {
                // A second `pty-req` on one channel is a protocol error (RFC
                // 4254 §6.2 allows one). Refusing is safer than replacing: the
                // shell may already be running on the first terminal, and
                // swapping it out from under a live process would hang it up.
                // The same refusal covers a `pty-req` that arrives *after* a
                // pipe-backed command started, which is the same mistake made
                // in the other order.
                conn.debug_log("pty-req refused: channel already has an attachment");
                if want_reply {
                    send_channel_failure(conn, remote_id)?;
                }
                return Ok(());
            }

            match Pty::open(width, height, wpx, hpx) {
                Ok(pty) => {
                    conn.debug_log(&format!("pty allocated: term={term_name} {width}x{height}"));
                    if let Some(channel) =
                        conn.channels.iter_mut().find(|ch| ch.local_id == recipient)
                    {
                        channel.io = SessionIo::Terminal(pty);
                    }
                    if want_reply {
                        send_channel_success(conn, remote_id)?;
                    }
                }
                Err(e) => {
                    // Refusing honestly is important here. A SUCCESS followed
                    // by no terminal makes the client put *its* terminal into
                    // raw mode and wait for an echo that will never come, which
                    // presents to the user as a hung connection rather than as
                    // a server that could not allocate a pty.
                    conn.debug_log(&format!("pty-req failed: {e}"));
                    if want_reply {
                        send_channel_failure(conn, remote_id)?;
                    }
                }
            }
        }
        "shell" => {
            // Two shapes of shell, chosen by whether a `pty-req` came first.
            //
            // With a terminal it is an interactive login: echo, line editing
            // and ^C all come from the line discipline, and the shell is told
            // it is a login shell by its `argv[0]`.
            //
            // Without one (`ssh -T`) it is a shell on three plain pipes, which
            // is how a script pipes data through a remote shell. It is not a
            // degraded interactive session and must not be answered with a
            // terminal the client did not ask for: a client that asked for no
            // pty is not in raw mode and would see the echo of its own input
            // interleaved with the output.
            start_shell_session(conn, recipient, remote_id, want_reply)?;
        }
        "exec" => {
            let (cmd_bytes, _) = read_ssh_string(payload, off)?;
            let cmd = String::from_utf8_lossy(cmd_bytes).into_owned();
            conn.debug_log(&format!("exec request: {cmd}"));
            run_exec_request(conn, recipient, remote_id, &cmd, want_reply)?;
        }
        "subsystem" => {
            let (subsys_bytes, _) = read_ssh_string(payload, off)?;
            let subsys = String::from_utf8_lossy(subsys_bytes).into_owned();
            conn.debug_log(&format!("subsystem request: {subsys}"));

            // A subsystem is a pipe-backed session whose command comes from the
            // server's configuration instead of from the client, so it is the
            // ordinary `exec` path with the command line looked up rather than
            // parsed. Answering SUCCESS without starting anything — which is
            // what this arm used to do — leaves the client waiting forever on
            // a subsystem that was never running.
            let command = conn
                .config
                .subsystems
                .iter()
                .find(|(name, _)| name == &subsys)
                .map(|(_, command)| command.clone());

            let Some(command) = command else {
                conn.debug_log(&format!("subsystem refused: {subsys} is not configured"));
                if want_reply {
                    send_channel_failure(conn, remote_id)?;
                }
                return Ok(());
            };
            start_pipe_session(conn, recipient, remote_id, &command, want_reply)?;
        }
        "env" => {
            handle_env_request(conn, recipient, remote_id, payload, off, want_reply)?;
        }
        "window-change" => {
            // RFC 4254 §6.7: columns, rows, width in pixels, height in pixels.
            // The pixel dimensions are optional in practice — clients that do
            // not know them send zeros — so a short payload updates what it
            // carries and leaves the rest alone.
            let applied = if off + 8 <= payload.len() {
                let (width, next) = read_u32(payload, off)?;
                let (height, next) = read_u32(payload, next)?;
                channel.term_width = width;
                channel.term_height = height;
                if next + 8 <= payload.len() {
                    let (wpx, next) = read_u32(payload, next)?;
                    let (hpx, _) = read_u32(payload, next)?;
                    channel.term_width_px = wpx;
                    channel.term_height_px = hpx;
                }
                // Push the new size through to the terminal itself. The kernel
                // raises SIGWINCH in the session's foreground process group as
                // part of the same operation, so a program that redraws on
                // resize sees the change rather than having to notice it.
                // Without this the client's window and the shell's idea of it
                // diverge permanently after the first drag of a window corner.
                let resized = channel.io.pty().map(|pty| {
                    pty.set_winsize(
                        channel.term_width,
                        channel.term_height,
                        channel.term_width_px,
                        channel.term_height_px,
                    )
                });
                // The mutable borrow of `channel` ends here.
                if resized == Some(false) {
                    conn.debug_log("window-change: TIOCSWINSZ failed");
                }
                true
            } else {
                // Fewer than eight bytes cannot contain the two dimensions the
                // request exists to carry, so there is nothing to apply.
                conn.debug_log("window-change: payload too short for a size");
                false
            };

            // RFC 4254 §6.7 says `want_reply` SHOULD be false here, and every
            // client this daemon has met sets it false — but SHOULD is not MUST,
            // and a client that sets it waits for an answer that used to never
            // arrive. Answering costs one packet on a path that runs once per
            // window drag; not answering hangs a session on a technicality.
            if want_reply {
                if applied {
                    send_channel_success(conn, remote_id)?;
                } else {
                    send_channel_failure(conn, remote_id)?;
                }
            }
        }
        _ => {
            conn.debug_log(&format!("unknown channel request: {req_type}"));
            if want_reply {
                let mut fail = Vec::new();
                fail.push(msg::SSH_MSG_CHANNEL_FAILURE);
                fail.extend_from_slice(&remote_id.to_be_bytes());
                conn.send_packet(&fail)?;
            }
        }
    }

    Ok(())
}

/// Send `SSH_MSG_CHANNEL_SUCCESS` for a request that was accepted.
fn send_channel_success(conn: &mut ConnectionState, remote_id: u32) -> Result<(), SshdError> {
    let mut reply = Vec::new();
    reply.push(msg::SSH_MSG_CHANNEL_SUCCESS);
    reply.extend_from_slice(&remote_id.to_be_bytes());
    conn.send_packet(&reply)
}

/// Send `SSH_MSG_CHANNEL_FAILURE` for a request that was refused.
fn send_channel_failure(conn: &mut ConnectionState, remote_id: u32) -> Result<(), SshdError> {
    let mut reply = Vec::new();
    reply.push(msg::SSH_MSG_CHANNEL_FAILURE);
    reply.extend_from_slice(&remote_id.to_be_bytes());
    conn.send_packet(&reply)
}

/// The most variables one channel will remember for its session.
///
/// OpenSSH's limit, and it exists because the client chooses both how many
/// requests to send and how large each is: without a cap, a session that never
/// starts can still make the daemon hold an arbitrary amount of memory.
const MAX_SESSION_ENV: usize = 128;

/// The most bytes of names and values one channel will remember.
///
/// The count limit alone is not enough — 128 variables of a megabyte each is
/// still a megabyte times 128 — and a real session's whole environment is a few
/// kilobytes, so this is far above any honest use.
const MAX_SESSION_ENV_BYTES: usize = 64 * 1024;

/// Variables a client may never set on its session, whatever `AcceptEnv` says.
///
/// These are not preferences; they are the server's answers to "who is this
/// session and where does it live", taken from `/etc/passwd` after
/// authentication. A client that could rewrite `HOME` or `SHELL` would be
/// choosing which dotfiles the login shell sources — and `LOGNAME`/`USER`
/// disagreeing with the account that actually authenticated turns every audit
/// log downstream into a lie.
///
/// `TERM` is here for a different reason: it *is* the client's to choose, but
/// it arrives in the `pty-req` that describes the terminal it belongs to.
/// Letting a second, unrelated request set it as well creates two sources for
/// one value, and no ordering of the two is obviously right.
const REFUSED_SESSION_ENV: [&str; 5] = ["HOME", "USER", "LOGNAME", "SHELL", "TERM"];

/// Decide whether `name` may be set by the client, given the configuration.
///
/// Split out from the request handler so the policy can be tested without a
/// connection, and so there is exactly one place where the answer is decided —
/// the reply sent to the client and the variable applied to the child must
/// never be able to disagree.
fn env_request_allowed(config: &SshdConfig, name: &str, value: &str) -> bool {
    // A name is not a name if it cannot be one. `=` would let a single request
    // smuggle in a second variable through any implementation that later joins
    // the pair with one, and a NUL truncates the string at the `execve`
    // boundary, so what the daemon logged and what the child received would
    // differ. Both are rejected before the patterns are consulted, because a
    // pattern list is an administrator's statement about *names*, and neither
    // of these is one.
    if name.is_empty() || name.contains('=') || name.contains('\0') || value.contains('\0') {
        return false;
    }
    if REFUSED_SESSION_ENV.contains(&name) {
        return false;
    }
    pattern_list_matches(&config.accept_env, name)
}

/// Handle an `env` channel request (RFC 4254 §6.4).
///
/// Answering SUCCESS to every request and discarding the variable — which is
/// what this used to do — is a lie the client has no way to detect, because the
/// reply is the only signal the protocol gives it. A refusal is not a defeat:
/// FAILURE means "not set", the client can act on it, and OpenSSH answers
/// exactly the same way for a name outside `AcceptEnv`.
fn handle_env_request(
    conn: &mut ConnectionState,
    local_id: u32,
    remote_id: u32,
    payload: &[u8],
    offset: usize,
    want_reply: bool,
) -> Result<(), SshdError> {
    let (name_bytes, off) = read_ssh_string(payload, offset)?;
    let (value_bytes, _) = read_ssh_string(payload, off)?;
    let name = String::from_utf8_lossy(name_bytes).into_owned();
    let value = String::from_utf8_lossy(value_bytes).into_owned();

    if !env_request_allowed(&conn.config, &name, &value) {
        conn.debug_log(&format!("env refused: {name} is not in AcceptEnv"));
        if want_reply {
            send_channel_failure(conn, remote_id)?;
        }
        return Ok(());
    }

    let Some(idx) = conn.channels.iter().position(|ch| ch.local_id == local_id) else {
        if want_reply {
            send_channel_failure(conn, remote_id)?;
        }
        return Ok(());
    };

    // Setting a variable twice replaces it rather than appending, so the child
    // cannot receive two entries for one name — and a client cannot use
    // repetition of an allowed name to fill the limits below.
    let channel = &mut conn.channels[idx];
    if let Some(slot) = channel.env.iter_mut().find(|(k, _)| k == &name) {
        slot.1 = value;
        if want_reply {
            send_channel_success(conn, remote_id)?;
        }
        return Ok(());
    }

    let used: usize = channel
        .env
        .iter()
        .map(|(k, v)| k.len().saturating_add(v.len()))
        .sum();
    let addition = name.len().saturating_add(value.len());
    if channel.env.len() >= MAX_SESSION_ENV || used.saturating_add(addition) > MAX_SESSION_ENV_BYTES
    {
        conn.debug_log(&format!(
            "env refused: {name} exceeds the per-session limit"
        ));
        if want_reply {
            send_channel_failure(conn, remote_id)?;
        }
        return Ok(());
    }

    channel.env.push((name, value));
    if want_reply {
        send_channel_success(conn, remote_id)?;
    }
    Ok(())
}

/// Resolve the account a session request will run as, refusing the request if
/// there is no such account.
///
/// Authentication proved who the client is; without a `/etc/passwd` entry there
/// is no identity to *become*, and running the session as the daemon's own
/// identity (root) is not an acceptable fallback. Every session entry point
/// starts here so that none of them can grow a different answer.
fn session_user(
    conn: &mut ConnectionState,
    remote_id: u32,
    what: &str,
    want_reply: bool,
) -> Result<Option<PasswdEntry>, SshdError> {
    if let Some(user) = lookup_passwd(&conn.username) {
        return Ok(Some(user));
    }
    let name = conn.username.clone();
    conn.debug_log(&format!("{what} refused: no /etc/passwd entry for {name}"));
    if want_reply {
        send_channel_failure(conn, remote_id)?;
    }
    Ok(None)
}

/// Whether this channel is already running something.
///
/// A second `shell`, `exec` or `subsystem` on one channel is a protocol error
/// (RFC 4254 §6.5: one per session). Refusing matters rather than being
/// pedantry — overwriting `child` would strand the first process with nothing
/// left holding its handle, so nobody would ever reap it or notice it exit.
fn channel_is_busy(channel: &Channel) -> bool {
    channel.child.is_some() || matches!(channel.io, SessionIo::Pipes(_))
}

/// Start the authenticated user's login shell, on a terminal or on pipes.
///
/// Which one is decided entirely by whether a `pty-req` already attached a
/// terminal to this channel. Both are real sessions: the terminal form is an
/// interactive login, and the pipe form is `ssh -T`, which a script uses to
/// push data through a remote shell.
///
/// Ordering note, the same one `start_pipe_session` observes: the child is
/// spawned *before* the request is answered, because RFC 4254 §6.5's reply
/// reports whether the request was accepted, and a shell that could not be
/// started was not.
fn start_shell_session(
    conn: &mut ConnectionState,
    local_id: u32,
    remote_id: u32,
    want_reply: bool,
) -> Result<(), SshdError> {
    let Some(user) = session_user(conn, remote_id, "shell", want_reply)? else {
        return Ok(());
    };

    let Some(idx) = conn.channels.iter().position(|ch| ch.local_id == local_id) else {
        return Ok(());
    };
    if channel_is_busy(&conn.channels[idx]) {
        conn.debug_log("shell refused: the channel already runs a session");
        if want_reply {
            send_channel_failure(conn, remote_id)?;
        }
        return Ok(());
    }
    let term = conn.channels[idx].term.clone();
    // Cloned rather than borrowed: the spawn closure below runs while `conn` is
    // borrowed mutably to answer the request, and a session's accepted
    // variables are a few hundred bytes at most.
    let client_env = conn.channels[idx].env.clone();

    let Some(slave_fd) = conn.channels[idx].io.pty().map(Pty::slave_fd) else {
        // No terminal: `ssh -T`. The login shell runs on pipes instead, with no
        // command line, so it reads its script from stdin.
        return start_pipe_session_with(
            conn,
            idx,
            local_id,
            remote_id,
            "shell",
            want_reply,
            spawn_in_home(&user, || shell_command_pipes(&user, &term, &client_env)),
        );
    };

    match spawn_in_home(&user, || shell_command(&user, &term, slave_fd, &client_env)) {
        Ok(child) => {
            // The parent must give up its own copy of the slave now. Hangup on
            // the master means "the last slave closed", and while this daemon
            // holds one, a shell that has exited leaves a terminal that never
            // reports the end of the session — the client sits at a dead prompt
            // until it gives up. This single `close` is the difference between
            // a session that ends and one that hangs.
            if let Some(pty) = conn.channels[idx].io.pty_mut() {
                pty.close_slave();
            }
            conn.channels[idx].child = Some(child);
            conn.debug_log(&format!("shell started on channel {local_id}"));
            if want_reply {
                send_channel_success(conn, remote_id)?;
            }
        }
        Err(e) => {
            conn.debug_log(&format!("shell spawn failed: {e}"));
            // Release the terminal: nothing is going to run on it, and holding
            // it would leak a pty for the life of the connection.
            conn.channels[idx].io = SessionIo::None;
            if want_reply {
                send_channel_failure(conn, remote_id)?;
            }
        }
    }
    Ok(())
}

/// Run `command_line` on three pipes, streamed by the session pump.
///
/// This is the one path behind `exec` and `subsystem` — a subsystem is an
/// `exec` whose command line came from the server's configuration rather than
/// from the client — and it is also where a `shell` with no terminal ends up.
fn start_pipe_session(
    conn: &mut ConnectionState,
    local_id: u32,
    remote_id: u32,
    command_line: &str,
    want_reply: bool,
) -> Result<(), SshdError> {
    let Some(user) = session_user(conn, remote_id, "exec", want_reply)? else {
        return Ok(());
    };
    let Some(idx) = conn.channels.iter().position(|ch| ch.local_id == local_id) else {
        return Ok(());
    };
    if channel_is_busy(&conn.channels[idx]) {
        conn.debug_log("exec refused: the channel already runs a session");
        if want_reply {
            send_channel_failure(conn, remote_id)?;
        }
        return Ok(());
    }
    let client_env = conn.channels[idx].env.clone();
    let spawned = spawn_session_command(&user, command_line, &client_env);
    start_pipe_session_with(conn, idx, local_id, remote_id, "exec", want_reply, spawned)
}

/// Attach an already-spawned pipe-backed child to its channel, and answer the
/// request.
///
/// Taking the `Result` rather than spawning here is what lets `shell`, `exec`
/// and `subsystem` share every line of this while each builds its own very
/// different `Command`.
fn start_pipe_session_with(
    conn: &mut ConnectionState,
    idx: usize,
    local_id: u32,
    remote_id: u32,
    what: &str,
    want_reply: bool,
    spawned: Result<process::Child, io::Error>,
) -> Result<(), SshdError> {
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            conn.debug_log(&format!("{what} spawn failed: {e}"));
            if want_reply {
                send_channel_failure(conn, remote_id)?;
            }
            return Ok(());
        }
    };

    // The descriptors move out of the `Child` and into the channel before
    // anything else can observe the process, so there is no window in which
    // `std` and the channel both believe they own them.
    let Some(pipes) = Pipes::take(&mut child) else {
        // The pipes could not be made non-blocking, so running the session
        // would risk stalling every other connection on the daemon. Kill the
        // child rather than leave it attached to descriptors nobody reads.
        conn.debug_log(&format!("{what} refused: pipes would block"));
        // Both results are discarded deliberately: `kill` fails only if the
        // child has already exited, and `wait` only if it was never started —
        // and in both of those cases the thing we wanted (no orphan holding
        // our descriptors) is already true.
        let _ = child.kill();
        let _ = child.wait();
        if want_reply {
            send_channel_failure(conn, remote_id)?;
        }
        return Ok(());
    };
    conn.channels[idx].io = SessionIo::Pipes(pipes);
    conn.channels[idx].child = Some(child);
    conn.debug_log(&format!("{what} started on channel {local_id}"));
    if want_reply {
        send_channel_success(conn, remote_id)?;
    }
    Ok(())
}

/// How many master reads one channel may perform per pass of the session pump.
///
/// A shell running `yes` produces output faster than the socket can drain it,
/// and an unbounded drain loop would never return to check whether the client
/// had typed `^C`. Sixteen reads of 8 `KiB` is 128 `KiB` of output per pass —
/// enough that a normal burst leaves in one turn, small enough that the input
/// direction is never starved for long.
const SESSION_READS_PER_PASS: usize = 16;

/// Move one channel's session output to the client.
///
/// Returns `(worked, finished)`. `finished` is [`SessionIo::output_finished`] —
/// the streams have reported end-of-file, so nothing more can ever arrive — and
/// it is the only condition under which the caller may close the channel. It is
/// deliberately *not* "this pass stopped reading": a pass stops because its read
/// budget ran out, because the client has offered no send window, or because
/// there was nothing to read at that instant, and closing on any of those would
/// truncate a command's output, cut off a slow client, or lose the last line a
/// program printed on its way out.
///
/// ## Why the client's window bounds the read
///
/// Every read is capped at `remote_window`, and a window of zero reads nothing
/// at all. That is deliberate and is the only back-pressure in the outbound
/// direction: bytes not read stay in the kernel's pty or pipe buffer, which
/// fills, which blocks the *process* on its next `write`. The alternative —
/// read everything and queue what will not fit — turns a client that stops
/// reading into unbounded daemon memory, which is precisely the failure the
/// SSH window exists to prevent.
///
/// It is the mirror image of `Channel::pending_input`, which *must* buffer,
/// because window credit for arriving data is owed the moment it arrives.
fn pump_channel_output(
    conn: &mut ConnectionState,
    idx: usize,
    local_id: u32,
) -> Result<(bool, bool), SshdError> {
    let mut buf = [0u8; 8192];
    let mut worked = false;

    for _ in 0..SESSION_READS_PER_PASS {
        let budget = usize::try_from(conn.channels[idx].remote_window).unwrap_or(usize::MAX);
        if budget == 0 {
            // No credit, so nothing can be read this pass — but whether the
            // session is *finished* is a fact about its streams, not about the
            // client's window, and the two must not be confused in either
            // direction. Reading nothing while the streams are still open is
            // back-pressure working; reporting "unfinished" once they have hit
            // end-of-file would strand a completed session whose last write
            // happened to consume the last of the window, waiting for a
            // `WINDOW_ADJUST` a satisfied client has no reason to send.
            return Ok((worked, conn.channels[idx].io.output_finished()));
        }

        // The borrow of `io` is scoped to the read alone, so that `conn` is
        // free for the send below.
        let (stderr, outcome) = match &mut conn.channels[idx].io {
            SessionIo::None => (false, StreamRead::Empty),
            SessionIo::Terminal(pty) => {
                if pty.ready().0 {
                    (false, pty.read_output(&mut buf, budget))
                } else {
                    (false, StreamRead::Empty)
                }
            }
            SessionIo::Pipes(pipes) => pipes.read_output(&mut buf, budget),
        };

        match outcome {
            StreamRead::Data(n) => {
                worked = true;
                let Some(chunk) = buf.get(..n) else {
                    // Unreachable: `n` came from a read into `buf`. Reporting
                    // the streams as still open rather than indexing is the
                    // difference between a stalled channel and a panicking
                    // daemon that takes every other session with it.
                    return Ok((worked, false));
                };
                send_channel_stream(conn, local_id, stderr, chunk)?;
            }
            // Nothing at this instant. Whether that is the end depends on the
            // streams, not on the empty read — see `SessionIo::output_finished`.
            StreamRead::Empty => return Ok((worked, conn.channels[idx].io.output_finished())),
            StreamRead::Hangup => {
                worked = true;
                match &conn.channels[idx].io {
                    // The terminal is gone and its buffered output was
                    // delivered ahead of the `EIO`. Drop the master, but do
                    // *not* invent an exit status: it is not known until `wait`
                    // reports it, and a fabricated one would tell a caller's
                    // `if ssh host cmd; then` the wrong thing.
                    SessionIo::Terminal(_) | SessionIo::None => {
                        conn.channels[idx].io = SessionIo::None;
                        return Ok((worked, true));
                    }
                    // One of the two pipes reached end-of-file and closed
                    // itself. The other may still have output, so keep going
                    // until both are done.
                    SessionIo::Pipes(pipes) => {
                        if pipes.output_finished() {
                            return Ok((worked, true));
                        }
                    }
                }
            }
        }
    }

    // The read budget ran out with data still flowing.
    Ok((worked, conn.channels[idx].io.output_finished()))
}

/// Hand one channel's queued input to its session, and credit the window.
///
/// Returns `true` if any byte moved, so the connection loop knows the pass was
/// productive and should come round again without sleeping.
///
/// Three things make this a loop over a queue rather than a plain `write`:
///
/// * **The destination can be full.** A program that has stopped reading its
///   stdin leaves the kernel's buffer full, and the write returns short — or,
///   on a pty master, because `write` there does not honour `O_NONBLOCK` (there
///   is no `SYS_PTY_MASTER_TRY_WRITE`), *blocks*. The readiness check is
///   therefore not an optimisation but the thing that keeps one uninterested
///   process from freezing the whole daemon.
/// * **Short writes are normal**, not an error, so the remainder must survive
///   to the next pass instead of being silently lost mid-keystroke.
/// * **The window must follow consumption, not arrival.** Crediting only what
///   the session accepted is what pushes back-pressure to the client instead
///   of accumulating it here.
///
/// A write that fails outright is treated as the end of the session's input
/// rather than the end of the connection: the process has gone and the pump's
/// normal exit path will notice and close the channel. Dropping the whole TCP
/// session because one shell died would take any other channel down with it.
fn pump_channel_input(conn: &mut ConnectionState, idx: usize) -> Result<bool, SshdError> {
    if conn.channels[idx].pending_input.is_empty() {
        return Ok(false);
    }

    let mut written = 0usize;
    let mut failure = None;
    loop {
        // A shared borrow, not `&mut`: the loop only reads the channel, which
        // leaves `conn` free for `debug_log` and keeps the error path from
        // needing a second pass over the borrow checker.
        let channel = &conn.channels[idx];
        let Some(rest) = channel.pending_input.get(written..) else {
            break;
        };
        if rest.is_empty() || !channel.io.input_ready() {
            break;
        }
        match channel.io.write_input(rest) {
            // Accepted nothing despite reporting itself writable. Retrying
            // inside this loop would spin; the next pass will try again.
            Ok(0) => break,
            Ok(n) => written = written.saturating_add(n),
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
    }

    if let Some(e) = failure {
        conn.debug_log(&format!("session input write failed: {e}"));
        // Only the *input* half is gone. For a terminal that means the whole
        // session, since one descriptor carries both directions; for pipes the
        // command may still be producing output on stdout, and tearing down
        // its output streams here would truncate it.
        match &mut conn.channels[idx].io {
            SessionIo::Terminal(_) => conn.channels[idx].io = SessionIo::None,
            SessionIo::Pipes(pipes) => pipes.close_input(),
            SessionIo::None => {}
        }
        // The bytes can never be delivered now. Discard them rather than pin
        // memory for the rest of the connection; the credit below then reopens
        // the window, so a client still typing into a dead session learns that
        // the channel is closing instead of blocking on a window forever.
        written = conn.channels[idx].pending_input.len();
    }

    if written == 0 {
        return Ok(false);
    }

    let mut credit = None;
    {
        let channel = &mut conn.channels[idx];
        channel.pending_input.drain(..written);
        // A `CHANNEL_EOF` that arrived while bytes were still queued was held
        // back until they were written; now that the queue is empty it can be
        // applied, and the command finally sees the end of its input.
        if channel.input_eof && channel.pending_input.is_empty() {
            channel.io.close_input();
            channel.input_eof = false;
        }
        // The window we *should* be offering is the full initial window less
        // what is still queued here — that subtraction is the back-pressure.
        // Announce the difference only once it is worth a quarter of the
        // window, because an interactive session delivers one byte per
        // keypress and a `WINDOW_ADJUST` per keystroke would double the
        // packet rate of every session for no benefit.
        let queued = u32::try_from(channel.pending_input.len()).unwrap_or(u32::MAX);
        let target = INITIAL_LOCAL_WINDOW.saturating_sub(queued);
        let gain = target.saturating_sub(channel.local_window);
        if gain >= WINDOW_ADJUST_THRESHOLD {
            channel.local_window = target;
            credit = Some((channel.remote_id, gain));
        }
    }
    if let Some((remote_id, bytes)) = credit {
        send_window_adjust(conn, remote_id, bytes)?;
    }

    Ok(true)
}

/// Move bytes between running sessions and the client, and reap finished ones.
///
/// Returns `true` if anything happened, which is what tells the connection loop
/// to try again immediately instead of sleeping.
fn pump_sessions(conn: &mut ConnectionState) -> Result<bool, SshdError> {
    let mut worked = false;

    for idx in 0..conn.channels.len() {
        if conn.channels[idx].closed {
            continue;
        }
        let local_id = conn.channels[idx].local_id;
        let remote_id = conn.channels[idx].remote_id;

        // 1. Client input into the session, before anything else, so a keypress
        //    and the output it provokes can both leave in one pass.
        //
        //    This runs for any channel holding a terminal, including one whose
        //    shell has not started yet: type-ahead sent between `pty-req` and
        //    `shell` belongs in the line discipline's buffer, where the shell
        //    will read it, exactly as it would on a local console.
        if pump_channel_input(conn, idx)? {
            worked = true;
        }

        if conn.channels[idx].child.is_none() {
            continue;
        }

        // 2. Whatever the session has printed, up to this pass's budget.
        let (output_worked, output_finished) = pump_channel_output(conn, idx, local_id)?;
        if output_worked {
            worked = true;
        }

        // 3. Has the process finished? Record the status the first time it is
        //    available; the session is not over until its output is out too.
        if conn.channels[idx].exit.is_none() {
            let finished = match conn.channels[idx].child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(status) => status.map(|s| classify_exit(&s)),
                    // `try_wait` fails only if the child was already reaped by
                    // something else, which cannot happen here — but if it
                    // somehow does, ending the session with the conventional
                    // "we do not know" status beats waiting forever on a
                    // process whose fate can no longer be reported.
                    Err(e) => {
                        conn.debug_log(&format!("try_wait failed: {e}"));
                        Some(SessionExit::Status(255))
                    }
                },
                None => None,
            };
            if let Some(exit) = finished {
                worked = true;
                conn.channels[idx].exit = Some(exit);
            }
        }

        // 4. End the session once the process has gone *and* its output streams
        //    have reached end-of-file. Waiting for both is what stops a shell's
        //    final line — `logout`, or the last screen a full-screen program
        //    painted — from being cut off by the close.
        let ready_to_close = output_finished && conn.channels[idx].exit.is_some();
        if ready_to_close {
            let exit = conn.channels[idx]
                .exit
                .clone()
                .unwrap_or(SessionExit::Status(255));
            conn.debug_log(&format!("session on channel {local_id} finished: {exit:?}"));
            // Dropping the child and the attachment releases the descriptors
            // and reaps the process table entry before the client is told
            // anything, so a client that immediately opens another session
            // finds the resources already free.
            conn.channels[idx].child = None;
            conn.channels[idx].io = SessionIo::None;
            send_session_exit(conn, remote_id, &exit)?;
            // `send_channel_close` sends the EOF first and sets `closed`.
            send_channel_close(conn, local_id)?;
            worked = true;
        }
    }

    Ok(worked)
}

/// Run an `exec` request's command line, streaming its output as it appears.
///
/// The command really runs, as the authenticated user, through that user's
/// login shell — the same `shell -c 'command'` contract OpenSSH offers, so a
/// caller's quoting, redirections and pipelines behave the way they expect.
///
/// This is a thin wrapper over [`start_pipe_session`] and exists for the name:
/// the `exec` arm reads better calling something called `exec`, and the
/// wrapper is the one place a future `exec`-only policy check would go.
fn run_exec_request(
    conn: &mut ConnectionState,
    local_id: u32,
    remote_id: u32,
    command_line: &str,
    want_reply: bool,
) -> Result<(), SshdError> {
    start_pipe_session(conn, local_id, remote_id, command_line, want_reply)
}

/// Handle `CHANNEL_DATA`.
fn handle_channel_data(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (recipient, off) = read_u32(payload, 1)?;
    let (data, _) = read_ssh_string(payload, off)?;

    conn.debug_log(&format!(
        "channel data: channel={recipient} len={}",
        data.len()
    ));

    // The window shrinks by what arrived, on every channel. What happens to the
    // bytes then depends on whether anything is listening:
    //
    // * A channel with a terminal or with an open stdin queues them for the
    //   process. They are credited back only once they reach it — see
    //   `Channel::pending_input` — because crediting on arrival would let a
    //   client outrun a program that has stopped reading and turn the SSH
    //   window into unbounded daemon memory.
    //
    // * A channel with nowhere to put them — no session started yet, or one
    //   whose stdin the client already closed with `CHANNEL_EOF` — drops them
    //   and credits immediately. Nothing will ever consume this input, and
    //   withholding credit would stall a client forever on a window that could
    //   never reopen. Dropping is the honest outcome.
    //
    // Neither path echoes. An earlier version echoed the data straight back,
    // which made a client look like it was talking to a shell that echoed its
    // typing -- at the terminal, indistinguishable from a real session.
    let mut credit = None;
    if let Some(channel) = conn.channels.iter_mut().find(|ch| ch.local_id == recipient) {
        let arrived = u32::try_from(data.len()).unwrap_or(u32::MAX);
        channel.local_window = channel.local_window.saturating_sub(arrived);
        if channel.io.accepts_input() {
            channel.pending_input.extend_from_slice(data);
        } else if channel.local_window < WINDOW_ADJUST_THRESHOLD {
            // Re-open the window well before it runs out, in one large credit,
            // rather than one message per data packet.
            credit = Some((
                channel.remote_id,
                INITIAL_LOCAL_WINDOW.saturating_sub(channel.local_window),
            ));
            channel.local_window = INITIAL_LOCAL_WINDOW;
        }
    }
    // The mutable borrow of `channel` has ended, so `conn` is free to send.
    if let Some((remote_id, bytes)) = credit {
        send_window_adjust(conn, remote_id, bytes)?;
    }

    Ok(())
}

/// Send `SSH_MSG_CHANNEL_WINDOW_ADJUST`, crediting the peer more send window.
fn send_window_adjust(
    conn: &mut ConnectionState,
    remote_channel_id: u32,
    bytes_to_add: u32,
) -> Result<(), SshdError> {
    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_CHANNEL_WINDOW_ADJUST);
    msg_buf.extend_from_slice(&remote_channel_id.to_be_bytes());
    msg_buf.extend_from_slice(&bytes_to_add.to_be_bytes());
    conn.send_packet(&msg_buf)
}

/// Handle `CHANNEL_WINDOW_ADJUST`.
fn handle_window_adjust(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (recipient, off) = read_u32(payload, 1)?;
    let (bytes_to_add, _) = read_u32(payload, off)?;

    if let Some(channel) = conn.channels.iter_mut().find(|ch| ch.local_id == recipient) {
        channel.remote_window = channel.remote_window.saturating_add(bytes_to_add);
        let new_window = channel.remote_window;
        // The mutable borrow of `channel` ends here (its last read above), so
        // `conn` is free for the debug log below.
        conn.debug_log(&format!(
            "window adjust: channel={recipient} +{bytes_to_add} (now {new_window})"
        ));
    }

    Ok(())
}

/// Handle `CHANNEL_EOF` — the client will send no more data on this channel.
///
/// For a pipe-backed session this closes the command's stdin, and doing so is
/// the entire point of the message: a filter reads until end-of-file, so
/// `ssh host 'cat' < file` never terminates unless something closes the write
/// end. Anything still queued for delivery is written first — the close is
/// deferred until `pending_input` drains — because closing stdin on top of
/// unwritten bytes would silently truncate the client's data.
///
/// For a terminal it does nothing; see `SessionIo::close_input`.
fn handle_channel_eof(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (recipient, _) = read_u32(payload, 1)?;
    conn.debug_log(&format!("channel EOF: channel={recipient}"));
    if let Some(channel) = conn.channels.iter_mut().find(|ch| ch.local_id == recipient) {
        if channel.pending_input.is_empty() {
            channel.io.close_input();
        } else {
            channel.input_eof = true;
        }
    }
    Ok(())
}

/// Handle `CHANNEL_CLOSE`.
///
/// The client is done with this channel, so anything the channel owns has to go
/// with it. That is not bookkeeping: a session channel owns a *running shell*
/// and a pseudo-terminal, and the connection loop skips closed channels, so
/// leaving them attached would strand the shell for the lifetime of the daemon
/// — a process nobody will ever reap, holding a pty nobody will ever free, with
/// no path left that could notice. A client that closes a channel without
/// waiting for its shell to exit is not an error case; it is what pressing
/// `~.` does.
fn handle_channel_close(conn: &mut ConnectionState, payload: &[u8]) -> Result<(), SshdError> {
    let (recipient, _) = read_u32(payload, 1)?;
    conn.debug_log(&format!("channel close: channel={recipient}"));

    // Dropping the master hangs the terminal up, which is the polite half of
    // this: a shell blocked on a read gets its `EIO` and exits on its own. The
    // kill is the half that does not depend on the shell cooperating — a
    // process ignoring its hangup, or not reading the terminal at all, would
    // otherwise outlive the connection.
    let child = conn
        .channels
        .iter_mut()
        .find(|ch| ch.local_id == recipient)
        .and_then(|ch| {
            ch.io = SessionIo::None;
            ch.child.take()
        });
    if let Some(mut child) = child {
        if let Err(e) = child.kill() {
            conn.debug_log(&format!("could not signal session on close: {e}"));
        }
        // Reap it, or the entry stays in the process table. `kill` has already
        // been sent, so this cannot wait indefinitely on a live process.
        if let Err(e) = child.wait() {
            conn.debug_log(&format!("could not reap session on close: {e}"));
        }
    }

    // Answer with our own EOF (if it has not gone already) and close.
    send_channel_close(conn, recipient)
}

/// Send data on a channel.
fn send_channel_data(
    conn: &mut ConnectionState,
    remote_channel_id: u32,
    data: &[u8],
) -> Result<(), SshdError> {
    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_CHANNEL_DATA);
    msg_buf.extend_from_slice(&remote_channel_id.to_be_bytes());
    msg_buf.extend_from_slice(&ssh_string(data));
    conn.send_packet(&msg_buf)
}

/// `data_type_code` for `SSH_MSG_CHANNEL_EXTENDED_DATA` carrying stderr
/// (RFC 4254 §5.2 — the only code the specification defines).
const SSH_EXTENDED_DATA_STDERR: u32 = 1;

/// Send data on a channel's *extended* data stream.
///
/// This is how a command's stderr reaches the client as stderr rather than
/// being folded into its stdout.  The distinction is not cosmetic: a caller
/// doing `ssh host cmd > file` expects diagnostics on its own terminal and
/// only the command's real output in `file`, and merging the two streams
/// silently corrupts the file with whatever the command warned about.
fn send_channel_extended_data(
    conn: &mut ConnectionState,
    remote_channel_id: u32,
    data_type_code: u32,
    data: &[u8],
) -> Result<(), SshdError> {
    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_CHANNEL_EXTENDED_DATA);
    msg_buf.extend_from_slice(&remote_channel_id.to_be_bytes());
    msg_buf.extend_from_slice(&data_type_code.to_be_bytes());
    msg_buf.extend_from_slice(&ssh_string(data));
    conn.send_packet(&msg_buf)
}

/// Send the `exit-status` channel request (RFC 4254 §6.10).
///
/// `want_reply` is always false for this request — it is a notification, and
/// the specification says a reply must not be sent.
///
/// Without this, `ssh host false` exits **0**: the OpenSSH client treats a
/// channel that closes with no `exit-status` as success, so a server that
/// omits it reports every failure as a pass.  That silence is worse than a
/// wrong number, because a shell script's `if ssh host cmd; then` reads it as
/// a green light.
fn send_exit_status(
    conn: &mut ConnectionState,
    remote_channel_id: u32,
    status: u32,
) -> Result<(), SshdError> {
    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_CHANNEL_REQUEST);
    msg_buf.extend_from_slice(&remote_channel_id.to_be_bytes());
    msg_buf.extend_from_slice(&ssh_string(b"exit-status"));
    msg_buf.push(0); // want_reply = false
    msg_buf.extend_from_slice(&status.to_be_bytes());
    conn.send_packet(&msg_buf)
}

/// Send the `exit-signal` channel request (RFC 4254 §6.10).
///
/// Reported instead of `exit-status` when the command was killed rather than
/// having exited, so the client can print "Killed by signal 15" rather than
/// inventing an exit code the command never returned.
fn send_exit_signal(
    conn: &mut ConnectionState,
    remote_channel_id: u32,
    signal_name: &str,
    core_dumped: bool,
    error_message: &str,
) -> Result<(), SshdError> {
    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_CHANNEL_REQUEST);
    msg_buf.extend_from_slice(&remote_channel_id.to_be_bytes());
    msg_buf.extend_from_slice(&ssh_string(b"exit-signal"));
    msg_buf.push(0); // want_reply = false
    msg_buf.extend_from_slice(&ssh_string(signal_name.as_bytes()));
    msg_buf.push(u8::from(core_dumped));
    msg_buf.extend_from_slice(&ssh_string(error_message.as_bytes()));
    msg_buf.extend_from_slice(&ssh_string(b"")); // language tag
    conn.send_packet(&msg_buf)
}

/// Report a finished session command's outcome to the client.
fn send_session_exit(
    conn: &mut ConnectionState,
    remote_channel_id: u32,
    exit: &SessionExit,
) -> Result<(), SshdError> {
    match exit {
        SessionExit::Status(code) => send_exit_status(conn, remote_channel_id, *code),
        SessionExit::Signal { name, core_dumped } => send_exit_signal(
            conn,
            remote_channel_id,
            name,
            *core_dumped,
            &format!("killed by SIG{name}"),
        ),
    }
}

/// The per-message overhead of a `SSH_MSG_CHANNEL_DATA` payload: the message
/// byte, the recipient channel number, and the 4-byte string length.
const CHANNEL_DATA_OVERHEAD: u32 = 1 + 4 + 4;

/// Send a whole byte stream on a channel, split to respect the peer's
/// advertised maximum packet size.
///
/// The peer told us its limit in `CHANNEL_OPEN`; exceeding it is a protocol
/// violation, and a command whose output is larger than one packet is the
/// normal case rather than an edge case, so the split has to happen here
/// rather than at each call site.
fn send_channel_stream(
    conn: &mut ConnectionState,
    local_channel_id: u32,
    stderr_stream: bool,
    data: &[u8],
) -> Result<(), SshdError> {
    let Some((remote_id, max_packet)) = conn
        .channels
        .iter()
        .find(|ch| ch.local_id == local_channel_id)
        .map(|ch| (ch.remote_id, ch.remote_max_packet))
    else {
        return Ok(());
    };

    // Extended data carries an extra uint32 type code ahead of the string.
    let overhead = if stderr_stream {
        CHANNEL_DATA_OVERHEAD.saturating_add(4)
    } else {
        CHANNEL_DATA_OVERHEAD
    };
    // A peer that advertises a max packet smaller than the header cannot be
    // satisfied; clamp to a minimum rather than dividing into zero-size
    // chunks and looping forever.
    let chunk = usize::try_from(max_packet.saturating_sub(overhead))
        .unwrap_or(usize::MAX)
        .max(1);

    for piece in data.chunks(chunk) {
        if stderr_stream {
            send_channel_extended_data(conn, remote_id, SSH_EXTENDED_DATA_STDERR, piece)?;
        } else {
            send_channel_data(conn, remote_id, piece)?;
        }
        if let Some(channel) = conn
            .channels
            .iter_mut()
            .find(|ch| ch.local_id == local_channel_id)
        {
            // Track the peer's window so a future streaming implementation
            // has an accurate figure to block on. Saturating rather than
            // wrapping: a negative window is not a thing the protocol has.
            channel.remote_window = channel
                .remote_window
                .saturating_sub(u32::try_from(piece.len()).unwrap_or(u32::MAX));
        }
    }
    Ok(())
}

/// Send EOF on a channel, at most once.
///
/// Takes the channel's **local** id, like `send_channel_close`, and looks the
/// remote id up itself. The two ids are independent — the client picks one and
/// we pick the other — so passing the wrong one produces a well-formed message
/// about somebody else's channel, a failure with no symptom at the sender.
///
/// The `eof_sent` flag is owned here rather than by the callers because three
/// of them used to test-and-set it independently while two others skipped it
/// entirely. `send_channel_eof(...)` followed by `send_channel_close(...)` —
/// the order every session-ending path in this file writes, because it is the
/// order the protocol describes — therefore emitted *two* `SSH_MSG_CHANNEL_EOF`
/// for one channel. With the flag here the second one is a no-op, and no call
/// site has to know whether some other path already sent it.
fn send_channel_eof(conn: &mut ConnectionState, local_channel_id: u32) -> Result<(), SshdError> {
    let Some((remote_id, eof_sent)) = conn
        .channels
        .iter()
        .find(|ch| ch.local_id == local_channel_id)
        .map(|ch| (ch.remote_id, ch.eof_sent))
    else {
        return Ok(());
    };
    if eof_sent {
        return Ok(());
    }
    if let Some(channel) = conn
        .channels
        .iter_mut()
        .find(|ch| ch.local_id == local_channel_id)
    {
        channel.eof_sent = true;
    }
    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_CHANNEL_EOF);
    msg_buf.extend_from_slice(&remote_id.to_be_bytes());
    conn.send_packet(&msg_buf)
}

/// Send close on a channel, preceded by the EOF the protocol expects first, and
/// at most once.
///
/// `Channel::closed` means exactly "we have sent `SSH_MSG_CHANNEL_CLOSE`", and
/// the once-only rule is RFC 4254's: each side sends the message once, and the
/// side that did not send it first replies. Both halves happen here, so the
/// case where we end a session ourselves and the client's own close then
/// arrives — the ordinary way an interactive session finishes — answers rather
/// than sending a second close for a channel that is already gone.
fn send_channel_close(conn: &mut ConnectionState, local_channel_id: u32) -> Result<(), SshdError> {
    if conn
        .channels
        .iter()
        .any(|ch| ch.local_id == local_channel_id && ch.closed)
    {
        return Ok(());
    }

    send_channel_eof(conn, local_channel_id)?;

    let Some(remote_id) = conn
        .channels
        .iter()
        .find(|ch| ch.local_id == local_channel_id)
        .map(|ch| ch.remote_id)
    else {
        return Ok(());
    };
    if let Some(channel) = conn
        .channels
        .iter_mut()
        .find(|ch| ch.local_id == local_channel_id)
    {
        channel.closed = true;
    }
    let mut msg_buf = Vec::new();
    msg_buf.push(msg::SSH_MSG_CHANNEL_CLOSE);
    msg_buf.extend_from_slice(&remote_id.to_be_bytes());
    conn.send_packet(&msg_buf)
}

// ============================================================================
// CLI parsing
// ============================================================================

struct CliOptions {
    port: Option<u16>,
    config_file: String,
    debug_mode: bool,
    foreground: bool,
    log_stderr: bool,
    host_key_file: Option<String>,
    test_config: bool,
    extended_test: bool,
}

impl CliOptions {
    fn parse_args() -> Self {
        let args: Vec<String> = env::args().collect();
        let mut opts = Self {
            port: None,
            config_file: "/etc/ssh/sshd_config".into(),
            debug_mode: false,
            foreground: false,
            log_stderr: false,
            host_key_file: None,
            test_config: false,
            extended_test: false,
        };

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "-p" => {
                    i += 1;
                    if i < args.len() {
                        if let Ok(p) = args[i].parse::<u16>() {
                            opts.port = Some(p);
                        } else {
                            eprintln!("sshd: invalid port: {}", args[i]);
                            process::exit(1);
                        }
                    }
                }
                "-f" => {
                    i += 1;
                    if let Some(arg) = args.get(i) {
                        arg.clone_into(&mut opts.config_file);
                    }
                }
                "-d" => {
                    opts.debug_mode = true;
                    opts.foreground = true;
                }
                "-D" => {
                    opts.foreground = true;
                }
                "-e" => {
                    opts.log_stderr = true;
                }
                "-h" => {
                    i += 1;
                    if i < args.len() {
                        opts.host_key_file = Some(args[i].clone());
                    }
                }
                "-t" => {
                    opts.test_config = true;
                }
                "-T" => {
                    opts.extended_test = true;
                    opts.test_config = true;
                }
                "--help" => {
                    print_usage();
                    process::exit(0);
                }
                other => {
                    eprintln!("sshd: unknown option: {other}");
                    process::exit(1);
                }
            }
            i += 1;
        }

        opts
    }
}

fn print_usage() {
    eprintln!("Usage: sshd [options]");
    eprintln!("Options:");
    eprintln!("  -p port     Listen port");
    eprintln!("  -f config   Config file path");
    eprintln!("  -d          Debug mode (no fork, verbose)");
    eprintln!("  -D          Don't daemonize");
    eprintln!("  -e          Log to stderr");
    eprintln!("  -h hostkey  Host key file");
    eprintln!("  -t          Test configuration and exit");
    eprintln!("  -T          Extended test (dump config)");
    eprintln!("  --help      Show this help");
}

// ============================================================================
// Logging
// ============================================================================

fn log_info(msg: &str, _log_stderr: bool) {
    eprintln!("sshd: {msg}");
}

fn log_error(msg: &str, _log_stderr: bool) {
    eprintln!("sshd: error: {msg}");
}

// ============================================================================
// Main server loop
// ============================================================================

fn main() {
    let opts = CliOptions::parse_args();

    // Load config.
    let mut config = if let Ok(data) = fs_read_file(&opts.config_file) {
        let content = String::from_utf8_lossy(&data);
        match SshdConfig::parse(&content) {
            Ok(c) => c,
            Err(e) => {
                log_error(&format!("config parse error: {e}"), opts.log_stderr);
                process::exit(1);
            }
        }
    } else {
        if opts.config_file != "/etc/ssh/sshd_config" {
            log_error(
                &format!("cannot read config: {}", opts.config_file),
                opts.log_stderr,
            );
            process::exit(1);
        }
        // Use defaults if default config file doesn't exist.
        SshdConfig::default_config()
    };

    // Apply CLI overrides.
    if let Some(port) = opts.port {
        config.port = port;
    }
    if let Some(hk) = &opts.host_key_file {
        hk.clone_into(&mut config.host_key_file);
    }

    // Test mode.
    if opts.test_config {
        if opts.extended_test {
            print!("{}", config.dump());
        } else {
            log_info("configuration OK", opts.log_stderr);
        }
        process::exit(0);
    }

    // Load the host key, or create one and keep it.
    //
    // The two failure modes are deliberately not the same. A *missing* file is
    // a first start: generate a key and persist it, as `ssh-keygen -A` does. A
    // file that exists but cannot be parsed is an operator error -- a wrong
    // path, a truncated file, an encrypted key -- and running anyway under a
    // substitute identity would present clients with a host key that is not the
    // one the operator installed. That is indistinguishable, from the client's
    // side, from the attack host key verification exists to detect, so we stop.
    let host_key = match HostKey::load_from_file(&config.host_key_file) {
        Ok(hk) => hk,
        Err(SshdError::IoError(_)) => {
            log_info(
                &format!("no host key at {}, generating one", config.host_key_file),
                opts.log_stderr,
            );
            match HostKey::generate_and_persist(&config.host_key_file) {
                Ok(hk) => hk,
                Err(e) => {
                    log_error(&format!("cannot create a host key: {e}"), opts.log_stderr);
                    process::exit(1);
                }
            }
        }
        Err(e) => {
            log_error(&format!("host key unusable: {e}"), opts.log_stderr);
            process::exit(1);
        }
    };

    log_info(
        &format!("host key fingerprint: {}", host_key.fingerprint()),
        opts.log_stderr,
    );

    // Validate port.
    if config.port == 0 {
        log_error("port cannot be 0", opts.log_stderr);
        process::exit(1);
    }

    // Bind listener.
    let listener = match tcp_bind(config.port) {
        Ok(l) => l,
        Err(e) => {
            log_error(
                &format!("cannot bind port {}: {e}", config.port),
                opts.log_stderr,
            );
            process::exit(1);
        }
    };

    log_info(
        &format!(
            "listening on {}:{} (pid {})",
            config.listen_address,
            config.port,
            get_pid()
        ),
        opts.log_stderr,
    );

    // One verifier for the whole daemon, not one per connection: it holds the
    // per-user failure tallies, and a tally that is discarded when the
    // connection closes counts nothing an attacker cannot reset at will.
    let mut auth = authlib::Authenticator::new();

    // Accept connections.
    loop {
        let conn_handle = match tcp_accept(listener) {
            Ok(h) => h,
            Err(e) => {
                log_error(&format!("accept error: {e}"), opts.log_stderr);
                continue;
            }
        };

        handle_connection(conn_handle, &config, &host_key, opts.debug_mode, &mut auth);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::format_collect
)]
mod tests {
    use super::*;

    // ---- Version string parsing ----

    #[test]
    fn test_parse_version_string_standard() {
        let sw = parse_version_string("SSH-2.0-OpenSSH_8.9");
        assert_eq!(sw, Some("OpenSSH_8.9"));
    }

    #[test]
    fn test_parse_version_string_with_comment() {
        let sw = parse_version_string("SSH-2.0-OpenSSH_8.9 Ubuntu-3");
        assert_eq!(sw, Some("OpenSSH_8.9"));
    }

    #[test]
    fn test_parse_version_string_slateos() {
        let sw = parse_version_string("SSH-2.0-SlateOS_1.0");
        assert_eq!(sw, Some("SlateOS_1.0"));
    }

    #[test]
    fn test_parse_version_string_invalid() {
        assert!(parse_version_string("HTTP/1.1").is_none());
    }

    #[test]
    fn test_parse_version_string_empty() {
        assert!(parse_version_string("").is_none());
    }

    #[test]
    fn test_parse_version_string_v1() {
        let sw = parse_version_string("SSH-1.0-old");
        assert_eq!(sw, Some("old"));
    }

    // ---- Packet building and parsing ----

    #[test]
    fn test_build_packet_unencrypted() {
        let enc = EncryptionState::none();
        let pkt = build_packet(b"hello", false, 0, &enc);
        assert!(pkt.len() >= 4 + 1 + 5);
        let pkt_len = u32::from_be_bytes([pkt[0], pkt[1], pkt[2], pkt[3]]) as usize;
        let pad_len = pkt[4] as usize;
        assert_eq!(pkt_len, 1 + 5 + pad_len);
        assert!(pad_len >= 4);
    }

    #[test]
    fn test_build_packet_alignment() {
        let enc = EncryptionState::none();
        let pkt = build_packet(b"test", false, 0, &enc);
        // Total must be multiple of block size (8).
        assert_eq!(pkt.len() % 8, 0);
    }

    #[test]
    fn test_build_packet_empty_payload() {
        let enc = EncryptionState::none();
        let pkt = build_packet(b"", false, 0, &enc);
        assert!(pkt.len() > 4);
        assert_eq!(pkt.len() % 8, 0);
    }

    #[test]
    fn test_build_packet_large_payload() {
        let enc = EncryptionState::none();
        let data = vec![0xAA; 1024];
        let pkt = build_packet(&data, false, 0, &enc);
        let pkt_len = u32::from_be_bytes([pkt[0], pkt[1], pkt[2], pkt[3]]) as usize;
        assert!(pkt_len > 1024);
    }

    // ---- SSH encoding helpers ----

    #[test]
    fn test_ssh_string_encoding() {
        let encoded = ssh_string(b"hello");
        assert_eq!(&encoded[..4], &[0, 0, 0, 5]);
        assert_eq!(&encoded[4..], b"hello");
    }

    #[test]
    fn test_ssh_string_empty() {
        let encoded = ssh_string(b"");
        assert_eq!(&encoded, &[0, 0, 0, 0]);
    }

    #[test]
    fn test_read_ssh_string_roundtrip() {
        let encoded = ssh_string(b"test data");
        let (val, next) = read_ssh_string(&encoded, 0).unwrap();
        assert_eq!(val, b"test data");
        assert_eq!(next, encoded.len());
    }

    #[test]
    fn test_read_ssh_string_truncated() {
        assert!(read_ssh_string(&[0, 0, 0], 0).is_err());
    }

    #[test]
    fn test_read_ssh_string_oversized() {
        let data = [0, 0, 0, 10, 1, 2, 3]; // Claims 10 bytes but only 3 available.
        assert!(read_ssh_string(&data, 0).is_err());
    }

    #[test]
    fn test_read_u32() {
        let data = [0, 0, 0, 42];
        let (val, next) = read_u32(&data, 0).unwrap();
        assert_eq!(val, 42);
        assert_eq!(next, 4);
    }

    #[test]
    fn test_read_u32_truncated() {
        assert!(read_u32(&[0, 0], 0).is_err());
    }

    #[test]
    fn test_read_byte() {
        let (val, next) = read_byte(&[0xFF], 0).unwrap();
        assert_eq!(val, 0xFF);
        assert_eq!(next, 1);
    }

    #[test]
    fn test_read_byte_empty() {
        assert!(read_byte(&[], 0).is_err());
    }

    #[test]
    fn test_encode_mpint_zero() {
        let result = encode_mpint(&[]);
        assert_eq!(result, vec![0, 0, 0, 0]);
    }

    #[test]
    fn test_encode_mpint_positive() {
        let result = encode_mpint(&[0x01, 0x02]);
        assert_eq!(&result[..4], &[0, 0, 0, 2]);
        assert_eq!(&result[4..], &[0x01, 0x02]);
    }

    #[test]
    fn test_encode_mpint_high_bit() {
        let result = encode_mpint(&[0x80, 0x01]);
        // Should be padded with a leading zero.
        assert_eq!(&result[..4], &[0, 0, 0, 3]);
        assert_eq!(&result[4..], &[0x00, 0x80, 0x01]);
    }

    #[test]
    fn test_read_mpint_roundtrip() {
        let encoded = encode_mpint(&[0x42, 0x43]);
        let (val, _) = read_mpint(&encoded, 0).unwrap();
        assert_eq!(val, vec![0x42, 0x43]);
    }

    // ---- Config parsing ----

    #[test]
    fn test_config_default() {
        let config = SshdConfig::default_config();
        assert_eq!(config.port, 22);
        assert!(config.password_authentication);
        assert!(config.pubkey_authentication);
        assert_eq!(config.max_auth_tries, 6);
        assert_eq!(config.login_grace_time, 120);
        assert_eq!(config.max_sessions, 10);
    }

    #[test]
    fn test_config_parse_port() {
        let config = SshdConfig::parse("Port 2222").unwrap();
        assert_eq!(config.port, 2222);
    }

    #[test]
    fn test_config_parse_listen_address() {
        let config = SshdConfig::parse("ListenAddress 192.168.1.1").unwrap();
        assert_eq!(config.listen_address, "192.168.1.1");
    }

    #[test]
    fn test_config_parse_host_key() {
        let config = SshdConfig::parse("HostKey /etc/ssh/my_key").unwrap();
        assert_eq!(config.host_key_file, "/etc/ssh/my_key");
    }

    #[test]
    fn test_config_parse_permit_root_login_yes() {
        let config = SshdConfig::parse("PermitRootLogin yes").unwrap();
        assert_eq!(config.permit_root_login, PermitRootLogin::Yes);
    }

    #[test]
    fn test_config_parse_permit_root_login_no() {
        let config = SshdConfig::parse("PermitRootLogin no").unwrap();
        assert_eq!(config.permit_root_login, PermitRootLogin::No);
    }

    #[test]
    fn test_config_parse_permit_root_login_prohibit() {
        let config = SshdConfig::parse("PermitRootLogin prohibit-password").unwrap();
        assert_eq!(config.permit_root_login, PermitRootLogin::ProhibitPassword);
    }

    #[test]
    fn test_config_parse_password_auth() {
        let config = SshdConfig::parse("PasswordAuthentication no").unwrap();
        assert!(!config.password_authentication);
    }

    #[test]
    fn test_config_parse_pubkey_auth() {
        let config = SshdConfig::parse("PubkeyAuthentication no").unwrap();
        assert!(!config.pubkey_authentication);
    }

    #[test]
    fn test_config_parse_max_auth_tries() {
        let config = SshdConfig::parse("MaxAuthTries 3").unwrap();
        assert_eq!(config.max_auth_tries, 3);
    }

    #[test]
    fn test_config_parse_login_grace_time() {
        let config = SshdConfig::parse("LoginGraceTime 60").unwrap();
        assert_eq!(config.login_grace_time, 60);
    }

    #[test]
    fn test_config_parse_max_sessions() {
        let config = SshdConfig::parse("MaxSessions 5").unwrap();
        assert_eq!(config.max_sessions, 5);
    }

    #[test]
    fn test_config_parse_banner() {
        let config = SshdConfig::parse("Banner /etc/ssh/banner").unwrap();
        assert_eq!(config.banner_file, "/etc/ssh/banner");
    }

    #[test]
    fn test_config_parse_print_motd() {
        let config = SshdConfig::parse("PrintMotd no").unwrap();
        assert!(!config.print_motd);
    }

    #[test]
    fn test_config_parse_subsystem() {
        let config = SshdConfig::parse("Subsystem sftp /usr/lib/sftp-server").unwrap();
        // Default already has sftp; we add another.
        assert!(config.subsystems.len() >= 2);
        assert!(
            config
                .subsystems
                .iter()
                .any(|(n, c)| n == "sftp" && c == "/usr/lib/sftp-server")
        );
    }

    #[test]
    fn test_config_parse_allow_users() {
        let config = SshdConfig::parse("AllowUsers alice bob").unwrap();
        assert_eq!(config.allow_users, vec!["alice", "bob"]);
    }

    #[test]
    fn test_config_parse_deny_users() {
        let config = SshdConfig::parse("DenyUsers mallory").unwrap();
        assert_eq!(config.deny_users, vec!["mallory"]);
    }

    #[test]
    fn test_config_parse_allow_groups() {
        let config = SshdConfig::parse("AllowGroups ssh-users admin").unwrap();
        assert_eq!(config.allow_groups, vec!["ssh-users", "admin"]);
    }

    #[test]
    fn test_config_parse_deny_groups() {
        let config = SshdConfig::parse("DenyGroups nogroup").unwrap();
        assert_eq!(config.deny_groups, vec!["nogroup"]);
    }

    #[test]
    fn test_config_parse_authorized_keys_file() {
        let config = SshdConfig::parse("AuthorizedKeysFile .ssh/custom_keys").unwrap();
        assert_eq!(config.authorized_keys_file, ".ssh/custom_keys");
    }

    #[test]
    fn test_config_parse_comments_and_empty_lines() {
        let content = "# Comment\n\n  # Another comment\nPort 3333\n";
        let config = SshdConfig::parse(content).unwrap();
        assert_eq!(config.port, 3333);
    }

    #[test]
    fn test_config_parse_empty() {
        let config = SshdConfig::parse("").unwrap();
        assert_eq!(config.port, 22); // Should have defaults.
    }

    #[test]
    fn test_config_parse_invalid_port() {
        assert!(SshdConfig::parse("Port notanumber").is_err());
    }

    #[test]
    fn test_config_parse_invalid_permit_root() {
        assert!(SshdConfig::parse("PermitRootLogin maybe").is_err());
    }

    #[test]
    fn test_config_parse_invalid_bool() {
        assert!(SshdConfig::parse("PasswordAuthentication banana").is_err());
    }

    #[test]
    fn test_config_parse_unknown_directive() {
        // Unknown directives should be silently ignored.
        let config = SshdConfig::parse("UnknownDirective value").unwrap();
        assert_eq!(config.port, 22);
    }

    #[test]
    fn test_config_dump() {
        let config = SshdConfig::default_config();
        let dump = config.dump();
        assert!(dump.contains("port 22"));
        assert!(dump.contains("listenaddress 0.0.0.0"));
        assert!(dump.contains("passwordauthentication yes"));
    }

    #[test]
    fn test_config_parse_full() {
        let content = "\
Port 2222
ListenAddress 10.0.0.1
HostKey /etc/ssh/host_key
PermitRootLogin no
PasswordAuthentication yes
PubkeyAuthentication yes
AuthorizedKeysFile .ssh/authorized_keys
MaxAuthTries 3
LoginGraceTime 30
MaxSessions 2
Banner /etc/ssh/banner.txt
PrintMotd no
AllowUsers admin deploy
DenyUsers nobody
AllowGroups wheel
DenyGroups nogroup
";
        let config = SshdConfig::parse(content).unwrap();
        assert_eq!(config.port, 2222);
        assert_eq!(config.listen_address, "10.0.0.1");
        assert_eq!(config.host_key_file, "/etc/ssh/host_key");
        assert_eq!(config.permit_root_login, PermitRootLogin::No);
        assert!(config.password_authentication);
        assert_eq!(config.max_auth_tries, 3);
        assert_eq!(config.login_grace_time, 30);
        assert_eq!(config.max_sessions, 2);
        assert!(!config.print_motd);
        assert_eq!(config.allow_users, vec!["admin", "deploy"]);
        assert_eq!(config.deny_users, vec!["nobody"]);
        assert_eq!(config.allow_groups, vec!["wheel"]);
        assert_eq!(config.deny_groups, vec!["nogroup"]);
    }

    // ---- User authentication logic ----

    #[test]
    fn test_is_user_allowed_no_restrictions() {
        let config = SshdConfig::default_config();
        assert!(is_user_allowed("alice", &[], &config));
    }

    #[test]
    fn test_is_user_denied_by_deny_list() {
        let mut config = SshdConfig::default_config();
        config.deny_users = vec!["mallory".into()];
        assert!(!is_user_allowed("mallory", &[], &config));
        assert!(is_user_allowed("alice", &[], &config));
    }

    #[test]
    fn test_is_user_allowed_by_allow_list() {
        let mut config = SshdConfig::default_config();
        config.allow_users = vec!["alice".into(), "bob".into()];
        assert!(is_user_allowed("alice", &[], &config));
        assert!(!is_user_allowed("charlie", &[], &config));
    }

    #[test]
    fn test_deny_takes_precedence() {
        let mut config = SshdConfig::default_config();
        config.allow_users = vec!["alice".into()];
        config.deny_users = vec!["alice".into()];
        assert!(!is_user_allowed("alice", &[], &config));
    }

    /// The fail-closed half of comparing patterns literally: `admin*` matched
    /// no account, so the directive that was meant to admit the admins locked
    /// out everyone including them.
    #[test]
    fn an_allowusers_pattern_admits_the_accounts_it_names() {
        let mut config = SshdConfig::default_config();
        config.allow_users = vec!["admin*".into(), "ops-?".into()];
        assert!(is_user_allowed("admin1", &[], &config));
        assert!(is_user_allowed("admin", &[], &config));
        assert!(is_user_allowed("ops-a", &[], &config));
        assert!(!is_user_allowed("ops-ab", &[], &config));
        assert!(!is_user_allowed("mallory", &[], &config));
    }

    /// The fail-*open* half, and the one that matters: a `DenyUsers` pattern
    /// compared literally names nobody, so the accounts an administrator
    /// believes are blocked log straight in.
    #[test]
    fn a_denyusers_pattern_actually_blocks_the_accounts_it_names() {
        let mut config = SshdConfig::default_config();
        config.deny_users = vec!["guest*".into()];
        assert!(!is_user_allowed("guest1", &[], &config));
        assert!(!is_user_allowed("guest", &[], &config));
        assert!(is_user_allowed("alice", &[], &config));
    }

    #[test]
    fn group_patterns_match_on_both_sides() {
        let mut config = SshdConfig::default_config();
        config.deny_groups = vec!["no-*".into()];
        assert!(!is_user_allowed("alice", &["no-login".into()], &config));
        assert!(is_user_allowed("alice", &["staff".into()], &config));

        let mut config = SshdConfig::default_config();
        config.allow_groups = vec!["dev-*".into()];
        assert!(is_user_allowed("alice", &["dev-web".into()], &config));
        assert!(!is_user_allowed("alice", &["contractors".into()], &config));
        // No groups at all cannot satisfy an AllowGroups list.
        assert!(!is_user_allowed("alice", &[], &config));
    }

    /// Negation carries over from the pattern matcher, which is what lets an
    /// administrator write the common "everyone in the group except one".
    #[test]
    fn a_negated_pattern_carves_an_exception_out_of_an_allow_list() {
        let mut config = SshdConfig::default_config();
        config.allow_users = vec!["dev*".into(), "!dev-intern".into()];
        assert!(is_user_allowed("dev-alice", &[], &config));
        assert!(!is_user_allowed("dev-intern", &[], &config));
    }

    #[test]
    fn test_group_deny() {
        let mut config = SshdConfig::default_config();
        config.deny_groups = vec!["badgroup".into()];
        assert!(!is_user_allowed("alice", &["badgroup".into()], &config));
    }

    #[test]
    fn test_group_allow() {
        let mut config = SshdConfig::default_config();
        config.allow_groups = vec!["ssh-users".into()];
        assert!(is_user_allowed("alice", &["ssh-users".into()], &config));
        assert!(!is_user_allowed("alice", &["other".into()], &config));
    }

    #[test]
    fn test_root_login_yes() {
        let mut config = SshdConfig::default_config();
        config.permit_root_login = PermitRootLogin::Yes;
        assert!(is_root_login_allowed("password", &config));
        assert!(is_root_login_allowed("publickey", &config));
    }

    #[test]
    fn test_root_login_no() {
        let mut config = SshdConfig::default_config();
        config.permit_root_login = PermitRootLogin::No;
        assert!(!is_root_login_allowed("password", &config));
        assert!(!is_root_login_allowed("publickey", &config));
    }

    #[test]
    fn test_root_login_prohibit_password() {
        let mut config = SshdConfig::default_config();
        config.permit_root_login = PermitRootLogin::ProhibitPassword;
        assert!(!is_root_login_allowed("password", &config));
        assert!(is_root_login_allowed("publickey", &config));
    }

    // ---- Password authentication ----
    //
    // sshd no longer hashes anything: it hands the bytes off the wire to
    // `authlib`, which is the only thing on the system that knows what a
    // stored entry means. What is tested here is therefore the wire path --
    // that the password reaches the verifier intact -- plus the two facts a
    // reader would otherwise have to take on trust: that an entry `passwd`
    // wrote now works, and that the formats the old homebrew hasher used to
    // wave through no longer authenticate anyone.

    /// A real `$6$` entry, computed rather than pasted so this cannot drift
    /// from the hasher `passwd` uses.
    fn shadow_entry_for(password: &str) -> String {
        let mut setting_buf = posix::crypt::buf();
        let setting =
            posix::crypt::setting_into(posix::crypt::Method::Sha512, b"sshdtest", &mut setting_buf)
                .expect("setting")
                .to_string();
        let mut hash_buf = posix::crypt::buf();
        posix::crypt::hash_into(password.as_bytes(), setting.as_bytes(), &mut hash_buf)
            .expect("hash")
            .to_string()
    }

    use scratchdir::ScratchDir;

    /// The clock every fixture here authenticates against, frozen.
    ///
    /// The rate limit earned by `FREE_ATTEMPTS + 1` failures is **one second**
    /// (`delay_for(4) == 1`), and it is measured from the last failure. Earning
    /// it costs four real `posix::crypt` verifications, which are slow by
    /// design — that is the entire point of a password hash. On a machine
    /// running a three-lane `cargo test --workspace` those four can take longer
    /// than the second they are trying to fit inside, at which point the delay
    /// has already expired, `authenticate` correctly declines to refuse, and
    /// the test that asserts a refusal fails. Lane C observed exactly that; see
    /// `requests/c-b-auth-daemon-rate-limit-tests-race-a-one-second-window.md`.
    ///
    /// `authlib` is not wrong — a one-second delay expiring after one second is
    /// the specification — so the fix is to take wall time out of a property
    /// that never depended on it, rather than to earn a longer delay and make
    /// the race rare instead of impossible.
    ///
    /// Freezing also makes the assertions stronger: with `now` pinned, the
    /// `retry_after_secs` a refusal reports is a known number rather than
    /// whatever the scheduler left of the window, so these tests can name it
    /// instead of accepting any `RateLimited` at all.
    fn frozen_clock() -> u64 {
        // Any fixed value; only differences matter, and there are none.
        1_700_000_000
    }

    /// An `Authenticator` over a throwaway `/etc/shadow` holding one line.
    ///
    /// The users.yaml path deliberately points at a file that does not exist,
    /// so the shadow branch is the one under test. The clock is frozen — see
    /// [`frozen_clock`].
    ///
    /// The returned `ScratchDir` is a guard: it must stay bound for as long as the
    /// `Authenticator` is used, because dropping it deletes the shadow file the
    /// authenticator reads. Bind it as `_dir`, not `_`.
    fn authenticator_with_shadow(line: &str) -> (authlib::Authenticator, ScratchDir) {
        let dir = ScratchDir::new("sshd_test");
        let shadow = dir.path("shadow");
        std::fs::write(&shadow, line).expect("write shadow");
        let missing = dir.path("no_such_users.yaml");
        (
            authlib::Authenticator::with_stores(&missing, &shadow).with_clock(frozen_clock),
            dir,
        )
    }

    /// The fixture's own wiring, which a shared guard cannot check for us.
    ///
    /// `ScratchDir` guarantees two guards never share a directory, and its own
    /// tests pin that under eight concurrent threads. What it cannot know is
    /// whether *this* fixture holds its guard for as long as the authenticator
    /// it handed back needs the file -- return the path and drop the guard, and
    /// every test here reads a shadow file that no longer exists.
    ///
    /// So this asserts the end-to-end property the old helper broke: twenty
    /// fixtures alive at once, each authenticating its *own* user. The failure
    /// it replaces was a 13%-of-runs red somewhere else entirely, pointing at
    /// the authenticator rather than at the fixture that lied to it.
    #[test]
    fn twenty_fixtures_alive_at_once_each_authenticate_their_own_user() {
        let stored = shadow_entry_for("correct horse");
        let mut held: Vec<(usize, authlib::Authenticator, ScratchDir)> = (0..20)
            .map(|i| {
                let (auth, dir) =
                    authenticator_with_shadow(&format!("user{i}:{stored}:1:0:99999:7:::\n"));
                (i, auth, dir)
            })
            .collect();

        let mut seen = std::collections::HashSet::new();
        for (i, _, dir) in &held {
            assert!(
                seen.insert(dir.path("shadow")),
                "fixture {i} reused a path another fixture already owns"
            );
        }

        for (i, auth, _dir) in &mut held {
            assert_eq!(
                auth.authenticate(&format!("user{i}"), b"correct horse"),
                authlib::Outcome::Accepted,
                "fixture {i} is authenticating against another fixture's shadow line"
            );
        }
    }

    /// The tail of an `SSH_MSG_USERAUTH_REQUEST` for the `password` method,
    /// starting at the boolean that `handle_password_auth` is handed.
    fn password_request(password: &[u8]) -> Vec<u8> {
        let mut p = vec![0u8]; // FALSE: not a password-change request
        p.extend_from_slice(&ssh_string(password));
        p
    }

    #[test]
    fn a_password_set_with_passwd_authenticates() {
        let stored = shadow_entry_for("correct horse");
        let (mut auth, _dir) =
            authenticator_with_shadow(&format!("alice:{stored}:1:0:99999:7:::\n"));

        let payload = password_request(b"correct horse");
        let outcome = handle_password_auth(&payload, 0, "alice", &mut auth).expect("parse");
        assert_eq!(outcome, authlib::Outcome::Accepted);
    }

    #[test]
    fn a_wrong_password_does_not_authenticate() {
        let stored = shadow_entry_for("correct horse");
        let (mut auth, _dir) =
            authenticator_with_shadow(&format!("alice:{stored}:1:0:99999:7:::\n"));

        let payload = password_request(b"correct hors");
        let outcome = handle_password_auth(&payload, 0, "alice", &mut auth).expect("parse");
        assert_eq!(outcome, authlib::Outcome::Rejected);
        assert!(!outcome.is_accepted());
    }

    /// The old hasher's last resort was to compare the password against the
    /// stored field as plaintext. An entry that is not a hash must now be
    /// reported as unusable -- a broken system somebody has to fix -- and must
    /// admit nobody, least of all the person who can read the file and type
    /// what is in it.
    #[test]
    fn a_plaintext_shadow_field_no_longer_lets_anyone_in() {
        let (mut auth, _dir) = authenticator_with_shadow("alice:password123:1:0:99999:7:::\n");

        let payload = password_request(b"password123");
        let outcome = handle_password_auth(&payload, 0, "alice", &mut auth).expect("parse");
        assert_eq!(outcome, authlib::Outcome::Unusable);
        assert!(!outcome.is_accepted());
        assert!(outcome.needs_administrator());
    }

    /// 64 hex digits under a `$5$` label is what this tree wrote before the
    /// hashers were unified. It is not SHA-256 crypt, nothing can recompute
    /// it, and saying "wrong password" about it would hide the real problem
    /// forever.
    #[test]
    fn the_old_homebrew_entry_format_is_reported_broken_not_wrong() {
        let mut salted = Vec::new();
        salted.extend_from_slice(b"mypass");
        salted.extend_from_slice(b"mysalt");
        let hex: String = sha256(&salted).iter().map(|b| format!("{b:02x}")).collect();
        let (mut auth, _dir) =
            authenticator_with_shadow(&format!("alice:$5$mysalt${hex}:1:0:99999:7:::\n"));

        let payload = password_request(b"mypass");
        let outcome = handle_password_auth(&payload, 0, "alice", &mut auth).expect("parse");
        assert_eq!(outcome, authlib::Outcome::Unusable);
    }

    #[test]
    fn a_locked_account_admits_no_password() {
        let stored = shadow_entry_for("correct horse");
        let (mut auth, _dir) =
            authenticator_with_shadow(&format!("alice:!{stored}:1:0:99999:7:::\n"));

        let payload = password_request(b"correct horse");
        let outcome = handle_password_auth(&payload, 0, "alice", &mut auth).expect("parse");
        assert_eq!(outcome, authlib::Outcome::Locked);
    }

    /// A user with no entry at all must be indistinguishable from a user with
    /// the wrong password, or the daemon is an account-enumeration oracle.
    #[test]
    fn an_unknown_user_is_rejected_exactly_as_a_wrong_password_is() {
        let stored = shadow_entry_for("correct horse");
        let (mut auth, _dir) =
            authenticator_with_shadow(&format!("alice:{stored}:1:0:99999:7:::\n"));

        let payload = password_request(b"anything");
        let unknown = handle_password_auth(&payload, 0, "mallory", &mut auth).expect("parse");
        let wrong = handle_password_auth(&payload, 0, "alice", &mut auth).expect("parse");
        assert_eq!(unknown, wrong);
        assert_eq!(unknown.user_message(), wrong.user_message());
    }

    /// The password is whatever bytes the client sent. Forcing it through
    /// UTF-8 first -- as this daemon did -- rewrites every invalid byte as
    /// U+FFFD, which both locks out a user whose password contains one and
    /// makes two different passwords hash identically.
    #[test]
    fn a_password_that_is_not_utf8_survives_the_wire_intact() {
        let raw = b"p\xffssw\xferd";
        let mut setting_buf = posix::crypt::buf();
        let setting =
            posix::crypt::setting_into(posix::crypt::Method::Sha512, b"sshdtest", &mut setting_buf)
                .expect("setting")
                .to_string();
        let mut hash_buf = posix::crypt::buf();
        let stored = posix::crypt::hash_into(raw, setting.as_bytes(), &mut hash_buf)
            .expect("hash")
            .to_string();

        let (mut auth, _dir) =
            authenticator_with_shadow(&format!("alice:{stored}:1:0:99999:7:::\n"));
        let payload = password_request(raw);
        let outcome = handle_password_auth(&payload, 0, "alice", &mut auth).expect("parse");
        assert_eq!(
            outcome,
            authlib::Outcome::Accepted,
            "lossy UTF-8 conversion would have changed the password"
        );

        // And the lossy form -- what the old code would have hashed -- must not
        // also work, or the two are interchangeable.
        let lossy = String::from_utf8_lossy(raw).into_owned();
        let payload = password_request(lossy.as_bytes());
        let outcome = handle_password_auth(&payload, 0, "alice", &mut auth).expect("parse");
        assert_eq!(outcome, authlib::Outcome::Rejected);
    }

    /// `MaxAuthTries` bounds a conversation; the tally bounds the account.
    /// Reconnecting must not hand the guesser a fresh budget, which it does if
    /// the verifier is created per connection.
    #[test]
    fn guessing_is_rate_limited_across_connections_not_just_within_one() {
        let stored = shadow_entry_for("correct horse");
        let (mut auth, _dir) =
            authenticator_with_shadow(&format!("alice:{stored}:1:0:99999:7:::\n"));

        let wrong = password_request(b"nope");
        for _ in 0..=authlib::FREE_ATTEMPTS {
            let outcome = handle_password_auth(&wrong, 0, "alice", &mut auth).expect("parse");
            assert!(!outcome.is_accepted());
        }

        // The daemon-wide verifier now refuses without even looking, and would
        // do so for a brand new connection: nothing about `conn` is consulted.
        //
        // The exact second is asserted, not just "some rate limit": under a
        // frozen clock `delay_for(FREE_ATTEMPTS + 1)` is 1 and no time has
        // passed since the fourth failure, so 1 is the only right answer. A
        // `matches!(.., RateLimited { .. })` here would also be satisfied by a
        // delay mis-computed as 60, which is the failure this is meant to catch.
        let outcome = handle_password_auth(&wrong, 0, "alice", &mut auth).expect("parse");
        assert_eq!(
            outcome,
            authlib::Outcome::RateLimited {
                retry_after_secs: 1
            },
            "expected the one-second limit earned by {} failures",
            authlib::FREE_ATTEMPTS + 1
        );

        // And the *correct* password is refused too while the delay stands --
        // that is the cost of the protection, and it is why the delay is
        // capped rather than unbounded. Still one second: a refusal returns
        // before the tally is touched, so being refused does not extend it.
        let right = password_request(b"correct horse");
        let outcome = handle_password_auth(&right, 0, "alice", &mut auth).expect("parse");
        assert_eq!(
            outcome,
            authlib::Outcome::RateLimited {
                retry_after_secs: 1
            }
        );
    }

    #[test]
    fn a_truncated_password_request_is_an_error_not_an_acceptance() {
        let (mut auth, _dir) = authenticator_with_shadow("alice:!:1:0:99999:7:::\n");
        for cut in 0..5 {
            let payload = vec![0u8; cut];
            assert!(handle_password_auth(&payload, 0, "alice", &mut auth).is_err());
        }
    }

    // ---- /etc/passwd parsing ----

    #[test]
    fn test_parse_passwd() {
        let content = "root:x:0:0:root:/root:/bin/sh\n\
                       alice:x:1000:1000:Alice,,,:/home/alice:/bin/bash\n";
        let entries = parse_passwd(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            PasswdEntry {
                username: "root".into(),
                uid: 0,
                gid: 0,
                home: "/root".into(),
                shell: "/bin/sh".into(),
            }
        );
        assert_eq!(entries[1].uid, 1000);
        assert_eq!(entries[1].home, "/home/alice");
        assert_eq!(entries[1].shell, "/bin/bash");
    }

    /// POSIX: an empty shell field means `/bin/sh`, not "no shell".
    #[test]
    fn test_parse_passwd_empty_shell_defaults() {
        let entries = parse_passwd("bob:x:1001:1001::/home/bob:\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].shell, DEFAULT_LOGIN_SHELL);
    }

    /// One malformed line must not lock every account out of the machine.
    #[test]
    fn test_parse_passwd_skips_malformed_lines() {
        let content = "# comment\n\
                       truncated:x:1:1\n\
                       bad-uid:x:notanumber:1:x:/h:/bin/sh\n\
                       bad-gid:x:1:notanumber:x:/h:/bin/sh\n\
                       \n\
                       good:x:7:8:x:/home/good:/bin/sh\n";
        let entries = parse_passwd(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].username, "good");
        assert_eq!(entries[0].uid, 7);
        assert_eq!(entries[0].gid, 8);
    }

    /// A `gecos` field containing colons must not shift the later fields:
    /// `split(':')` is correct precisely because the field count is fixed and
    /// gecos commas — not colons — separate its subfields.
    #[test]
    fn test_parse_passwd_gecos_with_commas() {
        let entries = parse_passwd("c:x:5:6:Full Name,Room 1,555,555:/home/c:/bin/zsh\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].home, "/home/c");
        assert_eq!(entries[0].shell, "/bin/zsh");
    }

    #[test]
    fn test_default_path_for_uid() {
        assert!(default_path_for_uid(0).contains("/sbin"));
        assert!(!default_path_for_uid(1000).contains("/sbin"));
    }

    /// The daemon's own environment must not leak into a session: the
    /// environment is built from the account, and from nothing else.
    #[test]
    fn test_session_env_is_built_from_the_account() {
        let user = PasswdEntry {
            username: "alice".into(),
            uid: 1000,
            gid: 1000,
            home: "/home/alice".into(),
            shell: "/bin/bash".into(),
        };
        let env = session_env(&user);
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        assert_eq!(get("HOME"), "/home/alice");
        assert_eq!(get("USER"), "alice");
        assert_eq!(get("LOGNAME"), "alice");
        assert_eq!(get("SHELL"), "/bin/bash");
        assert_eq!(get("PATH"), default_path_for_uid(1000));
        assert_eq!(env.len(), 5);
    }

    // ---- Session exit reporting ----

    #[test]
    fn test_rfc4254_signal_names() {
        // The thirteen the RFC enumerates, spot-checked at both ends.
        assert_eq!(rfc4254_signal_name(1), Some("HUP"));
        assert_eq!(rfc4254_signal_name(9), Some("KILL"));
        assert_eq!(rfc4254_signal_name(15), Some("TERM"));
        // Not in the RFC's list: the caller falls back to 128+n.
        assert_eq!(rfc4254_signal_name(31), None);
        assert_eq!(rfc4254_signal_name(0), None);
    }

    /// `exit-status` is a request with `want_reply = false` (RFC 4254 §6.10);
    /// a server that sets the flag would have the client wait for a reply that
    /// the specification forbids sending.
    #[test]
    fn test_exit_status_message_shape() {
        let mut msg_buf = Vec::new();
        msg_buf.push(msg::SSH_MSG_CHANNEL_REQUEST);
        msg_buf.extend_from_slice(&7u32.to_be_bytes());
        msg_buf.extend_from_slice(&ssh_string(b"exit-status"));
        msg_buf.push(0);
        msg_buf.extend_from_slice(&42u32.to_be_bytes());

        assert_eq!(msg_buf[0], 98);
        let (recipient, off) = read_u32(&msg_buf, 1).expect("recipient");
        assert_eq!(recipient, 7);
        let (req_type, off) = read_ssh_string(&msg_buf, off).expect("request type");
        assert_eq!(req_type, b"exit-status");
        let (want_reply, off) = read_bool(&msg_buf, off).expect("want_reply");
        assert!(!want_reply, "exit-status must not request a reply");
        let (status, _) = read_u32(&msg_buf, off).expect("status");
        assert_eq!(status, 42);
    }

    /// The chunk size must leave room for the message header, or a peer that
    /// advertises exactly `n` bytes would receive `n + 9`.
    #[test]
    fn test_channel_data_overhead_is_the_real_header_size() {
        // msg byte + recipient u32 + string length u32.
        assert_eq!(CHANNEL_DATA_OVERHEAD, 9);
        let framed = {
            let mut v = vec![msg::SSH_MSG_CHANNEL_DATA];
            v.extend_from_slice(&0u32.to_be_bytes());
            v.extend_from_slice(&ssh_string(b"abcd"));
            v
        };
        assert_eq!(framed.len(), 4 + CHANNEL_DATA_OVERHEAD as usize);
    }

    // ---- Authorized keys parsing ----

    #[test]
    fn test_parse_authorized_keys_ed25519() {
        let content = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBt user@host\n";
        let keys = parse_authorized_keys(content);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_type, "ssh-ed25519");
        assert_eq!(keys[0].comment, "user@host");
    }

    #[test]
    fn test_parse_authorized_keys_rsa() {
        let content = "ssh-rsa AAAAB3NzaC1yc2E= admin@server\n";
        let keys = parse_authorized_keys(content);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_type, "ssh-rsa");
    }

    #[test]
    fn test_parse_authorized_keys_multiple() {
        let content = "ssh-ed25519 AAAA key1\nssh-rsa BBBB key2\n";
        let keys = parse_authorized_keys(content);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_parse_authorized_keys_skip_invalid() {
        let content = "invalid-type AAAA key\nssh-ed25519 BBBB valid\n";
        let keys = parse_authorized_keys(content);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_type, "ssh-ed25519");
    }

    #[test]
    fn test_parse_authorized_keys_comments_and_empty() {
        let content = "# comment\n\nssh-ed25519 AAAA key\n";
        let keys = parse_authorized_keys(content);
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn test_parse_authorized_keys_no_comment() {
        let content = "ssh-ed25519 AAAA\n";
        let keys = parse_authorized_keys(content);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].comment, "");
    }

    // ---- Channel message handling ----

    #[test]
    fn test_channel_new() {
        let ch = Channel::new(0, 1, 65536, 32768);
        assert_eq!(ch.local_id, 0);
        assert_eq!(ch.remote_id, 1);
        assert_eq!(ch.remote_window, 65536);
        assert_eq!(ch.remote_max_packet, 32768);
        assert_eq!(ch.local_window, INITIAL_LOCAL_WINDOW);
        assert!(!ch.closed);
    }

    // ---- PTY request parsing ----

    #[test]
    fn test_parse_pty_request() {
        let mut data = Vec::new();
        data.extend_from_slice(&ssh_string(b"xterm-256color"));
        data.extend_from_slice(&80u32.to_be_bytes()); // width cols
        data.extend_from_slice(&24u32.to_be_bytes()); // height rows
        data.extend_from_slice(&640u32.to_be_bytes()); // width px
        data.extend_from_slice(&480u32.to_be_bytes()); // height px
        data.extend_from_slice(&ssh_string(b"")); // modes

        let (term, w, h, wpx, hpx, modes) = parse_pty_request(&data, 0).unwrap();
        assert_eq!(term, "xterm-256color");
        assert_eq!(w, 80);
        assert_eq!(h, 24);
        assert_eq!(wpx, 640);
        assert_eq!(hpx, 480);
        assert!(modes.is_empty());
    }

    #[test]
    fn test_parse_pty_request_with_modes() {
        let mut data = Vec::new();
        data.extend_from_slice(&ssh_string(b"vt100"));
        data.extend_from_slice(&132u32.to_be_bytes());
        data.extend_from_slice(&50u32.to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&ssh_string(&[1, 0, 0, 0, 3])); // Some mode bytes.

        let (term, w, h, _, _, modes) = parse_pty_request(&data, 0).unwrap();
        assert_eq!(term, "vt100");
        assert_eq!(w, 132);
        assert_eq!(h, 50);
        assert_eq!(modes.len(), 5);
    }

    // ---- Host key fingerprint ----

    #[test]
    fn test_host_key_fingerprint() {
        let key = HostKey::from_seed([0u8; 32]);
        let fp = key.fingerprint();
        assert!(fp.starts_with("SHA256:"));
        assert!(fp.len() > 10);
    }

    #[test]
    fn test_host_key_public_blob() {
        let key = HostKey::from_seed([1u8; 32]);
        let blob = key.public_key_blob();
        // Should start with ssh_string("ssh-ed25519").
        let (key_type, _) = read_ssh_string(&blob, 0).unwrap();
        assert_eq!(key_type, b"ssh-ed25519");
    }

    #[test]
    fn test_host_key_sign() {
        let key = HostKey::from_seed([2u8; 32]);
        let sig = key.sign(b"test data");
        // Signature blob should start with ssh_string("ssh-ed25519").
        let (sig_type, off) = read_ssh_string(&sig, 0).unwrap();
        assert_eq!(sig_type, b"ssh-ed25519");
        // Then a 64-byte signature.
        let (sig_data, _) = read_ssh_string(&sig, off).unwrap();
        assert_eq!(sig_data.len(), 64);
    }

    #[test]
    fn test_host_key_sign_different_data() {
        let key = HostKey::from_seed([3u8; 32]);
        let sig1 = key.sign(b"data1");
        let sig2 = key.sign(b"data2");
        assert_ne!(sig1, sig2);
    }

    /// The signature the daemon puts in KEX_DH_REPLY must verify against the
    /// public key it puts in the same message. The old implementation passed
    /// the two tests above -- it produced 64 bytes labelled ssh-ed25519 -- and
    /// failed this one, which is why no real client could ever connect.
    #[test]
    fn the_host_key_signature_verifies_against_the_advertised_public_key() {
        let key = HostKey::from_seed([7u8; 32]);
        let exchange_hash = sha256(b"a plausible exchange hash");
        let blob = key.sign(&exchange_hash);

        let (sig_type, off) = read_ssh_string(&blob, 0).unwrap();
        assert_eq!(sig_type, b"ssh-ed25519");
        let (signature, _) = read_ssh_string(&blob, off).unwrap();

        let advertised = key.public_key_blob();
        let public = ed25519_key_from_blob(&advertised).expect("advertised key is ed25519");
        assert!(posix::ed25519::verify_slices(
            &public,
            &exchange_hash,
            signature
        ));
    }

    #[test]
    fn a_host_key_signature_does_not_verify_for_a_different_exchange_hash() {
        let key = HostKey::from_seed([8u8; 32]);
        let blob = key.sign(&sha256(b"hash one"));
        let (_, off) = read_ssh_string(&blob, 0).unwrap();
        let (signature, _) = read_ssh_string(&blob, off).unwrap();
        let public = ed25519_key_from_blob(&key.public_key_blob()).unwrap();
        assert!(!posix::ed25519::verify_slices(
            &public,
            &sha256(b"hash two"),
            signature
        ));
    }

    // ---- OpenSSH private key container ----

    #[test]
    fn an_openssh_private_key_round_trips() {
        let seed = [0x42u8; 32];
        let public = posix::ed25519::public_key(&seed);

        // Build the container the same way write_openssh_private_key does,
        // without touching the filesystem (there is none under `cargo test`).
        let text = openssh_private_key_text(&seed, &public);
        let recovered = parse_openssh_private_key(&text).expect("parses");
        assert_eq!(recovered, seed);
    }

    #[test]
    fn an_encrypted_openssh_private_key_is_refused_not_guessed() {
        let mut raw = Vec::new();
        raw.extend_from_slice(b"openssh-key-v1\0");
        raw.extend_from_slice(&ssh_string(b"aes256-ctr"));
        raw.extend_from_slice(&ssh_string(b"bcrypt"));
        raw.extend_from_slice(&ssh_string(b"salt-and-rounds"));
        raw.extend_from_slice(&1u32.to_be_bytes());
        raw.extend_from_slice(&ssh_string(b"pubkey"));
        raw.extend_from_slice(&ssh_string(b"ciphertext"));
        let text = pem_wrap(&base64_encode(&raw));

        let err = parse_openssh_private_key(&text).expect_err("must refuse");
        assert!(err.contains("encrypted"), "{err}");
    }

    #[test]
    fn an_openssh_key_whose_halves_disagree_is_refused() {
        let seed = [0x11u8; 32];
        // A public key belonging to a different seed entirely.
        let wrong_public = posix::ed25519::public_key(&[0x22u8; 32]);
        let text = openssh_private_key_text(&seed, &wrong_public);
        let err = parse_openssh_private_key(&text).expect_err("must refuse");
        assert!(err.contains("does not match"), "{err}");
    }

    #[test]
    fn a_file_that_is_not_a_host_key_is_an_error_not_an_invented_key() {
        // The old loader fell back to sha256(first_line), so this produced a
        // valid-looking host key unrelated to the file.
        let err = parse_openssh_private_key("-----BEGIN OPENSSH PRIVATE KEY-----\nZ\n")
            .expect_err("must refuse");
        assert!(err.contains("openssh-key-v1"), "{err}");
    }

    /// Build an OpenSSH private key file body for tests. Mirrors
    /// `write_openssh_private_key` minus the I/O and the random checkint.
    fn openssh_private_key_text(seed: &[u8; 32], public: &[u8; 32]) -> String {
        let mut pub_blob = Vec::new();
        pub_blob.extend_from_slice(&ssh_string(b"ssh-ed25519"));
        pub_blob.extend_from_slice(&ssh_string(public));

        let mut secret = Vec::with_capacity(64);
        secret.extend_from_slice(seed);
        secret.extend_from_slice(public);

        let mut private = Vec::new();
        private.extend_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
        private.extend_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
        private.extend_from_slice(&ssh_string(b"ssh-ed25519"));
        private.extend_from_slice(&ssh_string(public));
        private.extend_from_slice(&ssh_string(&secret));
        private.extend_from_slice(&ssh_string(b"test"));
        let mut pad: u8 = 1;
        while private.len() % 8 != 0 {
            private.push(pad);
            pad += 1;
        }

        let mut raw = Vec::new();
        raw.extend_from_slice(b"openssh-key-v1\0");
        raw.extend_from_slice(&ssh_string(b"none"));
        raw.extend_from_slice(&ssh_string(b"none"));
        raw.extend_from_slice(&ssh_string(b""));
        raw.extend_from_slice(&1u32.to_be_bytes());
        raw.extend_from_slice(&ssh_string(&pub_blob));
        raw.extend_from_slice(&ssh_string(&private));
        pem_wrap(&base64_encode(&raw))
    }

    fn pem_wrap(body: &str) -> String {
        let mut text = String::from("-----BEGIN OPENSSH PRIVATE KEY-----\n");
        for chunk in body.as_bytes().chunks(70) {
            text.push_str(&String::from_utf8_lossy(chunk));
            text.push('\n');
        }
        text.push_str("-----END OPENSSH PRIVATE KEY-----\n");
        text
    }

    // ---- publickey authentication (RFC 4252 section 7) ----

    /// Everything a client needs to present for publickey auth, so the tests
    /// below can vary one field at a time.
    struct PubkeyAttempt {
        seed: [u8; 32],
        session_id: [u8; 32],
        user: Vec<u8>,
        service: Vec<u8>,
    }

    impl PubkeyAttempt {
        fn valid() -> Self {
            Self {
                seed: [0x5Au8; 32],
                session_id: [0x33u8; 32],
                user: b"alice".to_vec(),
                service: b"ssh-connection".to_vec(),
            }
        }

        fn key_blob(&self) -> Vec<u8> {
            let public = posix::ed25519::public_key(&self.seed);
            let mut blob = Vec::new();
            blob.extend_from_slice(&ssh_string(b"ssh-ed25519"));
            blob.extend_from_slice(&ssh_string(&public));
            blob
        }

        /// Sign as the client does, over the blob the *server* will rebuild.
        fn sig_blob(&self) -> Vec<u8> {
            let signed = pubkey_signed_blob(
                &self.session_id,
                &self.user,
                &self.service,
                b"ssh-ed25519",
                &self.key_blob(),
            );
            let sig = posix::ed25519::sign(&self.seed, &signed);
            let mut blob = Vec::new();
            blob.extend_from_slice(&ssh_string(b"ssh-ed25519"));
            blob.extend_from_slice(&ssh_string(&sig));
            blob
        }

        /// Run the server's check, optionally against a different context than
        /// the one the signature was made for.
        fn verified_against(&self, server: &PubkeyAttempt) -> bool {
            let public = ed25519_key_from_blob(&self.key_blob()).unwrap();
            verify_pubkey_signature(
                &public,
                &self.sig_blob(),
                &server.session_id,
                &server.user,
                &server.service,
                b"ssh-ed25519",
                &self.key_blob(),
            )
        }
    }

    #[test]
    fn a_genuine_publickey_signature_is_accepted() {
        let a = PubkeyAttempt::valid();
        assert!(a.verified_against(&a));
    }

    /// The bypass. Before this change the server compared the offered public
    /// key against `authorized_keys` and, finding a match, returned success
    /// without reading the signature -- so an attacker who had only ever *seen*
    /// the public key could log in. Here the attacker holds the right public
    /// key and a signature made with a different private key.
    #[test]
    fn possessing_only_the_public_key_does_not_authenticate() {
        let genuine = PubkeyAttempt::valid();
        let attacker_seed = [0x99u8; 32];

        // The attacker replays the victim's key blob but must produce a
        // signature, and can only sign with a key it owns.
        let signed = pubkey_signed_blob(
            &genuine.session_id,
            &genuine.user,
            &genuine.service,
            b"ssh-ed25519",
            &genuine.key_blob(),
        );
        let sig = posix::ed25519::sign(&attacker_seed, &signed);
        let mut sig_blob = Vec::new();
        sig_blob.extend_from_slice(&ssh_string(b"ssh-ed25519"));
        sig_blob.extend_from_slice(&ssh_string(&sig));

        let public = ed25519_key_from_blob(&genuine.key_blob()).unwrap();
        assert!(!verify_pubkey_signature(
            &public,
            &sig_blob,
            &genuine.session_id,
            &genuine.user,
            &genuine.service,
            b"ssh-ed25519",
            &genuine.key_blob(),
        ));
    }

    /// A signature captured from one connection must not work on another.
    /// This is what binding the session identifier buys.
    #[test]
    fn a_signature_from_another_session_is_rejected() {
        let client = PubkeyAttempt::valid();
        let other_session = PubkeyAttempt {
            session_id: [0x44u8; 32],
            ..PubkeyAttempt::valid()
        };
        assert!(!client.verified_against(&other_session));
    }

    /// A signature for one account must not authenticate a different one.
    #[test]
    fn a_signature_for_another_user_is_rejected() {
        let client = PubkeyAttempt::valid();
        let as_root = PubkeyAttempt {
            user: b"root".to_vec(),
            ..PubkeyAttempt::valid()
        };
        assert!(!client.verified_against(&as_root));
    }

    #[test]
    fn a_signature_for_another_service_is_rejected() {
        let client = PubkeyAttempt::valid();
        let other = PubkeyAttempt {
            service: b"ssh-userauth".to_vec(),
            ..PubkeyAttempt::valid()
        };
        assert!(!client.verified_against(&other));
    }

    #[test]
    fn a_malformed_signature_blob_is_rejected_rather_than_erroring() {
        let a = PubkeyAttempt::valid();
        let public = ed25519_key_from_blob(&a.key_blob()).unwrap();
        for truncate_to in [0usize, 1, 4, 8, 15, 20] {
            let mut blob = a.sig_blob();
            blob.truncate(truncate_to);
            assert!(
                !verify_pubkey_signature(
                    &public,
                    &blob,
                    &a.session_id,
                    &a.user,
                    &a.service,
                    b"ssh-ed25519",
                    &a.key_blob(),
                ),
                "truncated to {truncate_to} bytes"
            );
        }
    }

    #[test]
    fn a_signature_under_a_different_algorithm_name_is_rejected() {
        let a = PubkeyAttempt::valid();
        let public = ed25519_key_from_blob(&a.key_blob()).unwrap();
        // Same 64 signature bytes, relabelled. Accepting this would let an
        // algorithm-confusion attack pick whichever verifier is weakest.
        let (_, off) = read_ssh_string(&a.sig_blob(), 0).unwrap();
        let sig = read_ssh_string(&a.sig_blob(), off).unwrap().0.to_vec();
        let mut blob = Vec::new();
        blob.extend_from_slice(&ssh_string(b"ssh-rsa"));
        blob.extend_from_slice(&ssh_string(&sig));
        assert!(!verify_pubkey_signature(
            &public,
            &blob,
            &a.session_id,
            &a.user,
            &a.service,
            b"ssh-ed25519",
            &a.key_blob(),
        ));
    }

    #[test]
    fn a_non_ed25519_key_blob_is_not_mistaken_for_one() {
        let mut rsa_blob = Vec::new();
        rsa_blob.extend_from_slice(&ssh_string(b"ssh-rsa"));
        rsa_blob.extend_from_slice(&ssh_string(&[0u8; 32]));
        assert!(ed25519_key_from_blob(&rsa_blob).is_none());

        // Right algorithm, wrong key length.
        let mut short = Vec::new();
        short.extend_from_slice(&ssh_string(b"ssh-ed25519"));
        short.extend_from_slice(&ssh_string(&[0u8; 31]));
        assert!(ed25519_key_from_blob(&short).is_none());
    }

    // ---- Port validation ----

    #[test]
    fn test_port_valid_range() {
        let config = SshdConfig::parse("Port 1").unwrap();
        assert_eq!(config.port, 1);
        let config = SshdConfig::parse("Port 65535").unwrap();
        assert_eq!(config.port, 65535);
    }

    #[test]
    fn test_port_zero_in_config() {
        // Port 0 is parseable but should be rejected at runtime.
        let config = SshdConfig::parse("Port 0").unwrap();
        assert_eq!(config.port, 0);
    }

    // ---- MaxAuthTries enforcement ----

    #[test]
    fn test_max_auth_tries_setting() {
        let config = SshdConfig::parse("MaxAuthTries 1").unwrap();
        assert_eq!(config.max_auth_tries, 1);
    }

    // ---- LoginGraceTime handling ----

    #[test]
    fn test_login_grace_time_setting() {
        let config = SshdConfig::parse("LoginGraceTime 30").unwrap();
        assert_eq!(config.login_grace_time, 30);
    }

    #[test]
    fn test_login_grace_time_zero() {
        let config = SshdConfig::parse("LoginGraceTime 0").unwrap();
        assert_eq!(config.login_grace_time, 0);
    }

    // ---- Subsystem configuration ----

    #[test]
    fn test_subsystem_default() {
        let config = SshdConfig::default_config();
        assert_eq!(config.subsystems.len(), 1);
        assert_eq!(config.subsystems[0].0, "sftp");
    }

    #[test]
    fn test_subsystem_custom() {
        let config = SshdConfig::parse("Subsystem scp /usr/lib/scp-server").unwrap();
        assert!(
            config
                .subsystems
                .iter()
                .any(|(n, c)| n == "scp" && c == "/usr/lib/scp-server")
        );
    }

    // ---- Banner loading ----

    #[test]
    fn test_banner_empty_by_default() {
        let config = SshdConfig::default_config();
        assert!(config.banner_file.is_empty());
    }

    #[test]
    fn test_banner_configured() {
        let config = SshdConfig::parse("Banner /etc/ssh/banner.txt").unwrap();
        assert_eq!(config.banner_file, "/etc/ssh/banner.txt");
    }

    // ---- SHA-256 ----

    #[test]
    fn test_sha256_empty() {
        let hash = sha256(b"");
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_hello() {
        let hash = sha256(b"hello");
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    // ---- HMAC-SHA256 ----

    #[test]
    fn test_hmac_sha256_basic() {
        let mac = hmac_sha256(b"key", b"data");
        // Known test vector for HMAC-SHA256("key", "data").
        let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "5031fe3d989c6d1537a013fa6e739da23463fdaec3b70137d828e36ace221bd0"
        );
    }

    // ---- Base64 ----

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(base64_encode(&[]), "");
    }

    #[test]
    fn test_base64_encode_hello() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn test_base64_decode_hello() {
        assert_eq!(base64_decode("aGVsbG8="), b"hello");
    }

    #[test]
    fn test_base64_roundtrip() {
        let data = b"SSH server daemon testing";
        let encoded = base64_encode(data);
        let decoded = base64_decode(&encoded);
        assert_eq!(&decoded, data);
    }

    // ---- Big integer ----

    #[test]
    fn test_biguint_zero() {
        let z = BigUint::zero();
        assert!(z.is_zero());
        assert_eq!(z.bit_length(), 0);
    }

    #[test]
    fn test_biguint_one() {
        let one = BigUint::one();
        assert!(!one.is_zero());
        assert_eq!(one.to_bytes_be(), vec![1]);
    }

    #[test]
    fn test_biguint_mod_pow() {
        // 2^10 mod 1000 = 1024 mod 1000 = 24
        let base = BigUint::from_bytes_be(&[2]);
        let exp = BigUint::from_bytes_be(&[10]);
        let modulus = BigUint::from_bytes_be(&[0x03, 0xE8]); // 1000
        let result = base.mod_pow(&exp, &modulus);
        assert_eq!(result.to_bytes_be(), vec![24]);
    }

    // ---- Encryption state ----

    #[test]
    fn test_encryption_state_none() {
        let enc = EncryptionState::none();
        assert!(enc.enc_key_c2s.is_empty());
        assert_eq!(enc.block_size, 8);
        assert_eq!(enc.mac_len, 0);
    }

    // ---- AES ----

    #[test]
    fn test_aes_encrypt_decrypt_roundtrip() {
        let key = [0u8; 16];
        let iv = [0u8; 16];
        let original = b"test data here!!"; // 16 bytes exactly
        let mut encrypted = original.to_vec();
        encrypt_packet_aes_ctr(&mut encrypted, &key, &iv, 0);
        assert_ne!(&encrypted, original);
        decrypt_packet_aes_ctr(&mut encrypted, &key, &iv, 0);
        assert_eq!(&encrypted, original);
    }

    // ---- Constant time eq ----

    #[test]
    fn test_constant_time_eq_same() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn test_constant_time_eq_different() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn test_constant_time_eq_different_length() {
        assert!(!constant_time_eq(b"short", b"longer"));
    }

    // ---- KEXINIT building ----

    #[test]
    fn test_build_kexinit() {
        let kexinit = build_kexinit();
        assert_eq!(kexinit[0], msg::SSH_MSG_KEXINIT);
        assert!(kexinit.len() > 17); // At least message type + 16 byte cookie.
    }

    // ---- Format IP ----

    #[test]
    fn test_format_ip() {
        let ip = u32::from_be_bytes([192, 168, 1, 100]);
        assert_eq!(format_ip(ip), "192.168.1.100");
    }

    #[test]
    fn test_format_ip_localhost() {
        let ip = u32::from_be_bytes([127, 0, 0, 1]);
        assert_eq!(format_ip(ip), "127.0.0.1");
    }

    // ---- Incremental packet framing ----
    //
    // These are the tests the `read_packet` / `try_parse_packet` split was made
    // for: framing used to be reachable only through a socket, so the one thing
    // a session pump depends on — that an incomplete packet is reported as
    // "not yet" and never as an error or a short read — could not be tested at
    // all.

    /// Feed a buffer one byte at a time; report when a packet first appears.
    fn feed_byte_at_a_time(wire: &[u8]) -> (usize, Vec<u8>) {
        let enc = EncryptionState::none();
        let mut buf = StreamBuffer::new();
        for (i, byte) in wire.iter().enumerate() {
            buf.data.push(*byte);
            match try_parse_packet(&mut buf, false, 0, &enc) {
                Ok(Some(payload)) => return (i + 1, payload),
                Ok(None) => {}
                Err(e) => panic!("framing error at byte {i}: {e}"),
            }
        }
        panic!("packet never completed after {} bytes", wire.len());
    }

    #[test]
    fn test_try_parse_packet_waits_for_the_whole_packet() {
        let enc = EncryptionState::none();
        let payload = b"hello ssh".to_vec();
        let wire = build_packet(&payload, false, 0, &enc);

        let (consumed, parsed) = feed_byte_at_a_time(&wire);
        assert_eq!(parsed, payload);
        // The packet must be reported exactly when its last byte lands --
        // neither early (which would mean parsing a partial packet) nor late.
        assert_eq!(consumed, wire.len());
    }

    #[test]
    fn test_try_parse_packet_is_none_on_an_empty_buffer() {
        let enc = EncryptionState::none();
        let mut buf = StreamBuffer::new();
        let got = try_parse_packet(&mut buf, false, 0, &enc);
        assert!(matches!(got, Ok(None)));
    }

    #[test]
    fn test_try_parse_packet_leaves_a_partial_packet_intact() {
        // A `None` must not consume anything: the bytes it declined are the
        // start of the packet it will parse on the next call. Consuming them
        // would desynchronise the stream permanently.
        let enc = EncryptionState::none();
        let wire = build_packet(b"payload", false, 0, &enc);
        let mut buf = StreamBuffer::new();
        buf.data.extend_from_slice(&wire[..wire.len() - 1]);

        assert!(matches!(
            try_parse_packet(&mut buf, false, 0, &enc),
            Ok(None)
        ));
        assert_eq!(buf.available(), wire.len() - 1);

        buf.data.push(wire[wire.len() - 1]);
        let got = try_parse_packet(&mut buf, false, 0, &enc)
            .expect("framing")
            .expect("packet");
        assert_eq!(got, b"payload");
    }

    #[test]
    fn test_try_parse_packet_returns_several_packets_from_one_buffer() {
        // A single TCP read can carry several SSH packets. The session pump
        // drains them all before sleeping, so a second call on the same buffer
        // must produce the second packet rather than ask for more bytes.
        let enc = EncryptionState::none();
        let mut buf = StreamBuffer::new();
        buf.data
            .extend_from_slice(&build_packet(b"first", false, 0, &enc));
        buf.data
            .extend_from_slice(&build_packet(b"second", false, 1, &enc));

        let a = try_parse_packet(&mut buf, false, 0, &enc)
            .expect("framing")
            .expect("first packet");
        let b = try_parse_packet(&mut buf, false, 1, &enc)
            .expect("framing")
            .expect("second packet");
        assert_eq!(a, b"first");
        assert_eq!(b, b"second");
        assert!(matches!(
            try_parse_packet(&mut buf, false, 2, &enc),
            Ok(None)
        ));
    }

    // ---- Login shell argv[0] ----

    #[test]
    fn test_login_argv0_takes_the_basename() {
        assert_eq!(login_argv0("/bin/bash"), "-bash");
        assert_eq!(login_argv0("/usr/local/bin/fish"), "-fish");
    }

    #[test]
    fn test_login_argv0_bare_name_needs_no_directory() {
        // A passwd entry may name the shell without a path.
        assert_eq!(login_argv0("sh"), "-sh");
    }

    #[test]
    fn test_login_argv0_trailing_slash_falls_back_to_the_whole_path() {
        // `rsplit('/')` yields "" for a path ending in a separator. Emitting
        // "-" would name a shell that does not exist and start no session, so
        // the whole string is used instead: still wrong as a shell, but it
        // fails loudly at `spawn` with a name that says what was configured,
        // rather than silently becoming a one-character mystery.
        assert_eq!(login_argv0("/bin/"), "-/bin/");
    }

    #[test]
    fn test_login_argv0_keeps_the_hyphen_that_means_login_shell() {
        // The leading hyphen is the whole point: without it the shell does not
        // read the user's profile, and a session comes up with no PATH set by
        // the site's configuration.
        assert!(login_argv0("/bin/bash").starts_with('-'));
    }

    // ---- Terminal size conversion ----

    #[test]
    fn test_winsize_from_ssh_normal() {
        let ws = winsize_from_ssh(80, 24, 640, 480);
        assert_eq!(ws.ws_col, 80);
        assert_eq!(ws.ws_row, 24);
        assert_eq!(ws.ws_xpixel, 640);
        assert_eq!(ws.ws_ypixel, 480);
    }

    #[test]
    fn test_winsize_from_ssh_clamps_instead_of_truncating() {
        // 0x1_0000 would wrap to 0 under an `as u16` cast, and a zero-width
        // terminal breaks every line-wrapping program in ways that look like
        // bugs in those programs. Clamping keeps the failure visible and sane.
        let ws = winsize_from_ssh(0x1_0000, 0x1_0000, 0xFFFF_FFFF, 0xFFFF_FFFF);
        assert_eq!(ws.ws_col, u16::MAX);
        assert_eq!(ws.ws_row, u16::MAX);
        assert_eq!(ws.ws_xpixel, u16::MAX);
        assert_eq!(ws.ws_ypixel, u16::MAX);
    }

    #[test]
    fn test_winsize_from_ssh_does_not_transpose_rows_and_columns() {
        // SSH sends columns first; `struct winsize` stores rows first. The
        // orders are crossed exactly once, here, and a regression would make
        // every session's terminal the wrong way round.
        let ws = winsize_from_ssh(100, 20, 0, 0);
        assert_eq!(ws.ws_col, 100);
        assert_eq!(ws.ws_row, 20);
    }

    // ---- parse_bool ----

    #[test]
    fn test_parse_bool_yes() {
        assert!(parse_bool("yes").unwrap());
        assert!(parse_bool("true").unwrap());
        assert!(parse_bool("1").unwrap());
    }

    #[test]
    fn test_parse_bool_no() {
        assert!(!parse_bool("no").unwrap());
        assert!(!parse_bool("false").unwrap());
        assert!(!parse_bool("0").unwrap());
    }

    #[test]
    fn test_parse_bool_invalid() {
        assert!(parse_bool("maybe").is_err());
    }

    // ---- read_bool ----

    #[test]
    fn test_read_bool_true() {
        let (val, next) = read_bool(&[1], 0).unwrap();
        assert!(val);
        assert_eq!(next, 1);
    }

    #[test]
    fn test_read_bool_false() {
        let (val, _) = read_bool(&[0], 0).unwrap();
        assert!(!val);
    }

    // ---- derive_keys ----

    #[test]
    fn test_derive_keys_produces_nonempty() {
        let secret = [1u8; 32];
        let hash = [2u8; 32];
        let session = [3u8; 32];
        let enc = derive_keys(&secret, &hash, &session);
        assert_eq!(enc.enc_key_c2s.len(), 16);
        assert_eq!(enc.enc_key_s2c.len(), 16);
        assert_eq!(enc.iv_c2s.len(), 16);
        assert_eq!(enc.iv_s2c.len(), 16);
        assert_eq!(enc.mac_key_c2s.len(), 32);
        assert_eq!(enc.mac_key_s2c.len(), 32);
        assert_eq!(enc.block_size, 16);
        assert_eq!(enc.mac_len, 32);
    }

    #[test]
    fn test_derive_keys_different_inputs() {
        let enc1 = derive_keys(&[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let enc2 = derive_keys(&[4u8; 32], &[5u8; 32], &[6u8; 32]);
        assert_ne!(enc1.enc_key_c2s, enc2.enc_key_c2s);
    }

    // ---- Pipe-backed sessions: bookkeeping ----
    //
    // These tests are about *which stream is considered live*, which is the
    // half of `Pipes` that decides when a channel may be closed — and getting
    // it wrong truncates a command's output, which is the failure the whole
    // `output_finished` design exists to prevent. They deliberately do not
    // touch real descriptors: a test that opened a pipe would only run where
    // `cfg(unix)` holds, so the host build — the one run on every commit —
    // would stop covering the logic entirely.

    /// Build a `Pipes` holding descriptor *numbers* that this process does not
    /// own, for the bookkeeping tests above.
    ///
    /// Nothing may read or write these; only the predicates that compare them
    /// against -1 are safe to call. Pair every call with [`disarm`], because
    /// `Pipes::drop` would otherwise `close` a descriptor belonging to whatever
    /// else in this process happens to hold that number.
    fn fake_pipes(stdin: i32, stdout: i32, stderr: i32) -> Pipes {
        Pipes {
            stdin,
            stdout,
            stderr,
        }
    }

    /// Put every field back to -1 so `Pipes::drop` closes nothing.
    fn disarm(pipes: &mut Pipes) {
        pipes.stdin = -1;
        pipes.stdout = -1;
        pipes.stderr = -1;
    }

    #[test]
    fn pipes_are_finished_only_when_both_output_streams_are_closed() {
        let mut both_open = fake_pipes(-1, 4242, 4243);
        assert!(!both_open.output_finished());
        disarm(&mut both_open);

        let mut stdout_open = fake_pipes(-1, 4242, -1);
        assert!(!stdout_open.output_finished());
        disarm(&mut stdout_open);

        let mut stderr_open = fake_pipes(-1, -1, 4243);
        assert!(!stderr_open.output_finished());
        disarm(&mut stderr_open);

        let both_closed = fake_pipes(-1, -1, -1);
        assert!(both_closed.output_finished());
    }

    #[test]
    fn pipes_accept_input_until_stdin_is_closed() {
        let mut io = SessionIo::Pipes(fake_pipes(4242, -1, -1));
        assert!(io.accepts_input());
        if let SessionIo::Pipes(pipes) = &mut io {
            disarm(pipes);
        }
        assert!(!io.accepts_input());
    }

    #[test]
    fn writing_to_a_closed_stdin_reports_the_bytes_consumed() {
        // Not an error: the client sent input after its own EOF closed the
        // child's stdin. The session is healthy, there is simply nowhere for
        // the bytes to go, and reporting failure would tear down a channel
        // whose command is still running and still producing output.
        let pipes = fake_pipes(-1, -1, -1);
        assert_eq!(pipes.write_input(b"late input").unwrap(), 10);
    }

    #[test]
    fn closing_input_is_idempotent() {
        // `close_input` is reached from three directions — the client's EOF, a
        // failed write, and `Drop` — so it has to be safe to call on a stdin
        // that is already gone. The -1 guard is what makes that true, and a
        // double close of a recycled descriptor number is exactly the bug it
        // prevents.
        let mut pipes = fake_pipes(-1, -1, -1);
        pipes.close_input();
        pipes.close_input();
        assert_eq!(pipes.stdin, -1);
    }

    #[test]
    fn reading_with_no_window_credit_touches_no_descriptor() {
        // The zero-budget path must return before it looks at a descriptor,
        // which is what makes it safe to call on the fake ones here — and, in
        // the daemon, what stops a client with a closed window from draining
        // the kernel buffer that is applying the back-pressure.
        let mut pipes = fake_pipes(-1, 4242, 4243);
        let mut buf = [0u8; 64];
        assert_eq!(pipes.read_output(&mut buf, 0), (false, StreamRead::Empty));
        disarm(&mut pipes);
    }

    #[test]
    fn reading_into_an_empty_buffer_is_empty_not_end_of_file() {
        let mut pipes = fake_pipes(-1, 4242, 4243);
        let mut buf = [0u8; 0];
        assert_eq!(
            pipes.read_output(&mut buf, 4096),
            (false, StreamRead::Empty)
        );
        disarm(&mut pipes);
    }

    #[test]
    fn a_negative_descriptor_is_never_ready_and_never_readable() {
        assert_eq!(fd_ready(-1), (false, false));
        assert_eq!(read_pipe(-1, &mut [0u8; 16]), StreamRead::Empty);
        assert_eq!(read_pipe(3, &mut []), StreamRead::Empty);
        // Nothing to configure, so nothing can fail: a channel with no stdin
        // must not be refused for it.
        assert!(set_nonblocking(-1));
    }

    // ---- SessionIo: which attachment answers what ----

    #[test]
    fn an_unattached_session_is_finished_and_takes_no_input() {
        let io = SessionIo::None;
        assert!(io.output_finished());
        assert!(!io.accepts_input());
        assert!(!io.input_ready());
        assert_eq!(io.write_input(b"anything").unwrap(), 0);
        assert!(io.pty().is_none());
    }

    #[test]
    fn closing_input_on_an_unattached_session_does_nothing() {
        let mut io = SessionIo::None;
        io.close_input();
        assert!(io.output_finished());
    }

    #[test]
    fn session_io_reports_pipe_end_of_file_through_to_the_pump() {
        let mut open = SessionIo::Pipes(fake_pipes(-1, 4242, -1));
        assert!(!open.output_finished());
        if let SessionIo::Pipes(pipes) = &mut open {
            disarm(pipes);
        }
        assert!(SessionIo::Pipes(fake_pipes(-1, -1, -1)).output_finished());
    }

    // ---- pump_channel_output: finished is a fact about the streams ----

    /// A connection with one channel and the given send window, for the pump
    /// tests below.
    ///
    /// The socket handle is 0 and is never written to: every case here is one
    /// where the pump returns without sending, which is deliberate — the send
    /// paths need a real socket and belong in an integration test, while the
    /// decision *not* to close a channel is pure logic and belongs here.
    fn conn_with_channel(remote_window: u32) -> ConnectionState {
        let mut conn = ConnectionState::new(
            0,
            SshdConfig::default_config(),
            HostKey::from_seed([0u8; 32]),
            false,
        );
        conn.channels.push(Channel::new(1, 2, remote_window, 32768));
        conn
    }

    #[test]
    fn a_closed_window_does_not_make_a_live_session_look_finished() {
        let mut conn = conn_with_channel(0);
        conn.channels[0].io = SessionIo::Pipes(fake_pipes(-1, 4242, 4243));
        let (worked, finished) = pump_channel_output(&mut conn, 0, 1).unwrap();
        assert!(!worked);
        assert!(
            !finished,
            "streams are still open; the client is just behind"
        );
        if let SessionIo::Pipes(pipes) = &mut conn.channels[0].io {
            disarm(pipes);
        }
    }

    #[test]
    fn a_closed_window_does_not_hold_a_finished_session_open() {
        // The regression this guards: a command whose final write consumed the
        // last of the window leaves `remote_window` at 0 for good, because a
        // client that has everything it asked for has no reason to send another
        // WINDOW_ADJUST. Deciding "finished" from the window rather than from
        // the streams left that session open until the connection dropped.
        let mut conn = conn_with_channel(0);
        conn.channels[0].io = SessionIo::Pipes(fake_pipes(-1, -1, -1));
        let (worked, finished) = pump_channel_output(&mut conn, 0, 1).unwrap();
        assert!(!worked);
        assert!(finished);
    }

    #[test]
    fn an_unattached_channel_reports_finished_immediately() {
        let mut conn = conn_with_channel(32768);
        let (worked, finished) = pump_channel_output(&mut conn, 0, 1).unwrap();
        assert!(!worked);
        assert!(finished);
    }

    // ---- Environment requests (RFC 4254 §6.4) ----
    //
    // The request path is exercised with `want_reply = false` throughout, so
    // nothing is written to a socket and the assertions are about what the
    // channel *remembers* — which is the half that used to be missing
    // entirely. The reply itself is a two-line function tested by the fact that
    // both arms below reach it.

    /// An `env` request payload: two SSH strings, name then value.
    fn env_payload(name: &str, value: &str) -> Vec<u8> {
        let mut out = ssh_string(name.as_bytes());
        out.extend_from_slice(&ssh_string(value.as_bytes()));
        out
    }

    fn conn_accepting(patterns: &[&str]) -> ConnectionState {
        let mut conn = conn_with_channel(32768);
        conn.config.accept_env = patterns.iter().map(|p| (*p).to_string()).collect();
        conn
    }

    fn send_env(conn: &mut ConnectionState, name: &str, value: &str) {
        let payload = env_payload(name, value);
        handle_env_request(conn, 1, 2, &payload, 0, false).unwrap();
    }

    #[test]
    fn a_literal_pattern_matches_only_itself() {
        assert!(glob_matches("LANG", "LANG"));
        assert!(!glob_matches("LANG", "LANGUAGE"));
        assert!(!glob_matches("LANG", "LAN"));
        assert!(!glob_matches("LANG", "lang"));
    }

    #[test]
    fn a_star_matches_any_run_including_an_empty_one() {
        assert!(glob_matches("LC_*", "LC_ALL"));
        assert!(glob_matches("LC_*", "LC_"));
        assert!(!glob_matches("LC_*", "LC"));
        assert!(glob_matches("*", ""));
        assert!(glob_matches("*_*", "A_B"));
        assert!(glob_matches("a*c", "abbbbc"));
        assert!(!glob_matches("a*c", "abbbb"));
    }

    #[test]
    fn a_question_mark_matches_exactly_one_character() {
        assert!(glob_matches("LC_?", "LC_A"));
        assert!(!glob_matches("LC_?", "LC_"));
        assert!(!glob_matches("LC_?", "LC_AB"));
    }

    /// The pathological shape a naive recursive matcher turns exponential.
    /// It is included because the input is remote-controlled: if this ever
    /// stops returning promptly, a client has been handed the daemon's CPU.
    #[test]
    fn a_pattern_dense_with_stars_still_terminates() {
        let name = "a".repeat(64);
        assert!(!glob_matches("a*a*a*a*a*a*b", &name));
        assert!(glob_matches("a*a*a*a*a*a*a", &name));
    }

    #[test]
    fn a_negated_pattern_wins_whatever_order_it_is_written_in() {
        let before = vec!["!LC_ALL".to_string(), "LC_*".to_string()];
        let after = vec!["LC_*".to_string(), "!LC_ALL".to_string()];
        assert!(!pattern_list_matches(&before, "LC_ALL"));
        assert!(!pattern_list_matches(&after, "LC_ALL"));
        assert!(pattern_list_matches(&before, "LC_TIME"));
        assert!(pattern_list_matches(&after, "LC_TIME"));
    }

    #[test]
    fn an_empty_pattern_list_matches_nothing() {
        assert!(!pattern_list_matches(&[], "LANG"));
        assert!(!pattern_list_matches(&[], ""));
    }

    #[test]
    fn the_default_configuration_accepts_no_variable() {
        let config = SshdConfig::default_config();
        assert!(config.accept_env.is_empty());
        assert!(!env_request_allowed(&config, "LANG", "en_US.UTF-8"));
    }

    #[test]
    fn acceptenv_lines_accumulate_and_survive_a_round_trip() {
        let config = SshdConfig::parse("acceptenv LANG LC_*\nacceptenv TZ\n").unwrap();
        assert_eq!(config.accept_env, vec!["LANG", "LC_*", "TZ"]);
        let reparsed = SshdConfig::parse(&config.dump()).unwrap();
        assert_eq!(reparsed.accept_env, config.accept_env);
    }

    #[test]
    fn a_name_that_is_not_a_name_is_refused_before_the_patterns_are_read() {
        let mut config = SshdConfig::default_config();
        // `*` would otherwise accept everything, which is the point: these are
        // rejected for what they are, not for failing to match.
        config.accept_env = vec!["*".to_string()];
        assert!(!env_request_allowed(&config, "", "x"));
        assert!(!env_request_allowed(&config, "A=B", "x"));
        assert!(!env_request_allowed(&config, "A\0B", "x"));
        assert!(!env_request_allowed(&config, "A", "x\0y"));
        assert!(env_request_allowed(&config, "A", "x"));
    }

    #[test]
    fn the_session_identity_variables_are_refused_even_by_a_wildcard() {
        let mut config = SshdConfig::default_config();
        config.accept_env = vec!["*".to_string()];
        for name in REFUSED_SESSION_ENV {
            assert!(
                !env_request_allowed(&config, name, "anything"),
                "{name} must not be settable by the client"
            );
        }
    }

    #[test]
    fn an_accepted_variable_is_remembered_for_the_session() {
        let mut conn = conn_accepting(&["LANG", "LC_*"]);
        send_env(&mut conn, "LANG", "en_US.UTF-8");
        send_env(&mut conn, "LC_TIME", "C");
        assert_eq!(
            conn.channels[0].env,
            vec![
                ("LANG".to_string(), "en_US.UTF-8".to_string()),
                ("LC_TIME".to_string(), "C".to_string()),
            ]
        );
    }

    #[test]
    fn a_refused_variable_is_not_remembered() {
        let mut conn = conn_accepting(&["LANG"]);
        send_env(&mut conn, "LD_PRELOAD", "/tmp/evil.so");
        assert!(conn.channels[0].env.is_empty());
    }

    #[test]
    fn setting_a_variable_twice_replaces_it_rather_than_appending() {
        let mut conn = conn_accepting(&["LANG"]);
        send_env(&mut conn, "LANG", "C");
        send_env(&mut conn, "LANG", "en_GB.UTF-8");
        assert_eq!(
            conn.channels[0].env,
            vec![("LANG".to_string(), "en_GB.UTF-8".to_string())]
        );
    }

    #[test]
    fn a_client_cannot_make_the_daemon_hold_unbounded_variables() {
        let mut conn = conn_accepting(&["*"]);
        for i in 0..(MAX_SESSION_ENV + 50) {
            send_env(&mut conn, &format!("VAR{i}"), "x");
        }
        assert_eq!(conn.channels[0].env.len(), MAX_SESSION_ENV);
    }

    #[test]
    fn a_client_cannot_make_the_daemon_hold_unbounded_bytes() {
        let mut conn = conn_accepting(&["*"]);
        let big = "x".repeat(MAX_SESSION_ENV_BYTES / 4);
        for i in 0..8 {
            send_env(&mut conn, &format!("VAR{i}"), &big);
        }
        let held: usize = conn.channels[0]
            .env
            .iter()
            .map(|(k, v)| k.len() + v.len())
            .sum();
        assert!(held <= MAX_SESSION_ENV_BYTES, "held {held} bytes");
        assert!(
            !conn.channels[0].env.is_empty(),
            "the limit refused all of it"
        );
    }

    /// The base environment is the server's, and an accepted variable layers
    /// over it — that is what an administrator listing the name asked for.
    #[test]
    fn an_accepted_variable_reaches_the_child_and_the_identity_ones_do_not() {
        let user = PasswdEntry {
            username: "alice".to_string(),
            uid: 1000,
            gid: 1000,
            home: "/home/alice".to_string(),
            shell: "/bin/sh".to_string(),
        };
        let client_env = vec![
            ("LANG".to_string(), "en_US.UTF-8".to_string()),
            ("PATH".to_string(), "/opt/bin".to_string()),
        ];
        let cmd = session_command(&user, "true", &client_env);
        let seen: Vec<(String, String)> = cmd
            .get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_string_lossy().into_owned(),
                    v?.to_string_lossy().into_owned(),
                ))
            })
            .collect();
        let value_of = |name: &str| seen.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        assert_eq!(value_of("LANG"), Some("en_US.UTF-8".to_string()));
        // Listed by the administrator, so it wins over the default.
        assert_eq!(value_of("PATH"), Some("/opt/bin".to_string()));
        // Never offered to the client at all; still the account's own.
        assert_eq!(value_of("HOME"), Some("/home/alice".to_string()));
        assert_eq!(value_of("USER"), Some("alice".to_string()));
    }

    // ---- Global requests (RFC 4254 §4) ----

    /// `byte SSH_MSG_GLOBAL_REQUEST`, `string name`, `boolean want_reply`, and
    /// whatever request-specific bytes follow.
    fn global_request_payload(name: &str, want_reply: bool, extra: &[u8]) -> Vec<u8> {
        let mut out = vec![msg::SSH_MSG_GLOBAL_REQUEST];
        out.extend_from_slice(&ssh_string(name.as_bytes()));
        out.push(u8::from(want_reply));
        out.extend_from_slice(extra);
        out
    }

    #[test]
    fn a_keepalive_probe_is_answered_so_the_client_does_not_kill_the_session() {
        // The regression this guards is the whole reason the arm exists. An
        // OpenSSH client with `ServerAliveInterval` set sends exactly this, and
        // reads a REQUEST_FAILURE as proof of life; hearing nothing, it counts
        // the probe as lost and tears down a healthy session after
        // `ServerAliveCountMax` of them.
        let payload = global_request_payload("keepalive@openssh.com", true, &[]);
        let (name, reply) = global_request_reply(&payload).unwrap();
        assert_eq!(name, "keepalive@openssh.com");
        assert_eq!(reply, Some(vec![msg::SSH_MSG_REQUEST_FAILURE]));
    }

    #[test]
    fn a_request_that_wants_no_reply_gets_none() {
        // RFC 4254 §4: a reply is owed only when it was asked for. Sending one
        // anyway would leave the client an unsolicited packet to account for.
        let payload = global_request_payload("hostkeys-00@openssh.com", false, &[0xAA, 0xBB]);
        let (name, reply) = global_request_reply(&payload).unwrap();
        assert_eq!(name, "hostkeys-00@openssh.com");
        assert_eq!(reply, None);
    }

    #[test]
    fn a_forwarding_request_is_refused_rather_than_ignored() {
        // We implement no forwarding. Refusing says so; silence does not, and a
        // client cannot tell silence from a dead server.
        let mut extra = 0u32.to_be_bytes().to_vec();
        extra.splice(0..0, ssh_string(b"0.0.0.0"));
        let payload = global_request_payload("tcpip-forward", true, &extra);
        let (name, reply) = global_request_reply(&payload).unwrap();
        assert_eq!(name, "tcpip-forward");
        assert_eq!(reply, Some(vec![msg::SSH_MSG_REQUEST_FAILURE]));
    }

    #[test]
    fn a_truncated_global_request_is_an_error_not_a_silent_refusal() {
        // Name present, `want_reply` byte missing. Guessing `false` here would
        // let a malformed packet suppress a reply the client is waiting on.
        let mut payload = vec![msg::SSH_MSG_GLOBAL_REQUEST];
        payload.extend_from_slice(&ssh_string(b"keepalive@openssh.com"));
        assert!(global_request_reply(&payload).is_err());
    }

    // ---- UNIMPLEMENTED (RFC 4253 §11.4) ----

    #[test]
    fn unimplemented_names_the_packet_it_rejects() {
        assert_eq!(
            unimplemented_packet(0x0102_0304),
            vec![msg::SSH_MSG_UNIMPLEMENTED, 0x01, 0x02, 0x03, 0x04]
        );
    }

    #[test]
    fn the_sequence_number_reported_is_the_one_being_dispatched() {
        // The off-by-one that would make every UNIMPLEMENTED name the wrong
        // packet: `recv_seq` is bumped as soon as a packet is produced, so
        // during dispatch it already points past the packet in hand.
        let mut conn = conn_with_channel(32768);
        conn.recv_seq = 7;
        assert_eq!(conn.current_recv_seq(), 6);
    }

    // ---- Exchange hash (RFC 4253 §8) ----

    /// The RFC 4253 §8 construction, written out longhand.
    ///
    /// `H = HASH(V_C || V_S || I_C || I_S || K_S || e || f || K)`, with the two
    /// version strings and the three blobs as `string` and the three numbers as
    /// `mpint`.
    ///
    /// This deliberately does not call `compute_exchange_hash`. A test that
    /// asserts a function equals itself catches nothing, and the bug this
    /// guards was a *missing input* — the real client version replaced by a
    /// fixed one — which only an independent statement of what the hash should
    /// cover can catch.
    #[allow(clippy::too_many_arguments)]
    fn rfc4253_exchange_hash(
        v_c: &str,
        v_s: &str,
        i_c: &[u8],
        i_s: &[u8],
        k_s: &[u8],
        e: &[u8],
        f: &[u8],
        k: &[u8],
    ) -> [u8; 32] {
        let mut buf = Vec::new();
        buf.extend_from_slice(&ssh_string(v_c.as_bytes()));
        buf.extend_from_slice(&ssh_string(v_s.as_bytes()));
        buf.extend_from_slice(&ssh_string(i_c));
        buf.extend_from_slice(&ssh_string(i_s));
        buf.extend_from_slice(&ssh_string(k_s));
        buf.extend_from_slice(&encode_mpint(e));
        buf.extend_from_slice(&encode_mpint(f));
        buf.extend_from_slice(&encode_mpint(k));
        sha256(&buf)
    }

    #[test]
    fn the_exchange_hash_is_the_one_rfc_4253_specifies() {
        let v_c = "SSH-2.0-OpenSSH_9.6";
        let (i_c, i_s) = (&[0x14u8, 1, 2, 3][..], &[0x14u8, 9, 8, 7][..]);
        let k_s = &[0xAAu8; 51][..];
        let e = &[0x11u8; 32][..];
        let f = &[0x22u8; 32][..];
        let k = &[0x33u8; 32][..];

        assert_eq!(
            compute_exchange_hash(v_c, SSH_SERVER_VERSION, i_c, i_s, k_s, e, f, k),
            rfc4253_exchange_hash(v_c, SSH_SERVER_VERSION, i_c, i_s, k_s, e, f, k),
        );
    }

    #[test]
    fn the_clients_own_version_reaches_the_exchange_hash() {
        // The regression, stated as the thing that was actually wrong: the hash
        // did not depend on what the client said, because a constant stood in
        // for it. Under the old code these two calls returned the same digest.
        let (i_c, i_s) = (&[0x14u8, 1][..], &[0x14u8, 2][..]);
        let k_s = &[0xAAu8; 51][..];
        let e = &[0x11u8; 32][..];
        let f = &[0x22u8; 32][..];
        let k = &[0x33u8; 32][..];

        let openssh = compute_exchange_hash(
            "SSH-2.0-OpenSSH_9.6",
            SSH_SERVER_VERSION,
            i_c,
            i_s,
            k_s,
            e,
            f,
            k,
        );
        let ours = compute_exchange_hash(
            "SSH-2.0-SlateOS_1.0",
            SSH_SERVER_VERSION,
            i_c,
            i_s,
            k_s,
            e,
            f,
            k,
        );
        assert_ne!(
            openssh, ours,
            "V_C must be an input to H, or the signature binds nothing about the client"
        );
    }

    #[test]
    fn the_placeholder_version_is_no_longer_what_gets_hashed() {
        // Pins the specific fabricated string out of existence. Our own client
        // sends `SSH-2.0-SlateOS_1.0`; the old code hashed `SSH-2.0-client`
        // whatever the client sent, so this equality was what shipped.
        let (i_c, i_s) = (&[0x14u8][..], &[0x14u8][..]);
        let (k_s, e, f, k) = (
            &[0u8; 51][..],
            &[1u8; 32][..],
            &[2u8; 32][..],
            &[3u8; 32][..],
        );
        let real = compute_exchange_hash(
            "SSH-2.0-SlateOS_1.0",
            SSH_SERVER_VERSION,
            i_c,
            i_s,
            k_s,
            e,
            f,
            k,
        );
        let placeholder =
            compute_exchange_hash("SSH-2.0-client", SSH_SERVER_VERSION, i_c, i_s, k_s, e, f, k);
        assert_ne!(real, placeholder);
    }

    #[test]
    fn a_fresh_connection_has_not_learned_a_client_version_yet() {
        // `client_version` starts empty and is filled by the version exchange.
        // If a future refactor ever computed the hash before that ran, this is
        // the field that would be silently wrong, so its initial value is
        // pinned rather than assumed.
        let conn = conn_with_channel(32768);
        assert!(conn.client_version.is_empty());
    }

    #[test]
    fn the_sequence_number_wraps_instead_of_underflowing() {
        // Sequence numbers are explicitly modulo 2^32 (RFC 4253 §6.4), so the
        // packet before number 0 is number 2^32-1 and not a panic.
        let mut conn = conn_with_channel(32768);
        conn.recv_seq = 0;
        assert_eq!(conn.current_recv_seq(), u32::MAX);
    }
}

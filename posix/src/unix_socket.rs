//! Unix-domain sockets that can have a name: `socket(AF_UNIX, ...)`.
//!
//! The kernel's `ipc::unix_socket` (lane A's;
//! `requests/a-d-resource-type-33-is-unixsocket.md`), reached
//! through `SYS_UNIX_*` (1104-1118): stream, datagram and sequenced-packet
//! sockets, bound to a path or an abstract name, with `listen`, `accept` and
//! `connect`, sends and receives that carry the sender's address and
//! credentials, and the options `SO_PASSCRED`, `SO_RCVTIMEO` and
//! `SO_SNDTIMEO`. [`crate::socket`]'s entry points come here for a descriptor
//! of kind [`HandleKind::UnixSocket`]; so do `read`, `write`, `close` and
//! `poll` ([`crate::file`], [`crate::poll`]).
//!
//! # What the library does around the calls
//!
//! The kernel's half is described in `requests/a-d-resource-type-33-is-
//! unixsocket.md`; this is the C library's.
//!
//! - **Names.** `sun_path[0] == 0` is an abstract name: the bytes after that
//!   NUL, as many as `addrlen` says. Otherwise a path, ending at its first NUL
//!   or at `addrlen`. The kernel takes only absolute paths, so a relative one
//!   is resolved against the working directory first, as every native path
//!   call's is (design-decisions 648). The resolved path must fit in
//!   `sun_path`'s 108 bytes, or the call is `ENAMETOOLONG`; Linux resolves any
//!   length through the working directory, and reports the name as it was
//!   given, where this reports the resolved one.
//! - **Autobind.** `bind` with the family alone (`addrlen` 2) gives the socket
//!   a fresh abstract name of five hex digits, as Linux's `unix_autobind`
//!   does; the kernel takes no empty name.
//! - **Mode.** A path's node is made `0777 & ~umask`, as Linux makes it.
//! - **Blocking.** The kernel moves at most so much of a stream in one call;
//!   a blocking stream send repeats until all of it has gone, as Linux's
//!   `unix_stream_sendmsg` does, and `MSG_WAITALL` repeats a receive. A
//!   signal that interrupts a wait restarts it under `SA_RESTART` unless the
//!   socket has a timeout for the call, which is Linux's rule (signal(7)); the
//!   timeouts are the kernel's, and are remembered here too, in the socket's
//!   metadata, so that the rule can be read without asking.
//! - **Pieces.** `sendmsg` and `recvmsg` with several `iovec`s: a datagram's
//!   are gathered into one buffer and sent once, or received whole and handed
//!   out across them, since a datagram cannot be taken in parts; a stream's
//!   are sent and received one by one, in place, with no bound on the total
//!   -- only the first receive waits, unless `MSG_WAITALL`.
//! - **Descriptors** travel (`SCM_RIGHTS`) through `SYS_UNIX_SENDMSG` and
//!   `SYS_UNIX_RECVMSG`: a file, a pipe end, the console, an eventfd or one of
//!   these sockets -- each as its handle, its type and its status flags, never
//!   its number, which only this library knows. A socket that travels is a
//!   stream socket: the kernel has no call that tells a received one's type
//!   yet, so a datagram or sequenced-packet one is refused
//!   (`known-issues/D-POSIX-A-DATAGRAM-UNIX-SOCKET-CANNOT-BE-PASSED.md`).
//! - **Errors** are the kernel's, through [`errno::errno_for`], but two:
//!   `ConnectAlready` is `EISCONN` here (it is `EALREADY` for TCP), and a
//!   stream or sequenced-packet socket whose peer has gone (`ChannelClosed`,
//!   `BrokenPipe`) is `EPIPE` -- with `SIGPIPE`, unless `MSG_NOSIGNAL`.
//!
//! # Not yet
//!
//! - Stating credentials other than one's own (`EPERM`).
//! - Disconnecting a datagram socket by connecting it to `AF_UNSPEC`: there is
//!   no native call for it (`EOPNOTSUPP`).

use core::mem::MaybeUninit;

use crate::errno::{self, native};
use crate::fdtable::{self, HandleKind};
use crate::perprocess::process_global;
use crate::socket::{
    AF_UNIX, MSG_CTRUNC, MSG_DONTWAIT, MSG_ERRQUEUE, MSG_NOSIGNAL, MSG_OOB, MSG_PEEK, MSG_TRUNC,
    MSG_WAITALL, Msghdr, SCM_CREDENTIALS, SCM_RIGHTS, SOCK_DGRAM, SOCK_RAW, SOCK_SEQPACKET,
    SOCK_STREAM, SOL_SOCKET, Sockaddr, SocklenT,
};
#[cfg(target_os = "none")]
use crate::syscall::{SYS_MMAP, SYS_MUNMAP};
use crate::syscall::{
    SYS_UNIX_ACCEPT, SYS_UNIX_BIND, SYS_UNIX_CLOSE, SYS_UNIX_CONNECT, SYS_UNIX_GET_OPTION,
    SYS_UNIX_LISTEN, SYS_UNIX_NAME, SYS_UNIX_PAIR, SYS_UNIX_PEER_CRED, SYS_UNIX_POLL,
    SYS_UNIX_RECV, SYS_UNIX_RECVMSG, SYS_UNIX_SEND, SYS_UNIX_SENDMSG, SYS_UNIX_SET_OPTION,
    SYS_UNIX_SHUTDOWN, SYS_UNIX_SOCKET, UNIX_ADDR_LEN, UNIX_MSG_CTRUNC, UNIX_NAME_ABSTRACT,
    UNIX_NONBLOCK, UNIX_OPT_INQ, UNIX_OPT_PASSCRED, UNIX_OPT_RCVTIMEO, UNIX_OPT_SNDTIMEO,
    UNIX_PEEK, UNIX_RECV_INFO_LEN, UNIX_RIGHT_LEN, syscall1, syscall2, syscall3, syscall3_2ret,
    syscall4, syscall5, syscall6,
};

/// The bytes of `sun_path`.
pub(crate) const SUN_PATH_LEN: usize = 108;
/// `sizeof(sa_family_t)`, where `sun_path` starts.
const FAMILY_LEN: usize = 2;
/// `sizeof(struct sockaddr_un)`.
pub(crate) const SOCKADDR_UN_LEN: usize = FAMILY_LEN + SUN_PATH_LEN;
/// The longest datagram the kernel takes (its `MAX_DGRAM`), and so the most a
/// datagram receive can need.
pub(crate) const MAX_DGRAM: usize = 64 * 1024;
/// `PF_UNIX`'s only protocol besides 0, which Linux's `unix_create` accepts.
const PF_UNIX: i32 = 1;
/// The ids Linux reports for credentials it does not know: the overflow uid
/// and gid (`/proc/sys/kernel/overflowuid`).
const OVERFLOW_ID: u32 = 65534;

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

/// A name as the kernel takes it: an absolute path, or an abstract name.
#[derive(Clone, Copy)]
pub(crate) struct Name {
    bytes: [u8; SUN_PATH_LEN],
    len: usize,
    is_abstract: bool,
}

impl Name {
    /// The name's bytes.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or(&[])
    }

    /// The `UNIX_NAME_ABSTRACT` flag this name is passed with.
    fn flag(&self) -> u64 {
        if self.is_abstract {
            UNIX_NAME_ABSTRACT
        } else {
            0
        }
    }

    /// A name of `bytes`, which must fit.
    fn of(bytes: &[u8], is_abstract: bool) -> Option<Self> {
        let mut out = [0u8; SUN_PATH_LEN];
        out.get_mut(..bytes.len())?.copy_from_slice(bytes);
        Some(Self {
            bytes: out,
            len: bytes.len(),
            is_abstract,
        })
    }
}

/// What the `sockaddr` bytes `raw` -- the caller's `addrlen` of them -- name,
/// with a relative path resolved against `cwd`: `Ok(None)` for the family
/// alone, which `bind` turns into an autobind; else the name, or the errno.
///
/// Linux's `unix_validate_addr` and `unix_mkname_bsd`: a family other than
/// `AF_UNIX`, or more bytes than a `sockaddr_un`, is `EINVAL`.
pub(crate) fn parse_name(raw: &[u8], cwd: &[u8]) -> Result<Option<Name>, i32> {
    let (Some(&f0), Some(&f1)) = (raw.first(), raw.get(1)) else {
        return Err(errno::EINVAL);
    };
    if u16::from_ne_bytes([f0, f1]) != AF_UNIX as u16 || raw.len() > SOCKADDR_UN_LEN {
        return Err(errno::EINVAL);
    }
    let path = raw.get(FAMILY_LEN..).unwrap_or(&[]);
    let Some(&first) = path.first() else {
        return Ok(None);
    };
    if first == 0 {
        // Abstract: every byte after the NUL counts, NULs included. The
        // kernel takes no empty name, where Linux would bind one.
        let name = path.get(1..).unwrap_or(&[]);
        if name.is_empty() {
            return Err(errno::EINVAL);
        }
        return Name::of(name, true).map(Some).ok_or(errno::EINVAL);
    }
    let end = path.iter().position(|&b| b == 0).unwrap_or(path.len());
    let given = path.get(..end).unwrap_or(&[]);
    if given.first() == Some(&b'/') {
        return Name::of(given, false).map(Some).ok_or(errno::ENAMETOOLONG);
    }
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let n = crate::unistd::resolve_path_against(cwd, given, &mut resolved)
        .ok_or(errno::ENAMETOOLONG)?;
    let bytes = resolved.get(..n).ok_or(errno::ENAMETOOLONG)?;
    Name::of(bytes, false).map(Some).ok_or(errno::ENAMETOOLONG)
}

/// [`parse_name`] for the caller's `addr`/`addrlen`, against the working
/// directory.
///
/// # Safety
///
/// `addr` must be readable for `addrlen` bytes when it is not null.
unsafe fn name_from_user(addr: *const Sockaddr, addrlen: SocklenT) -> Result<Option<Name>, i32> {
    let len = addrlen as usize;
    if addr.is_null() {
        return Err(if len == 0 {
            errno::EINVAL
        } else {
            errno::EFAULT
        });
    }
    if len > SOCKADDR_UN_LEN {
        return Err(errno::EINVAL);
    }
    // SAFETY: readable for `len` bytes, the caller's contract; `len` is at
    // most a `sockaddr_un`.
    let raw = unsafe { core::slice::from_raw_parts(addr.cast::<u8>(), len) };
    let mut cwd = [0u8; crate::unistd::PATH_MAX];
    let n = crate::unistd::current_cwd(&mut cwd);
    parse_name(raw, cwd.get(..n).unwrap_or(&[]))
}

/// The `sockaddr_un` an address record says, as Linux's `unix_getname`
/// writes one: the family alone for an unnamed socket; a path with its NUL;
/// an abstract name after its leading NUL. Returns the bytes and their
/// length. (A received message's sender is [`store_sender`]'s.)
pub(crate) fn sockaddr_of(rec: &[u8]) -> ([u8; SOCKADDR_UN_LEN], usize) {
    let mut out = [0u8; SOCKADDR_UN_LEN];
    if let Some(family) = out.get_mut(..FAMILY_LEN) {
        family.copy_from_slice(&(AF_UNIX as u16).to_ne_bytes());
    }
    let word = |at: usize| -> u32 {
        rec.get(at..at.saturating_add(4))
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map_or(0, u32::from_le_bytes)
    };
    let kind = word(0);
    let n = (word(4) as usize).min(SUN_PATH_LEN);
    let bytes = rec.get(8..8_usize.saturating_add(n)).unwrap_or(&[]);
    let len = match kind {
        // A path, and its NUL when there is room for one.
        1 => {
            if let Some(dst) = out.get_mut(FAMILY_LEN..FAMILY_LEN.saturating_add(bytes.len())) {
                dst.copy_from_slice(bytes);
            }
            FAMILY_LEN
                .saturating_add(bytes.len())
                .saturating_add(1)
                .min(SOCKADDR_UN_LEN)
        }
        // An abstract name, after the NUL that marks one.
        2 => {
            let start = FAMILY_LEN.saturating_add(1);
            let bytes = bytes
                .get(..bytes.len().min(SUN_PATH_LEN.saturating_sub(1)))
                .unwrap_or(&[]);
            if let Some(dst) = out.get_mut(start..start.saturating_add(bytes.len())) {
                dst.copy_from_slice(bytes);
            }
            start.saturating_add(bytes.len())
        }
        _ => FAMILY_LEN,
    };
    (out, len)
}

/// Write the address record `rec` to the caller's `addr`/`addrlen`: as much
/// as `*addrlen` has room for, and `*addrlen` the whole length, as Linux's
/// `move_addr_to_user` does. Nothing for a null `addr` or `addrlen`.
///
/// # Safety
///
/// When neither is null, `addrlen` must be readable and writable, and `addr`
/// writable for `*addrlen` bytes.
pub(crate) unsafe fn store_address(rec: &[u8], addr: *mut Sockaddr, addrlen: *mut SocklenT) {
    if addr.is_null() || addrlen.is_null() {
        return;
    }
    let (sun, len) = sockaddr_of(rec);
    // SAFETY: the caller's contract.
    let room = unsafe { *addrlen } as usize;
    let n = room.min(len);
    if let Some(src) = sun.get(..n) {
        // SAFETY: `addr` is writable for `room >= n` bytes; `src` is `n`
        // bytes of a local array, so the two do not overlap.
        unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), addr.cast::<u8>(), n) };
    }
    // SAFETY: the caller's contract.
    unsafe { *addrlen = len as SocklenT };
}

/// Write a received message's sender, address record `rec`, to the caller's
/// `addr`/`addrlen`, as Linux's `unix_copy_addr` does: a sender with a name
/// as [`store_address`] writes it, and one without -- an unbound socket --
/// nothing at all, with `*addrlen` 0 (the length `recvfrom` and `recvmsg`
/// start from). Until 2026-10-16 an unnamed sender was the family alone,
/// length 2, which a server telling anonymous senders by a length of 0 did
/// not see as one, and whose `sendto` back was `EINVAL` (Opus's review of
/// lane D's round 202).
///
/// # Safety
///
/// As [`store_address`].
pub(crate) unsafe fn store_sender(rec: &[u8], addr: *mut Sockaddr, addrlen: *mut SocklenT) {
    if addr.is_null() || addrlen.is_null() {
        return;
    }
    let kind = rec
        .get(..4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map_or(0, u32::from_le_bytes);
    if kind == 1 || kind == 2 {
        // SAFETY: forwarded.
        unsafe { store_address(rec, addr, addrlen) };
    } else {
        // SAFETY: the caller's contract.
        unsafe { *addrlen = 0 };
    }
}

/// An abstract name of five lowercase hex digits for `n`, as Linux's
/// `unix_autobind` makes them (`"%05x"`).
pub(crate) fn autobind_name(n: u32) -> [u8; 5] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = [b'0'; 5];
    for (slot, shift) in out.iter_mut().zip([16u32, 12, 8, 4, 0]) {
        let digit = ((n >> shift) & 0xF) as usize;
        *slot = HEX.get(digit).copied().unwrap_or(b'0');
    }
    out
}

// ---------------------------------------------------------------------------
// Errors, flags, metadata
// ---------------------------------------------------------------------------

/// The errno for a `SYS_UNIX_*` call's negative answer `ret`.
pub(crate) fn errno_of(ret: i64) -> i32 {
    match ret {
        native::CONNECT_ALREADY => errno::EISCONN,
        native::CHANNEL_CLOSED | native::BROKEN_PIPE => errno::EPIPE,
        native::NOT_SUPPORTED => errno::EOPNOTSUPP,
        _ => errno::errno_for(ret),
    }
}

/// The errno for a negative answer `ret` from the call that makes a socket
/// (`SYS_UNIX_SOCKET`, `SYS_UNIX_PAIR`): [`errno_of`]'s, except that a kernel
/// with no such call -- every kernel before lane A's Unix sockets -- has none
/// to make, which is `absent`. For `socket` that is `EAFNOSUPPORT`, Linux's
/// answer when it is built without Unix-domain sockets and the library's
/// before it had them; programs test for exactly that, as `syslog` does to
/// choose the journal over `/dev/log` (`posix/src/syslog.rs`). The kernel's
/// own answer, `ENOSYS`, would have them treat the family as present and
/// broken.
fn creation_errno(ret: i64, absent: i32) -> i32 {
    if ret == native::NO_SUCH_SYSCALL {
        absent
    } else {
        errno_of(ret)
    }
}

/// Set `errno` for `ret` and answer -1.
fn fail(ret: i64) -> i32 {
    errno::set_errno(errno_of(ret));
    -1
}

/// `errno = e`, -1.
fn refuse(e: i32) -> i32 {
    errno::set_errno(e);
    -1
}

/// Whether `fd` is non-blocking.
fn nonblocking(fd: i32) -> bool {
    fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0
}

/// `fd`'s socket type: `SOCK_STREAM`, `SOCK_DGRAM` or `SOCK_SEQPACKET`.
pub(crate) fn sock_type(fd: i32) -> i32 {
    crate::socket::get_meta(fd).map_or(SOCK_STREAM, |m| m.sock_type)
}

/// The kernel's kind number for a socket type: Linux's numbers, with
/// `SOCK_RAW` a datagram socket, as Linux's `unix_create` makes it.
pub(crate) fn kernel_kind(sock_type: i32) -> Option<i32> {
    match sock_type {
        SOCK_STREAM | SOCK_DGRAM | SOCK_SEQPACKET => Some(sock_type),
        SOCK_RAW => Some(SOCK_DGRAM),
        _ => None,
    }
}

/// Put kernel socket `handle` in the descriptor table as a socket of
/// `sock_type`, closing it again when the table is full.
fn install(handle: u64, sock_type: i32, nonblock: bool, cloexec: bool) -> i32 {
    let status = crate::fcntl::O_RDWR
        | if nonblock {
            crate::fcntl::O_NONBLOCK
        } else {
            0
        };
    let Some(fd) = fdtable::alloc_fd_with_flags(HandleKind::UnixSocket, handle, status) else {
        // The handle is ours and nothing refers to it; its close cannot fail
        // in a way that changes this answer.
        let _ = syscall1(SYS_UNIX_CLOSE, handle);
        return refuse(errno::EMFILE);
    };
    if cloexec {
        let _ = fdtable::set_fd_flags(fd, 1); // FD_CLOEXEC; the fd was just made
    }
    crate::socket::set_unix_meta(fd, sock_type);
    fd
}

// ---------------------------------------------------------------------------
// socket, socketpair
// ---------------------------------------------------------------------------

/// `socket(AF_UNIX, sock_type, protocol)`, `sock_type` without its flags.
pub(crate) fn socket(sock_type: i32, nonblock: bool, cloexec: bool, protocol: i32) -> i32 {
    if protocol != 0 && protocol != PF_UNIX {
        return refuse(errno::EPROTONOSUPPORT);
    }
    let Some(kind) = kernel_kind(sock_type) else {
        return refuse(errno::ESOCKTNOSUPPORT);
    };
    let ret = syscall1(SYS_UNIX_SOCKET, kind as u64);
    if ret < 0 {
        return refuse(creation_errno(ret, errno::EAFNOSUPPORT));
    }
    install(ret as u64, kind, nonblock, cloexec)
}

/// `socketpair(AF_UNIX, sock_type, 0, sv)`: a pair of these sockets, of
/// any type, as [`crate::socket::socketpair`] makes every pair.
///
/// # Safety
///
/// `sv` must be writable for two `i32`s.
pub(crate) unsafe fn socketpair(
    sock_type: i32,
    nonblock: bool,
    cloexec: bool,
    sv: *mut [i32; 2],
) -> i32 {
    let Some(kind) = kernel_kind(sock_type) else {
        return refuse(errno::ESOCKTNOSUPPORT);
    };
    let (a, b) = syscall3_2ret(SYS_UNIX_PAIR, kind as u64, 0, 0);
    if a < 0 {
        // A kernel without them: `EOPNOTSUPP`, what the library answered for
        // a datagram or sequenced-packet pair before it could make one.
        return refuse(creation_errno(a, errno::EOPNOTSUPP));
    }
    let fd0 = install(a as u64, kind, nonblock, cloexec);
    if fd0 < 0 {
        let _ = syscall1(SYS_UNIX_CLOSE, b as u64); // as in `install`
        return -1;
    }
    let fd1 = install(b as u64, kind, nonblock, cloexec);
    if fd1 < 0 {
        let saved = errno::get_errno();
        let _ = crate::file::close(fd0); // undoing our own half-made pair
        errno::set_errno(saved);
        return -1;
    }
    // SAFETY: `sv` is writable for two i32s (the caller's contract).
    unsafe { *sv = [fd0, fd1] };
    0
}

// ---------------------------------------------------------------------------
// bind, listen, accept, connect
// ---------------------------------------------------------------------------

/// `bind` on a Unix socket.
///
/// # Safety
///
/// `addr` must be readable for `addrlen` bytes when it is not null.
pub(crate) unsafe fn bind(handle: u64, addr: *const Sockaddr, addrlen: SocklenT) -> i32 {
    // SAFETY: forwarded from the caller.
    let name = match unsafe { name_from_user(addr, addrlen) } {
        Ok(name) => name,
        Err(e) => return refuse(e),
    };
    let Some(name) = name else {
        return autobind(handle);
    };
    let mode = 0o777 & !crate::file::get_umask();
    let bytes = name.as_bytes();
    let ret = syscall5(
        SYS_UNIX_BIND,
        handle,
        bytes.as_ptr() as u64,
        bytes.len() as u64,
        u64::from(mode),
        name.flag(),
    );
    if ret < 0 { fail(ret) } else { 0 }
}

/// Bind `handle` to a fresh abstract name, as Linux's `unix_autobind` does:
/// five hex digits, tried in turn until one is free, and `ENOSPC` once every
/// one has been tried.
fn autobind(handle: u64) -> i32 {
    // Started from the handle, which differs between this process's sockets,
    // so that two of them do not walk the same names.
    let start = (handle as u32).wrapping_mul(0x9E37_79B1);
    for i in 0..=0xF_FFFF_u32 {
        let name = autobind_name(start.wrapping_add(i) & 0xF_FFFF);
        let ret = syscall5(
            SYS_UNIX_BIND,
            handle,
            name.as_ptr() as u64,
            name.len() as u64,
            0,
            UNIX_NAME_ABSTRACT,
        );
        if ret != native::ADDR_IN_USE {
            return if ret < 0 { fail(ret) } else { 0 };
        }
    }
    refuse(errno::ENOSPC)
}

/// `listen` on a Unix socket: the kernel clamps `backlog` to 1..=128, and a
/// negative one is the most, as Linux's `__sys_listen` makes it.
pub(crate) fn listen(fd: i32, handle: u64, backlog: i32) -> i32 {
    let backlog = u64::try_from(backlog).unwrap_or(128);
    let ret = syscall2(SYS_UNIX_LISTEN, handle, backlog);
    if ret < 0 {
        return fail(ret);
    }
    crate::socket::set_unix_listening(fd);
    0
}

/// `accept4` on a Unix socket (`accept` is `flags` 0).
///
/// # Safety
///
/// As [`store_address`] for `addr` and `addrlen`.
pub(crate) unsafe fn accept(
    fd: i32,
    handle: u64,
    addr: *mut Sockaddr,
    addrlen: *mut SocklenT,
    nonblock: bool,
    cloexec: bool,
) -> i32 {
    let call = if nonblocking(fd) { UNIX_NONBLOCK } else { 0 };
    let ret = if call != 0 {
        syscall2(SYS_UNIX_ACCEPT, handle, call)
    } else {
        crate::interrupt::restarting(crate::socket::wait_rule(fd, true), || {
            syscall2(SYS_UNIX_ACCEPT, handle, 0)
        })
    };
    if ret < 0 {
        return fail(ret);
    }
    let conn = ret as u64;
    let new = install(conn, sock_type(fd), nonblock, cloexec);
    if new < 0 {
        return -1;
    }
    // The connection's timeouts start as the listener's, in the kernel; the
    // copy kept for the restart rule follows them.
    crate::socket::copy_unix_timeouts(fd, new);
    if !addr.is_null() {
        let mut rec = [0u8; UNIX_ADDR_LEN];
        if syscall3(SYS_UNIX_NAME, conn, 1, rec.as_mut_ptr() as u64) < 0 {
            rec = [0u8; UNIX_ADDR_LEN]; // an unnamed peer, the family alone
        }
        // SAFETY: forwarded from the caller.
        unsafe { store_address(&rec, addr, addrlen) };
    }
    new
}

/// `connect` on a Unix socket.
///
/// # Safety
///
/// `addr` must be readable for `addrlen` bytes when it is not null.
pub(crate) unsafe fn connect(
    fd: i32,
    handle: u64,
    addr: *const Sockaddr,
    addrlen: SocklenT,
) -> i32 {
    // AF_UNSPEC: Linux dissolves a datagram socket's association; there is no
    // native call for it.
    if !addr.is_null() && addrlen as usize >= FAMILY_LEN {
        // SAFETY: readable for at least the family's two bytes (above).
        let family = unsafe { core::ptr::read_unaligned(addr.cast::<u16>()) };
        if family == crate::socket::AF_UNSPEC as u16 {
            return refuse(if sock_type(fd) == SOCK_DGRAM {
                errno::EOPNOTSUPP
            } else {
                errno::EINVAL
            });
        }
    }
    // SAFETY: forwarded from the caller.
    let name = match unsafe { name_from_user(addr, addrlen) } {
        Ok(Some(name)) => name,
        Ok(None) => return refuse(errno::EINVAL),
        Err(e) => return refuse(e),
    };
    let bytes = name.as_bytes();
    let nb = nonblocking(fd);
    let flags = name.flag() | if nb { UNIX_NONBLOCK } else { 0 };
    let call = || {
        syscall4(
            SYS_UNIX_CONNECT,
            handle,
            bytes.as_ptr() as u64,
            bytes.len() as u64,
            flags,
        )
    };
    // A connect waits for room in the listener's backlog, under the send
    // timeout, as Linux's `unix_stream_connect` does.
    let ret = if nb {
        call()
    } else {
        crate::interrupt::restarting(crate::socket::wait_rule(fd, false), call)
    };
    if ret < 0 { fail(ret) } else { 0 }
}

// ---------------------------------------------------------------------------
// send, receive
// ---------------------------------------------------------------------------

/// Send `len` bytes at `buf` on `fd`, to `dest` or down the connection.
/// Returns the bytes sent, or -1 with `errno` set.
///
/// # Safety
///
/// `buf` must be readable for `len` bytes.
pub(crate) unsafe fn send(
    fd: i32,
    handle: u64,
    buf: *const u8,
    len: usize,
    flags: i32,
    dest: Option<&Name>,
) -> isize {
    // SAFETY: forwarded.
    unsafe { send_carrying(fd, handle, buf, len, flags, dest, &[]) }
}

/// [`send`], carrying `rights` with the first bytes -- all or none, as the
/// kernel takes them -- through `SYS_UNIX_SENDMSG`. A kernel without that
/// call answers `EOPNOTSUPP` for them, as this library did before it.
///
/// # Safety
///
/// As [`send`].
unsafe fn send_carrying(
    fd: i32,
    handle: u64,
    buf: *const u8,
    len: usize,
    flags: i32,
    dest: Option<&Name>,
    rights: &[Right],
) -> isize {
    if flags & MSG_OOB != 0 {
        errno::set_errno(errno::EOPNOTSUPP);
        return -1;
    }
    let nb = nonblocking(fd) || flags & MSG_DONTWAIT != 0;
    let call_flags = dest.map_or(0, Name::flag) | if nb { UNIX_NONBLOCK } else { 0 };
    let (name_ptr, name_len) =
        dest.map_or((0, 0), |n| (n.as_bytes().as_ptr() as u64, n.len as u64));
    let stream = sock_type(fd) == SOCK_STREAM;
    let mut sent = 0usize;
    let mut carry = rights;
    loop {
        let rest = len.saturating_sub(sent);
        let at = buf.wrapping_add(sent) as u64;
        let record: [u64; 6] = [
            at,
            rest as u64,
            name_ptr,
            name_len,
            carry.as_ptr() as u64,
            carry.len() as u64,
        ];
        let call = || {
            if carry.is_empty() {
                syscall6(
                    SYS_UNIX_SEND,
                    handle,
                    at,
                    rest as u64,
                    name_ptr,
                    name_len,
                    call_flags,
                )
            } else {
                syscall3(SYS_UNIX_SENDMSG, handle, record.as_ptr() as u64, call_flags)
            }
        };
        let ret = if nb {
            call()
        } else {
            crate::interrupt::restarting(crate::socket::wait_rule(fd, false), call)
        };
        if ret < 0 {
            // What a stream moved before the failure is the answer, as
            // Linux's `unix_stream_sendmsg` answers it.
            if sent > 0 {
                return sent as isize;
            }
            if !carry.is_empty() && crate::syscall::lacks_call(ret) {
                errno::set_errno(errno::EOPNOTSUPP);
                return -1;
            }
            if ret == native::CHANNEL_CLOSED || ret == native::BROKEN_PIPE {
                return crate::signal::broken_pipe(flags & MSG_NOSIGNAL != 0);
            }
            errno::set_errno(errno_of(ret));
            return -1;
        }
        sent = sent.saturating_add(ret as usize);
        // The descriptors went with the first bytes.
        carry = &[];
        // A blocking stream send moves all of it, a call at a time; anything
        // else is one call.
        if !stream || nb || ret == 0 || sent >= len {
            return sent as isize;
        }
    }
}

/// Receive into `len` bytes at `buf` from `fd`. `info`, when not null, gets
/// `SYS_UNIX_RECV`'s record of the last call. Returns the bytes received --
/// or, with `MSG_TRUNC` on a datagram or sequenced-packet socket, the
/// message's whole length -- or -1 with `errno` set.
///
/// # Safety
///
/// `buf` must be writable for `len` bytes, and `info`, when not null, for
/// [`UNIX_RECV_INFO_LEN`] bytes.
pub(crate) unsafe fn recv(
    fd: i32,
    handle: u64,
    buf: *mut u8,
    len: usize,
    flags: i32,
    info: *mut u8,
) -> isize {
    // SAFETY: forwarded.
    unsafe { recv_taking(fd, handle, buf, len, flags, info, None).0 }
}

process_global! {
    /// Set once the kernel has answered `NoSuchSyscall` to
    /// `SYS_UNIX_RECVMSG`: it has no receive that carries descriptors, and
    /// every receive after goes straight to `SYS_UNIX_RECV` rather than ask
    /// again.
    fn recvmsg_missing_ptr() -> core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);
}

/// [`recv`], taking into `rights` the descriptors a message carries, through
/// `SYS_UNIX_RECVMSG` -- with no `rights`, no room, so that the kernel
/// releases them and says so; from a kernel without that call, none, as
/// before it. A stream's `MSG_WAITALL` loop ends after the receive that
/// brought some, taken or released, as Linux's `unix_stream_read_generic`
/// ends after the message whose descriptors it took: a later message's are
/// the next call's. Answers what [`recv`] does, and whether a receive
/// brought descriptors.
///
/// # Safety
///
/// As [`recv`].
unsafe fn recv_taking(
    fd: i32,
    handle: u64,
    buf: *mut u8,
    len: usize,
    flags: i32,
    info: *mut u8,
    mut rights: Option<&mut RightsIn<'_>>,
) -> (isize, bool) {
    if flags & MSG_OOB != 0 {
        // No out-of-band data is ever queued here.
        errno::set_errno(errno::EINVAL);
        return (-1, false);
    }
    if flags & MSG_ERRQUEUE != 0 {
        // Nor any error queue.
        errno::set_errno(errno::EAGAIN);
        return (-1, false);
    }
    let nb = nonblocking(fd) || flags & MSG_DONTWAIT != 0;
    let peek = flags & MSG_PEEK != 0;
    let call_flags = if nb { UNIX_NONBLOCK } else { 0 } | if peek { UNIX_PEEK } else { 0 };
    let stream = sock_type(fd) == SOCK_STREAM;
    let mut own = [0u8; UNIX_RECV_INFO_LEN];
    let info_ptr = if !info.is_null() {
        info
    } else if !stream && flags & MSG_TRUNC != 0 {
        own.as_mut_ptr()
    } else {
        core::ptr::null_mut()
    };
    let wait_all = stream && flags & MSG_WAITALL != 0 && !nb && !peek;
    let mut got = 0usize;
    let mut brought = false;
    loop {
        let at = buf.wrapping_add(got) as u64;
        let rest = len.saturating_sub(got) as u64;
        let call = || syscall5(SYS_UNIX_RECV, handle, at, rest, info_ptr as u64, call_flags);
        let rule = crate::socket::wait_rule(fd, true);
        let (recs, room) = match rights.as_deref_mut() {
            Some(taken) => (taken.recs.as_mut_ptr() as u64, taken.recs.len() as u64),
            None => (0, 0),
        };
        let mut record: [u64; 7] = [at, rest, info_ptr as u64, recs, room, 0, 0];
        let at_record = record.as_mut_ptr() as u64;
        let call_rights = || syscall3(SYS_UNIX_RECVMSG, handle, at_record, call_flags);
        // SAFETY: an atomic, shared by every thread.
        let missing = unsafe { &*recvmsg_missing_ptr() };
        let mut ret = if missing.load(core::sync::atomic::Ordering::Relaxed) {
            native::NO_SUCH_SYSCALL
        } else if nb {
            call_rights()
        } else {
            crate::interrupt::restarting(rule, call_rights)
        };
        let mut came = false;
        if crate::syscall::lacks_call(ret) {
            missing.store(true, core::sync::atomic::Ordering::Relaxed);
            ret = if nb {
                call()
            } else {
                crate::interrupt::restarting(rule, call)
            };
        } else if ret >= 0 {
            let [.., written, msg_flags] = record;
            if let Some(taken) = rights.as_deref_mut() {
                taken.got = usize::try_from(written).unwrap_or(0).min(taken.recs.len());
                taken.cut = msg_flags & UNIX_MSG_CTRUNC != 0;
            }
            came = descriptors_came(written, msg_flags);
        }
        if ret < 0 {
            if got > 0 {
                return (got as isize, brought);
            }
            errno::set_errno(errno_of(ret));
            return (-1, brought);
        }
        got = got.saturating_add(ret as usize);
        brought |= came;
        if !wait_all || ret == 0 || got >= len || came {
            break;
        }
    }
    if !stream && flags & MSG_TRUNC != 0 && !info_ptr.is_null() {
        // SAFETY: `info_ptr` is writable -- so readable -- for the record's
        // bytes, of which the first eight are the whole length.
        let whole = unsafe { core::ptr::read_unaligned(info_ptr.cast::<u64>()) };
        return (
            isize::try_from(u64::from_le(whole)).unwrap_or(isize::MAX),
            brought,
        );
    }
    (got as isize, brought)
}

/// Take descriptor `fd`, which holds Unix socket `handle` and came from
/// elsewhere -- through `SCM_RIGHTS`, or across `exec` -- for what it is: a
/// stream socket, the only kind that travels
/// (`known-issues/D-POSIX-A-DATAGRAM-UNIX-SOCKET-CANNOT-BE-PASSED.md`), and a
/// listening one if it listens, which the kernel says by refusing to count a
/// listener's bytes (`UNIX_OPT_INQ`): a listener handed to a server must
/// answer `SO_ACCEPTCONN` 1 and accept.
pub(crate) fn adopt(fd: i32, handle: u64) {
    crate::socket::set_unix_meta(fd, SOCK_STREAM);
    if syscall2(SYS_UNIX_GET_OPTION, handle, UNIX_OPT_INQ) == native::INVALID_ARGUMENT {
        crate::socket::set_unix_listening(fd);
    }
}

/// `sendto` on a Unix socket: [`send`], to `addr` when it is not null.
///
/// # Safety
///
/// As [`send`], and `addr` readable for `addrlen` bytes when not null.
pub(crate) unsafe fn sendto(
    fd: i32,
    handle: u64,
    buf: *const u8,
    len: usize,
    flags: i32,
    addr: *const Sockaddr,
    addrlen: SocklenT,
) -> isize {
    if addr.is_null() {
        // SAFETY: forwarded.
        return unsafe { send(fd, handle, buf, len, flags, None) };
    }
    // SAFETY: forwarded.
    match unsafe { name_from_user(addr, addrlen) } {
        // SAFETY: forwarded.
        Ok(Some(name)) => unsafe { send(fd, handle, buf, len, flags, Some(&name)) },
        Ok(None) => refuse(errno::EINVAL) as isize,
        Err(e) => refuse(e) as isize,
    }
}

/// `recvfrom` on a Unix socket: [`recv`], with the sender's address.
///
/// # Safety
///
/// As [`recv`] and [`store_address`].
pub(crate) unsafe fn recvfrom(
    fd: i32,
    handle: u64,
    buf: *mut u8,
    len: usize,
    flags: i32,
    addr: *mut Sockaddr,
    addrlen: *mut SocklenT,
) -> isize {
    let mut info = [0u8; UNIX_RECV_INFO_LEN];
    // SAFETY: forwarded; `info` is the record's length.
    let n = unsafe { recv(fd, handle, buf, len, flags, info.as_mut_ptr()) };
    if n >= 0 {
        // SAFETY: forwarded.
        unsafe { store_sender(info.get(28..).unwrap_or(&[]), addr, addrlen) };
    }
    n
}

/// Read a control message's `cmsghdr` fields at `at`: (length, level, type).
///
/// # Safety
///
/// `at` must be readable for a `cmsghdr`.
unsafe fn cmsg_header(at: *const u8) -> (usize, i32, i32) {
    // SAFETY: the caller's contract; unaligned reads, since the caller's
    // buffer need not be aligned.
    unsafe {
        (
            crate::socket::cmsg_len_at(at),
            core::ptr::read_unaligned(at.add(8).cast::<i32>()),
            core::ptr::read_unaligned(at.add(12).cast::<i32>()),
        )
    }
}

/// `CMSG_ALIGN`: a control message's length rounded up to eight bytes.
fn cmsg_align(n: usize) -> usize {
    n.saturating_add(7) & !7
}

/// The size of a `cmsghdr`, and so where its data starts.
const CMSG_HDR: usize = 16;
/// A `struct ucred`: pid, uid, gid.
const UCRED_LEN: usize = 12;

/// Check `msg`'s control messages before a send, and gather the descriptors
/// its `SCM_RIGHTS` messages name into the first of `rights` -- or, with no
/// `rights`, only check and count them: their number. Linux's
/// `__scm_send`, `scm_fp_copy` and `scm_check_creds` -- every `SCM_RIGHTS`
/// message's descriptors appended to one list of at most [`SCM_MAX_FD`]
/// (`EINVAL` beyond), each open (`EBADF`) and of a kind that can travel
/// ([`right_of`]); `SCM_CREDENTIALS` stating only what the receiver will
/// be told ([`may_state`]).
///
/// # Safety
///
/// `msg`'s control buffer must be readable for its stated length.
unsafe fn check_control(
    msg: &Msghdr,
    mut rights: Option<&mut [MaybeUninit<Right>; SCM_MAX_FD]>,
) -> Result<usize, i32> {
    let base = msg.msg_control.cast_const();
    let mut count = 0usize;
    // SAFETY: the caller's contract.
    for message in unsafe { control_messages(msg) } {
        let ControlMessage {
            off,
            len,
            level,
            kind,
        } = message?;
        if level == SOL_SOCKET && kind == SCM_RIGHTS {
            let n = len.saturating_sub(CMSG_HDR) / 4;
            if count.saturating_add(n) > SCM_MAX_FD {
                return Err(errno::EINVAL);
            }
            for i in 0..n {
                // SAFETY: the `n` ints lie inside this message, which lies
                // inside the buffer.
                let fd = unsafe {
                    core::ptr::read_unaligned(
                        base.add(
                            off.saturating_add(CMSG_HDR)
                                .saturating_add(i.saturating_mul(4)),
                        )
                        .cast::<i32>(),
                    )
                };
                let right = right_of(fd)?;
                if let Some(rights) = rights.as_deref_mut() {
                    rights.get_mut(count).ok_or(errno::EINVAL)?.write(right);
                }
                count = count.saturating_add(1);
            }
        } else if level == SOL_SOCKET && kind == SCM_CREDENTIALS {
            if len != CMSG_HDR + UCRED_LEN {
                return Err(errno::EINVAL);
            }
            // SAFETY: the data's twelve bytes are inside the buffer.
            let (pid, uid, gid) = unsafe {
                let at = base.add(off).add(CMSG_HDR);
                (
                    core::ptr::read_unaligned(at.cast::<i32>()),
                    core::ptr::read_unaligned(at.add(4).cast::<u32>()),
                    core::ptr::read_unaligned(at.add(8).cast::<u32>()),
                )
            };
            if !may_state(
                crate::process::getpid(),
                &crate::unistd::ids(),
                (pid, uid, gid),
            ) {
                return Err(errno::EPERM);
            }
        } else if level == SOL_SOCKET {
            return Err(errno::EINVAL);
        }
    }
    Ok(count)
}

/// One control message in a buffer: where it begins, its stated length,
/// its level and its type.
#[derive(Clone, Copy)]
struct ControlMessage {
    off: usize,
    len: usize,
    level: i32,
    kind: i32,
}

/// `msg`'s control messages, in order, each header read where it lies inside
/// the buffer -- or `Err(EINVAL)` at one shorter than a header or whose
/// stated length runs past the buffer, which ends them. The one walk
/// [`check_control`] and [`carries_rights`] take.
///
/// # Safety
///
/// `msg`'s control buffer must be readable for its stated length while the
/// walk goes on.
unsafe fn control_messages(msg: &Msghdr) -> impl Iterator<Item = Result<ControlMessage, i32>> {
    let base = msg.msg_control.cast_const();
    let total = if base.is_null() {
        0
    } else {
        msg.msg_controllen
    };
    let mut off = 0usize;
    let mut done = false;
    core::iter::from_fn(move || {
        if done || off.saturating_add(CMSG_HDR) > total {
            return None;
        }
        // SAFETY: the header lies inside the buffer (the bound above), which
        // the caller's contract makes readable.
        let (len, level, kind) = unsafe { cmsg_header(base.add(off)) };
        if len < CMSG_HDR || off.saturating_add(len) > total {
            done = true;
            return Some(Err(errno::EINVAL));
        }
        let message = ControlMessage {
            off,
            len,
            level,
            kind,
        };
        off = off.saturating_add(cmsg_align(len));
        Some(Ok(message))
    })
}

/// Whether a sender whose pid is `own_pid` and whose ids are `ids` may state
/// the credentials `(pid, uid, gid)` in an `SCM_CREDENTIALS` message: only
/// what the receiver will be told -- its own pid and real ids -- since the
/// native send carries no stated credentials, and the kernel tells the
/// receiver the sender's real ones. Linux's `scm_check_creds` lets a sender
/// state its effective or saved ids too, and root anything, and passes on
/// what was stated; here that is refused (`EPERM`) rather than accepted and
/// then replaced
/// (`known-issues/D-POSIX-STATED-CREDENTIALS-ARE-ONLY-THE-REAL-ONES.md`).
fn may_state(own_pid: i32, ids: &crate::unistd::Ids, (pid, uid, gid): (i32, u32, u32)) -> bool {
    use crate::unistd::REAL;
    pid == own_pid && ids.uid.get(REAL) == Some(&uid) && ids.gid.get(REAL) == Some(&gid)
}

/// Linux's `SCM_MAX_FD`: the most descriptors one message may carry.
const SCM_MAX_FD: usize = 253;

/// One descriptor as the kernel carries it: its handle (u64), its type
/// (u32, [`TRAVELLING`]'s codes) and its open file description's status
/// flags (u32), little-endian, [`UNIX_RIGHT_LEN`] bytes.
type Right = [u8; UNIX_RIGHT_LEN];

/// The kinds of descriptor that can travel, and the type code each goes as
/// -- the codes `posix_spawn`'s `fd_map` uses: a file, a pipe end, the
/// console, an eventfd, one of these sockets. The kernel refuses the rest.
const TRAVELLING: [(u32, HandleKind); 5] = [
    (0, HandleKind::File),
    (1, HandleKind::Pipe),
    (4, HandleKind::Console),
    (5, HandleKind::Eventfd),
    (8, HandleKind::UnixSocket),
];

/// The record that carries descriptor `fd`, or what a send of it answers:
/// `EBADF` for one that is not open, `EOPNOTSUPP` for a kind that cannot
/// travel -- a TCP or UDP socket, a terminal, an epoll, as the kernel
/// answers them -- and for a datagram or sequenced-packet Unix socket, which
/// its receiver would take for a stream's
/// (`known-issues/D-POSIX-A-DATAGRAM-UNIX-SOCKET-CANNOT-BE-PASSED.md`). The
/// status flags go with it, the access mode among them, for the receiver's
/// descriptor.
fn right_of(fd: i32) -> Result<Right, i32> {
    let entry = fdtable::get_fd(fd).ok_or(errno::EBADF)?;
    let (code, _) = TRAVELLING
        .iter()
        .find(|&&(_, kind)| kind == entry.kind)
        .ok_or(errno::EOPNOTSUPP)?;
    if entry.kind == HandleKind::UnixSocket && sock_type(fd) != SOCK_STREAM {
        return Err(errno::EOPNOTSUPP);
    }
    let status = fdtable::get_status_flags(fd).unwrap_or(0).cast_unsigned();
    let mut right = [0u8; UNIX_RIGHT_LEN];
    let bytes = entry
        .handle
        .to_le_bytes()
        .into_iter()
        .chain(code.to_le_bytes())
        .chain(status.to_le_bytes());
    for (to, from) in right.iter_mut().zip(bytes) {
        *to = from;
    }
    Ok(right)
}

/// A record's handle, type code and status flags.
fn decode_right(right: &Right) -> (u64, u32, i32) {
    let word = |at: usize| {
        right
            .get(at..at.saturating_add(4))
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map_or(0, u32::from_le_bytes)
    };
    let handle = right
        .first_chunk::<8>()
        .copied()
        .map_or(0, u64::from_le_bytes);
    (handle, word(8), word(12).cast_signed())
}

/// The first `n` of `recs`, at most all of them.
///
/// # Safety
///
/// The first `n` of `recs` must have been written.
unsafe fn written(recs: &[MaybeUninit<Right>], n: usize) -> &[Right] {
    let n = n.min(recs.len());
    // SAFETY: the caller's contract; a `MaybeUninit<Right>` has a `Right`'s
    // layout.
    unsafe { core::slice::from_raw_parts(recs.as_ptr().cast::<Right>(), n) }
}

/// The descriptors a receive may take (`SYS_UNIX_RECVMSG`), and what came.
struct RightsIn<'a> {
    /// Room for the kernel's records: one for each descriptor the caller's
    /// control buffer can hold, at most [`SCM_MAX_FD`]. Unwritten until the
    /// kernel writes the first [`got`](Self::got).
    recs: &'a mut [MaybeUninit<Right>],
    /// How many the kernel wrote.
    got: usize,
    /// Whether the kernel released some: no room for them, or a kind with
    /// no type here.
    cut: bool,
}

impl<'a> RightsIn<'a> {
    /// Room for `recs.len()` descriptors, none taken yet.
    fn new(recs: &'a mut [MaybeUninit<Right>]) -> Self {
        Self {
            recs,
            got: 0,
            cut: false,
        }
    }

    /// The records the kernel wrote.
    fn taken(&self) -> &[Right] {
        // SAFETY: `got` is what the kernel reported writing, after writing
        // them (`SYS_UNIX_RECVMSG`).
        unsafe { written(self.recs, self.got) }
    }
}

/// Whether a receive's message brought descriptors, by `SYS_UNIX_RECVMSG`'s
/// record: `written` taken, or some released for want of room (`msg_flags`'
/// `UNIX_MSG_CTRUNC`) -- after which a stream's receive goes no further.
fn descriptors_came(written: u64, msg_flags: u64) -> bool {
    written > 0 || msg_flags & UNIX_MSG_CTRUNC != 0
}

/// How many descriptors fit in `msg`'s control buffer after the
/// credentials [`write_credentials`] puts first: an `SCM_RIGHTS` header and
/// an `int` each -- Linux's `scm_detach_fds` count -- at most
/// [`SCM_MAX_FD`].
fn rights_room(msg: &Msghdr, passcred: bool) -> usize {
    if msg.msg_control.is_null() {
        return 0;
    }
    let room = msg.msg_controllen;
    let credentials = if passcred && room >= CMSG_HDR {
        cmsg_align(CMSG_HDR + UCRED_LEN).min(room)
    } else {
        0
    };
    (room.saturating_sub(credentials).saturating_sub(CMSG_HDR) / 4).min(SCM_MAX_FD)
}

/// Install the descriptors a receive brought, and write their `SCM_RIGHTS`
/// message into `msg`'s control buffer at `at`, after the credentials: the
/// control data's new length, and whether any were cut -- released by the
/// kernel for want of room or of a type, or here for want of a free
/// descriptor, where Linux stops and closes the rest (`EMFILE`). A handle
/// this process held already comes back as itself, and is installed as a
/// second descriptor sharing it, as `dup` installs one, so that only the
/// last close releases it. `cloexec` is `MSG_CMSG_CLOEXEC`.
///
/// # Safety
///
/// `msg.msg_control` must be writable for `msg.msg_controllen` bytes, and
/// `at` within them.
unsafe fn land_rights(
    msg: &Msghdr,
    rights: &RightsIn<'_>,
    at: usize,
    cloexec: bool,
) -> (usize, bool) {
    let room = if msg.msg_control.is_null() {
        0
    } else {
        msg.msg_controllen.saturating_sub(at)
    };
    // The ints that fit after the header: as many as `rights_room` gave the
    // kernel room for.
    let fits = room.saturating_sub(CMSG_HDR) / 4;
    let mut landed = 0usize;
    let mut cut = rights.cut;
    let mut full = false;
    for right in rights.taken() {
        let (handle, code, status) = decode_right(right);
        // The kernel sends only the types it was given; any other it
        // released itself and reported as cut.
        let Some(&(_, kind)) = TRAVELLING.iter().find(|&&(c, _)| c == code) else {
            cut = true;
            continue;
        };
        let fd = if full || landed >= fits {
            None
        } else {
            fdtable::alloc_fd_with_flags(kind, handle, status)
        };
        let Some(fd) = fd else {
            full = true;
            cut = true;
            if !fdtable::is_handle_referenced(kind, handle) {
                // Nobody here holds it: its close cannot fail in a way
                // anyone could act on.
                let _ = crate::file::close_kernel_handle(kind, handle);
            }
            continue;
        };
        if cloexec {
            let _ = fdtable::set_fd_flags(fd, fdtable::FD_CLOEXEC);
        }
        if kind == HandleKind::UnixSocket {
            adopt(fd, handle);
        }
        let int_at = at
            .saturating_add(CMSG_HDR)
            .saturating_add(landed.saturating_mul(4));
        // SAFETY: `landed < fits`, so the int lies inside the caller's
        // buffer (its contract), after the header's room.
        unsafe { core::ptr::write_unaligned(msg.msg_control.add(int_at).cast::<i32>(), fd) };
        landed = landed.saturating_add(1);
    }
    if landed == 0 {
        return (at, cut);
    }
    let len = CMSG_HDR.saturating_add(landed.saturating_mul(4));
    let mut header = [0u8; CMSG_HDR];
    let bytes = len
        .to_ne_bytes()
        .into_iter()
        .chain(SOL_SOCKET.to_ne_bytes())
        .chain(SCM_RIGHTS.to_ne_bytes());
    for (to, from) in header.iter_mut().zip(bytes) {
        *to = from;
    }
    // SAFETY: the caller's contract; a descriptor landed, so `fits > 0`
    // and the header's bytes lie inside the buffer after `at`.
    unsafe { core::ptr::copy_nonoverlapping(header.as_ptr(), msg.msg_control.add(at), CMSG_HDR) };
    (at.saturating_add(cmsg_align(len).min(room)), cut)
}

/// The `iovec`s of `msg` as (base, length) pairs, and their total.
///
/// # Safety
///
/// `msg.msg_iov` must be readable for `msg.msg_iovlen` `iovec`s.
unsafe fn iov_total(msg: &Msghdr) -> Result<usize, i32> {
    if msg.msg_iovlen > crate::linux_limits::IOV_MAX {
        return Err(errno::EMSGSIZE);
    }
    if msg.msg_iovlen > 0 && msg.msg_iov.is_null() {
        return Err(errno::EFAULT);
    }
    let mut total = 0usize;
    for i in 0..msg.msg_iovlen {
        // SAFETY: the caller's contract.
        let iov = unsafe { &*msg.msg_iov.add(i) };
        total = total.checked_add(iov.iov_len).ok_or(errno::EINVAL)?;
    }
    Ok(total)
}

/// `sendmsg` on a Unix socket.
///
/// # Safety
///
/// `msg` must be a valid `msghdr` whose buffers are readable as stated.
pub(crate) unsafe fn sendmsg(fd: i32, handle: u64, msg: &Msghdr, flags: i32) -> isize {
    // SAFETY: forwarded.
    if unsafe { carries_rights(msg) } {
        // SAFETY: forwarded.
        return unsafe { sendmsg_with_rights(fd, handle, msg, flags) };
    }
    // SAFETY: forwarded.
    match unsafe { check_control(msg, None) } {
        // SAFETY: forwarded.
        Ok(_) => unsafe { sendmsg_carrying(fd, handle, msg, flags, &[]) },
        Err(e) => refuse(e) as isize,
    }
}

/// Whether `msg`'s control data holds an `SCM_RIGHTS` message: its headers
/// read as far as they lie inside the buffer, and nothing else -- no
/// descriptor looked up, no credential checked -- so that a send is checked
/// once, by [`check_control`], with records only where there are
/// descriptors. A header [`check_control`] refuses ends the scan: the check
/// then answers for it.
///
/// # Safety
///
/// As [`check_control`].
unsafe fn carries_rights(msg: &Msghdr) -> bool {
    // SAFETY: the caller's contract.
    unsafe { control_messages(msg) }
        .map_while(Result::ok)
        .any(|m| m.level == SOL_SOCKET && m.kind == SCM_RIGHTS)
}

/// [`sendmsg`] of a message that carries descriptors: their records, 4 KB
/// of stack, in a frame of their own, which a message without any never
/// pays for.
///
/// # Safety
///
/// As [`sendmsg`].
#[inline(never)]
unsafe fn sendmsg_with_rights(fd: i32, handle: u64, msg: &Msghdr, flags: i32) -> isize {
    let mut gathered = [MaybeUninit::<Right>::uninit(); SCM_MAX_FD];
    // SAFETY: forwarded.
    match unsafe { check_control(msg, Some(&mut gathered)) } {
        Ok(n) => {
            // SAFETY: `check_control` wrote the first `n`.
            let rights = unsafe { written(&gathered, n) };
            // SAFETY: forwarded.
            unsafe { sendmsg_carrying(fd, handle, msg, flags, rights) }
        }
        Err(e) => refuse(e) as isize,
    }
}

/// [`sendmsg`]'s body: `rights` the records of the descriptors it carries.
///
/// # Safety
///
/// As [`sendmsg`].
unsafe fn sendmsg_carrying(
    fd: i32,
    handle: u64,
    msg: &Msghdr,
    flags: i32,
    rights: &[Right],
) -> isize {
    let dest = if msg.msg_name.is_null() || msg.msg_namelen == 0 {
        None
    } else {
        // SAFETY: forwarded.
        match unsafe {
            name_from_user(
                msg.msg_name.cast_const().cast::<Sockaddr>(),
                msg.msg_namelen,
            )
        } {
            Ok(Some(name)) => Some(name),
            Ok(None) => return refuse(errno::EINVAL) as isize,
            Err(e) => return refuse(e) as isize,
        }
    };
    // SAFETY: forwarded.
    let total = match unsafe { iov_total(msg) } {
        Ok(t) => t,
        Err(e) => return refuse(e) as isize,
    };
    if msg.msg_iovlen <= 1 || total == 0 {
        let (base, len) = if msg.msg_iovlen == 1 {
            // SAFETY: one iovec, checked by iov_total.
            let iov = unsafe { &*msg.msg_iov };
            (iov.iov_base.cast_const(), iov.iov_len)
        } else {
            (core::ptr::null(), 0)
        };
        // SAFETY: forwarded.
        return unsafe { send_carrying(fd, handle, base, len, flags, dest.as_ref(), rights) };
    }
    if sock_type(fd) == SOCK_STREAM {
        // SAFETY: forwarded.
        return unsafe { send_gather(fd, handle, msg, flags, dest.as_ref(), rights) };
    }
    if total > MAX_DGRAM {
        return refuse(errno::EMSGSIZE) as isize;
    }
    // A message in several pieces is one message: gathered into one buffer
    // and sent once, as a datagram must be.
    let Some(bounce) = Bounce::new(total) else {
        return refuse(errno::ENOBUFS) as isize;
    };
    let mut at = 0usize;
    for i in 0..msg.msg_iovlen {
        // SAFETY: iov_total checked them; the bounce buffer holds `total`.
        unsafe {
            let iov = &*msg.msg_iov.add(i);
            if iov.iov_len > 0 {
                core::ptr::copy_nonoverlapping(
                    iov.iov_base.cast_const(),
                    bounce.ptr.add(at),
                    iov.iov_len,
                );
            }
            at = at.saturating_add(iov.iov_len);
        }
    }
    // SAFETY: `bounce` holds `total` initialised bytes.
    unsafe { send_carrying(fd, handle, bounce.ptr, total, flags, dest.as_ref(), rights) }
}

/// A stream's `sendmsg` from several pieces: each sent in turn, as Linux's
/// `unix_stream_sendmsg` sends them, with no copy of the whole -- which a
/// stream, unlike a datagram, has no bound on. A piece that goes short (a
/// non-blocking socket whose peer's buffer filled) ends it. Only a call that
/// sent nothing raises `SIGPIPE` for a closed peer; one that had sent some
/// returns the count, as Linux's does. `rights` go with the first piece,
/// as Linux's attach them to the first buffer.
///
/// # Safety
///
/// `msg.msg_iov` must be `msg.msg_iovlen` `iovec`s, each readable as stated.
unsafe fn send_gather(
    fd: i32,
    handle: u64,
    msg: &Msghdr,
    flags: i32,
    dest: Option<&Name>,
    rights: &[Right],
) -> isize {
    let caller_errno = errno::get_errno();
    let mut sent = 0usize;
    for i in 0..msg.msg_iovlen {
        // SAFETY: the caller's contract.
        let iov = unsafe { &*msg.msg_iov.add(i) };
        if iov.iov_len == 0 {
            continue;
        }
        let piece_flags = if sent > 0 {
            flags | MSG_NOSIGNAL
        } else {
            flags
        };
        let carried = if sent == 0 { rights } else { &[] };
        // SAFETY: forwarded.
        let n = unsafe {
            send_carrying(
                fd,
                handle,
                iov.iov_base.cast_const(),
                iov.iov_len,
                piece_flags,
                dest,
                carried,
            )
        };
        if n < 0 {
            if sent == 0 {
                return n;
            }
            errno::set_errno(caller_errno);
            return sent as isize;
        }
        sent = sent.saturating_add(n as usize);
        if (n as usize) < iov.iov_len {
            break;
        }
    }
    sent as isize
}

/// `recvmsg` on a Unix socket.
///
/// # Safety
///
/// `msg` must be a valid `msghdr` whose buffers are writable as stated.
pub(crate) unsafe fn recvmsg(fd: i32, handle: u64, msg: &mut Msghdr, flags: i32) -> isize {
    // SAFETY: forwarded.
    unsafe { recvmsg_reporting(fd, handle, msg, flags, true) }
}

/// [`recvmsg`] for the library's own `readv`, whose caller sees neither
/// control data nor `msg_flags`: the credentials option is not asked for,
/// one kernel call fewer per read (Fable's review of lane D's round 202).
///
/// # Safety
///
/// As [`recvmsg`].
pub(crate) unsafe fn recv_vector(fd: i32, handle: u64, msg: &mut Msghdr, flags: i32) -> isize {
    // SAFETY: forwarded.
    unsafe { recvmsg_reporting(fd, handle, msg, flags, false) }
}

/// [`recvmsg`]'s body; `control` whether the caller can see what the
/// credentials option decides -- the control data and `MSG_CTRUNC`.
///
/// # Safety
///
/// As [`recvmsg`].
unsafe fn recvmsg_reporting(
    fd: i32,
    handle: u64,
    msg: &mut Msghdr,
    flags: i32,
    control: bool,
) -> isize {
    // SAFETY: forwarded.
    let total = match unsafe { iov_total(msg) } {
        Ok(t) => t,
        Err(e) => return refuse(e) as isize,
    };
    let passcred = control && syscall2(SYS_UNIX_GET_OPTION, handle, UNIX_OPT_PASSCRED) == 1;
    // Descriptors are taken only where the caller sees the control data; a
    // message's that are not are released by the kernel, as Linux's are.
    if !control {
        // SAFETY: forwarded.
        return unsafe { recvmsg_into(fd, handle, msg, flags, total, passcred, None) };
    }
    match rights_room(msg, passcred) {
        // No room: the kernel releases what comes, and says so.
        0 => {
            let mut none = RightsIn::new(&mut []);
            // SAFETY: forwarded.
            unsafe { recvmsg_into(fd, handle, msg, flags, total, passcred, Some(&mut none)) }
        }
        // SAFETY: forwarded.
        room => unsafe { recvmsg_with_room(fd, handle, msg, flags, total, passcred, room) },
    }
}

/// [`recvmsg_into`] with room for `room` descriptors: their records, 4 KB of
/// stack, in a frame of their own, which a receive with no room for any
/// never pays for.
///
/// # Safety
///
/// As [`recvmsg`].
#[inline(never)]
unsafe fn recvmsg_with_room(
    fd: i32,
    handle: u64,
    msg: &mut Msghdr,
    flags: i32,
    total: usize,
    passcred: bool,
    room: usize,
) -> isize {
    let mut recs = [MaybeUninit::<Right>::uninit(); SCM_MAX_FD];
    let mut rights = RightsIn::new(recs.get_mut(..room).unwrap_or_default());
    // SAFETY: forwarded.
    unsafe { recvmsg_into(fd, handle, msg, flags, total, passcred, Some(&mut rights)) }
}

/// [`recvmsg_reporting`]'s body: `total` the pieces' length, `passcred`
/// whether the credentials were asked for, `rights` where the descriptors a
/// message brings go -- none where the caller cannot see them.
///
/// # Safety
///
/// As [`recvmsg`].
unsafe fn recvmsg_into(
    fd: i32,
    handle: u64,
    msg: &mut Msghdr,
    flags: i32,
    total: usize,
    passcred: bool,
    mut rights: Option<&mut RightsIn<'_>>,
) -> isize {
    let mut info = [0u8; UNIX_RECV_INFO_LEN];
    let stream = sock_type(fd) == SOCK_STREAM;
    let got = if msg.msg_iovlen <= 1 || total == 0 {
        let (base, len) = if msg.msg_iovlen == 1 {
            // SAFETY: one iovec, checked by iov_total.
            let iov = unsafe { &*msg.msg_iov };
            (iov.iov_base, iov.iov_len)
        } else {
            (core::ptr::null_mut(), 0)
        };
        // SAFETY: forwarded; `info` is the record's length.
        unsafe {
            recv_taking(
                fd,
                handle,
                base,
                len,
                flags & !MSG_TRUNC,
                info.as_mut_ptr(),
                rights.as_deref_mut(),
            )
            .0
        }
    } else if stream && flags & MSG_PEEK == 0 {
        // SAFETY: forwarded; `info` is the record's length.
        unsafe {
            recv_scatter(
                fd,
                handle,
                msg,
                flags,
                info.as_mut_ptr(),
                rights.as_deref_mut(),
            )
        }
    } else {
        // Received whole into one buffer and handed out across the pieces: a
        // datagram cannot be taken in parts, and a peek at a stream would see
        // the same first bytes again for every piece. Neither is ever more
        // than the kernel's largest datagram, or one stream receive's worth.
        let cap = total.min(MAX_DGRAM);
        let Some(bounce) = Bounce::new(cap) else {
            return refuse(errno::ENOBUFS) as isize;
        };
        // SAFETY: the bounce buffer is writable for `cap`.
        let n = unsafe {
            recv_taking(
                fd,
                handle,
                bounce.ptr,
                cap,
                flags & !MSG_TRUNC,
                info.as_mut_ptr(),
                rights.as_deref_mut(),
            )
            .0
        };
        if n > 0 {
            let mut left = n as usize;
            let mut from = 0usize;
            for i in 0..msg.msg_iovlen {
                if left == 0 {
                    break;
                }
                // SAFETY: iov_total checked the iovecs; `from + take` stays
                // inside the `n` bytes received.
                unsafe {
                    let iov = &*msg.msg_iov.add(i);
                    let take = iov.iov_len.min(left);
                    if take > 0 {
                        core::ptr::copy_nonoverlapping(bounce.ptr.add(from), iov.iov_base, take);
                    }
                    from = from.saturating_add(take);
                    left = left.saturating_sub(take);
                }
            }
        }
        n
    };
    if got < 0 {
        return got;
    }
    msg.msg_flags = 0;
    let whole = u64::from_le_bytes(
        info.get(..8)
            .and_then(|b| <[u8; 8]>::try_from(b).ok())
            .unwrap_or([0; 8]),
    );
    let mut answer = got;
    if !stream && whole > got as u64 {
        msg.msg_flags |= MSG_TRUNC;
        if flags & MSG_TRUNC != 0 {
            answer = isize::try_from(whole).unwrap_or(isize::MAX);
        }
    }
    if !msg.msg_name.is_null() {
        let mut len = msg.msg_namelen;
        // SAFETY: `msg_name` is writable for `msg_namelen` bytes.
        unsafe {
            store_sender(
                info.get(28..).unwrap_or(&[]),
                msg.msg_name.cast::<Sockaddr>(),
                &raw mut len,
            )
        };
        msg.msg_namelen = len;
    }
    // SAFETY: `msg_control` is writable for `msg_controllen` bytes.
    let (mut written, mut cut) = unsafe { write_credentials(msg, passcred, &info) };
    if let Some(rights) = &rights {
        let cloexec = flags & crate::socket::MSG_CMSG_CLOEXEC != 0;
        // SAFETY: as above; `written` is within the buffer.
        let (now, cut_now) = unsafe { land_rights(msg, rights, written, cloexec) };
        written = now;
        cut |= cut_now;
    }
    msg.msg_controllen = written;
    if cut {
        msg.msg_flags |= MSG_CTRUNC;
    }
    answer
}

/// A stream's `recvmsg` into several pieces: each received into directly, in
/// order, as Linux's `unix_stream_read_generic` fills them. The first piece is
/// received as the caller asked -- waiting, unless non-blocking -- and the
/// rest without waiting, so the call returns what had arrived; with
/// `MSG_WAITALL` every piece waits, until all of them are full. A piece that
/// comes back short ends it. `info` gets the first receive's record, which
/// carries the peer's address and credentials, and `rights` the descriptors
/// any piece's message brought -- which end it too, as the message that
/// carries them ends Linux's read.
///
/// # Safety
///
/// `msg.msg_iov` must be `msg.msg_iovlen` `iovec`s, each writable as stated,
/// and `info` writable for [`UNIX_RECV_INFO_LEN`] bytes.
unsafe fn recv_scatter(
    fd: i32,
    handle: u64,
    msg: &Msghdr,
    flags: i32,
    info: *mut u8,
    mut rights: Option<&mut RightsIn<'_>>,
) -> isize {
    let wait_all = flags & MSG_WAITALL != 0;
    let caller_errno = errno::get_errno();
    let mut got = 0usize;
    let mut first = true;
    for i in 0..msg.msg_iovlen {
        // SAFETY: the caller's contract.
        let iov = unsafe { &*msg.msg_iov.add(i) };
        if iov.iov_len == 0 {
            continue;
        }
        let piece_flags = if first || wait_all {
            flags
        } else {
            flags | MSG_DONTWAIT
        };
        let record = if first { info } else { core::ptr::null_mut() };
        // SAFETY: forwarded.
        let (n, brought) = unsafe {
            recv_taking(
                fd,
                handle,
                iov.iov_base,
                iov.iov_len,
                piece_flags,
                record,
                rights.as_deref_mut(),
            )
        };
        if n < 0 {
            if got == 0 {
                return n;
            }
            // What arrived before the failure is the answer -- including the
            // later pieces' "nothing more yet" -- as Linux answers it, and
            // a call that succeeds leaves `errno` as it found it.
            errno::set_errno(caller_errno);
            return got as isize;
        }
        first = false;
        got = got.saturating_add(n as usize);
        if (n as usize) < iov.iov_len || brought {
            break;
        }
    }
    got as isize
}

/// The credentials an info record carries, as Linux reports them in an
/// `SCM_CREDENTIALS` message: the sender's pid, uid and gid, or, where they
/// are not known, pid 0 and the overflow ids.
pub(crate) fn credentials_of(info: &[u8]) -> (i32, u32, u32) {
    let word = |at: usize| -> u32 {
        info.get(at..at.saturating_add(4))
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map_or(0, u32::from_le_bytes)
    };
    if word(24) == 0 {
        return (0, OVERFLOW_ID, OVERFLOW_ID);
    }
    let pid = info
        .get(8..16)
        .and_then(|b| <[u8; 8]>::try_from(b).ok())
        .map_or(0, u64::from_le_bytes);
    (i32::try_from(pid).unwrap_or(0), word(16), word(20))
}

/// Write an `SCM_CREDENTIALS` message to `msg`'s control buffer when
/// `passcred`. Returns the bytes written -- the new `msg_controllen` -- and
/// whether to report `MSG_CTRUNC`.
///
/// Linux's `scm_recv` and `put_cmsg`, case for case: no buffer at all is
/// `MSG_CTRUNC` with nothing written; a buffer too small for even the header
/// is nothing written and *no* `MSG_CTRUNC` (`put_cmsg`'s "XXX: return
/// error?"); one too small for the credentials gets as much of the message
/// as fits, its `cmsg_len` the buffer's length, and `MSG_CTRUNC`; otherwise
/// the whole message, and `msg_controllen` its padded size, or the buffer's
/// if that is less.
///
/// # Safety
///
/// `msg.msg_control` must be writable for `msg.msg_controllen` bytes.
unsafe fn write_credentials(msg: &Msghdr, passcred: bool, info: &[u8]) -> (usize, bool) {
    if !passcred {
        return (0, false);
    }
    if msg.msg_control.is_null() {
        return (0, true);
    }
    let room = msg.msg_controllen;
    if room < CMSG_HDR {
        return (0, false);
    }
    let need = CMSG_HDR + UCRED_LEN;
    let len = need.min(room);
    let (pid, uid, gid) = credentials_of(info);
    let mut cmsg = [0u8; CMSG_HDR + UCRED_LEN];
    cmsg[..8].copy_from_slice(&len.to_ne_bytes());
    cmsg[8..12].copy_from_slice(&SOL_SOCKET.to_ne_bytes());
    cmsg[12..16].copy_from_slice(&SCM_CREDENTIALS.to_ne_bytes());
    cmsg[16..20].copy_from_slice(&pid.to_ne_bytes());
    cmsg[20..24].copy_from_slice(&uid.to_ne_bytes());
    cmsg[24..28].copy_from_slice(&gid.to_ne_bytes());
    // SAFETY: the buffer is writable for `room` bytes, and `len <= room`.
    unsafe { core::ptr::copy_nonoverlapping(cmsg.as_ptr(), msg.msg_control, len) };
    if len < need {
        (len, true)
    } else {
        (cmsg_align(need).min(room), false)
    }
}

/// A buffer of anonymous memory straight from the kernel, given back when
/// dropped.
///
/// Not the library's `malloc`, whose lock a signal handler could find held
/// by the thread it interrupted: `sendmsg` and `recvmsg` are
/// async-signal-safe (POSIX), and a handler logging through a datagram
/// socket with two iovecs re-entered `malloc` until 2026-10-16 (Opus's
/// review of lane D's round 202). `SYS_MMAP` and `SYS_MUNMAP` take no lock of
/// the library's. A host build has no kernel to ask, and no signal handler
/// runs its tests, so it uses `malloc`.
struct Bounce {
    ptr: *mut u8,
    len: usize,
}

impl Bounce {
    /// `len` bytes, or `None` when there is no memory (a zero length still
    /// gets a buffer, so that the pointer is never null).
    fn new(len: usize) -> Option<Self> {
        let len = len.max(1);
        #[cfg(target_os = "none")]
        {
            const READ_WRITE: u64 = 3;
            let ret = syscall6(SYS_MMAP, 0, len as u64, READ_WRITE, 0, 0, 0);
            (ret > 0).then(|| Self {
                ptr: core::ptr::with_exposed_provenance_mut(ret.cast_unsigned() as usize),
                len,
            })
        }
        #[cfg(not(target_os = "none"))]
        {
            let ptr = crate::malloc::malloc(len);
            (!ptr.is_null()).then_some(Self { ptr, len })
        }
    }
}

impl Drop for Bounce {
    fn drop(&mut self) {
        #[cfg(target_os = "none")]
        {
            // The mapping is this buffer's own, made by `new`; an unmap of
            // it cannot be refused for a reason anyone could act on.
            let _ = syscall2(
                SYS_MUNMAP,
                self.ptr.expose_provenance() as u64,
                self.len as u64,
            );
        }
        #[cfg(not(target_os = "none"))]
        {
            let _ = self.len;
            // SAFETY: `ptr` came from `malloc` and is freed once, here.
            unsafe { crate::malloc::free(self.ptr) };
        }
    }
}

// ---------------------------------------------------------------------------
// names, options, shutdown, close, poll
// ---------------------------------------------------------------------------

/// `getsockname` (`which` 0) or `getpeername` (1) on a Unix socket.
///
/// # Safety
///
/// As [`store_address`].
pub(crate) unsafe fn name(
    handle: u64,
    which: u64,
    addr: *mut Sockaddr,
    addrlen: *mut SocklenT,
) -> i32 {
    if addr.is_null() || addrlen.is_null() {
        return refuse(errno::EFAULT);
    }
    let mut rec = [0u8; UNIX_ADDR_LEN];
    let ret = syscall3(SYS_UNIX_NAME, handle, which, rec.as_mut_ptr() as u64);
    if ret < 0 {
        return fail(ret);
    }
    // SAFETY: forwarded.
    unsafe { store_address(&rec, addr, addrlen) };
    0
}

/// A `struct timeval` as the kernel's nanoseconds: 0 is no limit, a negative
/// time is no limit (Linux's `sock_set_timeout`), and microseconds outside
/// 0..1_000_000 are `EDOM`.
pub(crate) fn timeval_to_ns(sec: i64, usec: i64) -> Result<u64, i32> {
    if !(0..1_000_000).contains(&usec) {
        return Err(errno::EDOM);
    }
    if sec < 0 {
        return Ok(0);
    }
    let ns = (sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add((usec as u64).saturating_mul(1_000));
    Ok(ns)
}

/// The kernel's nanoseconds as a `struct timeval` (seconds, microseconds).
pub(crate) fn ns_to_timeval(ns: u64) -> (i64, i64) {
    let sec = i64::try_from(ns / 1_000_000_000).unwrap_or(i64::MAX);
    let usec = ((ns % 1_000_000_000) / 1_000) as i64;
    (sec, usec)
}

/// `setsockopt` at `SOL_SOCKET` on a Unix socket for the options the kernel
/// keeps: `Some(answer)`, or `None` for an option this does not handle.
///
/// # Safety
///
/// `optval` must be readable for `optlen` bytes.
pub(crate) unsafe fn setsockopt(
    fd: i32,
    handle: u64,
    optname: i32,
    optval: *const u8,
    optlen: SocklenT,
) -> Option<i32> {
    use crate::socket::{SO_PASSCRED, SO_RCVTIMEO, SO_SNDTIMEO};
    match optname {
        SO_PASSCRED => {
            if optval.is_null() {
                return Some(refuse(errno::EFAULT));
            }
            if (optlen as usize) < core::mem::size_of::<i32>() {
                return Some(refuse(errno::EINVAL));
            }
            // SAFETY: readable for an i32 (checked).
            let on = unsafe { core::ptr::read_unaligned(optval.cast::<i32>()) } != 0;
            let ret = syscall3(
                SYS_UNIX_SET_OPTION,
                handle,
                UNIX_OPT_PASSCRED,
                u64::from(on),
            );
            Some(if ret < 0 { fail(ret) } else { 0 })
        }
        SO_RCVTIMEO | SO_SNDTIMEO => {
            if optval.is_null() {
                return Some(refuse(errno::EFAULT));
            }
            if (optlen as usize) < 16 {
                return Some(refuse(errno::EINVAL));
            }
            // SAFETY: readable for a 16-byte timeval (checked).
            let (sec, usec) = unsafe {
                (
                    core::ptr::read_unaligned(optval.cast::<i64>()),
                    core::ptr::read_unaligned(optval.add(8).cast::<i64>()),
                )
            };
            let ns = match timeval_to_ns(sec, usec) {
                Ok(ns) => ns,
                Err(e) => return Some(refuse(e)),
            };
            let (option, receiving) = if optname == SO_RCVTIMEO {
                (UNIX_OPT_RCVTIMEO, true)
            } else {
                (UNIX_OPT_SNDTIMEO, false)
            };
            let ret = syscall3(SYS_UNIX_SET_OPTION, handle, option, ns);
            if ret < 0 {
                return Some(fail(ret));
            }
            crate::socket::set_unix_timeout(fd, receiving, ns);
            Some(0)
        }
        _ => None,
    }
}

/// `getsockopt` at `SOL_SOCKET` on a Unix socket for the options this
/// family answers itself: `Some(answer)`, or `None` for the rest.
///
/// # Safety
///
/// `optval` must be writable for `*optlen` bytes and `optlen` readable and
/// writable, when not null.
pub(crate) unsafe fn getsockopt(
    fd: i32,
    handle: u64,
    optname: i32,
    optval: *mut u8,
    optlen: *mut SocklenT,
) -> Option<i32> {
    use crate::socket::{
        SO_ACCEPTCONN, SO_DOMAIN, SO_PASSCRED, SO_PEERCRED, SO_PROTOCOL, SO_RCVTIMEO, SO_SNDTIMEO,
    };
    let mut value = [0u8; 16];
    let len: usize = match optname {
        SO_PEERCRED => {
            // An unconnected socket, or a peer with no process, is Linux's
            // "no credentials": pid 0 and the ids -1.
            let mut raw = [0u8; 16];
            let (pid, uid, gid) =
                if syscall2(SYS_UNIX_PEER_CRED, handle, raw.as_mut_ptr() as u64) < 0 {
                    (0i32, u32::MAX, u32::MAX)
                } else {
                    let w = |at: usize| {
                        raw.get(at..)
                            .and_then(<[u8]>::first_chunk::<4>)
                            .map_or(0, |b| u32::from_le_bytes(*b))
                    };
                    let pid = raw
                        .get(..8)
                        .and_then(|b| <[u8; 8]>::try_from(b).ok())
                        .map_or(0, u64::from_le_bytes);
                    (i32::try_from(pid).unwrap_or(0), w(8), w(12))
                };
            value[..4].copy_from_slice(&pid.to_ne_bytes());
            value[4..8].copy_from_slice(&uid.to_ne_bytes());
            value[8..12].copy_from_slice(&gid.to_ne_bytes());
            UCRED_LEN
        }
        SO_PASSCRED => {
            let ret = syscall2(SYS_UNIX_GET_OPTION, handle, UNIX_OPT_PASSCRED);
            if ret < 0 {
                return Some(fail(ret));
            }
            value[..4].copy_from_slice(&i32::from(ret == 1).to_ne_bytes());
            4
        }
        SO_RCVTIMEO | SO_SNDTIMEO => {
            let option = if optname == SO_RCVTIMEO {
                UNIX_OPT_RCVTIMEO
            } else {
                UNIX_OPT_SNDTIMEO
            };
            let ret = syscall2(SYS_UNIX_GET_OPTION, handle, option);
            if ret < 0 {
                return Some(fail(ret));
            }
            let (sec, usec) = ns_to_timeval(ret as u64);
            value[..8].copy_from_slice(&sec.to_ne_bytes());
            value[8..16].copy_from_slice(&usec.to_ne_bytes());
            16
        }
        SO_DOMAIN => {
            value[..4].copy_from_slice(&AF_UNIX.to_ne_bytes());
            4
        }
        SO_PROTOCOL => 4, // PF_UNIX has no protocols: 0
        SO_ACCEPTCONN => {
            let listening = crate::socket::get_meta(fd).is_some_and(|m| m.unix_listening);
            value[..4].copy_from_slice(&i32::from(listening).to_ne_bytes());
            4
        }
        _ => return None,
    };
    if optval.is_null() || optlen.is_null() {
        return Some(refuse(errno::EFAULT));
    }
    // SAFETY: the caller's contract for `optlen`.
    let room = unsafe { *optlen } as usize;
    let n = room.min(len);
    if let Some(src) = value.get(..n) {
        // SAFETY: `optval` is writable for `room >= n` bytes.
        unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), optval, n) };
    }
    // SAFETY: as above.
    unsafe { *optlen = n as SocklenT };
    Some(0)
}

/// `shutdown` on a Unix socket.
pub(crate) fn shutdown(handle: u64, how: i32) -> i32 {
    let Ok(how) = u64::try_from(how) else {
        return refuse(errno::EINVAL);
    };
    if how > 2 {
        return refuse(errno::EINVAL);
    }
    let ret = syscall2(SYS_UNIX_SHUTDOWN, handle, how);
    if ret < 0 { fail(ret) } else { 0 }
}

/// Let go of kernel socket `handle`: the kernel's answer.
pub(crate) fn close(handle: u64) -> i64 {
    syscall1(SYS_UNIX_CLOSE, handle)
}

/// A Unix socket's readiness for `poll`: (readable, writable, hang-up, error).
pub(crate) fn readiness(handle: u64) -> (bool, bool, bool, bool) {
    let status = syscall1(SYS_UNIX_POLL, handle);
    if status < 0 {
        return (false, false, true, true);
    }
    let s = status as u16;
    (s & 0x01 != 0, s & 0x04 != 0, s & 0x10 != 0, s & 0x08 != 0)
}

/// `read` on a Unix socket: [`recv`] with no flags.
///
/// # Safety
///
/// `buf` must be writable for `count` bytes.
pub(crate) unsafe fn read(fd: i32, handle: u64, buf: *mut u8, count: usize) -> i64 {
    // SAFETY: forwarded.
    unsafe { recv(fd, handle, buf, count, 0, core::ptr::null_mut()) as i64 }
}

/// `write` on a Unix socket: [`send`] with no flags, so a peer gone is
/// `SIGPIPE` and `EPIPE`.
///
/// # Safety
///
/// `buf` must be readable for `count` bytes.
pub(crate) unsafe fn write(fd: i32, handle: u64, buf: *const u8, count: usize) -> i64 {
    // SAFETY: forwarded.
    unsafe { send(fd, handle, buf, count, 0, None) as i64 }
}

/// The bytes a receive on Unix socket `handle` would find waiting, for
/// `FIONREAD`: the kernel's count (`UNIX_OPT_INQ`), as Linux's
/// `unix_inq_len` counts them -- everything queued at a stream or
/// sequenced-packet socket, the next datagram at a datagram one, 0 at one
/// not connected -- and `EINVAL` at a listener.
pub(crate) fn bytes_waiting(handle: u64) -> Result<usize, i32> {
    let ret = syscall2(SYS_UNIX_GET_OPTION, handle, UNIX_OPT_INQ);
    if ret < 0 {
        return Err(errno_of(ret));
    }
    Ok(usize::try_from(ret).unwrap_or(usize::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sockaddr(family: u16, path: &[u8]) -> std::vec::Vec<u8> {
        let mut v = family.to_ne_bytes().to_vec();
        v.extend_from_slice(path);
        v
    }

    #[test]
    fn a_path_is_taken_as_far_as_its_nul() {
        let raw = sockaddr(AF_UNIX as u16, b"/run/syslog\0junk");
        let name = parse_name(&raw, b"/").unwrap().unwrap();
        assert_eq!(name.as_bytes(), b"/run/syslog");
        assert_eq!(name.flag(), 0);
    }

    #[test]
    fn a_path_without_a_nul_ends_at_addrlen() {
        let raw = sockaddr(AF_UNIX as u16, b"/tmp/s");
        assert_eq!(
            parse_name(&raw, b"/").unwrap().unwrap().as_bytes(),
            b"/tmp/s"
        );
    }

    #[test]
    fn a_relative_path_is_resolved_against_the_working_directory() {
        let raw = sockaddr(AF_UNIX as u16, b"sock\0");
        let name = parse_name(&raw, b"/home/ann").unwrap().unwrap();
        assert_eq!(name.as_bytes(), b"/home/ann/sock");
        let raw = sockaddr(AF_UNIX as u16, b"../b/./s\0");
        assert_eq!(
            parse_name(&raw, b"/home/ann").unwrap().unwrap().as_bytes(),
            b"/home/b/s"
        );
    }

    #[test]
    fn a_resolved_path_longer_than_sun_path_is_enametoolong() {
        let deep = [b'd'; 100];
        let mut cwd = b"/".to_vec();
        cwd.extend_from_slice(&deep);
        let raw = sockaddr(AF_UNIX as u16, b"socket-name\0");
        assert_eq!(parse_name(&raw, &cwd).err(), Some(errno::ENAMETOOLONG));
    }

    #[test]
    fn an_abstract_name_is_every_byte_after_the_leading_nul() {
        let raw = sockaddr(AF_UNIX as u16, b"\0a\0b");
        let name = parse_name(&raw, b"/").unwrap().unwrap();
        assert_eq!(name.as_bytes(), b"a\0b");
        assert_eq!(name.flag(), UNIX_NAME_ABSTRACT);
    }

    #[test]
    fn the_family_alone_is_an_autobind_and_a_bare_nul_is_refused() {
        assert!(
            parse_name(&sockaddr(AF_UNIX as u16, b""), b"/")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            parse_name(&sockaddr(AF_UNIX as u16, b"\0"), b"/").err(),
            Some(errno::EINVAL)
        );
    }

    #[test]
    fn another_family_or_too_much_is_einval() {
        assert_eq!(
            parse_name(&sockaddr(2, b"/x\0"), b"/").err(),
            Some(errno::EINVAL)
        );
        assert_eq!(parse_name(&[1u8], b"/").err(), Some(errno::EINVAL));
        let long = vec![b'a'; SUN_PATH_LEN + 1];
        assert_eq!(
            parse_name(&sockaddr(AF_UNIX as u16, &long), b"/").err(),
            Some(errno::EINVAL)
        );
    }

    fn record(kind: u32, bytes: &[u8]) -> [u8; UNIX_ADDR_LEN] {
        let mut rec = [0u8; UNIX_ADDR_LEN];
        rec[..4].copy_from_slice(&kind.to_le_bytes());
        rec[4..8].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        rec[8..8 + bytes.len()].copy_from_slice(bytes);
        rec
    }

    #[test]
    fn addresses_come_back_as_linux_writes_them() {
        let (sun, len) = sockaddr_of(&record(0, b""));
        assert_eq!(len, 2);
        assert_eq!(&sun[..2], &(AF_UNIX as u16).to_ne_bytes());
        let (sun, len) = sockaddr_of(&record(1, b"/run/s"));
        assert_eq!(len, 2 + 6 + 1);
        assert_eq!(&sun[2..9], b"/run/s\0");
        let (sun, len) = sockaddr_of(&record(2, b"x1"));
        assert_eq!(len, 2 + 1 + 2);
        assert_eq!(&sun[2..5], b"\0x1");
        let long = [b'p'; SUN_PATH_LEN];
        let (_, len) = sockaddr_of(&record(1, &long));
        assert_eq!(
            len, SOCKADDR_UN_LEN,
            "a full sun_path has no room for its NUL"
        );
    }

    #[test]
    fn store_address_cuts_to_the_room_and_reports_the_whole_length() {
        let rec = record(1, b"/run/socket");
        let mut out = [0xAAu8; 8];
        let mut len: SocklenT = 8;
        // SAFETY: `out` is writable for the 8 bytes `len` says, and `len`
        // is a live local.
        unsafe { store_address(&rec, out.as_mut_ptr().cast(), &raw mut len) };
        assert_eq!(len, 2 + 11 + 1);
        assert_eq!(&out[2..8], b"/run/s");
    }

    /// Who may state which credentials: only what the receiver will be told
    /// -- the sender's own pid and real ids -- not its effective, saved or
    /// filesystem ids, and nothing else for root
    /// (`known-issues/D-POSIX-STATED-CREDENTIALS-ARE-ONLY-THE-REAL-ONES.md`).
    #[test]
    fn a_sender_may_state_only_what_the_receiver_is_told() {
        let ids = crate::unistd::Ids {
            uid: [1000, 1002, 1001, 1004],
            gid: [100, 101, 102, 104],
        };
        let me = 77;
        assert!(may_state(me, &ids, (me, 1000, 100)), "the real ones");
        assert!(
            !may_state(me, &ids, (me, 1002, 101)),
            "not the effective ones"
        );
        assert!(!may_state(me, &ids, (me, 1001, 102)), "not the saved ones");
        assert!(!may_state(me, &ids, (me, 1004, 104)), "nor the fs ones");
        assert!(!may_state(me, &ids, (me, 1002, 100)), "an effective uid");
        assert!(!may_state(me, &ids, (me, 1001, 100)), "a saved uid");
        assert!(!may_state(me, &ids, (me, 1000, 101)), "a gid not the real");
        assert!(!may_state(me, &ids, (78, 1000, 100)), "another pid");
        let root = crate::unistd::Ids {
            uid: [1000, 0, 1000, 0],
            gid: [100, 100, 100, 100],
        };
        assert!(!may_state(me, &root, (5, 6, 7)), "root states no other");
        assert!(may_state(me, &root, (me, 1000, 100)));
    }

    /// A received message's sender without a name is no address at all, as
    /// Linux's `unix_copy_addr` leaves it: nothing written, length 0. One
    /// with a name is written as `getsockname` writes it.
    #[test]
    fn an_unnamed_sender_is_length_zero() {
        let mut out = [0xAAu8; 8];
        let mut len: SocklenT = 8;
        // SAFETY: `out` is writable for the 8 bytes `len` says, and `len`
        // is a live local.
        unsafe { store_sender(&record(0, b""), out.as_mut_ptr().cast(), &raw mut len) };
        assert_eq!(len, 0);
        assert_eq!(out, [0xAA; 8], "nothing written");
        let mut len: SocklenT = 8;
        // SAFETY: as above.
        unsafe { store_sender(&record(1, b"/a"), out.as_mut_ptr().cast(), &raw mut len) };
        assert_eq!(len, 2 + 2 + 1);
        assert_eq!(&out[2..5], b"/a\0");
        // getsockname's unnamed socket is still the family alone.
        let (_, len) = sockaddr_of(&record(0, b""));
        assert_eq!(len, 2);
    }

    #[test]
    fn autobind_names_are_five_lowercase_hex_digits() {
        assert_eq!(&autobind_name(0), b"00000");
        assert_eq!(&autobind_name(0xABCDE), b"abcde");
        assert_eq!(&autobind_name(0x1_2345), b"12345");
    }

    #[test]
    fn a_kernel_without_unix_sockets_answers_as_one_without_the_family() {
        // No such call: the family is absent -- EAFNOSUPPORT for `socket`,
        // EOPNOTSUPP for a datagram pair -- never ENOSYS, which a program
        // would read as "present and broken".
        assert_eq!(
            creation_errno(native::NO_SUCH_SYSCALL, errno::EAFNOSUPPORT),
            errno::EAFNOSUPPORT
        );
        assert_eq!(
            creation_errno(native::NO_SUCH_SYSCALL, errno::EOPNOTSUPP),
            errno::EOPNOTSUPP
        );
        // Any other answer is the call's own.
        assert_eq!(
            creation_errno(native::CONNECT_ALREADY, errno::EAFNOSUPPORT),
            errno::EISCONN
        );
        assert_eq!(
            creation_errno(native::NOT_SUPPORTED, errno::EAFNOSUPPORT),
            errno::EOPNOTSUPP
        );
    }

    #[test]
    fn this_familys_two_errors_differ_from_the_shared_table() {
        assert_eq!(errno_of(native::CONNECT_ALREADY), errno::EISCONN);
        assert_eq!(errno_of(native::CHANNEL_CLOSED), errno::EPIPE);
        assert_eq!(errno_of(native::BROKEN_PIPE), errno::EPIPE);
        assert_eq!(errno_of(native::ADDR_IN_USE), errno::EADDRINUSE);
        assert_eq!(errno_of(native::WRONG_SOCKET_TYPE), errno::EPROTOTYPE);
    }

    #[test]
    fn socket_types_map_to_the_kernels_kinds() {
        assert_eq!(kernel_kind(SOCK_STREAM), Some(1));
        assert_eq!(kernel_kind(SOCK_DGRAM), Some(2));
        assert_eq!(kernel_kind(SOCK_SEQPACKET), Some(5));
        assert_eq!(
            kernel_kind(SOCK_RAW),
            Some(SOCK_DGRAM),
            "Linux makes a raw Unix socket a datagram one"
        );
        assert_eq!(kernel_kind(4), None);
    }

    #[test]
    fn timevals_and_the_kernels_nanoseconds() {
        assert_eq!(timeval_to_ns(0, 0), Ok(0));
        assert_eq!(timeval_to_ns(2, 500_000), Ok(2_500_000_000));
        assert_eq!(
            timeval_to_ns(-1, 0),
            Ok(0),
            "a negative time is no limit, as Linux takes it"
        );
        assert_eq!(timeval_to_ns(1, 1_000_000), Err(errno::EDOM));
        assert_eq!(timeval_to_ns(1, -1), Err(errno::EDOM));
        assert_eq!(ns_to_timeval(2_500_000_000), (2, 500_000));
        assert_eq!(ns_to_timeval(0), (0, 0));
    }

    #[test]
    fn credentials_are_the_senders_or_linuxs_unknown_ones() {
        let mut info = [0u8; UNIX_RECV_INFO_LEN];
        assert_eq!(credentials_of(&info), (0, OVERFLOW_ID, OVERFLOW_ID));
        info[8..16].copy_from_slice(&1234u64.to_le_bytes());
        info[16..20].copy_from_slice(&1000u32.to_le_bytes());
        info[20..24].copy_from_slice(&100u32.to_le_bytes());
        info[24..28].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(credentials_of(&info), (1234, 1000, 100));
    }

    /// A `msghdr` with `control` as its control buffer and nothing else.
    fn with_control(control: *mut u8, controllen: usize) -> Msghdr {
        Msghdr {
            msg_name: core::ptr::null_mut(),
            msg_namelen: 0,
            msg_iov: core::ptr::null_mut(),
            msg_iovlen: 0,
            msg_control: control,
            msg_controllen: controllen,
            msg_flags: 0,
        }
    }

    #[test]
    fn credentials_are_written_and_truncated_as_linuxs_put_cmsg_does() {
        let mut info = [0u8; UNIX_RECV_INFO_LEN];
        info[8..16].copy_from_slice(&77u64.to_le_bytes());
        info[16..20].copy_from_slice(&1000u32.to_le_bytes());
        info[20..24].copy_from_slice(&100u32.to_le_bytes());
        info[24..28].copy_from_slice(&1u32.to_le_bytes());
        let word = |b: &[u8], at: usize| u32::from_ne_bytes(b[at..at + 4].try_into().unwrap());
        let mut buf = [0xAAu8; 64];

        // Not asked for: nothing, and no MSG_CTRUNC.
        let msg = with_control(buf.as_mut_ptr(), 64);
        // SAFETY: the buffer is 64 bytes.
        assert_eq!(unsafe { write_credentials(&msg, false, &info) }, (0, false));
        assert!(buf.iter().all(|&b| b == 0xAA));

        // No buffer at all: MSG_CTRUNC (scm_recv).
        let msg = with_control(core::ptr::null_mut(), 0);
        // SAFETY: nothing is written to a null buffer.
        assert_eq!(unsafe { write_credentials(&msg, true, &info) }, (0, true));

        // Smaller than a header: nothing, and no MSG_CTRUNC (put_cmsg).
        let msg = with_control(buf.as_mut_ptr(), 15);
        // SAFETY: the buffer is 64 bytes.
        assert_eq!(unsafe { write_credentials(&msg, true, &info) }, (0, false));
        assert!(buf.iter().all(|&b| b == 0xAA));

        // A header and part of the credentials: as much as fits, cmsg_len
        // the room, MSG_CTRUNC.
        let msg = with_control(buf.as_mut_ptr(), 20);
        // SAFETY: the buffer is 64 bytes.
        assert_eq!(unsafe { write_credentials(&msg, true, &info) }, (20, true));
        assert_eq!(usize::from_ne_bytes(buf[..8].try_into().unwrap()), 20);
        assert_eq!(word(&buf, 16), 77);
        assert_eq!(buf[20], 0xAA, "nothing past the room");

        // Room for all of it: the whole message, msg_controllen its padded
        // size (CMSG_SPACE of a ucred, 32), or the room when that is less.
        buf.fill(0xAA);
        let msg = with_control(buf.as_mut_ptr(), 64);
        // SAFETY: the buffer is 64 bytes.
        assert_eq!(unsafe { write_credentials(&msg, true, &info) }, (32, false));
        assert_eq!(usize::from_ne_bytes(buf[..8].try_into().unwrap()), 28);
        assert_eq!(word(&buf, 8) as i32, SOL_SOCKET);
        assert_eq!(word(&buf, 12) as i32, SCM_CREDENTIALS);
        assert_eq!(
            (word(&buf, 16), word(&buf, 20), word(&buf, 24)),
            (77, 1000, 100)
        );
        let msg = with_control(buf.as_mut_ptr(), 30);
        // SAFETY: the buffer is 64 bytes.
        assert_eq!(unsafe { write_credentials(&msg, true, &info) }, (30, false));
    }

    /// An `SCM_RIGHTS` message's bytes: the header, then the descriptors.
    fn rights_message(fds: &[i32]) -> std::vec::Vec<u8> {
        let len = CMSG_HDR + 4 * fds.len();
        let mut m = len.to_ne_bytes().to_vec();
        m.extend_from_slice(&SOL_SOCKET.to_ne_bytes());
        m.extend_from_slice(&SCM_RIGHTS.to_ne_bytes());
        for fd in fds {
            m.extend_from_slice(&fd.to_ne_bytes());
        }
        m.resize(cmsg_align(len), 0);
        m
    }

    /// A send's `SCM_RIGHTS` messages become one list of records, in order --
    /// handle, type code and status flags -- every message's appended, as
    /// Linux's `scm_fp_copy` appends them; a descriptor not open is `EBADF`,
    /// one of a kind that cannot travel `EOPNOTSUPP`, more than 253 in all
    /// `EINVAL`.
    #[test]
    fn a_sends_descriptors_are_gathered_as_records() {
        use crate::fcntl::{O_APPEND, O_NONBLOCK, O_RDONLY, O_WRONLY};
        let file =
            fdtable::alloc_fd_with_flags(HandleKind::File, 0x51, O_WRONLY | O_APPEND).unwrap();
        let pipe =
            fdtable::alloc_fd_with_flags(HandleKind::Pipe, 0x52, O_RDONLY | O_NONBLOCK).unwrap();
        let epoll = fdtable::alloc_fd_with_flags(HandleKind::Epoll, 0x53, O_RDONLY).unwrap();
        let mut control = rights_message(&[file, pipe]);
        control.extend(rights_message(&[file]));
        let msg = with_control(control.as_mut_ptr(), control.len());
        let mut rights = [MaybeUninit::<Right>::uninit(); SCM_MAX_FD];
        // SAFETY: the control buffer is `control`.
        assert_eq!(unsafe { check_control(&msg, None) }, Ok(3), "counted");
        // SAFETY: as above.
        assert_eq!(unsafe { check_control(&msg, Some(&mut rights)) }, Ok(3));
        // SAFETY: those three were written.
        let records = unsafe { written(&rights, 3) };
        assert_eq!(decode_right(&records[0]), (0x51, 0, O_WRONLY | O_APPEND));
        assert_eq!(decode_right(&records[1]), (0x52, 1, O_RDONLY | O_NONBLOCK));
        assert_eq!(decode_right(&records[2]), (0x51, 0, O_WRONLY | O_APPEND));

        let mut control = rights_message(&[file, 9_999]);
        let msg = with_control(control.as_mut_ptr(), control.len());
        // SAFETY: as above.
        assert_eq!(
            unsafe { check_control(&msg, Some(&mut rights)) },
            Err(errno::EBADF)
        );
        let mut control = rights_message(&[epoll]);
        let msg = with_control(control.as_mut_ptr(), control.len());
        // SAFETY: as above.
        assert_eq!(
            unsafe { check_control(&msg, Some(&mut rights)) },
            Err(errno::EOPNOTSUPP)
        );
        let mut control = rights_message(&[file; 200]);
        control.extend(rights_message(&[file; 54]));
        let msg = with_control(control.as_mut_ptr(), control.len());
        // SAFETY: as above.
        assert_eq!(
            unsafe { check_control(&msg, Some(&mut rights)) },
            Err(errno::EINVAL)
        );
        let mut control = rights_message(&[]);
        let msg = with_control(control.as_mut_ptr(), control.len());
        // SAFETY: as above.
        assert_eq!(
            unsafe { check_control(&msg, Some(&mut rights)) },
            Ok(0),
            "none is none"
        );

        for fd in [file, pipe, epoll] {
            assert!(fdtable::close_fd(fd).is_some());
        }
    }

    /// Only an `SCM_RIGHTS` message makes a send gather records; the scan
    /// reads headers alone, and stops at one the check refuses.
    #[test]
    fn a_send_carries_rights_only_with_an_scm_rights_message() {
        let mut rights = rights_message(&[0]);
        let msg = with_control(rights.as_mut_ptr(), rights.len());
        // SAFETY: the control buffer is `rights`.
        assert!(unsafe { carries_rights(&msg) });
        let mut both = std::vec![0u8; CMSG_HDR + 16];
        both[..8].copy_from_slice(&(CMSG_HDR + UCRED_LEN).to_ne_bytes());
        both[8..12].copy_from_slice(&SOL_SOCKET.to_ne_bytes());
        both[12..16].copy_from_slice(&SCM_CREDENTIALS.to_ne_bytes());
        let msg = with_control(both.as_mut_ptr(), both.len());
        // SAFETY: the control buffer is `both`.
        assert!(!unsafe { carries_rights(&msg) }, "credentials alone");
        both.extend(rights_message(&[0]));
        let msg = with_control(both.as_mut_ptr(), both.len());
        // SAFETY: as above.
        assert!(unsafe { carries_rights(&msg) }, "credentials, then rights");
        let msg = with_control(core::ptr::null_mut(), 64);
        // SAFETY: no buffer is read.
        assert!(!unsafe { carries_rights(&msg) }, "no control data");
        let mut short = rights_message(&[0]);
        short[..8].copy_from_slice(&4usize.to_ne_bytes());
        let msg = with_control(short.as_mut_ptr(), short.len());
        // SAFETY: as above.
        assert!(!unsafe { carries_rights(&msg) }, "a header too short");
    }

    /// What a receive's control buffer has room for: the descriptors that
    /// fit after the header and, when asked for, the credentials.
    #[test]
    fn the_room_for_descriptors_is_what_follows_the_credentials() {
        let mut buf = [0u8; 256];
        let mut room = |len: usize, passcred: bool| {
            let msg = with_control(buf.as_mut_ptr(), len);
            rights_room(&msg, passcred)
        };
        assert_eq!(room(0, false), 0);
        assert_eq!(room(16, false), 0, "a header and no int");
        assert_eq!(room(20, false), 1);
        assert_eq!(room(24, false), 2);
        assert_eq!(room(52, true), 1, "32 for the credentials, then 16 and 4");
        assert_eq!(room(20, true), 0, "the credentials take it all");
        let msg = with_control(core::ptr::null_mut(), 64);
        assert_eq!(rights_room(&msg, false), 0, "no buffer");
        let mut big = std::vec![0u8; 16 + 4 * 300];
        let msg = with_control(big.as_mut_ptr(), big.len());
        assert_eq!(rights_room(&msg, false), SCM_MAX_FD);
    }

    /// The descriptors a receive brought become descriptors here, each of
    /// its kind with its handle and status flags, and one `SCM_RIGHTS`
    /// message lists them after the credentials; `MSG_CMSG_CLOEXEC` marks
    /// them close-on-exec, a socket is a stream's until the kernel can say,
    /// and what the kernel released is reported as cut.
    #[test]
    fn received_descriptors_are_installed_and_listed() {
        use crate::fcntl::{O_RDONLY, O_RDWR, O_WRONLY};
        let record = |handle: u64, code: u32, status: i32| {
            let mut r = [0u8; UNIX_RIGHT_LEN];
            r[..8].copy_from_slice(&handle.to_le_bytes());
            r[8..12].copy_from_slice(&code.to_le_bytes());
            r[12..].copy_from_slice(&status.cast_unsigned().to_le_bytes());
            r
        };
        let mut recs = [MaybeUninit::<Right>::uninit(); 8];
        recs[0] = MaybeUninit::new(record(0x61, 0, O_RDONLY));
        recs[1] = MaybeUninit::new(record(0x62, 1, O_WRONLY));
        recs[2] = MaybeUninit::new(record(0x63, 8, O_RDWR));
        let mut rights = RightsIn::new(&mut recs);
        rights.got = 3;
        rights.cut = true;
        let mut buf = [0xAAu8; 64];
        let msg = with_control(buf.as_mut_ptr(), 64);
        // SAFETY: the buffer is 64 bytes; the records are made up, and
        // nothing here asks the kernel about them.
        let (written, cut) = unsafe { land_rights(&msg, &rights, 8, true) };
        assert_eq!((written, cut), (8 + 32, true), "16 and three ints, padded");
        let word = |at: usize| i32::from_ne_bytes(buf[at..at + 4].try_into().unwrap());
        assert_eq!(usize::from_ne_bytes(buf[8..16].try_into().unwrap()), 28);
        assert_eq!((word(16), word(20)), (SOL_SOCKET, SCM_RIGHTS));
        let fds = [word(24), word(28), word(32)];
        let want = [
            (HandleKind::File, 0x61, O_RDONLY),
            (HandleKind::Pipe, 0x62, O_WRONLY),
            (HandleKind::UnixSocket, 0x63, O_RDWR),
        ];
        for (fd, (kind, handle, status)) in fds.into_iter().zip(want) {
            let entry = fdtable::get_fd(fd).unwrap();
            assert_eq!((entry.kind, entry.handle), (kind, handle));
            assert_eq!(fdtable::get_status_flags(fd), Some(status));
            assert_eq!(fdtable::get_fd_flags(fd), Some(fdtable::FD_CLOEXEC));
        }
        assert_eq!(sock_type(fds[2]), SOCK_STREAM);
        assert!(
            !crate::socket::get_meta(fds[2]).is_some_and(|m| m.unix_listening),
            "the host's kernel counts its bytes"
        );
        crate::socket::clear_meta(fds[2]);
        for fd in fds {
            assert!(fdtable::close_fd(fd).is_some());
        }

        // Nothing came: nothing written, and only the kernel's word on cuts.
        let mut room = [MaybeUninit::<Right>::uninit(); 8];
        let none = RightsIn::new(&mut room);
        // SAFETY: as above.
        assert_eq!(unsafe { land_rights(&msg, &none, 8, false) }, (8, false));
    }

    /// A Unix socket travels only as a stream: a datagram or
    /// sequenced-packet one's receiver could not learn its type, and would
    /// drive it as a stream's.
    #[test]
    fn only_a_stream_socket_travels() {
        let stream = fdtable::alloc_fd(HandleKind::UnixSocket, 0x71).unwrap();
        crate::socket::set_unix_meta(stream, SOCK_STREAM);
        let (_, code, _) = decode_right(&right_of(stream).unwrap());
        assert_eq!(code, 8);
        for kind in [SOCK_DGRAM, SOCK_SEQPACKET] {
            let fd = fdtable::alloc_fd(HandleKind::UnixSocket, 0x72).unwrap();
            crate::socket::set_unix_meta(fd, kind);
            assert_eq!(right_of(fd), Err(errno::EOPNOTSUPP), "type {kind}");
            crate::socket::clear_meta(fd);
            assert!(fdtable::close_fd(fd).is_some());
        }
        crate::socket::clear_meta(stream);
        assert!(fdtable::close_fd(stream).is_some());
    }

    /// What a receive brought, as a stream's receive reads it to decide
    /// whether to go on: descriptors taken, or only released -- with room
    /// or without; and never more records than its room.
    #[test]
    fn a_receive_brought_descriptors_when_it_took_or_released_some() {
        assert!(!descriptors_came(0, 0));
        assert!(descriptors_came(0, UNIX_MSG_CTRUNC), "released, no room");
        assert!(descriptors_came(2, 0), "taken");
        assert!(!descriptors_came(0, !UNIX_MSG_CTRUNC), "another flag");
        let mut none = RightsIn::new(&mut []);
        none.got = 5;
        assert!(none.taken().is_empty(), "never more than its room");
    }
}

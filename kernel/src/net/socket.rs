//! AF_INET / AF_INET6 SOCK_STREAM socket objects backed by the userspace
//! `net.stack` daemon.
//!
//! This is the object layer for the Path B userspace-netstack cutover
//! (`design-decisions.md` §63/§66, migration increment 5.5). Before this, the
//! Linux socket syscalls (`sys_socket`/`connect`/`sendto`/`recvfrom`/… in
//! `kernel/src/syscall/linux.rs`) were pure errno-gating stubs that returned
//! `ENOSYS` — there was no AF_INET socket object at all. This module gives the
//! Linux ABI its first real stream socket: a kernel-side handle that wraps a
//! [`NetstackConn`] (one SHM ring + one daemon TCP connection) and is driven
//! `socket()` → `connect()` → `send/recv` → `close()`.
//!
//! ## Switch-gated
//!
//! Socket creation is only offered when the `net.userspace` boot switch is set
//! ([`crate::net::netstack_client::userspace_enabled`]); otherwise the syscall
//! layer keeps returning `ENOSYS` and the in-kernel resident stack stays
//! authoritative (staged cutover, §66 Q22b).
//!
//! ## Lock discipline
//!
//! A socket's daemon operations (`connect`/`send`/`recv`, and teardown) block on
//! the wire while the daemon drains the ring. We must therefore **never** hold
//! the global [`SOCKET_TABLE`] lock across one. Each slot's mutable state lives
//! behind its own `Arc<Mutex<SocketInner>>`; an operation clones the `Arc` under
//! a brief table-lock, releases the table lock, then takes the per-socket lock
//! for the (possibly blocking) round-trip. The per-socket lock only serializes
//! operations on the *same* socket, which is the correct semantics (a stream
//! socket has one send and one receive side). `close()` likewise removes the
//! slot under the table lock but performs the final `Arc` drop — which may run
//! [`NetstackConn`] teardown (a blocking daemon round-trip) — *after* releasing
//! it.
//!
//! ## Refcounting
//!
//! `dup`/`dup2`/`fork`/`pidfd_getfd` share one underlying socket (Linux
//! open-file-description semantics). Each fd reference is one `refcount` on the
//! slot; the slot (and its `NetstackConn`) is dropped only when the last fd
//! closes — mirroring the `memfd`/`eventfd`/`pipe` dup pattern.

use crate::error::{KernelError, KernelResult};
use crate::net::netstack_client::NetstackConn;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

/// Unique per-socket id (also the value stored in `FdEntry::raw_handle`).
type SocketId = u64;

/// Counter for generating unique socket ids. Starts at 1 so 0 is never a valid
/// handle.
static NEXT_SOCKET_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_socket_id() -> SocketId {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed)
}

/// Opaque handle to a stream socket. Stored as `FdEntry::raw_handle` (a `u64`)
/// by the Linux fd-table dispatch layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SocketHandle(u64);

impl SocketHandle {
    /// Reconstruct from the raw `u64` stored in the fd table.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Raw `u64` for storage in `FdEntry::raw_handle`.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    fn id(self) -> SocketId {
        self.0
    }
}

/// Connection lifecycle of a stream socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SockState {
    /// Created (`socket()`) but not yet connected. No daemon session exists.
    Created,
    /// A **non-blocking** `connect()` has started (SYN sent) but the TCP handshake
    /// has not yet completed. The daemon holds a half-open connection; the socket
    /// resolves to [`Connected`](SockState::Connected) or
    /// [`Failed`](SockState::Failed) on a later [`poll_ready`]. Linux reports this
    /// state's `connect` as `EINPROGRESS` and a repeated `connect` as `EALREADY`.
    Connecting,
    /// A `connect()` succeeded; the daemon holds a live TCP connection.
    Connected,
    /// A prior `connect()` failed, or the peer/connection is gone. Terminal for
    /// this handle (Linux would require a fresh socket to retry).
    Failed,
}

/// The socket's transport type. A stream socket (`SOCK_STREAM`) is driven by the
/// connect→send/recv→close TCP lifecycle above; a datagram socket (`SOCK_DGRAM`)
/// is connectionless — it `bind`s a local port and exchanges datagrams with
/// explicit peer addresses via [`dgram_send_to`]/[`dgram_recv_from`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SockKind {
    /// `SOCK_STREAM` — a daemon-backed TCP connection.
    Stream,
    /// `SOCK_DGRAM` — a daemon-backed connectionless UDP socket.
    Dgram,
}

/// Outcome of a [`connect`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectOutcome {
    /// The connection is established (a blocking connect, or a non-blocking connect
    /// whose handshake completed synchronously). Maps to a `0` `connect` return.
    Established,
    /// A non-blocking connect started but the handshake is still pending. Maps to
    /// `EINPROGRESS`; the caller polls for `POLLOUT` then checks `SO_ERROR`.
    InProgress,
}

/// Per-socket mutable state. Guarded by its own mutex (see module lock
/// discipline) so a blocking daemon round-trip never holds the table lock.
struct SocketInner {
    /// The daemon-backed connection driving this socket.
    conn: NetstackConn,
    /// Stream vs datagram transport.
    kind: SockKind,
    /// Datagram sockets only: whether the daemon-side UDP socket has been bound
    /// yet (an explicit `bind(2)` or the implicit ephemeral auto-bind on the
    /// first `sendto`/`recvfrom`, matching Linux). Meaningless for a stream socket.
    bound: bool,
    /// Datagram sockets only: the bound local port (for `getsockname`). `0` until
    /// bound.
    local_port: u16,
    /// Connection lifecycle state.
    state: SockState,
    /// Remembered peer address (for `getpeername`), set on a successful connect.
    peer_ip: [u8; 4],
    /// Remembered IPv6 peer address (for `getpeername` on an `AF_INET6` socket),
    /// set on a successful [`connect6`]. `None` for an IPv4 socket.
    peer_ip6: Option<[u8; 16]>,
    /// Remembered peer port.
    peer_port: u16,
    /// Whether the latched `SO_ERROR` has already been consumed by a
    /// `getsockopt(SO_ERROR)` read. `SO_ERROR` is one-shot in Linux: the first read
    /// after a failed connect returns the errno, subsequent reads return `0`.
    so_error_read: bool,
}

/// One entry in the global socket table: the shared per-socket state plus the
/// fd reference count.
struct SocketSlot {
    inner: Arc<Mutex<SocketInner>>,
    /// Number of fds referencing this socket (dup/fork bump; close drops).
    refcount: u32,
}

/// Global table of live stream sockets.
static SOCKET_TABLE: Mutex<BTreeMap<SocketId, SocketSlot>> = Mutex::new(BTreeMap::new());

/// Create a new (unconnected) stream socket.
///
/// Allocates the SHM ring backing the daemon connection but does **not** contact
/// the daemon yet — that happens on [`connect`]. The `O_NONBLOCK` flag is *not*
/// tracked here: it lives authoritatively in the Linux fd table's status flags
/// (`fcntl(F_GETFL/F_SETFL)`). It now alters *receive* behaviour — a nonblocking
/// `recv`/`read` returns `EAGAIN` rather than blocking (see [`recv`]) — but
/// `connect`/`send` are still synchronous (see known-issues D-NETSOCK-SYNC).
///
/// # Errors
///
/// Propagates the [`NetstackConn::open`] error if the SHM ring cannot be
/// allocated/initialised.
pub fn create() -> KernelResult<SocketHandle> {
    create_kind(SockKind::Stream)
}

/// Create a new (unbound) connectionless datagram (`SOCK_DGRAM`) socket.
///
/// Allocates the SHM ring backing the daemon connection but does **not** contact
/// the daemon yet — the daemon-side UDP socket is created on the first `bind(2)`
/// (explicit) or on the first `sendto`/`recvfrom` (implicit ephemeral auto-bind,
/// matching Linux).
///
/// # Errors
///
/// Propagates the [`NetstackConn::open`] error if the SHM ring cannot be
/// allocated/initialised.
pub fn create_dgram() -> KernelResult<SocketHandle> {
    create_kind(SockKind::Dgram)
}

/// Shared constructor for [`create`] (stream) and [`create_dgram`] (datagram).
fn create_kind(kind: SockKind) -> KernelResult<SocketHandle> {
    let conn = NetstackConn::open()?;
    let inner = SocketInner {
        conn,
        kind,
        bound: false,
        local_port: 0,
        state: SockState::Created,
        peer_ip: [0; 4],
        peer_ip6: None,
        peer_port: 0,
        so_error_read: false,
    };
    let id = alloc_socket_id();
    let slot = SocketSlot {
        inner: Arc::new(Mutex::new(inner)),
        refcount: 1,
    };
    SOCKET_TABLE.lock().insert(id, slot);
    Ok(SocketHandle(id))
}

/// Add one fd reference to a socket (fork / dup / pidfd_getfd). Returns the same
/// handle.
///
/// # Errors
///
/// - `InvalidHandle` — the handle is not in the table (already fully closed) or
///   the refcount would overflow.
pub fn dup(handle: SocketHandle) -> KernelResult<SocketHandle> {
    let mut table = SOCKET_TABLE.lock();
    let slot = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    slot.refcount = slot
        .refcount
        .checked_add(1)
        .ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// Drop one fd reference. When the last reference goes, the slot is removed and
/// its [`NetstackConn`] torn down.
///
/// The final `Arc` drop (which may run a blocking daemon teardown round-trip) is
/// deliberately performed **after** releasing the table lock.
pub fn close(handle: SocketHandle) {
    let removed = {
        let mut table = SOCKET_TABLE.lock();
        match table.get_mut(&handle.id()) {
            Some(slot) => {
                slot.refcount = slot.refcount.saturating_sub(1);
                if slot.refcount == 0 {
                    table.remove(&handle.id())
                } else {
                    None
                }
            }
            None => None,
        }
    };
    // `removed` drops here, outside the table lock. If it held the last `Arc`
    // reference, `NetstackConn`'s Drop runs the (blocking) session teardown now —
    // safely, with no lock held.
    drop(removed);
}

/// Look up a socket's shared state, cloning the `Arc` under a brief table lock.
fn inner_of(handle: SocketHandle) -> KernelResult<Arc<Mutex<SocketInner>>> {
    let table = SOCKET_TABLE.lock();
    let slot = table.get(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    Ok(slot.inner.clone())
}

/// Connect the socket to `ip:port` via the daemon.
///
/// When `nonblock` is clear, this is a **blocking** connect: it returns
/// [`ConnectOutcome::Established`] on success, or maps a daemon "no upstream" /
/// refused result to `ECONNREFUSED` (marking the socket `Failed`).
///
/// When `nonblock` is set, the connect is issued non-blocking: if the handshake
/// completes synchronously (fast/loopback peer) it returns
/// [`ConnectOutcome::Established`]; otherwise it returns
/// [`ConnectOutcome::InProgress`] (the socket enters [`SockState::Connecting`]) and
/// the caller polls for `POLLOUT` then checks `SO_ERROR` ([`take_so_error`]).
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `AlreadyExists` — the socket is already connected (Linux `EISCONN`).
/// - `ConnectAlready` — a non-blocking connect is already in progress (Linux
///   `EALREADY`).
/// - `ConnectionRefused` — the daemon could not establish the connection.
/// - protocol faults propagated from [`NetstackConn::connect`].
pub fn connect(
    handle: SocketHandle,
    ip: &[u8; 4],
    port: u16,
    nonblock: bool,
) -> KernelResult<ConnectOutcome> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    match guard.state {
        SockState::Connected => return Err(KernelError::AlreadyExists), // EISCONN
        SockState::Connecting => return Err(KernelError::ConnectAlready), // EALREADY
        _ => {}
    }
    let res = guard.conn.connect(ip, port, nonblock)?;
    if res == netipc::ring::ERR_IN_PROGRESS {
        // Non-blocking handshake pending: remember the peer now so getpeername works
        // once it resolves, and enter Connecting.
        guard.state = SockState::Connecting;
        guard.peer_ip = *ip;
        guard.peer_port = port;
        return Ok(ConnectOutcome::InProgress);
    }
    if res < 0 {
        guard.state = SockState::Failed;
        return Err(KernelError::ConnectionRefused);
    }
    guard.state = SockState::Connected;
    guard.peer_ip = *ip;
    guard.peer_port = port;
    Ok(ConnectOutcome::Established)
}

/// Connect an `AF_INET6` socket to `ip6:port` via the daemon.
///
/// IPv6 sibling of [`connect`]: identical lifecycle/outcome semantics, but drives
/// [`NetstackConn::connect6`] (which carries the 16-byte peer address in the ring
/// data window) and remembers the peer in `peer_ip6` for `getpeername`.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `AlreadyExists` — the socket is already connected (Linux `EISCONN`).
/// - `ConnectAlready` — a non-blocking connect is already in progress (Linux
///   `EALREADY`).
/// - `ConnectionRefused` — the daemon could not establish the connection.
/// - protocol faults propagated from [`NetstackConn::connect6`].
pub fn connect6(
    handle: SocketHandle,
    ip6: &[u8; 16],
    port: u16,
    nonblock: bool,
) -> KernelResult<ConnectOutcome> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    match guard.state {
        SockState::Connected => return Err(KernelError::AlreadyExists), // EISCONN
        SockState::Connecting => return Err(KernelError::ConnectAlready), // EALREADY
        _ => {}
    }
    let res = guard.conn.connect6(ip6, port, nonblock)?;
    if res == netipc::ring::ERR_IN_PROGRESS {
        // Non-blocking handshake pending: remember the peer now so getpeername works
        // once it resolves, and enter Connecting.
        guard.state = SockState::Connecting;
        guard.peer_ip6 = Some(*ip6);
        guard.peer_port = port;
        return Ok(ConnectOutcome::InProgress);
    }
    if res < 0 {
        guard.state = SockState::Failed;
        return Err(KernelError::ConnectionRefused);
    }
    guard.state = SockState::Connected;
    guard.peer_ip6 = Some(*ip6);
    guard.peer_port = port;
    Ok(ConnectOutcome::Established)
}

/// Send `buf` on a connected socket. Returns the number of bytes accepted.
///
/// When `nonblock` is set (the fd's `O_NONBLOCK` status flag), a send that would
/// block on a full send window returns [`KernelError::WouldBlock`] (→ `EAGAIN`)
/// instead of waiting for the peer's ACK; otherwise it blocks up to the daemon's
/// send deadline.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `NotConnected` — the socket is not connected (Linux `ENOTCONN`/`EPIPE`).
/// - `WouldBlock` — `nonblock` was set and the window was full (→ `EAGAIN`).
/// - protocol faults propagated from [`NetstackConn::send`].
pub fn send(handle: SocketHandle, buf: &[u8], nonblock: bool) -> KernelResult<i32> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.state != SockState::Connected {
        return Err(KernelError::NotConnected);
    }
    guard.conn.send(buf, nonblock)
}

/// Receive up to `buf.len()` bytes on a connected socket. Returns the number of
/// bytes copied (`0` = peer closed / no data).
///
/// When `nonblock` is set (the fd's `O_NONBLOCK` status flag), a receive with no
/// data ready returns [`KernelError::WouldBlock`] (→ `EAGAIN`) instead of blocking
/// on the daemon; otherwise it blocks up to the daemon's receive deadline.
///
/// When `peek` is set (the caller's `MSG_PEEK`), buffered bytes are copied out
/// without being consumed, so a subsequent `recv` returns the same data.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `NotConnected` — the socket is not connected.
/// - `WouldBlock` — `nonblock` was set and no data was ready.
/// - protocol faults propagated from [`NetstackConn::recv`].
pub fn recv(
    handle: SocketHandle,
    buf: &mut [u8],
    nonblock: bool,
    peek: bool,
) -> KernelResult<i32> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.state != SockState::Connected {
        return Err(KernelError::NotConnected);
    }
    guard.conn.recv(buf, nonblock, peek)
}

/// Non-destructively probe a stream socket's readiness for the poll/epoll engine.
///
/// Returns `(readable, writable, error)`:
/// - A **connected** socket queries the daemon (via [`NetstackConn::poll_ready`])
///   for its honest state: `readable` iff it has buffered bytes or the peer has
///   closed (so a `recv` returns data/EOF promptly), `writable` iff the connection
///   can accept a send. This replaces the former "always ready" placeholder so
///   `POLLIN` no longer spins a poller that then reads `EAGAIN`.
/// - A **connecting** socket (non-blocking connect in flight) queries the daemon
///   and, when the handshake resolves, transitions to `Connected` (writable, no
///   error) or `Failed` (writable **and** error — Linux wakes `POLLOUT`+`POLLERR`
///   for a failed non-blocking connect). While still pending it is neither
///   readable nor writable.
/// - An **unconnected**/`Created` socket is reported writable-only: a `connect` may
///   still proceed, but there is nothing to read.
/// - A **failed** socket is reported writable **and** error (the error is latched
///   until read via [`take_so_error`]).
///
/// Does not consume buffered data — a subsequent [`recv`] still returns it.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - protocol faults propagated from [`NetstackConn::poll_ready`].
pub fn poll_ready(handle: SocketHandle) -> KernelResult<(bool, bool, bool)> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    // A datagram socket has no connection lifecycle: readiness is whatever the
    // daemon-side UDP socket reports (readable iff a datagram is queued, always
    // writable). An *unbound* datagram socket has no daemon UDP socket yet, so it
    // can only be written (an implicit ephemeral bind happens on the first
    // `sendto`); report writable-only, nothing to read, no error.
    if guard.kind == SockKind::Dgram {
        if guard.bound {
            return guard.conn.poll_ready();
        }
        return Ok((false, true, false));
    }
    match guard.state {
        SockState::Connected => guard.conn.poll_ready(),
        SockState::Connecting => {
            let (readable, writable, error) = guard.conn.poll_ready()?;
            // Resolve the pending handshake: an error latches Failed; becoming
            // writable (with no error) means ESTABLISHED. Otherwise stay Connecting.
            if error {
                guard.state = SockState::Failed;
            } else if writable {
                guard.state = SockState::Connected;
            }
            Ok((readable, writable, error))
        }
        // A failed connect: writable + error so poll(POLLOUT) wakes and the caller
        // reads SO_ERROR.
        SockState::Failed => Ok((false, true, true)),
        // Created but never connected: a connect may still proceed (writable),
        // nothing to read, no error.
        SockState::Created => Ok((false, true, false)),
    }
}

/// Read and clear the pending socket error (`getsockopt(SOL_SOCKET, SO_ERROR)`).
///
/// Returns the Linux errno for the socket's current error condition and clears it
/// (a `Failed` socket is left `Failed` — Linux keeps the socket unusable — but
/// SO_ERROR is a one-shot read, so a second call returns `0`):
/// - a `Failed` socket returns `ECONNREFUSED` (111) once, then `0`;
/// - a `Connecting` socket (handshake still pending) returns `0` (no error yet);
/// - any other state returns `0`.
///
/// # Errors
///
/// `InvalidHandle` if the handle has been closed.
pub fn take_so_error(handle: SocketHandle) -> KernelResult<i32> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.state == SockState::Failed && !guard.so_error_read {
        guard.so_error_read = true;
        // ECONNREFUSED — the only failure our synchronous/handshake path surfaces.
        Ok(111)
    } else {
        Ok(0)
    }
}

/// Whether the socket is currently connected.
///
/// # Errors
///
/// `InvalidHandle` if the handle has been closed.
pub fn is_connected(handle: SocketHandle) -> KernelResult<bool> {
    let inner = inner_of(handle)?;
    let guard = inner.lock();
    Ok(guard.state == SockState::Connected)
}

/// The remembered peer address `(ip, port)` set by a successful connect.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `NotConnected` — never successfully connected.
pub fn peer(handle: SocketHandle) -> KernelResult<([u8; 4], u16)> {
    let inner = inner_of(handle)?;
    let guard = inner.lock();
    if guard.state != SockState::Connected {
        return Err(KernelError::NotConnected);
    }
    Ok((guard.peer_ip, guard.peer_port))
}

/// The remembered IPv6 peer address `(ip6, port)` for a socket connected via
/// [`connect6`]. Returns `None` (as `NotConnected` would be wrong here) only when
/// the socket was connected over IPv4 — callers that need `getpeername` on an
/// `AF_INET6` socket use this; an IPv4 socket uses [`peer`].
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `NotConnected` — never successfully connected.
pub fn peer6(handle: SocketHandle) -> KernelResult<(Option<[u8; 16]>, u16)> {
    let inner = inner_of(handle)?;
    let guard = inner.lock();
    if guard.state != SockState::Connected {
        return Err(KernelError::NotConnected);
    }
    Ok((guard.peer_ip6, guard.peer_port))
}

/// The socket's **local** endpoint for `getsockname`, queried live from the daemon
/// (which owns the interface IP and the ephemeral source port it assigned).
///
/// Only meaningful once the socket is connected — an unconnected socket has no
/// daemon-assigned local port yet. Returns a [`LocalEndpoint`] whose variant
/// conveys the address family.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `NotConnected` — the socket is not connected (no local port assigned).
/// - protocol faults propagated from [`NetstackConn::local_addr`].
pub fn local(handle: SocketHandle) -> KernelResult<crate::net::netstack_client::LocalEndpoint> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.state != SockState::Connected {
        return Err(KernelError::NotConnected);
    }
    guard.conn.local_addr()
}

/// Half- or full-close a connected stream socket per `shutdown(2)`.
///
/// `how` is the Linux value (`SHUT_RD`=0, `SHUT_WR`=1, `SHUT_RDWR`=2). The socket
/// object stays alive and its fd remains valid; only the requested direction(s)
/// close (see [`NetstackConn::shutdown`]). Callers must validate `how` before
/// calling — an out-of-range value is treated as `SHUT_RDWR` by the daemon.
///
/// # Errors
///
/// - `NotConnected` — the socket is not in the connected state.
/// - protocol faults propagated from [`NetstackConn::shutdown`].
pub fn shutdown(handle: SocketHandle, how: u64) -> KernelResult<()> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.state != SockState::Connected {
        return Err(KernelError::NotConnected);
    }
    guard.conn.shutdown(how)
}

/// Explicitly `bind(2)` a datagram socket to `port` (host byte order; `0` asks the
/// daemon for an ephemeral port). Returns the actually-bound local port.
///
/// Mirrors Linux `bind(2)` on a `SOCK_DGRAM` fd: it creates the daemon-side UDP
/// socket and reserves the local port. A socket may be bound only once — a second
/// `bind` (explicit, or after the implicit ephemeral auto-bind on first `sendto`)
/// is `EINVAL`.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a datagram socket, or already bound (Linux `EINVAL`).
/// - `AddrInUse` — the requested port is already in use (Linux `EADDRINUSE`).
/// - protocol faults propagated from [`NetstackConn::udp_bind`].
pub fn dgram_bind(handle: SocketHandle, port: u16) -> KernelResult<u16> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.kind != SockKind::Dgram {
        return Err(KernelError::InvalidArgument);
    }
    if guard.bound {
        return Err(KernelError::InvalidArgument); // EINVAL — already bound
    }
    let local = guard.conn.udp_bind(port)?;
    guard.bound = true;
    guard.local_port = local;
    Ok(local)
}

/// Ensure a datagram socket has a daemon-side UDP socket, performing the implicit
/// ephemeral auto-bind (Linux binds an unbound `SOCK_DGRAM` to an ephemeral port on
/// the first `sendto`/`recvfrom`). No-op if already bound. Caller holds the guard.
fn ensure_bound(guard: &mut SocketInner) -> KernelResult<()> {
    if !guard.bound {
        let local = guard.conn.udp_bind(0)?;
        guard.bound = true;
        guard.local_port = local;
    }
    Ok(())
}

/// Send `buf` as a single datagram to `ip:port` from a datagram socket. Returns the
/// number of bytes accepted (a datagram is all-or-nothing, so this equals
/// `buf.len()` on success). Auto-binds an ephemeral local port on first use.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a datagram socket.
/// - `MsgSize` — the datagram exceeds the maximum a single `sendto` can carry
///   (Linux `EMSGSIZE`).
/// - protocol faults propagated from [`NetstackConn::udp_send_to`].
pub fn dgram_send_to(
    handle: SocketHandle,
    ip: &[u8; 4],
    port: u16,
    buf: &[u8],
) -> KernelResult<i32> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.kind != SockKind::Dgram {
        return Err(KernelError::InvalidArgument);
    }
    ensure_bound(&mut guard)?;
    guard.conn.udp_send_to(ip, port, buf)
}

/// Receive one datagram into `buf` from a datagram socket. Returns the payload
/// length copied plus the source `(ip, port)`. Auto-binds an ephemeral local port on
/// first use (so a `recvfrom` before any `sendto` still listens on a port).
///
/// When `nonblock` is set (the fd's `O_NONBLOCK` status flag), a receive with no
/// datagram queued returns [`KernelError::WouldBlock`] (→ `EAGAIN`).
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a datagram socket.
/// - `WouldBlock` — `nonblock` was set and no datagram was ready.
/// - protocol faults propagated from [`NetstackConn::udp_recv_from`].
pub fn dgram_recv_from(
    handle: SocketHandle,
    buf: &mut [u8],
    nonblock: bool,
) -> KernelResult<(i32, [u8; 4], u16)> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.kind != SockKind::Dgram {
        return Err(KernelError::InvalidArgument);
    }
    ensure_bound(&mut guard)?;
    guard.conn.udp_recv_from(buf, nonblock)
}

/// The bound local port of a datagram socket (`getsockname`), or `0` if not yet
/// bound. The daemon owns the interface IP, so callers pair this with the
/// interface address for a full `sockaddr_in`.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a datagram socket.
pub fn dgram_local_port(handle: SocketHandle) -> KernelResult<u16> {
    let inner = inner_of(handle)?;
    let guard = inner.lock();
    if guard.kind != SockKind::Dgram {
        return Err(KernelError::InvalidArgument);
    }
    Ok(guard.local_port)
}

/// Whether a socket handle refers to a datagram (`SOCK_DGRAM`) socket. Lets the
/// syscall layer route `bind`/`sendto`/`recvfrom`/`getsockname` to the datagram
/// path without duplicating the table lookup logic.
///
/// # Errors
///
/// `InvalidHandle` if the handle has been closed.
pub fn is_dgram(handle: SocketHandle) -> KernelResult<bool> {
    let inner = inner_of(handle)?;
    let guard = inner.lock();
    Ok(guard.kind == SockKind::Dgram)
}

/// Number of live sockets (test/introspection helper).
#[must_use]
pub fn live_count() -> usize {
    SOCKET_TABLE.lock().len()
}

/// Self-test the stream-socket object layer: creation, dup/close refcounting,
/// table accounting, and the closed-handle error surface.
///
/// This exercises only the local object machinery — it allocates the SHM ring
/// (via [`NetstackConn::open`]) but never contacts the daemon, so it is safe to
/// run at boot regardless of the `net.userspace` switch or whether a daemon is
/// present.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if any invariant fails (the caller
/// prints and propagates it as a self-test failure).
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[net::socket] Running stream-socket self-test...");
    let baseline = live_count();

    // Create → one live socket, unconnected, no peer.
    let h = create()?;
    if live_count() != baseline + 1 {
        crate::serial_println!("[net::socket] FAIL: live_count did not increment on create");
        return Err(KernelError::InternalError);
    }
    if is_connected(h)? {
        crate::serial_println!("[net::socket] FAIL: fresh socket reports connected");
        return Err(KernelError::InternalError);
    }
    if !matches!(peer(h), Err(KernelError::NotConnected)) {
        crate::serial_println!("[net::socket] FAIL: unconnected peer() not NotConnected");
        return Err(KernelError::InternalError);
    }

    // dup → second fd reference on the *same* slot: table length unchanged.
    let h2 = dup(h)?;
    if h2 != h || live_count() != baseline + 1 {
        crate::serial_println!("[net::socket] FAIL: dup changed handle/table length");
        return Err(KernelError::InternalError);
    }

    // First close drops one ref; the slot survives (still one fd open).
    close(h);
    if live_count() != baseline + 1 {
        crate::serial_println!("[net::socket] FAIL: slot freed while a ref remained");
        return Err(KernelError::InternalError);
    }
    // Second close drops the last ref; the slot is removed.
    close(h);
    if live_count() != baseline {
        crate::serial_println!("[net::socket] FAIL: slot not freed after last close");
        return Err(KernelError::InternalError);
    }

    // Operations on a fully-closed handle report InvalidHandle, and a
    // redundant close is a harmless no-op.
    if !matches!(is_connected(h), Err(KernelError::InvalidHandle)) {
        crate::serial_println!("[net::socket] FAIL: closed handle did not report InvalidHandle");
        return Err(KernelError::InternalError);
    }
    if !matches!(dup(h), Err(KernelError::InvalidHandle)) {
        crate::serial_println!("[net::socket] FAIL: dup of closed handle succeeded");
        return Err(KernelError::InternalError);
    }
    close(h); // no-op, must not panic or underflow

    crate::serial_println!("[net::socket] Stream-socket self-test PASSED");
    Ok(())
}

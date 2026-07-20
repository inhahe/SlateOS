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
use crate::sync::Mutex;

/// Unique per-socket id (also the value stored in `FdEntry::raw_handle`).
type SocketId = u64;

/// Counter for generating unique socket ids. Starts at 1 so 0 is never a valid
/// handle.
static NEXT_SOCKET_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_socket_id() -> SocketId {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed)
}

/// `AF_INET6` — the address family whose accept path reads the 18-byte peer form.
const AF_INET6: u16 = 10;

/// Session-local id under which a listening socket's daemon-side listener is
/// registered. Each listening socket owns its own ring session, so a single fixed
/// id is unique within that session (mirrors the boot self-test's `LISTENER_ID`).
const LISTENER_ID: u32 = 100;

/// First id handed to an accepted connection on a listener's session. Accepted ids
/// increase from here; the daemon demuxes by 4-tuple, so they need only be unique
/// within the one session.
const ACCEPT_ID_BASE: u32 = 101;

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
    /// A `connect()` succeeded; the daemon holds a live TCP connection. Also the
    /// state of a **server-side accepted** connection (see [`accept`]), which is
    /// live from birth.
    Connected,
    /// A prior `connect()` failed, or the peer/connection is gone. Terminal for
    /// this handle (Linux would require a fresh socket to retry).
    Failed,
    /// A passive `listen()` was issued: the socket is a server listener with a
    /// daemon-side listener registered on its ring session. Accepted connections
    /// ([`accept`]) share this socket's ring session under their own connection ids
    /// (see [`SessionRef`]). A listener never sends/receives data itself.
    Listening,
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

/// A connected datagram socket's default peer, set by `connect(2)` on a
/// `SOCK_DGRAM` fd. Once set, [`dgram_send_connected`] targets it (so `send`/
/// `write` need no explicit destination) and [`dgram_recv_from`] filters incoming
/// datagrams to those from it — Linux drops datagrams from any other source on a
/// connected UDP socket. The address itself carries the family (a v4 peer is a
/// [`V4`](DgramPeer::V4), a v6 peer a [`V6`](DgramPeer::V6)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DgramPeer {
    /// IPv4 default peer `(addr, port)`.
    V4([u8; 4], u16),
    /// IPv6 default peer `(addr, port)`.
    V6([u8; 16], u16),
}

/// Whether a received datagram's source `(family, ip16, port)` matches a connected
/// UDP socket's default `peer` — the filter Linux applies to a connected
/// `SOCK_DGRAM` socket (only datagrams from the connected peer are delivered).
fn dgram_peer_matches(peer: &DgramPeer, family: u16, ip16: &[u8; 16], port: u16) -> bool {
    match peer {
        DgramPeer::V4(ip, p) => {
            family == netipc::ring::UDP_AF_INET && port == *p && ip16.iter().take(4).eq(ip.iter())
        }
        DgramPeer::V6(ip6, p) => {
            family == netipc::ring::UDP_AF_INET6 && port == *p && ip16.iter().eq(ip6.iter())
        }
    }
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

/// A server-side connection (or listener) that **shares** a listener's ring
/// session with the listener and any sibling accepted connections, addressed by
/// its own `conn_id` (Q23 Option A: one refcounted session, no daemon-ABI change).
///
/// The shared [`NetstackConn`] lives behind its own `Arc<Mutex<…>>` so the listener
/// and every accepted socket — each in a *separate* [`SocketInner`] mutex — can
/// reach the one ring session. The daemon session is single-producer/
/// single-consumer, so this inner mutex serialises all ops on that session (the
/// documented Option-A concurrency limitation; see `known-issues` D-NETSOCK-SYNC).
///
/// On drop, an accepted connection tells the daemon to close *just* its `conn_id`
/// (`OP_CLOSE`), leaving the shared session — and the listener — alive. The
/// listener itself closes nothing here; the session is torn down (`OP_STOP`) only
/// when the last `Arc` reference drops, running [`NetstackConn`]'s own `Drop`.
struct SharedConn {
    /// The shared ring session (listener + all its accepted connections).
    session: Arc<Mutex<NetstackConn>>,
    /// The connection id this socket drives on the shared session — the listener's
    /// [`LISTENER_ID`] for a listener, or a per-accept id for an accepted socket.
    conn_id: u32,
    /// Whether this handle is the listener (whose id is a listener id, not a live
    /// connection). A listener does not `OP_CLOSE` a connection on drop.
    is_listener: bool,
}

impl Drop for SharedConn {
    fn drop(&mut self) {
        if self.is_listener {
            // The listener owns no connection to close; the session teardown
            // (OP_STOP on the final Arc drop) tears the listener down.
            return;
        }
        // Best-effort: close only this accepted connection id, leaving the shared
        // listener session alive for its siblings. Runs outside the table lock
        // (SharedConn is dropped when the owning SocketInner drops).
        let mut guard = self.session.lock();
        let _ = guard.close_conn(self.conn_id);
    }
}

/// How a socket reaches its daemon ring session.
///
/// Client stream sockets and datagram sockets own their session outright
/// ([`Owned`](SessionRef::Owned)); a listener and its accepted connections share
/// one session ([`Shared`](SessionRef::Shared), Q23 Option A). [`Taken`](SessionRef::Taken)
/// is a transient sentinel used only while `listen()` moves an owned session into a
/// shared one — no operation ever observes it.
enum SessionRef {
    /// A uniquely-owned ring session (client stream socket, datagram socket, or a
    /// listener before its session is published as shared).
    Owned(NetstackConn),
    /// A shared listener session addressed by a specific `conn_id`.
    Shared(SharedConn),
    /// Transient placeholder held only during `listen()`'s Owned→Shared move.
    Taken,
}

/// Per-socket mutable state. Guarded by its own mutex (see module lock
/// discipline) so a blocking daemon round-trip never holds the table lock.
struct SocketInner {
    /// The daemon-backed session driving this socket (owned, or a shared listener
    /// session addressed by a `conn_id`).
    session: SessionRef,
    /// Stream vs datagram transport.
    kind: SockKind,
    /// The address family the socket was created with (`AF_INET` = 2 /
    /// `AF_INET6` = 10). Recorded so `getsockname` on an unconnected/bound socket
    /// reports the correct family (`sockaddr_in` vs `sockaddr_in6`) even before any
    /// address is otherwise known.
    domain: u16,
    /// Datagram sockets only: whether the daemon-side UDP socket has been bound
    /// yet (an explicit `bind(2)` or the implicit ephemeral auto-bind on the
    /// first `sendto`/`recvfrom`, matching Linux). Meaningless for a stream socket.
    bound: bool,
    /// Datagram sockets only: the bound local port (for `getsockname`). `0` until
    /// bound.
    local_port: u16,
    /// Datagram sockets only: the connected default peer set by `connect(2)`, or
    /// `None` for an unconnected datagram socket. When set, `send`/`write` (no
    /// explicit destination) target it and `recv`/`read` filter to it. Meaningless
    /// for a stream socket.
    dgram_peer: Option<DgramPeer>,
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
    /// Listener sockets only: the next connection id to hand an accepted connection
    /// on this listener's session (starts at [`ACCEPT_ID_BASE`], bumped per
    /// successful [`accept`]). Meaningless for a non-listener.
    next_accept_id: u32,
}

impl SocketInner {
    /// Borrow the underlying [`NetstackConn`] of an **owned** session. Datagram
    /// sockets, client `connect`, and listener setup only ever run on owned
    /// sessions, so a shared (accepted/listener) session is a caller/logic error and
    /// reports `InvalidArgument`.
    fn owned_conn_mut(&mut self) -> KernelResult<&mut NetstackConn> {
        match &mut self.session {
            SessionRef::Owned(c) => Ok(c),
            SessionRef::Shared(_) | SessionRef::Taken => Err(KernelError::InvalidArgument),
        }
    }

    /// Run `f` against the socket's stream connection and its effective `conn_id`.
    ///
    /// Works for both an owned client stream socket (the [`NetstackConn`]'s own
    /// [`conn_id`](NetstackConn::conn_id)) and a shared accepted connection (its
    /// per-accept id, under the shared session lock). Used by the stream ops that
    /// apply to both — `send`/`recv`/`poll`/`shutdown`/`getsockname`.
    fn with_stream_conn<R>(
        &mut self,
        f: impl FnOnce(&mut NetstackConn, u32) -> KernelResult<R>,
    ) -> KernelResult<R> {
        match &mut self.session {
            SessionRef::Owned(c) => {
                let cid = c.conn_id();
                f(c, cid)
            }
            SessionRef::Shared(s) => {
                let cid = s.conn_id;
                let mut guard = s.session.lock();
                f(&mut guard, cid)
            }
            SessionRef::Taken => Err(KernelError::InvalidHandle),
        }
    }
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

/// Create a new (unconnected) stream socket in address family `domain`
/// (`AF_INET` = 2 / `AF_INET6` = 10).
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
pub fn create(domain: u16) -> KernelResult<SocketHandle> {
    create_kind(SockKind::Stream, domain)
}

/// Create a new (unbound) connectionless datagram (`SOCK_DGRAM`) socket in address
/// family `domain` (`AF_INET` = 2 / `AF_INET6` = 10).
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
pub fn create_dgram(domain: u16) -> KernelResult<SocketHandle> {
    create_kind(SockKind::Dgram, domain)
}

/// Shared constructor for [`create`] (stream) and [`create_dgram`] (datagram).
fn create_kind(kind: SockKind, domain: u16) -> KernelResult<SocketHandle> {
    let conn = NetstackConn::open()?;
    let inner = SocketInner {
        session: SessionRef::Owned(conn),
        kind,
        domain,
        bound: false,
        local_port: 0,
        dgram_peer: None,
        state: SockState::Created,
        peer_ip: [0; 4],
        peer_ip6: None,
        peer_port: 0,
        so_error_read: false,
        next_accept_id: ACCEPT_ID_BASE,
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
    let res = guard.owned_conn_mut()?.connect(ip, port, nonblock)?;
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
    let res = guard.owned_conn_mut()?.connect6(ip6, port, nonblock)?;
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
    guard.with_stream_conn(|c, cid| c.send_on(cid, buf, nonblock))
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
    guard.with_stream_conn(|c, cid| c.recv_on(cid, buf, nonblock, peek))
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
            return guard.owned_conn_mut()?.poll_ready();
        }
        return Ok((false, true, false));
    }
    match guard.state {
        SockState::Connected => guard.with_stream_conn(|c, cid| c.poll_on(cid)),
        SockState::Connecting => {
            let (readable, writable, error) = guard.with_stream_conn(|c, cid| c.poll_on(cid))?;
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
        // A listener is "readable" (→ POLLIN) when a completed connection waits in
        // the backlog, so a poll-driven `accept` wakes. The daemon reports this via
        // OP_POLL on the listener id; a `NotConnected` means it has no such state to
        // report yet — treat as not-ready rather than an error.
        SockState::Listening => match guard.with_stream_conn(|c, cid| c.poll_on(cid)) {
            Ok(ready) => Ok(ready),
            Err(KernelError::NotConnected) => Ok((false, false, false)),
            Err(e) => Err(e),
        },
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
    guard.with_stream_conn(|c, cid| c.local_addr_on(cid))
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
    guard.with_stream_conn(|c, cid| c.shutdown_on(cid, how))
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
    let local = guard.owned_conn_mut()?.udp_bind(port)?;
    guard.bound = true;
    guard.local_port = local;
    Ok(local)
}

/// Ensure a datagram socket has a daemon-side UDP socket, performing the implicit
/// ephemeral auto-bind (Linux binds an unbound `SOCK_DGRAM` to an ephemeral port on
/// the first `sendto`/`recvfrom`). No-op if already bound. Caller holds the guard.
fn ensure_bound(guard: &mut SocketInner) -> KernelResult<()> {
    if !guard.bound {
        let local = guard.owned_conn_mut()?.udp_bind(0)?;
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
    guard.owned_conn_mut()?.udp_send_to(ip, port, buf)
}

/// Send `buf` as a single datagram to `[ip6]:port` over IPv6 from a datagram
/// socket. The IPv6 sibling of [`dgram_send_to`]; auto-binds an ephemeral local
/// port on first use. Returns the number of bytes accepted (`buf.len()` on
/// success — a datagram is all-or-nothing).
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a datagram socket.
/// - `MsgSize` — the datagram exceeds the maximum a single `sendto` can carry
///   (Linux `EMSGSIZE`).
/// - protocol faults propagated from [`NetstackConn::udp_send_to6`].
pub fn dgram_send_to6(
    handle: SocketHandle,
    ip6: &[u8; 16],
    port: u16,
    buf: &[u8],
) -> KernelResult<i32> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.kind != SockKind::Dgram {
        return Err(KernelError::InvalidArgument);
    }
    ensure_bound(&mut guard)?;
    guard.owned_conn_mut()?.udp_send_to6(ip6, port, buf)
}

/// Receive one datagram into `buf` from a datagram socket. Returns the payload
/// length copied plus the source `(family, ip16, port)` — `family` is
/// [`UDP_AF_INET`](netipc::ring::UDP_AF_INET) or
/// [`UDP_AF_INET6`](netipc::ring::UDP_AF_INET6), and `ip16` is the fixed 16-byte
/// address (IPv4 in `ip16[0..4]`). The syscall layer uses the reported family to
/// write back the matching `sockaddr_in` / `sockaddr_in6`. Auto-binds an ephemeral
/// local port on first use (so a `recvfrom` before any `sendto` still listens).
///
/// When `nonblock` is set (the fd's `O_NONBLOCK` status flag), a receive with no
/// datagram queued returns [`KernelError::WouldBlock`] (→ `EAGAIN`).
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a datagram socket.
/// - `WouldBlock` — `nonblock` was set and no datagram was ready.
/// - protocol faults propagated from [`NetstackConn::udp_recv_any`].
pub fn dgram_recv_from(
    handle: SocketHandle,
    buf: &mut [u8],
    nonblock: bool,
) -> KernelResult<(i32, u16, [u8; 16], u16)> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.kind != SockKind::Dgram {
        return Err(KernelError::InvalidArgument);
    }
    ensure_bound(&mut guard)?;
    let peer = guard.dgram_peer;
    // An *unconnected* datagram socket delivers from any source. A *connected*
    // one (a `connect(2)` set a default peer) only delivers datagrams from that
    // peer — Linux drops the rest at input, so we discard non-matching datagrams
    // here and keep receiving. On a non-blocking socket the discard loop drains to
    // `WouldBlock` (→ `EAGAIN`) once the queue holds no matching datagram; on a
    // blocking socket it waits for the next one, matching a connected UDP `recv`.
    loop {
        let got = guard.owned_conn_mut()?.udp_recv_any(buf, nonblock)?;
        match peer {
            None => return Ok(got),
            Some(p) => {
                let (_, family, ip16, port) = got;
                if dgram_peer_matches(&p, family, &ip16, port) {
                    return Ok(got);
                }
                // Source does not match the connected peer — drop and receive again.
            }
        }
    }
}

/// `connect(2)` a datagram (`SOCK_DGRAM`) socket to a default peer.
///
/// Sets (or replaces — a UDP socket may be re-`connect`ed) the socket's default
/// destination. After this, [`dgram_send_connected`] (`send`/`write` with no
/// explicit address) targets `peer`, and [`dgram_recv_from`] filters incoming
/// datagrams to those from `peer`. Auto-binds an ephemeral local port if the
/// socket is unbound (Linux assigns a source port on a UDP `connect`).
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a datagram socket.
/// - protocol faults propagated from the implicit ephemeral auto-bind.
pub fn dgram_connect(handle: SocketHandle, peer: DgramPeer) -> KernelResult<()> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.kind != SockKind::Dgram {
        return Err(KernelError::InvalidArgument);
    }
    ensure_bound(&mut guard)?;
    guard.dgram_peer = Some(peer);
    Ok(())
}

/// Dissolve a datagram socket's connected peer per `connect(AF_UNSPEC)` — Linux
/// disconnects the UDP socket, so a later `send`/`write` again needs an explicit
/// destination and `recv` again accepts datagrams from any source.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a datagram socket.
pub fn dgram_disconnect(handle: SocketHandle) -> KernelResult<()> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.kind != SockKind::Dgram {
        return Err(KernelError::InvalidArgument);
    }
    guard.dgram_peer = None;
    Ok(())
}

/// The datagram socket's connected default peer (`getpeername`), or `None` if it
/// has not been `connect`ed.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a datagram socket.
pub fn dgram_peer(handle: SocketHandle) -> KernelResult<Option<DgramPeer>> {
    let inner = inner_of(handle)?;
    let guard = inner.lock();
    if guard.kind != SockKind::Dgram {
        return Err(KernelError::InvalidArgument);
    }
    Ok(guard.dgram_peer)
}

/// Send `buf` as a single datagram to the socket's connected default peer
/// (`send(2)`/`write(2)` with no explicit destination). Returns the number of
/// bytes accepted (`buf.len()` on success — a datagram is all-or-nothing).
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a datagram socket.
/// - `NotConnected` — no default peer set (the syscall layer maps this to
///   `EDESTADDRREQ`).
/// - `MsgSize` — the datagram exceeds the maximum a single send can carry.
/// - protocol faults propagated from [`NetstackConn::udp_send_to`]/`udp_send_to6`.
pub fn dgram_send_connected(handle: SocketHandle, buf: &[u8]) -> KernelResult<i32> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.kind != SockKind::Dgram {
        return Err(KernelError::InvalidArgument);
    }
    let peer = guard.dgram_peer.ok_or(KernelError::NotConnected)?;
    ensure_bound(&mut guard)?;
    match peer {
        DgramPeer::V4(ip, port) => guard.owned_conn_mut()?.udp_send_to(&ip, port, buf),
        DgramPeer::V6(ip6, port) => guard.owned_conn_mut()?.udp_send_to6(&ip6, port, buf),
    }
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

/// The address family the socket was created with (`AF_INET` = 2 /
/// `AF_INET6` = 10). Lets the syscall layer report the correct `getsockname`
/// family for a socket whose local address is otherwise only a port (an
/// unconnected/bound datagram socket bound to the wildcard address).
///
/// # Errors
///
/// `InvalidHandle` if the handle has been closed.
pub fn domain(handle: SocketHandle) -> KernelResult<u16> {
    let inner = inner_of(handle)?;
    let guard = inner.lock();
    Ok(guard.domain)
}

/// The peer address returned by [`accept`], carrying its address family so the
/// syscall layer writes back the matching `sockaddr_in` / `sockaddr_in6`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptedPeer {
    /// IPv4 peer `(addr, port)`.
    V4([u8; 4], u16),
    /// IPv6 peer `(addr, port)`.
    V6([u8; 16], u16),
}

/// `bind(2)` a stream socket to local `port` (host byte order).
///
/// A stream `bind` is recorded kernel-side only: the daemon has no separate
/// stream-bind op — the port is carried into the daemon on [`listen`] (`OP_LISTEN`).
/// A socket may be bound only once, and only before `connect`/`listen`.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a stream socket, already bound, or not in the
///   `Created` state (Linux `EINVAL`).
pub fn bind_stream(handle: SocketHandle, port: u16) -> KernelResult<()> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.kind != SockKind::Stream {
        return Err(KernelError::InvalidArgument);
    }
    if guard.bound || guard.state != SockState::Created {
        return Err(KernelError::InvalidArgument); // EINVAL — already bound/connected/listening
    }
    guard.bound = true;
    guard.local_port = port;
    Ok(())
}

/// `listen(2)` on a stream socket: register a passive daemon-side listener and make
/// the socket a server that [`accept`] can dequeue connections from.
///
/// The backlog is currently advisory — the daemon manages its own backlog — so
/// `_backlog` is accepted for ABI shape but not forwarded. Listening moves the
/// socket's owned ring session into a **shared** session ([`SharedConn`]) so
/// accepted connections address the same session under their own ids (Q23 Option A).
/// A repeated `listen` on an already-listening socket is a no-op (Linux tolerates
/// re-`listen`).
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a stream socket, or already connected/connecting
///   (Linux `EINVAL`).
/// - `AddrInUse` — the daemon rejected the listener (port/id already in use).
/// - protocol faults propagated from [`NetstackConn::listen`].
pub fn listen(handle: SocketHandle, _backlog: i32) -> KernelResult<()> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.kind != SockKind::Stream {
        return Err(KernelError::InvalidArgument); // ENOTSUP-ish: listen on a dgram socket
    }
    match guard.state {
        SockState::Created => {}
        SockState::Listening => return Ok(()), // already listening — idempotent
        _ => return Err(KernelError::InvalidArgument), // connected/connecting/failed → EINVAL
    }
    let port = guard.local_port; // 0 if never bound (wildcard local port)
    // Move the owned session out to issue OP_LISTEN, then republish it as a shared
    // session so accepted connections can reach the same ring.
    let mut conn = match core::mem::replace(&mut guard.session, SessionRef::Taken) {
        SessionRef::Owned(c) => c,
        other => {
            // Not an owned session (shouldn't happen given the state check above);
            // restore and reject.
            guard.session = other;
            return Err(KernelError::InvalidArgument);
        }
    };
    let res = match conn.listen(LISTENER_ID, port) {
        Ok(r) => r,
        Err(e) => {
            guard.session = SessionRef::Owned(conn);
            return Err(e);
        }
    };
    if res != 0 {
        guard.session = SessionRef::Owned(conn);
        return Err(KernelError::AddrInUse);
    }
    guard.session = SessionRef::Shared(SharedConn {
        session: Arc::new(Mutex::new(conn)),
        conn_id: LISTENER_ID,
        is_listener: true,
    });
    guard.state = SockState::Listening;
    Ok(())
}

/// `accept(2)` one established connection from a listening socket.
///
/// Dequeues a completed connection from the daemon's backlog and installs it as a
/// **new** socket that shares the listener's ring session under a fresh `conn_id`
/// (Q23 Option A). Returns the new socket handle and the peer address. This is a
/// single, non-blocking daemon round-trip: an empty backlog reports
/// [`KernelError::WouldBlock`] (→ `EAGAIN`), which a blocking `accept(2)` caller
/// retries.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — the socket is not listening.
/// - `WouldBlock` — no completed connection waiting (→ `EAGAIN`).
/// - `InternalError` — the daemon rejected the accept (unknown listener / id clash).
/// - protocol faults propagated from [`NetstackConn::accept`].
pub fn accept(handle: SocketHandle) -> KernelResult<(SocketHandle, AcceptedPeer)> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.state != SockState::Listening {
        return Err(KernelError::InvalidArgument); // EINVAL — not listening
    }
    let domain = guard.domain;
    let new_id = guard.next_accept_id;
    let (arc, listener_id) = match &guard.session {
        SessionRef::Shared(s) if s.is_listener => (s.session.clone(), s.conn_id),
        _ => return Err(KernelError::InvalidArgument),
    };
    // Issue OP_ACCEPT on the shared session under its lock. The listener's domain
    // selects the peer form (an AF_INET6 listener reads the 18-byte peer window).
    let (res, peer) = {
        let mut c = arc.lock();
        if domain == AF_INET6 {
            let mut p = [0u8; 18];
            let r = c.accept6(listener_id, new_id, &mut p)?;
            let ip6: [u8; 16] = p
                .get(..16)
                .and_then(|s| <[u8; 16]>::try_from(s).ok())
                .ok_or(KernelError::InternalError)?;
            let port = u16::from_be_bytes([
                p.get(16).copied().unwrap_or(0),
                p.get(17).copied().unwrap_or(0),
            ]);
            (r, AcceptedPeer::V6(ip6, port))
        } else {
            let mut p = [0u8; 6];
            let r = c.accept(listener_id, new_id, &mut p)?;
            let ip4: [u8; 4] = p
                .get(..4)
                .and_then(|s| <[u8; 4]>::try_from(s).ok())
                .ok_or(KernelError::InternalError)?;
            let port = u16::from_be_bytes([
                p.get(4).copied().unwrap_or(0),
                p.get(5).copied().unwrap_or(0),
            ]);
            (r, AcceptedPeer::V4(ip4, port))
        }
    };
    if res == netipc::ring::ERR_WOULD_BLOCK {
        return Err(KernelError::WouldBlock); // EAGAIN — nothing in the backlog
    }
    if res != 0 {
        return Err(KernelError::InternalError); // -1 unknown listener / id install failed
    }
    // Consume the id only now that the accept has succeeded.
    guard.next_accept_id = new_id.checked_add(1).ok_or(KernelError::InternalError)?;
    let (peer_ip, peer_ip6, peer_port) = match peer {
        AcceptedPeer::V4(ip, port) => (ip, None, port),
        AcceptedPeer::V6(ip6, port) => ([0; 4], Some(ip6), port),
    };
    let accepted = SocketInner {
        session: SessionRef::Shared(SharedConn {
            session: arc,
            conn_id: new_id,
            is_listener: false,
        }),
        kind: SockKind::Stream,
        domain,
        bound: false,
        local_port: 0,
        dgram_peer: None,
        state: SockState::Connected,
        peer_ip,
        peer_ip6,
        peer_port,
        so_error_read: false,
        next_accept_id: ACCEPT_ID_BASE,
    };
    let id = alloc_socket_id();
    let slot = SocketSlot {
        inner: Arc::new(Mutex::new(accepted)),
        refcount: 1,
    };
    SOCKET_TABLE.lock().insert(id, slot);
    Ok((SocketHandle(id), peer))
}

/// Whether a socket handle refers to a listening (server) socket. Lets the syscall
/// layer route `accept` and reject `connect`/`send` on a listener.
///
/// # Errors
///
/// `InvalidHandle` if the handle has been closed.
pub fn is_listening(handle: SocketHandle) -> KernelResult<bool> {
    let inner = inner_of(handle)?;
    let guard = inner.lock();
    Ok(guard.state == SockState::Listening)
}

/// The bound local port of a stream socket (`getsockname` on a bound/listening
/// socket whose local address is otherwise only a port). `0` if never bound.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `InvalidArgument` — not a stream socket.
pub fn stream_local_port(handle: SocketHandle) -> KernelResult<u16> {
    let inner = inner_of(handle)?;
    let guard = inner.lock();
    if guard.kind != SockKind::Stream {
        return Err(KernelError::InvalidArgument);
    }
    Ok(guard.local_port)
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
    let h = create(2)?;
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

/// Object-layer server-socket self-test: exercises the `bind_stream` → `listen`
/// → `accept` state machine that the `bind(2)`/`listen(2)`/`accept(2)` syscalls
/// drive (Q23 Option A). This is the object-layer companion to
/// [`crate::net::netstack_client::self_test_listen_accept`], which already proves
/// the full ring-level data path (a loopback connection accepted and echoed both
/// ways within one daemon session).
///
/// The object layer keeps the listener and any client on **separate** daemon
/// sessions, so a full client→listener data loopback is not reachable here (the
/// daemon's per-session loopback divert routes within a single session). This
/// test therefore validates the state transitions, the `Owned`→`Shared` session
/// conversion on `listen`, `getsockname`-style port reporting, idempotent
/// re-`listen`, and the empty-backlog `WouldBlock`/`EAGAIN` path — the exact
/// object-layer surface the new syscalls call into.
///
/// Returns `Ok(None)` when no daemon session can be opened (networking down), so
/// the caller can log a skip rather than a failure.
///
/// # Errors
///
/// [`KernelError::InternalError`] if any state transition or error mapping
/// diverges from the Option A contract; daemon faults are propagated.
pub fn self_test_server() -> KernelResult<Option<()>> {
    /// A high, unlikely-to-clash loopback listen port for the object-layer test.
    const PORT: u16 = 9098;

    // A listener needs a live daemon session. If we cannot open one, the network
    // is down — skip rather than fail (mirrors the netstack_client self-tests).
    let listener = match create(2) {
        Ok(h) => h,
        Err(_) => return Ok(None),
    };

    // Guard so every early return still tears the listener down (no session leak).
    let cleanup = |h: SocketHandle, msg: &str| -> KernelResult<Option<()>> {
        close(h);
        crate::serial_println!("[net::socket] FAIL(server): {}", msg);
        Err(KernelError::InternalError)
    };

    // bind_stream records the port kernel-side and requires the Created state.
    if bind_stream(listener, PORT).is_err() {
        return cleanup(listener, "bind_stream on a fresh stream socket failed");
    }
    // A second bind is rejected (already bound) — EINVAL.
    if bind_stream(listener, PORT).is_ok() {
        return cleanup(listener, "double bind_stream unexpectedly succeeded");
    }
    // Not listening yet.
    if is_listening(listener).unwrap_or(true) {
        return cleanup(listener, "socket reported Listening before listen()");
    }

    // listen() registers the passive listener with the daemon and converts the
    // owned session into the shared server session.
    if listen(listener, 8).is_err() {
        return cleanup(listener, "listen() on a bound stream socket failed");
    }
    if !is_listening(listener).unwrap_or(false) {
        return cleanup(listener, "socket did not report Listening after listen()");
    }
    // getsockname-style port reporting must echo the bound port.
    if stream_local_port(listener).unwrap_or(0) != PORT {
        return cleanup(listener, "stream_local_port did not report the bound port");
    }
    // Re-listen is idempotent (Linux tolerates a repeat listen).
    if listen(listener, 8).is_err() {
        return cleanup(listener, "idempotent re-listen() failed");
    }

    // An empty backlog must surface as WouldBlock (→ EAGAIN), the single-shot
    // accept contract the syscall layer's blocking policy relies on.
    match accept(listener) {
        Err(KernelError::WouldBlock) => {}
        Ok((accepted, _peer)) => {
            close(accepted);
            return cleanup(listener, "accept() dequeued a connection from an empty backlog");
        }
        Err(_) => {
            return cleanup(listener, "accept() on an empty backlog returned a non-WouldBlock error");
        }
    }

    // connect on a listening socket must be rejected (it is not Created).
    // (connect() routes through owned_conn_mut, which errors on a Shared session.)
    if connect(listener, &[127, 0, 0, 1], PORT, true).is_ok() {
        return cleanup(listener, "connect() on a listening socket unexpectedly succeeded");
    }

    close(listener);

    // A datagram socket rejects bind_stream/listen (stream-only ops).
    let dgram = match create_dgram(2) {
        Ok(h) => h,
        Err(_) => return Ok(None),
    };
    if bind_stream(dgram, PORT).is_ok() {
        close(dgram);
        crate::serial_println!("[net::socket] FAIL(server): bind_stream on a dgram socket succeeded");
        return Err(KernelError::InternalError);
    }
    if listen(dgram, 8).is_ok() {
        close(dgram);
        crate::serial_println!("[net::socket] FAIL(server): listen on a dgram socket succeeded");
        return Err(KernelError::InternalError);
    }
    close(dgram);

    crate::serial_println!("[net::socket] Server-socket (bind/listen/accept) self-test PASSED");
    Ok(Some(()))
}

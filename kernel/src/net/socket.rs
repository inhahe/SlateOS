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
    /// A `connect()` succeeded; the daemon holds a live TCP connection.
    Connected,
    /// A prior `connect()` failed, or the peer/connection is gone. Terminal for
    /// this handle (Linux would require a fresh socket to retry).
    Failed,
}

/// Per-socket mutable state. Guarded by its own mutex (see module lock
/// discipline) so a blocking daemon round-trip never holds the table lock.
struct SocketInner {
    /// The daemon-backed connection driving this socket.
    conn: NetstackConn,
    /// Connection lifecycle state.
    state: SockState,
    /// Remembered peer address (for `getpeername`), set on a successful connect.
    peer_ip: [u8; 4],
    /// Remembered peer port.
    peer_port: u16,
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
    let conn = NetstackConn::open()?;
    let inner = SocketInner {
        conn,
        state: SockState::Created,
        peer_ip: [0; 4],
        peer_port: 0,
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
/// Returns `Ok(())` on success. Maps a daemon "no upstream" / refused result to
/// `ECONNREFUSED` and marks the socket `Failed`.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `AlreadyExists` — the socket is already connected (Linux `EISCONN`).
/// - `ConnectionRefused` — the daemon could not establish the connection.
/// - protocol faults propagated from [`NetstackConn::connect`].
pub fn connect(handle: SocketHandle, ip: &[u8; 4], port: u16) -> KernelResult<()> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.state == SockState::Connected {
        return Err(KernelError::AlreadyExists);
    }
    let res = guard.conn.connect(ip, port)?;
    if res < 0 {
        guard.state = SockState::Failed;
        return Err(KernelError::ConnectionRefused);
    }
    guard.state = SockState::Connected;
    guard.peer_ip = *ip;
    guard.peer_port = port;
    Ok(())
}

/// Send `buf` on a connected socket. Returns the number of bytes accepted.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `NotConnected` — the socket is not connected (Linux `ENOTCONN`/`EPIPE`).
/// - protocol faults propagated from [`NetstackConn::send`].
pub fn send(handle: SocketHandle, buf: &[u8]) -> KernelResult<i32> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.state != SockState::Connected {
        return Err(KernelError::NotConnected);
    }
    guard.conn.send(buf)
}

/// Receive up to `buf.len()` bytes on a connected socket. Returns the number of
/// bytes copied (`0` = peer closed / no data).
///
/// When `nonblock` is set (the fd's `O_NONBLOCK` status flag), a receive with no
/// data ready returns [`KernelError::WouldBlock`] (→ `EAGAIN`) instead of blocking
/// on the daemon; otherwise it blocks up to the daemon's receive deadline.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - `NotConnected` — the socket is not connected.
/// - `WouldBlock` — `nonblock` was set and no data was ready.
/// - protocol faults propagated from [`NetstackConn::recv`].
pub fn recv(handle: SocketHandle, buf: &mut [u8], nonblock: bool) -> KernelResult<i32> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    if guard.state != SockState::Connected {
        return Err(KernelError::NotConnected);
    }
    guard.conn.recv(buf, nonblock)
}

/// Non-destructively probe a stream socket's readiness for the poll/epoll engine.
///
/// Returns `(readable, writable)`:
/// - A **connected** socket queries the daemon (via
///   [`NetstackConn::poll_ready`]) for its honest state: `readable` iff it has
///   buffered bytes or the peer has closed (so a `recv` returns data/EOF
///   promptly), `writable` iff the connection can accept a send. This replaces
///   the former "always ready" placeholder so `POLLIN` no longer spins a poller
///   that then reads `EAGAIN`.
/// - An **unconnected** but live socket (never connected, or a failed connect) is
///   reported writable-only: a `connect` may still proceed, but there is nothing
///   to read.
///
/// Does not consume buffered data — a subsequent [`recv`] still returns it.
///
/// # Errors
///
/// - `InvalidHandle` — closed handle.
/// - protocol faults propagated from [`NetstackConn::poll_ready`].
pub fn poll_ready(handle: SocketHandle) -> KernelResult<(bool, bool)> {
    let inner = inner_of(handle)?;
    let mut guard = inner.lock();
    match guard.state {
        SockState::Connected => guard.conn.poll_ready(),
        // Not connected: a connect may still proceed (writable), nothing to read.
        _ => Ok((false, true)),
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

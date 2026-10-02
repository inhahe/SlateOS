//! Unix-domain sockets with names: `AF_UNIX` `SOCK_STREAM` and `SOCK_DGRAM`,
//! bound to a filesystem path or to an abstract name, and reached by
//! `connect` and `sendto`.
//!
//! ## Why this exists
//!
//! Every ported program's C library logs by sending a datagram to the
//! socket at `/dev/log`, and X11, D-Bus, PostgreSQL, `ssh-agent`, `tmux` and
//! `screen` all meet their clients at a path. Until 2026-10-02 the only
//! Unix-domain sockets here were unnamed stream pairs (`socketpair`,
//! [`super::stream_socket`]), so none of that could work
//! (`requests/b-ad-a-unix-socket-cannot-be-bound-to-a-path-so-nothing-can-receive-syslog.md`).
//! SlateOS's own services talk over channels; this is the POSIX door, and it
//! is IPC, so it lives here with the other IPC objects.
//!
//! ## Names
//!
//! A path name is a node in the filesystem ([`EntryType::Socket`]) that holds
//! nothing. `bind` makes the node (the syscall layer calls
//! [`crate::fs::Vfs::mknod_socket`]) and records here that its identity --
//! its [`FileId`] -- leads to this socket. `connect` and `sendto` look the
//! path up, take the node's identity, and find the socket by it. Keying on
//! the identity rather than the path gives Linux's behaviour for free: a
//! renamed node still leads to its socket, a second hard link does too, and
//! an unlinked node leads nowhere. Closing the socket leaves the node, and
//! `connect` to it is then refused -- which is why a server removes a stale
//! node before it binds.
//!
//! The table of names is one of the per-file tables [`crate::fs::perfile`]
//! keeps honest: an entry ends with its node, so a reused inode number never
//! leads a client to a stranger.
//!
//! An abstract name (Linux's, `sun_path[0] == 0`) has no node and is kept
//! here by its bytes; it ends when its socket closes.
//!
//! [`EntryType::Socket`]: crate::fs::vfs::EntryType::Socket
//!
//! ## The two kinds
//!
//! - **Stream.** `listen` turns a bound socket into a listener with a
//!   backlog. `connect` makes a [`super::stream_socket`] pair: the client
//!   keeps one end, and the other waits in the backlog until `accept` hands
//!   it out as a new socket. After that the bytes are the pair's, so a
//!   connected socket reads, writes, polls and shuts down exactly as a
//!   `socketpair` end does.
//! - **Datagram.** Each socket has a queue of datagrams, each kept whole with
//!   its sender's address and credentials. `sendto` names a destination;
//!   `connect` sets the one `send` uses. A full queue makes a sender wait, as
//!   Linux's does, rather than drop -- a syslog client is the case that cares.
//!
//! ## Credentials
//!
//! A connected stream socket knows its peer's process, uid and gid as they
//! were at `connect` (on the server side) or `listen` (on the client side),
//! which is Linux's `SO_PEERCRED`. Every datagram carries its sender's, which
//! is what `SCM_CREDENTIALS` reports. A process cannot state these; the
//! kernel records them. Kernel context has none to record.
//!
//! ## Lock order
//!
//! `TABLE` -> `stream_socket`'s `PAIRS` -> `SCHED`. Nothing here is called
//! with a filesystem lock held, and the filesystem is never entered with
//! `TABLE` held: [`crate::fs::perfile`] calls in only after the VFS has
//! released its locks.

use super::channel::PeerCred;
use super::stream_socket::{self, StreamSocketHandle};
use super::waiters::{
    WaiterSet, current_user_pid, deliverable_signal_pending, park_interruptible, wake_all,
};
use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::FileId;
use crate::sched::{self, task::TaskId};
use crate::sync::PreemptSpinMutex as Mutex;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// The largest datagram one `sendto` carries. Larger is `MsgSize`
/// (`EMSGSIZE`), as on Linux, whose limit is the send buffer (~208 KiB by
/// default); 64 KiB is what a channel message and a pipe's buffer are here.
pub const MAX_DGRAM: usize = 64 * 1024;

/// Datagrams a socket holds unreceived before senders wait.
const MAX_DGRAM_QUEUE: usize = 128;

/// Bytes a socket holds unreceived before senders wait.
const MAX_DGRAM_BYTES: usize = 1024 * 1024;

/// The longest backlog `listen` grants (Linux's `SOMAXCONN` default is 4096;
/// 128 was its value for decades and is plenty for the services here).
pub const MAX_BACKLOG: usize = 128;

/// The most bytes an abstract name may have: `sun_path` is 108 bytes and the
/// first is the NUL that marks the name abstract.
pub const MAX_ABSTRACT_NAME: usize = 107;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A Unix-domain socket, as the descriptor table and the native ABI carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnixHandle(u64);

impl UnixHandle {
    /// The handle a raw value names.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw value.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Which of the two kinds a socket is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `SOCK_STREAM`: a connection carrying a byte stream.
    Stream,
    /// `SOCK_DGRAM`: datagrams, each kept whole.
    Dgram,
}

/// A socket's address, as `getsockname`, `getpeername` and `recvfrom` report
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Address {
    /// No name: a socket never bound, or a client's end.
    #[default]
    Unnamed,
    /// A filesystem name, as the binder gave it (Linux reports the path
    /// `bind` was given, not where the node is now).
    Path(Vec<u8>),
    /// An abstract name, without the leading NUL.
    Abstract(Vec<u8>),
}

/// What a name leads to, for `bind`, `connect` and `sendto`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Name {
    /// A socket's node in the filesystem, by its identity.
    Node(FileId),
    /// An abstract name.
    Abstract(Vec<u8>),
}

/// A connection `accept` handed out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    /// The new socket, connected to the client.
    pub handle: UnixHandle,
    /// The client's address: [`Address::Unnamed`] unless it bound one.
    pub peer: Address,
}

/// What one receive took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    /// Bytes copied into the caller's buffer.
    pub len: usize,
    /// The datagram's whole length -- more than `len` when the buffer was too
    /// small and the rest was discarded (`MSG_TRUNC`). Equal to `len` for a
    /// stream.
    pub full_len: usize,
    /// Who sent it: the sender's bound address, or the peer's for a stream.
    pub from: Address,
    /// The sender's credentials, as the kernel recorded them.
    pub cred: Option<PeerCred>,
}

/// One datagram waiting in a queue.
struct Datagram {
    data: Vec<u8>,
    from: Address,
    cred: Option<PeerCred>,
}

/// A connection waiting in a listener's backlog.
struct Pending {
    /// The server's end of the pair; the client holds the other.
    stream: StreamSocketHandle,
    /// The client's address.
    peer: Address,
    /// The client's credentials at `connect`.
    peer_cred: Option<PeerCred>,
}

/// Where a stream socket is in its life.
enum State {
    /// Neither listening nor connected (and every datagram socket).
    Idle,
    /// After `listen`: connections waiting to be accepted.
    Listening {
        backlog: VecDeque<Pending>,
        max: usize,
    },
    /// After `connect` or `accept`.
    Connected {
        stream: StreamSocketHandle,
        peer: Address,
        peer_cred: Option<PeerCred>,
    },
}

/// One socket.
struct Socket {
    kind: Kind,
    /// Descriptors that hold it (`dup`, `fork`); it ends at the last close.
    refs: u32,
    /// The name it is bound to, and the address that reports.
    bound: Option<(Name, Address)>,
    state: State,
    /// The credentials a connecting client reads as its peer's: the
    /// listener's, taken at `listen`.
    listen_cred: Option<PeerCred>,
    /// Datagram sockets: what has arrived and not been received.
    queue: VecDeque<Datagram>,
    queued_bytes: usize,
    /// Datagram sockets: where `send` without an address goes (`connect`).
    default_peer: Option<u64>,
    /// `shutdown`: a datagram socket's own halves. A stream's are its pair's.
    rd_shut: bool,
    wr_shut: bool,
    /// Tasks waiting for something to take -- a datagram, a connection to
    /// accept -- or polling.
    readers: WaiterSet,
    /// Tasks waiting for room here: datagram senders whose destination this
    /// is, clients whose listener this is.
    room: WaiterSet,
}

impl Socket {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            refs: 1,
            bound: None,
            state: State::Idle,
            listen_cred: None,
            queue: VecDeque::new(),
            queued_bytes: 0,
            default_peer: None,
            rd_shut: false,
            wr_shut: false,
            readers: WaiterSet::new(),
            room: WaiterSet::new(),
        }
    }

    /// Whether one more datagram of `len` bytes fits.
    fn has_room(&self, len: usize) -> bool {
        self.queue.len() < MAX_DGRAM_QUEUE
            && self.queued_bytes.saturating_add(len) <= MAX_DGRAM_BYTES
    }

    /// The address this socket reports for itself.
    fn local(&self) -> Address {
        self.bound
            .as_ref()
            .map_or(Address::Unnamed, |(_, a)| a.clone())
    }
}

/// What a node's name leads to, with the name it is reported under.
struct Binding {
    /// The socket; 0 for the entry `perfile`'s self-test plants, which leads
    /// to nothing (socket ids start at 1).
    socket: u64,
    /// The node's current path, moved by renames: what a listing of bound
    /// sockets shows, and what `perfile` asks the table about.
    reported: PathBuf,
}

/// Every socket and every name.
struct Table {
    sockets: BTreeMap<u64, Socket>,
    /// Names to sockets. A path name's entry ends with its node (`perfile`)
    /// or its socket; an abstract name's with its socket.
    names: BTreeMap<Name, Binding>,
    next_id: u64,
}

impl Table {
    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    fn socket_mut(&mut self, h: UnixHandle) -> KernelResult<&mut Socket> {
        self.sockets.get_mut(&h.0).ok_or(KernelError::InvalidHandle)
    }

    fn socket(&self, h: UnixHandle) -> KernelResult<&Socket> {
        self.sockets.get(&h.0).ok_or(KernelError::InvalidHandle)
    }

    /// The live socket `name` leads to.
    fn lookup(&self, name: &Name) -> Option<u64> {
        let id = self.names.get(name)?.socket;
        self.sockets.contains_key(&id).then_some(id)
    }
}

/// Lock order: `TABLE` -> `PAIRS` -> `SCHED`.
static TABLE: Mutex<Table> = Mutex::new(Table {
    sockets: BTreeMap::new(),
    names: BTreeMap::new(),
    next_id: 1,
});

/// The calling process's credentials, or `None` in kernel context -- which
/// is reported as "unknown", never as root (see `service`'s
/// `current_process_cred` for why).
fn current_cred() -> Option<PeerCred> {
    let pid = crate::proc::thread::owner_process(sched::current_task_id())?;
    if pid == 0 {
        return None;
    }
    let (uid, gid) = crate::proc::pcb::process_uid_gid(pid)?;
    Some(PeerCred { pid, uid, gid })
}

/// The wait record a task parked on `h` publishes.
fn wait_on(h: UnixHandle) -> crate::wchan::Wait {
    crate::wchan::Wait::new(crate::wchan::WaitChannel::Socket, h.0)
}

// ---------------------------------------------------------------------------
// Life
// ---------------------------------------------------------------------------

/// A new socket of `kind`, unbound and unconnected.
///
/// # Errors
///
/// None today; `KernelResult` so a limit on sockets can be added without
/// changing callers.
pub fn create(kind: Kind) -> KernelResult<UnixHandle> {
    let mut t = TABLE.lock();
    let id = t.alloc_id();
    t.sockets.insert(id, Socket::new(kind));
    Ok(UnixHandle(id))
}

/// Two connected sockets of `kind` (`socketpair`): a stream pair, or two
/// datagram sockets each the other's default destination.
///
/// # Errors
///
/// As [`create`].
pub fn pair(kind: Kind) -> KernelResult<(UnixHandle, UnixHandle)> {
    let cred = current_cred();
    let mut t = TABLE.lock();
    let (a, b) = (t.alloc_id(), t.alloc_id());
    let mut sa = Socket::new(kind);
    let mut sb = Socket::new(kind);
    match kind {
        Kind::Stream => {
            let (ea, eb) = stream_socket::create();
            sa.state = State::Connected {
                stream: ea,
                peer: Address::Unnamed,
                peer_cred: cred,
            };
            sb.state = State::Connected {
                stream: eb,
                peer: Address::Unnamed,
                peer_cred: cred,
            };
        }
        Kind::Dgram => {
            sa.default_peer = Some(b);
            sb.default_peer = Some(a);
        }
    }
    t.sockets.insert(a, sa);
    t.sockets.insert(b, sb);
    Ok((UnixHandle(a), UnixHandle(b)))
}

/// One more holder of `h` (`dup`, `fork`).
///
/// # Errors
///
/// `InvalidHandle` if `h` is not a live socket or its count would overflow.
pub fn dup(h: UnixHandle) -> KernelResult<UnixHandle> {
    let mut t = TABLE.lock();
    let s = t.socket_mut(h)?;
    s.refs = s.refs.checked_add(1).ok_or(KernelError::InvalidHandle)?;
    Ok(h)
}

/// One holder of `h` lets go. At the last, the socket ends: its name leads
/// nowhere (the node stays, as on Linux), waiting connections are refused,
/// a connection is closed, and every task waiting on it wakes.
pub fn close(h: UnixHandle) {
    let mut wakes = Vec::new();
    let mut streams = Vec::new();
    {
        let mut t = TABLE.lock();
        let Some(s) = t.sockets.get_mut(&h.0) else {
            return;
        };
        s.refs = s.refs.saturating_sub(1);
        if s.refs > 0 {
            return;
        }
        let Some(mut s) = t.sockets.remove(&h.0) else {
            return;
        };
        if let Some((name, _)) = &s.bound
            && t.names.get(name).is_some_and(|b| b.socket == h.0)
        {
            t.names.remove(name);
        }
        wakes.append(&mut s.readers.take_all());
        wakes.append(&mut s.room.take_all());
        match core::mem::replace(&mut s.state, State::Idle) {
            State::Listening { backlog, .. } => {
                streams.extend(backlog.into_iter().map(|p| p.stream));
            }
            State::Connected { stream, .. } => streams.push(stream),
            State::Idle => {}
        }
    }
    // Outside TABLE: closing a pair end takes PAIRS and may wake its peer.
    for stream in streams {
        stream_socket::close(stream);
    }
    wake_all(wakes);
}

/// The kind of `h`, or `None` if it is not a live socket.
#[must_use]
pub fn kind(h: UnixHandle) -> Option<Kind> {
    TABLE.lock().sockets.get(&h.0).map(|s| s.kind)
}

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

/// Whether `h` is bound already: `bind` checks before it makes a node, so
/// that a second `bind` does not leave one behind.
///
/// # Errors
///
/// `InvalidHandle`.
pub fn is_bound(h: UnixHandle) -> KernelResult<bool> {
    Ok(TABLE.lock().socket(h)?.bound.is_some())
}

/// Bind `h` to the node `id` that `bind` just made at `path`, reporting
/// `addr`.
///
/// # Errors
///
/// `InvalidHandle`; `InvalidArgument` if `h` is bound already (`EINVAL`);
/// `AddrInUse` if the node leads to a socket already.
pub fn bind_node(h: UnixHandle, id: FileId, path: &Path, addr: Address) -> KernelResult<()> {
    let mut t = TABLE.lock();
    if t.socket(h)?.bound.is_some() {
        return Err(KernelError::InvalidArgument);
    }
    let name = Name::Node(id);
    if t.lookup(&name).is_some() {
        return Err(KernelError::AddrInUse);
    }
    t.names.insert(
        name.clone(),
        Binding {
            socket: h.0,
            reported: path.to_path_buf(),
        },
    );
    t.socket_mut(h)?.bound = Some((name, addr));
    Ok(())
}

/// Bind `h` to a new socket node at the resolved path `path`, with
/// permission bits `mode` (already umask-masked), reporting `reported` -- the
/// path as the binder gave it -- as its address. Both ABIs' `bind` to a path.
///
/// # Errors
///
/// `InvalidHandle`; `InvalidArgument` if `h` is bound already (checked before
/// a node is made, so a refused `bind` leaves none behind); `AddrInUse` if the
/// name exists; what [`crate::fs::Vfs::mknod_socket`] reports otherwise.
pub fn bind_path(h: UnixHandle, path: &Path, reported: Vec<u8>, mode: u16) -> KernelResult<()> {
    if is_bound(h)? {
        return Err(KernelError::InvalidArgument);
    }
    // No lock held here: the VFS is never entered with TABLE held.
    let id = crate::fs::Vfs::mknod_socket(path, mode).map_err(|e| match e {
        KernelError::AlreadyExists => KernelError::AddrInUse,
        e => e,
    })?;
    bind_node(h, id, path, Address::Path(reported)).inspect_err(|_| {
        // Bound meanwhile by another thread: the node this call made leads
        // nowhere, so it goes again. A failure to remove it leaves only an
        // inert node.
        let _ = crate::fs::Vfs::remove(path);
    })
}

/// The socket a resolved filesystem path leads to, for `connect` and
/// `sendto`: Linux's checks, in its order.
///
/// # Errors
///
/// `NotFound` if nothing is there; `ConnectionRefused` if it is not a
/// socket's node (or has no identity to find one by); `PermissionDenied` if
/// the caller may not write the node -- Linux asks for write permission, and
/// a read-only mount does not stop a connect (its `sb_permission` spares
/// sockets), so that refusal alone is passed over.
pub fn name_at(path: &Path) -> KernelResult<Name> {
    let st = crate::fs::Vfs::stat(path)?;
    if st.entry_type != crate::fs::vfs::EntryType::Socket {
        return Err(KernelError::ConnectionRefused);
    }
    match crate::fs::Vfs::access(path, crate::fs::vfs::W_OK) {
        Ok(()) | Err(KernelError::ReadOnlyFilesystem) => {}
        Err(e) => return Err(e),
    }
    crate::fs::Vfs::file_identity(path)?
        .map(Name::Node)
        .ok_or(KernelError::ConnectionRefused)
}

/// Bind `h` to the abstract name `name` (without its leading NUL).
///
/// # Errors
///
/// `InvalidHandle`; `InvalidArgument` if `h` is bound already or the name is
/// longer than [`MAX_ABSTRACT_NAME`]; `AddrInUse` if a live socket has it.
pub fn bind_abstract(h: UnixHandle, name: &[u8]) -> KernelResult<()> {
    if name.len() > MAX_ABSTRACT_NAME {
        return Err(KernelError::InvalidArgument);
    }
    let mut t = TABLE.lock();
    if t.socket(h)?.bound.is_some() {
        return Err(KernelError::InvalidArgument);
    }
    let key = Name::Abstract(name.to_vec());
    if t.lookup(&key).is_some() {
        return Err(KernelError::AddrInUse);
    }
    let mut reported = PathBuf::new();
    reported.extend_bytes(b"@");
    reported.extend_bytes(name);
    t.names.insert(
        key.clone(),
        Binding {
            socket: h.0,
            reported,
        },
    );
    t.socket_mut(h)?.bound = Some((key, Address::Abstract(name.to_vec())));
    Ok(())
}

/// What `h` reports as its own address (`getsockname`).
///
/// # Errors
///
/// `InvalidHandle`.
pub fn local_address(h: UnixHandle) -> KernelResult<Address> {
    Ok(TABLE.lock().socket(h)?.local())
}

/// The connected peer's address (`getpeername`).
///
/// # Errors
///
/// `InvalidHandle`; `NotConnected` if `h` has no peer.
pub fn peer_address(h: UnixHandle) -> KernelResult<Address> {
    let t = TABLE.lock();
    let s = t.socket(h)?;
    match &s.state {
        State::Connected { peer, .. } => Ok(peer.clone()),
        _ => {
            let peer = s.default_peer.ok_or(KernelError::NotConnected)?;
            t.sockets
                .get(&peer)
                .map(Socket::local)
                .ok_or(KernelError::NotConnected)
        }
    }
}

/// The connected peer's credentials as the kernel recorded them
/// (`SO_PEERCRED`): `Ok(None)` when it has none to report.
///
/// # Errors
///
/// `InvalidHandle`; `NotConnected` if `h` is not a connected stream.
pub fn peer_cred(h: UnixHandle) -> KernelResult<Option<PeerCred>> {
    match &TABLE.lock().socket(h)?.state {
        State::Connected { peer_cred, .. } => Ok(*peer_cred),
        _ => Err(KernelError::NotConnected),
    }
}

/// Whether `h` is listening (`SO_ACCEPTCONN`).
#[must_use]
pub fn is_listening(h: UnixHandle) -> bool {
    TABLE
        .lock()
        .sockets
        .get(&h.0)
        .is_some_and(|s| matches!(s.state, State::Listening { .. }))
}

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

/// Listen on the bound stream socket `h`, with up to `backlog` connections
/// waiting (clamped to 1..=[`MAX_BACKLOG`]). A second `listen` changes the
/// backlog, as on Linux.
///
/// # Errors
///
/// `InvalidHandle`; `NotSupported` for a datagram socket (`EOPNOTSUPP`);
/// `InvalidArgument` if `h` is unbound or connected.
pub fn listen(h: UnixHandle, backlog: usize) -> KernelResult<()> {
    let cred = current_cred();
    let mut t = TABLE.lock();
    let s = t.socket_mut(h)?;
    if s.kind != Kind::Stream {
        return Err(KernelError::NotSupported);
    }
    if s.bound.is_none() {
        return Err(KernelError::InvalidArgument);
    }
    let max = backlog.clamp(1, MAX_BACKLOG);
    match &mut s.state {
        State::Idle => {
            s.state = State::Listening {
                backlog: VecDeque::new(),
                max,
            };
            s.listen_cred = cred;
            Ok(())
        }
        State::Listening { max: m, .. } => {
            *m = max;
            Ok(())
        }
        State::Connected { .. } => Err(KernelError::InvalidArgument),
    }
}

/// Connect `h` to the socket `target` names.
///
/// A stream socket joins the listener's backlog, waiting -- unless
/// `nonblocking` -- while the backlog is full; it is connected when this
/// returns, and `accept` on the other side hands out the server's end. A
/// datagram socket only records `target` as where `send` goes.
///
/// # Errors
///
/// `InvalidHandle`; `ConnectionRefused` if nothing live is bound there, or
/// a stream socket bound there is not listening; `WrongSocketType`
/// (`EPROTOTYPE`) if the socket there is of the other kind;
/// `ConnectAlready` if `h` is a connected stream (`EISCONN`);
/// `InvalidArgument` if it is listening; `WouldBlock` for a full backlog
/// when `nonblocking`; `Interrupted`.
pub fn connect(h: UnixHandle, target: &Name, nonblocking: bool) -> KernelResult<()> {
    let kind = kind(h).ok_or(KernelError::InvalidHandle)?;
    match kind {
        Kind::Dgram => {
            let mut t = TABLE.lock();
            let peer = t.lookup(target).ok_or(KernelError::ConnectionRefused)?;
            if t.sockets.get(&peer).map(|s| s.kind) != Some(Kind::Dgram) {
                return Err(KernelError::WrongSocketType);
            }
            t.socket_mut(h)?.default_peer = Some(peer);
            Ok(())
        }
        Kind::Stream => connect_stream(h, target, nonblocking),
    }
}

/// The stream half of [`connect`].
fn connect_stream(h: UnixHandle, target: &Name, nonblocking: bool) -> KernelResult<()> {
    let pid = current_user_pid();
    let task = sched::current_task_id();
    let cred = current_cred();
    loop {
        let mut wakes = Vec::new();
        {
            let mut t = TABLE.lock();
            let me = t.socket(h)?;
            match me.state {
                State::Connected { .. } => return Err(KernelError::ConnectAlready),
                State::Listening { .. } => return Err(KernelError::InvalidArgument),
                State::Idle => {}
            }
            let my_addr = me.local();
            let Some(server) = t.lookup(target) else {
                return Err(KernelError::ConnectionRefused);
            };
            let s = t
                .sockets
                .get_mut(&server)
                .ok_or(KernelError::ConnectionRefused)?;
            s.room.remove(task);
            if s.kind != Kind::Stream {
                return Err(KernelError::WrongSocketType);
            }
            let server_cred = s.listen_cred;
            let server_addr = s.local();
            let State::Listening { backlog, max } = &mut s.state else {
                return Err(KernelError::ConnectionRefused);
            };
            if backlog.len() < *max {
                let (client_end, server_end) = stream_socket::create();
                backlog.push_back(Pending {
                    stream: server_end,
                    peer: my_addr,
                    peer_cred: cred,
                });
                wakes.append(&mut s.readers.take_all());
                let me = t.socket_mut(h)?;
                me.state = State::Connected {
                    stream: client_end,
                    peer: server_addr,
                    peer_cred: server_cred,
                };
                wakes.append(&mut me.readers.take_all());
                drop(t);
                wake_all(wakes);
                return Ok(());
            }
            if nonblocking {
                return Err(KernelError::WouldBlock);
            }
            if deliverable_signal_pending(pid) {
                return Err(KernelError::Interrupted);
            }
            s.room.insert(task);
        }
        park_interruptible(pid, task, wait_on(h));
    }
}

/// Take the next connection waiting on the listener `h`, as a new socket.
///
/// # Errors
///
/// `InvalidHandle`; `InvalidArgument` if `h` is not listening; `WouldBlock`
/// for an empty backlog when `nonblocking`; `Interrupted`.
pub fn accept(h: UnixHandle, nonblocking: bool) -> KernelResult<Accepted> {
    let pid = current_user_pid();
    let task = sched::current_task_id();
    loop {
        {
            let mut t = TABLE.lock();
            let s = t.socket_mut(h)?;
            s.readers.remove(task);
            // Linux: an accepted socket reports the listener's name.
            let bound = s.bound.clone();
            let State::Listening { backlog, .. } = &mut s.state else {
                return Err(KernelError::InvalidArgument);
            };
            let next = backlog.pop_front();
            match next {
                Some(p) => {
                    let wakes = s.room.take_all();
                    let id = t.alloc_id();
                    let mut sock = Socket::new(Kind::Stream);
                    sock.state = State::Connected {
                        stream: p.stream,
                        peer: p.peer.clone(),
                        peer_cred: p.peer_cred,
                    };
                    sock.bound = bound;
                    t.sockets.insert(id, sock);
                    drop(t);
                    wake_all(wakes);
                    return Ok(Accepted {
                        handle: UnixHandle(id),
                        peer: p.peer,
                    });
                }
                None => {
                    if nonblocking {
                        return Err(KernelError::WouldBlock);
                    }
                    if deliverable_signal_pending(pid) {
                        return Err(KernelError::Interrupted);
                    }
                    s.readers.insert(task);
                }
            }
        }
        park_interruptible(pid, task, wait_on(h));
    }
}

/// The stream end of a connected stream socket.
fn stream_of(h: UnixHandle) -> KernelResult<StreamSocketHandle> {
    match TABLE.lock().socket(h)?.state {
        State::Connected { stream, .. } => Ok(stream),
        _ => Err(KernelError::NotConnected),
    }
}

// ---------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------

/// Send `data` on `h`: down a connected stream, or as one datagram to the
/// destination `connect` set.
///
/// # Errors
///
/// `InvalidHandle`; `NotConnected`; `MsgSize` for a datagram over
/// [`MAX_DGRAM`]; `BrokenPipe` after `shutdown` of the write side;
/// `ChannelClosed` when the stream's peer has gone (`EPIPE`);
/// `ConnectionRefused` when the datagram destination has closed;
/// `WouldBlock` when `nonblocking` and there is no room; `Interrupted`.
pub fn send(h: UnixHandle, data: &[u8], nonblocking: bool) -> KernelResult<usize> {
    match kind(h).ok_or(KernelError::InvalidHandle)? {
        Kind::Stream => {
            let stream = stream_of(h)?;
            if data.is_empty() {
                return Ok(0);
            }
            if nonblocking {
                stream_socket::try_send(stream, data)
            } else {
                stream_socket::send(stream, data)
            }
        }
        Kind::Dgram => {
            let peer = TABLE
                .lock()
                .socket(h)?
                .default_peer
                .ok_or(KernelError::NotConnected)?;
            send_dgram(h, peer, data, nonblocking)
        }
    }
}

/// Send `data` from the datagram socket `h` to the socket `target` names.
///
/// # Errors
///
/// As [`send`]; `ConnectionRefused` if nothing live is bound at `target`,
/// `WrongSocketType` (`EPROTOTYPE`) if a stream socket is. On a stream
/// socket, `ConnectAlready` (`EISCONN`) if connected and `NotSupported`
/// (`EOPNOTSUPP`) if not, as Linux answers a stream `sendto` that names an
/// address.
pub fn send_to(
    h: UnixHandle,
    data: &[u8],
    target: &Name,
    nonblocking: bool,
) -> KernelResult<usize> {
    match kind(h).ok_or(KernelError::InvalidHandle)? {
        Kind::Stream => match stream_of(h) {
            Ok(_) => Err(KernelError::ConnectAlready),
            Err(_) => Err(KernelError::NotSupported),
        },
        Kind::Dgram => {
            let peer = TABLE
                .lock()
                .lookup(target)
                .ok_or(KernelError::ConnectionRefused)?;
            send_dgram(h, peer, data, nonblocking)
        }
    }
}

/// Queue one datagram from `h` on the socket `peer`, waiting for room unless
/// `nonblocking`.
fn send_dgram(h: UnixHandle, peer: u64, data: &[u8], nonblocking: bool) -> KernelResult<usize> {
    if data.len() > MAX_DGRAM {
        return Err(KernelError::MsgSize);
    }
    let pid = current_user_pid();
    let task = sched::current_task_id();
    let cred = current_cred();
    loop {
        {
            let mut t = TABLE.lock();
            let me = t.socket(h)?;
            if me.wr_shut {
                return Err(KernelError::BrokenPipe);
            }
            let from = me.local();
            let Some(dest) = t.sockets.get_mut(&peer) else {
                return Err(KernelError::ConnectionRefused);
            };
            dest.room.remove(task);
            if dest.kind != Kind::Dgram {
                return Err(KernelError::WrongSocketType);
            }
            if dest.rd_shut {
                // Linux discards datagrams to a socket whose reading half is
                // shut and reports success; there is no one to tell.
                return Ok(data.len());
            }
            if dest.has_room(data.len()) {
                dest.queue.push_back(Datagram {
                    data: data.to_vec(),
                    from,
                    cred,
                });
                dest.queued_bytes = dest.queued_bytes.saturating_add(data.len());
                let wakes = dest.readers.take_all();
                drop(t);
                wake_all(wakes);
                return Ok(data.len());
            }
            if nonblocking {
                return Err(KernelError::WouldBlock);
            }
            if deliverable_signal_pending(pid) {
                return Err(KernelError::Interrupted);
            }
            dest.room.insert(task);
        }
        park_interruptible(pid, task, wait_on(h));
    }
}

/// Receive into `buf` from `h`: bytes from a connected stream, or the next
/// datagram, cut to `buf` (the rest discarded, and `full_len` saying how much
/// there was). `peek` leaves what it read in place (`MSG_PEEK`).
///
/// # Errors
///
/// `InvalidHandle`; `NotConnected` for an unconnected stream; `WouldBlock`
/// when `nonblocking` and nothing is waiting; `Interrupted`. End of file -- a
/// stream's peer gone, or a shut reading half -- is `Ok` with `len` 0.
pub fn recv(
    h: UnixHandle,
    buf: &mut [u8],
    nonblocking: bool,
    peek: bool,
) -> KernelResult<Received> {
    match kind(h).ok_or(KernelError::InvalidHandle)? {
        Kind::Stream => {
            let (stream, from, cred) = match &TABLE.lock().socket(h)?.state {
                State::Connected {
                    stream,
                    peer,
                    peer_cred,
                } => (*stream, peer.clone(), *peer_cred),
                _ => return Err(KernelError::NotConnected),
            };
            let len = if buf.is_empty() {
                0
            } else if peek {
                // Peeking never waits here; a blocking peek waits by
                // receiving nothing until something arrives.
                loop {
                    match stream_socket::peek(stream, buf) {
                        Err(KernelError::WouldBlock) if !nonblocking => {
                            wait_readable(stream)?;
                        }
                        other => break other?,
                    }
                }
            } else if nonblocking {
                stream_socket::try_recv(stream, buf)?
            } else {
                stream_socket::recv(stream, buf)?
            };
            Ok(Received {
                len,
                full_len: len,
                from,
                cred,
            })
        }
        Kind::Dgram => recv_dgram(h, buf, nonblocking, peek),
    }
}

/// Wait until the stream end `stream` has something to receive or is at its
/// end.
fn wait_readable(stream: StreamSocketHandle) -> KernelResult<()> {
    let pid = current_user_pid();
    let task = sched::current_task_id();
    loop {
        if stream_socket::poll_status(stream) & 0x11 != 0 {
            return Ok(());
        }
        if deliverable_signal_pending(pid) {
            return Err(KernelError::Interrupted);
        }
        stream_socket::register_waiter(stream, task);
        // Re-check after registering: a send between the poll and the
        // registration would otherwise go unseen.
        if stream_socket::poll_status(stream) & 0x11 != 0 {
            stream_socket::deregister_waiter(stream, task);
            return Ok(());
        }
        park_interruptible(
            pid,
            task,
            crate::wchan::Wait::new(crate::wchan::WaitChannel::Socket, stream.raw()),
        );
        stream_socket::deregister_waiter(stream, task);
    }
}

/// The datagram half of [`recv`].
fn recv_dgram(
    h: UnixHandle,
    buf: &mut [u8],
    nonblocking: bool,
    peek: bool,
) -> KernelResult<Received> {
    let pid = current_user_pid();
    let task = sched::current_task_id();
    loop {
        {
            let mut t = TABLE.lock();
            let s = t.socket_mut(h)?;
            s.readers.remove(task);
            let taken = if peek {
                s.queue
                    .front()
                    .map(|d| (d.data.clone(), d.from.clone(), d.cred))
            } else {
                s.queue.pop_front().map(|d| (d.data, d.from, d.cred))
            };
            if let Some((data, from, cred)) = taken {
                let len = data.len().min(buf.len());
                if let (Some(dst), Some(src)) = (buf.get_mut(..len), data.get(..len)) {
                    dst.copy_from_slice(src);
                }
                let mut wakes = Vec::new();
                if !peek {
                    s.queued_bytes = s.queued_bytes.saturating_sub(data.len());
                    wakes = s.room.take_all();
                }
                drop(t);
                wake_all(wakes);
                return Ok(Received {
                    len,
                    full_len: data.len(),
                    from,
                    cred,
                });
            }
            if s.rd_shut {
                return Ok(Received {
                    len: 0,
                    full_len: 0,
                    from: Address::Unnamed,
                    cred: None,
                });
            }
            if nonblocking {
                return Err(KernelError::WouldBlock);
            }
            if deliverable_signal_pending(pid) {
                return Err(KernelError::Interrupted);
            }
            s.readers.insert(task);
        }
        park_interruptible(pid, task, wait_on(h));
    }
}

/// Shut down one or both halves of `h` (`how`: [`stream_socket::SHUT_RD`],
/// `SHUT_WR` or `SHUT_RDWR`).
///
/// # Errors
///
/// `InvalidHandle`; `InvalidArgument` for an unknown `how`; `NotConnected`
/// for a stream that is not connected.
pub fn shutdown(h: UnixHandle, how: u32) -> KernelResult<()> {
    use stream_socket::{SHUT_RD, SHUT_RDWR, SHUT_WR};
    if how != SHUT_RD && how != SHUT_WR && how != SHUT_RDWR {
        return Err(KernelError::InvalidArgument);
    }
    let mut wakes = Vec::new();
    let stream = {
        let mut t = TABLE.lock();
        let s = t.socket_mut(h)?;
        match (&s.state, s.kind) {
            (State::Connected { stream, .. }, _) => Some(*stream),
            (_, Kind::Dgram) => {
                if how != SHUT_WR {
                    s.rd_shut = true;
                }
                if how != SHUT_RD {
                    s.wr_shut = true;
                }
                wakes.append(&mut s.readers.take_all());
                wakes.append(&mut s.room.take_all());
                None
            }
            _ => return Err(KernelError::NotConnected),
        }
    };
    wake_all(wakes);
    match stream {
        Some(stream) => stream_socket::shutdown(stream, how),
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Readiness
// ---------------------------------------------------------------------------

/// `h`'s readiness, in [`stream_socket::poll_status`]'s encoding: 0x01
/// readable, 0x04 writable, 0x08 error, 0x10 hang-up.
#[must_use]
pub fn poll_status(h: UnixHandle) -> u16 {
    let stream = {
        let t = TABLE.lock();
        let Some(s) = t.sockets.get(&h.0) else {
            return 0x10;
        };
        match &s.state {
            State::Connected { stream, .. } => *stream,
            State::Listening { backlog, .. } => {
                return if backlog.is_empty() { 0 } else { 0x01 };
            }
            // An unconnected stream socket is writable and hung up, as
            // Linux's unix_poll reports one.
            State::Idle if s.kind == Kind::Stream => return 0x04 | 0x10,
            State::Idle => {
                let mut flags = 0u16;
                if !s.queue.is_empty() || s.rd_shut {
                    flags |= 0x01;
                }
                let writable = match s.default_peer.and_then(|p| t.sockets.get(&p)) {
                    Some(peer) => peer.has_room(0),
                    // Unconnected: a sendto names its own destination.
                    None => s.default_peer.is_none(),
                };
                if writable && !s.wr_shut {
                    flags |= 0x04;
                }
                return flags;
            }
        }
    };
    stream_socket::poll_status(stream)
}

/// Park `task` so that any change to `h` wakes it -- for `poll`, `select`
/// and `epoll`. A connected stream's changes are its pair's, so the task is
/// registered there as well. Pair with [`deregister_waiter`].
pub fn register_waiter(h: UnixHandle, task: TaskId) {
    let stream = {
        let mut t = TABLE.lock();
        let Some(s) = t.sockets.get_mut(&h.0) else {
            return;
        };
        s.readers.insert(task);
        s.room.insert(task);
        match s.state {
            State::Connected { stream, .. } => Some(stream),
            _ => None,
        }
    };
    if let Some(stream) = stream {
        stream_socket::register_waiter(stream, task);
    }
}

/// Undo [`register_waiter`]. Safe on a socket that has gone.
pub fn deregister_waiter(h: UnixHandle, task: TaskId) {
    let stream = {
        let mut t = TABLE.lock();
        let Some(s) = t.sockets.get_mut(&h.0) else {
            return;
        };
        s.readers.remove(task);
        s.room.remove(task);
        match s.state {
            State::Connected { stream, .. } => Some(stream),
            _ => None,
        }
    };
    if let Some(stream) = stream {
        stream_socket::deregister_waiter(stream, task);
    }
}

// ---------------------------------------------------------------------------
// Per-file state: the table of node names
// ---------------------------------------------------------------------------

/// The table of node names, as [`crate::fs::perfile`] keeps it attached to
/// the right nodes.
pub(crate) const PER_FILE_STATE: crate::fs::perfile::Table = crate::fs::perfile::Table {
    name: "unix_socket",
    forget: forget_node,
    rename: rename_nodes,
    unmounted: forget_filesystem,
    plant: plant_node,
    finds: finds_node,
    reports: reports_node,
};

/// A node is gone: its name leads nowhere now. The socket keeps the address
/// it reports, as on Linux.
fn forget_node(id: Option<FileId>, _path: &Path) {
    if let Some(id) = id {
        TABLE.lock().names.remove(&Name::Node(id));
    }
}

/// Nodes were renamed: move the names they are reported under. What leads
/// to a socket is the node's identity, which a rename does not change.
fn rename_nodes(rename: &crate::fs::perfile::NameMap<'_>) {
    for binding in TABLE.lock().names.values_mut() {
        if let Some(new) = rename(&binding.reported) {
            binding.reported = new;
        }
    }
}

/// A filesystem went away: its nodes' names lead nowhere.
fn forget_filesystem(fs_id: u64) {
    TABLE
        .lock()
        .names
        .retain(|name, _| !matches!(name, Name::Node(id) if id.fs_id == fs_id));
}

/// Self-test support: a name that leads to no socket, on the file at `path`.
fn plant_node(path: &Path) -> KernelResult<()> {
    let id = crate::fs::Vfs::file_identity(path)?.ok_or(KernelError::NotSupported)?;
    TABLE.lock().names.insert(
        Name::Node(id),
        Binding {
            socket: 0,
            reported: path.to_path_buf(),
        },
    );
    Ok(())
}

/// Self-test support: whether the file at `path` has a name here.
fn finds_node(path: &Path) -> bool {
    match crate::fs::Vfs::file_identity(path) {
        Ok(Some(id)) => TABLE.lock().names.contains_key(&Name::Node(id)),
        _ => false,
    }
}

/// Self-test support: whether a name here is reported as `name`.
fn reports_node(name: &Path) -> bool {
    TABLE
        .lock()
        .names
        .values()
        .any(|b| b.reported.as_path() == name)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// The socket objects, in kernel context: datagrams and streams over
/// abstract and path names, a node's rename and removal, the backlog's
/// limit, refusals, `socketpair`, readiness, and what closing ends.
///
/// Nothing here blocks: every call that could wait is made when it need
/// not, or with `nonblocking`.
///
/// # Errors
///
/// `InternalError` naming the first check that failed.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[unix_socket] Running self-test...");
    let mut opened: Vec<UnixHandle> = Vec::new();
    let result = run_self_test(&mut opened);
    for h in opened {
        close(h);
    }
    for path in ["/tmp/unix-selftest.sock", "/tmp/unix-selftest-moved.sock"] {
        // Gone already unless a check failed first.
        let _ = crate::fs::Vfs::remove(path);
    }
    if let Err(why) = result {
        crate::serial_println!("[unix_socket]   FAIL: {}", why);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[unix_socket] Self-test PASSED");
    Ok(())
}

/// Open a socket of `kind` for the self-test, recording it in `opened` for
/// the caller to close whatever happens.
fn opened_socket(opened: &mut Vec<UnixHandle>, kind: Kind) -> Result<UnixHandle, &'static str> {
    let h = create(kind).map_err(|_| "create failed")?;
    opened.push(h);
    Ok(h)
}

/// Close `h` early, and stop the caller closing it again.
fn close_early(opened: &mut Vec<UnixHandle>, h: UnixHandle) {
    close(h);
    opened.retain(|&o| o != h);
}

/// The checks, recording every socket they open in `opened`.
#[allow(clippy::too_many_lines)] // one linear script; splitting it hides the order
fn run_self_test(opened: &mut Vec<UnixHandle>) -> Result<(), &'static str> {
    let mut buf = [0u8; 16];

    // --- datagrams by abstract name ---
    let server = opened_socket(opened, Kind::Dgram)?;
    let client = opened_socket(opened, Kind::Dgram)?;
    let name = Name::Abstract(b"slate-selftest-dgram".to_vec());
    bind_abstract(server, b"slate-selftest-dgram").map_err(|_| "bind_abstract failed")?;
    if bind_abstract(client, b"slate-selftest-dgram") != Err(KernelError::AddrInUse) {
        return Err("a second socket took a name in use");
    }
    if bind_abstract(server, b"other") != Err(KernelError::InvalidArgument) {
        return Err("a bound socket was bound again");
    }
    if recv(server, &mut buf, true, false) != Err(KernelError::WouldBlock) {
        return Err("an empty datagram socket did not answer WouldBlock");
    }
    if send_to(client, b"hello", &name, true) != Ok(5) {
        return Err("send_to an abstract name failed");
    }
    if poll_status(server) & 0x01 == 0 {
        return Err("a socket with a datagram waiting is not readable");
    }
    let peeked = recv(server, &mut buf, true, true).map_err(|_| "peek failed")?;
    let got = recv(server, &mut buf, true, false).map_err(|_| "recv failed")?;
    if peeked.len != 5 || got.len != 5 || buf.get(..5) != Some(&b"hello"[..]) {
        return Err("the datagram did not arrive whole, or peek consumed it");
    }
    if got.from != Address::Unnamed || got.cred.is_some() {
        return Err("an unbound kernel sender was reported with a name or credentials");
    }
    // Cut to the buffer, with the whole length reported.
    send_to(client, b"0123456789", &name, true).map_err(|_| "send_to failed")?;
    let mut small = [0u8; 4];
    let cut = recv(server, &mut small, true, false).map_err(|_| "recv failed")?;
    if cut.len != 4 || cut.full_len != 10 || small != *b"0123" {
        return Err("a datagram larger than the buffer was not cut with its length reported");
    }
    if recv(server, &mut buf, true, false) != Err(KernelError::WouldBlock) {
        return Err("the rest of a cut datagram was not discarded");
    }
    if send(client, b"x", true) != Err(KernelError::NotConnected) {
        return Err("send without a destination was not NotConnected");
    }
    connect(client, &name, true).map_err(|_| "datagram connect failed")?;
    if send(client, b"x", true) != Ok(1)
        || recv(server, &mut buf, true, false).map(|r| r.len) != Ok(1)
    {
        return Err("send to the connected destination did not arrive");
    }
    if send_to(client, b"y", &Name::Abstract(b"nobody-here".to_vec()), true)
        != Err(KernelError::ConnectionRefused)
    {
        return Err("send_to a name nothing has was not refused");
    }

    // --- datagrams by path: the node's identity leads to the socket ---
    let path = Path::new("/tmp/unix-selftest.sock");
    let moved = Path::new("/tmp/unix-selftest-moved.sock");
    let bound = opened_socket(opened, Kind::Dgram)?;
    let id = crate::fs::Vfs::mknod_socket(path, 0o755).map_err(|_| "mknod_socket failed")?;
    bind_node(bound, id, path, Address::Path(path.as_bytes().to_vec()))
        .map_err(|_| "bind_node failed")?;
    if local_address(bound) != Ok(Address::Path(path.as_bytes().to_vec())) {
        return Err("a path-bound socket does not report its path");
    }
    if send_to(client, b"p", &Name::Node(id), true) != Ok(1) {
        return Err("send_to a node failed");
    }
    crate::fs::Vfs::rename(path, moved).map_err(|_| "rename of the node failed")?;
    let after = crate::fs::Vfs::file_identity(moved)
        .ok()
        .flatten()
        .ok_or("the renamed node has no identity")?;
    if after != id || send_to(client, b"q", &Name::Node(after), true) != Ok(1) {
        return Err("a renamed node no longer leads to its socket");
    }
    let first = recv(bound, &mut buf, true, false).map_err(|_| "recv failed")?;
    if first.len != 1 || buf.first() != Some(&b'p') {
        return Err("datagrams to a node did not arrive in order");
    }
    crate::fs::Vfs::remove(moved).map_err(|_| "remove of the node failed")?;
    if send_to(client, b"r", &Name::Node(id), true) != Err(KernelError::ConnectionRefused) {
        return Err("a removed node still leads to its socket");
    }

    // --- streams ---
    let listener = opened_socket(opened, Kind::Stream)?;
    let stream_name = Name::Abstract(b"slate-selftest-stream".to_vec());
    let listener_addr = Address::Abstract(b"slate-selftest-stream".to_vec());
    if listen(listener, 4) != Err(KernelError::InvalidArgument) {
        return Err("an unbound socket was allowed to listen");
    }
    bind_abstract(listener, b"slate-selftest-stream").map_err(|_| "bind failed")?;
    listen(listener, 1).map_err(|_| "listen failed")?;
    if accept(listener, true).map(|a| a.handle) != Err(KernelError::WouldBlock) {
        return Err("accept with nothing waiting did not answer WouldBlock");
    }
    let c1 = opened_socket(opened, Kind::Stream)?;
    let c2 = opened_socket(opened, Kind::Stream)?;
    connect(c1, &stream_name, true).map_err(|_| "stream connect failed")?;
    if connect(c2, &stream_name, true) != Err(KernelError::WouldBlock) {
        return Err("a connection past the backlog's limit did not answer WouldBlock");
    }
    if connect(c1, &stream_name, true) != Err(KernelError::ConnectAlready) {
        return Err("a connected socket connected again");
    }
    if poll_status(listener) & 0x01 == 0 {
        return Err("a listener with a connection waiting is not readable");
    }
    let accepted = accept(listener, true).map_err(|_| "accept failed")?;
    opened.push(accepted.handle);
    let s1 = accepted.handle;
    if accepted.peer != Address::Unnamed {
        return Err("an unbound client was reported with a name");
    }
    if local_address(s1) != Ok(listener_addr.clone()) || peer_address(c1) != Ok(listener_addr) {
        return Err("the accepted socket or the client does not report the listener's name");
    }
    if peer_cred(c1) != Ok(None) || peer_cred(s1) != Ok(None) {
        return Err("kernel-context peers were reported with credentials");
    }
    if send(c1, b"ping", true) != Ok(4) {
        return Err("send on a connected stream failed");
    }
    let r = recv(s1, &mut buf, true, false).map_err(|_| "stream recv failed")?;
    if r.len != 4 || buf.get(..4) != Some(&b"ping"[..]) {
        return Err("the stream did not carry the bytes");
    }
    if send(s1, b"pong", true) != Ok(4) || recv(c1, &mut buf, true, false).map(|r| r.len) != Ok(4) {
        return Err("the stream did not carry bytes back");
    }
    // The listener's backlog has room again; c2 gets in.
    connect(c2, &stream_name, true).map_err(|_| "a connection after accept was refused")?;
    // Closing the client ends the server's stream.
    close_early(opened, c1);
    if recv(s1, &mut buf, true, false).map(|r| r.len) != Ok(0) {
        return Err("the server did not see end of file after the client closed");
    }
    // Closing the listener refuses what is still waiting: c2 sees the end.
    close_early(opened, listener);
    if recv(c2, &mut buf, true, false).map(|r| r.len) != Ok(0) {
        return Err("a client waiting in a closed listener's backlog did not see the end");
    }
    let c3 = opened_socket(opened, Kind::Stream)?;
    if connect(c3, &stream_name, true) != Err(KernelError::ConnectionRefused) {
        return Err("a closed listener's name still took connections");
    }
    if connect(c3, &name, true) != Err(KernelError::WrongSocketType) {
        return Err("a stream connecting to a datagram socket was not WrongSocketType");
    }

    // --- socketpair ---
    let (a, b) = pair(Kind::Stream).map_err(|_| "a stream pair failed")?;
    opened.push(a);
    opened.push(b);
    if send(a, b"ab", true) != Ok(2) || recv(b, &mut buf, true, false).map(|r| r.len) != Ok(2) {
        return Err("a stream pair did not carry bytes");
    }
    let (d1, d2) = pair(Kind::Dgram).map_err(|_| "a datagram pair failed")?;
    opened.push(d1);
    opened.push(d2);
    if send(d2, b"dg", true) != Ok(2) || recv(d1, &mut buf, true, false).map(|r| r.len) != Ok(2) {
        return Err("a datagram pair did not carry a datagram");
    }
    shutdown(d1, stream_socket::SHUT_RD).map_err(|_| "datagram shutdown failed")?;
    if recv(d1, &mut buf, true, false).map(|r| r.len) != Ok(0) {
        return Err("a datagram socket with its reading half shut did not read the end");
    }

    // --- the abstract name is free once its socket has gone ---
    close_early(opened, server);
    let again = opened_socket(opened, Kind::Dgram)?;
    bind_abstract(again, b"slate-selftest-dgram")
        .map_err(|_| "a closed socket's abstract name was not freed")?;
    Ok(())
}

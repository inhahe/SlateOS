//! Unix-domain sockets with names: `AF_UNIX` `SOCK_STREAM`, `SOCK_DGRAM` and
//! `SOCK_SEQPACKET`,
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
//! ## The three kinds
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
//! - **Sequenced packets** (`SOCK_SEQPACKET`). Connected like a stream --
//!   `listen`, `connect`, `accept`, `socketpair` -- but each send arrives
//!   whole, like a datagram: it is one, queued at the peer socket. `connect`
//!   makes the server's socket at once, waiting in the backlog (an *embryo*),
//!   so what the client sends before `accept` is queued there; `accept` hands
//!   it out. A peer gone, or its writing half shut, is end of file once its
//!   queue is read; sending to a peer gone, or whose reading half is shut, is
//!   `EPIPE`.
//!
//! ## Credentials
//!
//! A connected stream socket knows its peer's process, uid and gid as they
//! were at `connect` (on the server side) or `listen` (on the client side),
//! which is Linux's `SO_PEERCRED`. Every datagram carries its sender's, which
//! is what `SCM_CREDENTIALS` reports. The kernel records them; a sender may
//! instead state them for one datagram (`SCM_CREDENTIALS` on a send), but
//! only what Linux would let it claim -- its own, or, for root, another live
//! process's ([`check_stated_cred`]). Kernel context has none to record.
//!
//! A stream reports the credentials of its connection, not of each write:
//! the bytes are a [`super::stream_socket`] pair's, which marks only the
//! stretches descriptors ride on. Linux records them per write, which
//! differs only when one connection is written by several processes, or root
//! states another process's credentials on a stream.
//!
//! ## Descriptors (`SCM_RIGHTS`)
//!
//! A send may carry descriptors: a [`Bundle`] of references the syscall layer
//! took from the sender's ([`super::passed`]). A datagram carries its bundle
//! whole. On a stream the bundle rides on the bytes that send wrote
//! (`stream_socket`'s marks), and a receive that reaches them takes the
//! bundle and stops at their end, as Linux's does. A receive hands the bundle
//! up ([`Received::rights`]); one that only looks (`MSG_PEEK`) hands up a
//! copy, from which the receiver takes references of its own. Whatever is
//! never received is released: a socket's queue when the socket ends, a
//! stream's marks when the end that would read them closes.
//!
//! Descriptors come off a queue only with `TABLE` held -- a datagram is
//! always taken under it, and a stream's receive takes its marks under it --
//! which is what lets [`collect_garbage`] look at every queue at once.
//!
//! ## Garbage
//!
//! A socket can be kept alive by nothing but references in flight to it: its
//! own descriptor sent to itself and then closed, or two sockets each queued
//! at the other. No process can ever receive those, so nothing would ever
//! end them -- Linux's `unix_gc` finds such sets, and [`collect_garbage`]
//! does the same. A socket is a *candidate* when every holder of it is a
//! reference queued at some socket. A candidate a reference queued at a
//! non-candidate leads to, directly or through other candidates, can still be
//! received; the rest are garbage, and the descriptors queued at them are
//! released, which ends them. It runs whenever a socket loses a holder while
//! a socket is in flight anywhere. A bundle a receive is looking at right now
//! (another copy exists) keeps what it carries out of the reckoning, since
//! that receive may be about to give it to a process.
//!
//! ## Lock order
//!
//! `TABLE` -> `stream_socket`'s `PAIRS` -> `SCHED`. Nothing here is called
//! with a filesystem lock held, and the filesystem is never entered with
//! `TABLE` held: [`crate::fs::perfile`] calls in only after the VFS has
//! released its locks. A [`Bundle`] dropped under `TABLE` only queues its
//! releases; the calls that can drop one drain them once `TABLE` is released.

use super::channel::PeerCred;
use super::passed::{self, Bundle};
use super::stream_socket::{self, StreamSocketHandle};
use super::waiters::{
    WaiterSet, current_user_pid, deliverable_signal_pending, park_interruptible, wake_all,
};
use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::FileId;
use crate::sched::{self, task::TaskId};
use crate::sync::Mutex;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

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

/// Which of the three kinds a socket is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `SOCK_STREAM`: a connection carrying a byte stream.
    Stream,
    /// `SOCK_DGRAM`: datagrams, each kept whole.
    Dgram,
    /// `SOCK_SEQPACKET`: a connection whose sends arrive whole.
    SeqPacket,
}

impl Kind {
    /// Whether this kind connects (`listen`, `connect`, `accept`).
    #[must_use]
    pub const fn connects(self) -> bool {
        matches!(self, Self::Stream | Self::SeqPacket)
    }

    /// Whether each send arrives whole (a datagram).
    #[must_use]
    pub const fn keeps_messages(self) -> bool {
        matches!(self, Self::Dgram | Self::SeqPacket)
    }
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
    /// The descriptors the message carried (`SCM_RIGHTS`), for the caller to
    /// install or drop -- or, from a peek, a copy, whose
    /// [`Bundle::into_passed`] takes new references. Call that, and drop what
    /// it gives, with no lock held.
    pub rights: Option<Bundle>,
}

/// One datagram waiting in a queue.
struct Datagram {
    data: Vec<u8>,
    from: Address,
    cred: Option<PeerCred>,
    rights: Option<Bundle>,
}

/// A connection waiting in a listener's backlog.
struct Pending {
    /// The server's side of it.
    link: Link,
    /// The client's address.
    peer: Address,
    /// The client's credentials at `connect`.
    peer_cred: Option<PeerCred>,
}

/// The server's side of a connection not yet accepted.
#[derive(Clone, Copy)]
enum Link {
    /// A stream: the server's end of the pair; the client holds the other.
    Stream(StreamSocketHandle),
    /// Sequenced packets: the server's socket itself, made at `connect` so
    /// the client can send at once -- an embryo, held by the backlog.
    Socket(u64),
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
    /// A sequenced-packet socket after `connect`, `accept` or `socketpair`:
    /// what it sends is queued at `peer_id`.
    Paired {
        peer_id: u64,
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
    /// Datagram and sequenced-packet sockets: what has arrived and not been
    /// received.
    queue: VecDeque<Datagram>,
    queued_bytes: usize,
    /// A sequenced-packet socket waiting in a listener's backlog, not yet
    /// accepted: its holder is the backlog, and garbage collection counts
    /// what is queued at it as queued at the listener.
    embryo: bool,
    /// Datagram sockets: where `send` without an address goes (`connect`).
    default_peer: Option<u64>,
    /// `shutdown`: a datagram socket's own halves. A stream's are its pair's.
    rd_shut: bool,
    wr_shut: bool,
    /// `SO_PASSCRED`: each receive asks for the sender's credentials as a
    /// control message. The credentials are recorded either way; this only
    /// says whether a receive hands them back.
    passcred: bool,
    /// `SO_RCVTIMEO`: how long a blocking receive or `accept` waits before
    /// `WouldBlock` -- `None` for as long as it takes (the default), `Some(0)`
    /// not at all.
    rcvtimeo: Option<u64>,
    /// `SO_SNDTIMEO`: the same for a blocking send or `connect`.
    sndtimeo: Option<u64>,
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
            embryo: false,
            default_peer: None,
            rd_shut: false,
            wr_shut: false,
            passcred: false,
            rcvtimeo: None,
            sndtimeo: None,
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
///
/// The tracked type, which lockdep watches: `PAIRS` (`stream_socket`) is
/// taken under it, so it is no leaf. Until 2026-10-02 it was a
/// `PreemptSpinMutex`, whose claim is that nothing nests inside it, and the
/// boot's leaf check caught the nesting (design-decisions §975). Never
/// taken in interrupt context -- a wait's timer wakes the task and touches
/// nothing here -- and a socket call is no hot path, so the tracking's cost
/// (about 235 ns an acquisition) is nothing to weigh.
static TABLE: Mutex<Table> = Mutex::named(
    Table {
        sockets: BTreeMap::new(),
        names: BTreeMap::new(),
        next_id: 1,
    },
    b"UNIX_SOCKETS",
);

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

/// Check credentials the caller states for one message it sends
/// (`SCM_CREDENTIALS` on a send), as Linux's `scm_check_creds` and
/// `__scm_send` do: the pid must be the caller's own, or -- for root
/// (`CAP_SYS_ADMIN`) -- any live process's; the uid and gid the caller's own,
/// or any for root (`CAP_SETUID`, `CAP_SETGID`). Root is uid 0, as the Linux
/// layer's `set*id` calls take it; the caller has one uid and one gid, so
/// "one of its real, effective or saved ids" is that one. Kernel context may
/// state anything that names a live process.
///
/// # Errors
///
/// `InvalidArgument` for a uid or gid of -1 (no id); `NotPermitted`
/// (`EPERM`) for a claim the caller may not make, or a caller whose identity
/// cannot be read; `NoSuchProcess` (`ESRCH`) for a pid naming no process.
pub fn check_stated_cred(stated: PeerCred) -> KernelResult<PeerCred> {
    let caller = match crate::proc::thread::owner_process(sched::current_task_id()) {
        None | Some(0) => None,
        Some(pid) => {
            let (uid, gid) =
                crate::proc::pcb::process_uid_gid(pid).ok_or(KernelError::NotPermitted)?;
            Some(PeerCred { pid, uid, gid })
        }
    };
    check_stated_cred_for(caller, stated, |pid| crate::proc::pcb::state(pid).is_some())
}

/// [`check_stated_cred`] for a caller whose own credentials are `caller`
/// (`None`: kernel context), with `alive` saying whether a pid names a
/// process -- a zombie counts, as Linux finds a pid until it is reaped.
fn check_stated_cred_for(
    caller: Option<PeerCred>,
    stated: PeerCred,
    alive: impl Fn(crate::proc::pcb::ProcessId) -> bool,
) -> KernelResult<PeerCred> {
    if stated.uid == u32::MAX || stated.gid == u32::MAX {
        return Err(KernelError::InvalidArgument);
    }
    if let Some(me) = caller {
        // Root may claim anything; anyone else only exactly themselves.
        let may =
            me.uid == 0 || (stated.pid == me.pid && stated.uid == me.uid && stated.gid == me.gid);
        if !may {
            return Err(KernelError::NotPermitted);
        }
    }
    if !alive(stated.pid) {
        return Err(KernelError::NoSuchProcess);
    }
    Ok(stated)
}

/// The wait record a task parked on `h` publishes.
fn wait_on(h: UnixHandle) -> crate::wchan::Wait {
    crate::wchan::Wait::new(crate::wchan::WaitChannel::Socket, h.0)
}

/// Which of a socket's two timeouts a wait obeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `SO_RCVTIMEO`: receiving, and `accept`.
    Receive,
    /// `SO_SNDTIMEO`: sending, and `connect`.
    Send,
}

/// Set `h`'s timeout for `dir`: `None` waits as long as it takes, `Some(0)`
/// not at all, `Some(ns)` at most `ns` before a blocking call answers
/// `WouldBlock` (`EAGAIN`), as Linux's `SO_RCVTIMEO`/`SO_SNDTIMEO`.
///
/// # Errors
///
/// `InvalidHandle`.
pub fn set_timeout(h: UnixHandle, dir: Direction, limit: Option<u64>) -> KernelResult<()> {
    let mut t = TABLE.lock();
    let s = t.socket_mut(h)?;
    match dir {
        Direction::Receive => s.rcvtimeo = limit,
        Direction::Send => s.sndtimeo = limit,
    }
    Ok(())
}

/// `h`'s timeout for `dir`, as [`set_timeout`] takes it.
///
/// # Errors
///
/// `InvalidHandle`.
pub fn timeout(h: UnixHandle, dir: Direction) -> KernelResult<Option<u64>> {
    let t = TABLE.lock();
    let s = t.socket(h)?;
    Ok(match dir {
        Direction::Receive => s.rcvtimeo,
        Direction::Send => s.sndtimeo,
    })
}

/// The limit a blocking call on `h` waits under: its timeout for `dir`, or
/// none for a non-blocking call (which never waits) or a stale handle (whose
/// call fails before it would).
fn limit_for(h: UnixHandle, dir: Direction, nonblocking: bool) -> Option<u64> {
    if nonblocking {
        return None;
    }
    timeout(h, dir).ok().flatten()
}

/// A blocking wait's limit: the socket's timeout made a deadline when the
/// wait begins, and a timer that wakes the waiter at it -- cancelled when the
/// wait ends, however it ends.
struct Limit {
    deadline: Option<u64>,
    timer: Option<crate::hrtimer::HrTimerHandle>,
}

impl Limit {
    /// The limit for a wait by `task` that may last `limit` (see
    /// [`set_timeout`]).
    fn new(limit: Option<u64>, task: TaskId) -> Self {
        /// The timer's wake, from any context: a direct one, or deferred if
        /// the scheduler's lock is taken.
        fn wake(task: u64) {
            if !sched::try_wake(task) {
                sched::defer_wake(task);
            }
        }
        match limit {
            None => Self {
                deadline: None,
                timer: None,
            },
            Some(ns) => Self {
                deadline: Some(crate::hrtimer::now_ns().saturating_add(ns)),
                timer: (ns > 0).then(|| crate::hrtimer::schedule_ns(ns, wake, task)),
            },
        }
    }

    /// Whether the wait has run out of time.
    fn passed(&self) -> bool {
        self.deadline.is_some_and(|d| crate::hrtimer::now_ns() >= d)
    }
}

impl Drop for Limit {
    fn drop(&mut self) {
        if let Some(t) = self.timer.take() {
            // A timer that already fired is simply not found.
            crate::hrtimer::cancel(t);
        }
    }
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

/// Two connected sockets of `kind` (`socketpair`): a stream pair, two
/// datagram sockets each the other's default destination, or two
/// sequenced-packet sockets paired.
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
        Kind::SeqPacket => {
            sa.state = State::Paired {
                peer_id: b,
                peer: Address::Unnamed,
                peer_cred: cred,
            };
            sb.state = State::Paired {
                peer_id: a,
                peer: Address::Unnamed,
                peer_cred: cred,
            };
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
    // Sequenced-packet connections waiting in a closed listener's backlog:
    // ended after TABLE is let go.
    let mut embryos = Vec::new();
    let ended = {
        let mut t = TABLE.lock();
        let Some(s) = t.sockets.get_mut(&h.0) else {
            return;
        };
        s.refs = s.refs.saturating_sub(1);
        if s.refs > 0 {
            false
        } else if let Some(mut s) = t.sockets.remove(&h.0) {
            if let Some((name, _)) = &s.bound
                && t.names.get(name).is_some_and(|b| b.socket == h.0)
            {
                t.names.remove(name);
            }
            wakes.append(&mut s.readers.take_all());
            wakes.append(&mut s.room.take_all());
            match core::mem::replace(&mut s.state, State::Idle) {
                State::Listening { backlog, .. } => {
                    for p in backlog {
                        match p.link {
                            Link::Stream(stream) => streams.push(stream),
                            Link::Socket(id) => embryos.push(UnixHandle(id)),
                        }
                    }
                }
                State::Connected { stream, .. } => streams.push(stream),
                State::Paired { peer_id, .. } => {
                    // The peer reads to the end of its queue, then end of
                    // file; its senders find the pipe broken.
                    if let Some(p) = t.sockets.get_mut(&peer_id) {
                        wakes.append(&mut p.readers.take_all());
                        wakes.append(&mut p.room.take_all());
                    }
                }
                State::Idle => {}
            }
            // `s` ends here, its queue with it: the descriptors queued there
            // are released by the drain below.
            true
        } else {
            false
        }
    };
    if ended {
        // Outside TABLE: closing a pair end takes PAIRS and may wake its peer.
        for stream in streams {
            stream_socket::close(stream);
        }
        wake_all(wakes);
        passed::drain();
        // Each held only by the backlog: this ends it, and its client sees
        // the end.
        for embryo in embryos {
            close(embryo);
        }
    }
    // One holder fewer can leave sockets that only references in flight
    // hold -- this one, or what its queue held.
    collect_garbage();
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
/// the caller may not write the node, and `NotPermitted` if it is immutable
/// -- Linux asks for write permission, and a read-only mount does not stop a
/// connect (its `sb_permission` spares sockets), so that refusal alone is
/// passed over.
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
        State::Connected { peer, .. } | State::Paired { peer, .. } => Ok(peer.clone()),
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
        State::Connected { peer_cred, .. } | State::Paired { peer_cred, .. } => Ok(*peer_cred),
        _ => Err(KernelError::NotConnected),
    }
}

/// Ask (or stop asking) for the sender's credentials with each receive on
/// `h` (`SO_PASSCRED`).
///
/// # Errors
///
/// `InvalidHandle`.
pub fn set_passcred(h: UnixHandle, on: bool) -> KernelResult<()> {
    TABLE.lock().socket_mut(h)?.passcred = on;
    Ok(())
}

/// Whether receives on `h` hand back the sender's credentials.
#[must_use]
pub fn passcred(h: UnixHandle) -> bool {
    TABLE.lock().sockets.get(&h.0).is_some_and(|s| s.passcred)
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

/// Listen on the bound stream or sequenced-packet socket `h`, with up to
/// `backlog` connections waiting (clamped to 1..=[`MAX_BACKLOG`]). A second
/// `listen` changes the backlog, as on Linux.
///
/// # Errors
///
/// `InvalidHandle`; `NotSupported` for a datagram socket (`EOPNOTSUPP`);
/// `InvalidArgument` if `h` is unbound or connected.
pub fn listen(h: UnixHandle, backlog: usize) -> KernelResult<()> {
    let cred = current_cred();
    let mut t = TABLE.lock();
    let s = t.socket_mut(h)?;
    if !s.kind.connects() {
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
        State::Connected { .. } | State::Paired { .. } => Err(KernelError::InvalidArgument),
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
        Kind::Stream | Kind::SeqPacket => connect_stream(h, kind, target, nonblocking),
    }
}

/// The connecting half of [`connect`], for a stream or a sequenced-packet
/// socket (`kind`).
fn connect_stream(h: UnixHandle, kind: Kind, target: &Name, nonblocking: bool) -> KernelResult<()> {
    let pid = current_user_pid();
    let task = sched::current_task_id();
    let cred = current_cred();
    // SO_SNDTIMEO bounds the wait for room in the backlog, as Linux's
    // unix_stream_connect waits under sock_sndtimeo.
    let limit = Limit::new(limit_for(h, Direction::Send, nonblocking), task);
    loop {
        let mut wakes = Vec::new();
        {
            let mut t = TABLE.lock();
            let me = t.socket(h)?;
            match me.state {
                State::Connected { .. } | State::Paired { .. } => {
                    return Err(KernelError::ConnectAlready);
                }
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
            if s.kind != kind {
                return Err(KernelError::WrongSocketType);
            }
            let server_cred = s.listen_cred;
            let server_addr = s.local();
            let State::Listening { backlog, max } = &s.state else {
                return Err(KernelError::ConnectionRefused);
            };
            if backlog.len() < *max {
                // The server's side: a stream pair's end, or -- sequenced
                // packets -- the server's socket itself, so what the client
                // sends before `accept` has somewhere to wait.
                let (link, mine) = if kind == Kind::Stream {
                    let (client_end, server_end) = stream_socket::create();
                    (
                        Link::Stream(server_end),
                        State::Connected {
                            stream: client_end,
                            peer: server_addr,
                            peer_cred: server_cred,
                        },
                    )
                } else {
                    let id = t.alloc_id();
                    let mut embryo = Socket::new(Kind::SeqPacket);
                    embryo.embryo = true;
                    embryo.state = State::Paired {
                        peer_id: h.0,
                        peer: my_addr.clone(),
                        peer_cred: cred,
                    };
                    t.sockets.insert(id, embryo);
                    (
                        Link::Socket(id),
                        State::Paired {
                            peer_id: id,
                            peer: server_addr,
                            peer_cred: server_cred,
                        },
                    )
                };
                let s = t
                    .sockets
                    .get_mut(&server)
                    .ok_or(KernelError::ConnectionRefused)?;
                if let State::Listening { backlog, .. } = &mut s.state {
                    backlog.push_back(Pending {
                        link,
                        peer: my_addr,
                        peer_cred: cred,
                    });
                }
                wakes.append(&mut s.readers.take_all());
                let me = t.socket_mut(h)?;
                me.state = mine;
                wakes.append(&mut me.readers.take_all());
                drop(t);
                wake_all(wakes);
                return Ok(());
            }
            if nonblocking || limit.passed() {
                return Err(KernelError::WouldBlock);
            }
            if deliverable_signal_pending(pid) {
                return Err(KernelError::Interrupted);
            }
            if let Some(s) = t.sockets.get_mut(&server) {
                s.room.insert(task);
            }
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
    let limit = Limit::new(limit_for(h, Direction::Receive, nonblocking), task);
    loop {
        {
            let mut t = TABLE.lock();
            let s = t.socket_mut(h)?;
            s.readers.remove(task);
            // Linux: an accepted socket reports the listener's name, and
            // inherits its SO_PASSCRED and timeouts (sk_clone copies them).
            let bound = s.bound.clone();
            let passcred = s.passcred;
            let (rcvtimeo, sndtimeo) = (s.rcvtimeo, s.sndtimeo);
            let State::Listening { backlog, .. } = &mut s.state else {
                return Err(KernelError::InvalidArgument);
            };
            let next = backlog.pop_front();
            match next {
                Some(p) => {
                    let wakes = s.room.take_all();
                    let id = match p.link {
                        Link::Stream(stream) => {
                            let id = t.alloc_id();
                            let mut sock = Socket::new(Kind::Stream);
                            sock.state = State::Connected {
                                stream,
                                peer: p.peer.clone(),
                                peer_cred: p.peer_cred,
                            };
                            t.sockets.insert(id, sock);
                            id
                        }
                        // The embryo, made at connect: its holder is the
                        // accepting process now, in the backlog's place.
                        Link::Socket(id) => id,
                    };
                    if let Some(sock) = t.sockets.get_mut(&id) {
                        sock.embryo = false;
                        sock.bound = bound;
                        sock.passcred = passcred;
                        sock.rcvtimeo = rcvtimeo;
                        sock.sndtimeo = sndtimeo;
                    }
                    drop(t);
                    wake_all(wakes);
                    return Ok(Accepted {
                        handle: UnixHandle(id),
                        peer: p.peer,
                    });
                }
                None => {
                    if nonblocking || limit.passed() {
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
    send_as(h, data, None, None, nonblocking)
}

/// [`send`], with the credentials a datagram carries stated by the sender
/// (`Some`, already through [`check_stated_cred`]) rather than recorded by the
/// kernel (`None`) -- a stream ignores them: it reports its connection's --
/// and with descriptors (`SCM_RIGHTS`): a datagram carries them; on a stream
/// they ride on the bytes this send writes, which must then be some. Not
/// sent, they are dropped, and released before this returns.
///
/// # Errors
///
/// As [`send`].
pub fn send_as(
    h: UnixHandle,
    data: &[u8],
    stated: Option<PeerCred>,
    rights: Option<Bundle>,
    nonblocking: bool,
) -> KernelResult<usize> {
    let carried = rights.is_some();
    let sent = send_inner(h, data, stated, rights, nonblocking);
    if carried {
        // Descriptors a refused send dropped.
        passed::drain();
    }
    sent
}

/// [`send_as`] before its drain.
fn send_inner(
    h: UnixHandle,
    data: &[u8],
    stated: Option<PeerCred>,
    rights: Option<Bundle>,
    nonblocking: bool,
) -> KernelResult<usize> {
    match kind(h).ok_or(KernelError::InvalidHandle)? {
        Kind::Stream => {
            let stream = stream_of(h)?;
            if data.is_empty() {
                // Nothing for descriptors to ride on (Linux's stream sendmsg
                // of no bytes sends none either).
                return Ok(0);
            }
            if let Some(bundle) = rights {
                return send_stream_marked(h, stream, data, bundle, nonblocking);
            }
            match (nonblocking, limit_for(h, Direction::Send, nonblocking)) {
                (true, _) | (false, Some(0)) => stream_socket::try_send(stream, data),
                (false, None) => stream_socket::send(stream, data),
                // SO_SNDTIMEO: the timed send's TimedOut is Linux's EAGAIN.
                (false, Some(ns)) => {
                    stream_socket::send_timeout(stream, data, ns).map_err(timed_out_is_would_block)
                }
            }
        }
        Kind::Dgram => {
            let peer = TABLE
                .lock()
                .socket(h)?
                .default_peer
                .ok_or(KernelError::NotConnected)?;
            send_dgram(h, peer, data, stated, rights, nonblocking)
        }
        Kind::SeqPacket => {
            let peer = match TABLE.lock().socket(h)?.state {
                State::Paired { peer_id, .. } => peer_id,
                _ => return Err(KernelError::NotConnected),
            };
            send_dgram(h, peer, data, stated, rights, nonblocking)
        }
    }
}

/// A stream send carrying `bundle`: the bytes and the descriptors go in
/// together or not at all, waiting -- unless `nonblocking` -- for room, as
/// long as `SO_SNDTIMEO` allows.
fn send_stream_marked(
    h: UnixHandle,
    stream: StreamSocketHandle,
    data: &[u8],
    bundle: Bundle,
    nonblocking: bool,
) -> KernelResult<usize> {
    let task = sched::current_task_id();
    let limit = Limit::new(limit_for(h, Direction::Send, nonblocking), task);
    let mut bundle = bundle;
    loop {
        // Under TABLE, as every change to what is queued in flight is, so
        // garbage collection sees the mark whole or not at all.
        let tried = {
            let _table = TABLE.lock();
            stream_socket::try_send_marked(stream, data, bundle)
        };
        match tried {
            Ok(n) => return Ok(n),
            Err((KernelError::WouldBlock, back)) if !nonblocking && !limit.passed() => {
                bundle = back;
                wait_ready(stream, WRITABLE, &limit)?;
            }
            Err((e, _)) => return Err(e),
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
    send_to_as(h, data, target, None, None, nonblocking)
}

/// [`send_to`], with the datagram's credentials stated by the sender (`Some`,
/// already through [`check_stated_cred`]) rather than recorded by the kernel
/// (`None`), and with descriptors (`SCM_RIGHTS`) -- released before this
/// returns if the datagram is not sent.
///
/// # Errors
///
/// As [`send_to`].
pub fn send_to_as(
    h: UnixHandle,
    data: &[u8],
    target: &Name,
    stated: Option<PeerCred>,
    rights: Option<Bundle>,
    nonblocking: bool,
) -> KernelResult<usize> {
    let carried = rights.is_some();
    let sent = match kind(h).ok_or(KernelError::InvalidHandle) {
        Err(e) => Err(e),
        Ok(Kind::Stream) => match stream_of(h) {
            Ok(_) => Err(KernelError::ConnectAlready),
            Err(_) => Err(KernelError::NotSupported),
        },
        Ok(Kind::Dgram) => {
            let peer = TABLE.lock().lookup(target);
            match peer {
                Some(peer) => send_dgram(h, peer, data, stated, rights, nonblocking),
                None => Err(KernelError::ConnectionRefused),
            }
        }
        // Linux's unix_seqpacket_sendmsg ignores the address: a sequenced-
        // packet socket sends to its peer or not at all.
        Ok(Kind::SeqPacket) => send_inner(h, data, stated, rights, nonblocking),
    };
    if carried {
        passed::drain();
    }
    sent
}

/// Queue one datagram from `h` on the socket `peer`, waiting for room unless
/// `nonblocking`. It carries `stated`, or else the caller's credentials, and
/// `rights`. From a sequenced-packet socket `peer` is its peer, and a peer
/// gone, or one whose reading half is shut, is `BrokenPipe` (`EPIPE`), as
/// Linux answers.
fn send_dgram(
    h: UnixHandle,
    peer: u64,
    data: &[u8],
    stated: Option<PeerCred>,
    rights: Option<Bundle>,
    nonblocking: bool,
) -> KernelResult<usize> {
    if data.len() > MAX_DGRAM {
        return Err(KernelError::MsgSize);
    }
    let pid = current_user_pid();
    let task = sched::current_task_id();
    let cred = stated.or_else(current_cred);
    let limit = Limit::new(limit_for(h, Direction::Send, nonblocking), task);
    loop {
        {
            let mut t = TABLE.lock();
            let me = t.socket(h)?;
            if me.wr_shut {
                return Err(KernelError::BrokenPipe);
            }
            let seq = me.kind == Kind::SeqPacket;
            let from = me.local();
            let Some(dest) = t.sockets.get_mut(&peer) else {
                return Err(if seq {
                    KernelError::BrokenPipe
                } else {
                    KernelError::ConnectionRefused
                });
            };
            dest.room.remove(task);
            if dest.kind != me_kind(seq) {
                return Err(KernelError::WrongSocketType);
            }
            if dest.rd_shut {
                if seq {
                    return Err(KernelError::BrokenPipe);
                }
                // Linux discards datagrams to a socket whose reading half is
                // shut and reports success; there is no one to tell.
                return Ok(data.len());
            }
            if dest.has_room(data.len()) {
                dest.queue.push_back(Datagram {
                    data: data.to_vec(),
                    from,
                    cred,
                    rights,
                });
                dest.queued_bytes = dest.queued_bytes.saturating_add(data.len());
                let wakes = dest.readers.take_all();
                drop(t);
                wake_all(wakes);
                return Ok(data.len());
            }
            if nonblocking || limit.passed() {
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
            if buf.is_empty() {
                return Ok(Received {
                    len: 0,
                    full_len: 0,
                    from,
                    cred,
                    rights: None,
                });
            }
            // SO_RCVTIMEO bounds the wait, peeking or not.
            let task = sched::current_task_id();
            let limit = Limit::new(limit_for(h, Direction::Receive, nonblocking), task);
            loop {
                // Under TABLE: descriptors come off a queue only with it held
                // ("Descriptors").
                let took = {
                    let _table = TABLE.lock();
                    stream_socket::try_recv_marked(stream, buf, peek)
                };
                match took {
                    Ok(t) => {
                        return Ok(Received {
                            len: t.len,
                            full_len: t.len,
                            from,
                            cred,
                            rights: t.bundle,
                        });
                    }
                    Err(KernelError::WouldBlock) if !nonblocking && !limit.passed() => {
                        wait_ready(stream, READABLE, &limit)?;
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Kind::Dgram | Kind::SeqPacket => recv_dgram(h, buf, nonblocking, peek),
    }
}

/// A timed wait's `TimedOut`, as Linux answers an expired `SO_RCVTIMEO` or
/// `SO_SNDTIMEO`: `WouldBlock` (`EAGAIN`).
fn timed_out_is_would_block(e: KernelError) -> KernelError {
    match e {
        KernelError::TimedOut => KernelError::WouldBlock,
        other => other,
    }
}

/// `stream_socket::poll_status` bits: something to receive, or the end.
const READABLE: u16 = 0x11;
/// `stream_socket::poll_status` bits: room to send, or a send that would fail
/// at once.
const WRITABLE: u16 = 0x0C;

/// Wait until the stream end `stream` is ready as `mask` says ([`READABLE`],
/// [`WRITABLE`]), or `limit` passes (`WouldBlock`).
fn wait_ready(stream: StreamSocketHandle, mask: u16, limit: &Limit) -> KernelResult<()> {
    let pid = current_user_pid();
    let task = sched::current_task_id();
    loop {
        if stream_socket::poll_status(stream) & mask != 0 {
            return Ok(());
        }
        if limit.passed() {
            return Err(KernelError::WouldBlock);
        }
        if deliverable_signal_pending(pid) {
            return Err(KernelError::Interrupted);
        }
        stream_socket::register_waiter(stream, task);
        // Re-check after registering: a change between the poll and the
        // registration would otherwise go unseen.
        if stream_socket::poll_status(stream) & mask != 0 {
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

/// The kind a datagram's destination must be: the sender's own.
const fn me_kind(seq: bool) -> Kind {
    if seq { Kind::SeqPacket } else { Kind::Dgram }
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
    let limit = Limit::new(limit_for(h, Direction::Receive, nonblocking), task);
    loop {
        {
            let mut t = TABLE.lock();
            // A sequenced-packet socket reads its peer's end as its own: the
            // peer gone, or its writing half shut, is end of file once the
            // queue is read.
            let peer_done = match t.socket(h)?.state {
                State::Paired { peer_id, .. } => t.sockets.get(&peer_id).is_none_or(|p| p.wr_shut),
                State::Idle if t.socket(h)?.kind == Kind::SeqPacket => {
                    return Err(KernelError::NotConnected);
                }
                State::Listening { .. } => return Err(KernelError::NotConnected),
                _ => false,
            };
            let s = t.socket_mut(h)?;
            s.readers.remove(task);
            let taken = if peek {
                s.queue
                    .front()
                    .map(|d| (d.data.clone(), d.from.clone(), d.cred, d.rights.clone()))
            } else {
                s.queue
                    .pop_front()
                    .map(|d| (d.data, d.from, d.cred, d.rights))
            };
            if let Some((data, from, cred, rights)) = taken {
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
                    rights,
                });
            }
            if s.rd_shut || peer_done {
                return Ok(Received {
                    len: 0,
                    full_len: 0,
                    from: Address::Unnamed,
                    cred: None,
                    rights: None,
                });
            }
            if nonblocking || limit.passed() {
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

/// A garbage collection is running.
static GC_RUNNING: AtomicBool = AtomicBool::new(false);
/// A garbage collection was asked for since the running one last looked.
static GC_AGAIN: AtomicBool = AtomicBool::new(false);

/// Release the sockets that nothing but references in flight keeps alive
/// (module documentation, "Garbage"). Call with no lock held. Cheap while no
/// socket is in flight anywhere; a call while one is running leaves it to go
/// round again, so a release inside a collection never recurses into another.
pub fn collect_garbage() {
    if passed::unix_sockets_in_flight() == 0 {
        return;
    }
    GC_AGAIN.store(true, Ordering::Release);
    while !GC_RUNNING.swap(true, Ordering::AcqRel) {
        while GC_AGAIN.swap(false, Ordering::AcqRel) {
            let doomed = take_garbage(&mut TABLE.lock());
            if !doomed.is_empty() {
                drop(doomed);
                passed::drain();
            }
        }
        GC_RUNNING.store(false, Ordering::Release);
        // Asked for again between the last look and letting go: go round,
        // unless another caller has taken over.
        if !GC_AGAIN.load(Ordering::Acquire) {
            break;
        }
    }
}

/// Find the garbage and take the descriptors queued at it, for the caller to
/// drop once `TABLE` is released (module documentation, "Garbage").
fn take_garbage(t: &mut Table) -> Vec<Bundle> {
    // What waits to be received at each socket, as the sockets it carries:
    // its datagrams' descriptors, and those riding on the bytes its stream
    // end -- or, listening, each waiting connection's -- has yet to read.
    let mut queued_at: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut in_flight: BTreeMap<u64, u32> = BTreeMap::new();
    let mut pinned: BTreeSet<u64> = BTreeSet::new();
    for (&id, s) in &t.sockets {
        if s.embryo {
            // Counted with its listener, below.
            continue;
        }
        let mut carried: Vec<u64> = Vec::new();
        let mut visit = |b: &Bundle| {
            for u in b.unix_sockets() {
                carried.push(u);
                if b.is_shared() {
                    pinned.insert(u);
                }
            }
        };
        for d in &s.queue {
            if let Some(b) = &d.rights {
                visit(b);
            }
        }
        match &s.state {
            State::Connected { stream, .. } => stream_socket::bundles_toward(*stream, &mut visit),
            State::Listening { backlog, .. } => {
                for p in backlog {
                    match p.link {
                        Link::Stream(stream) => stream_socket::bundles_toward(stream, &mut visit),
                        Link::Socket(e) => {
                            for d in t.sockets.get(&e).into_iter().flat_map(|e| &e.queue) {
                                if let Some(b) = &d.rights {
                                    visit(b);
                                }
                            }
                        }
                    }
                }
            }
            State::Idle | State::Paired { .. } => {}
        }
        for &u in &carried {
            let n = in_flight.entry(u).or_insert(0);
            *n = n.saturating_add(1);
        }
        if !carried.is_empty() {
            queued_at.insert(id, carried);
        }
    }
    // Candidates: every holder is a reference queued somewhere.
    let candidates: BTreeSet<u64> = in_flight
        .iter()
        .filter(|&(id, &n)| !pinned.contains(id) && t.sockets.get(id).is_some_and(|s| s.refs == n))
        .map(|(&id, _)| id)
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    // Holders from outside the candidates: each candidate's count, less the
    // references to it queued at candidates.
    let mut outside: BTreeMap<u64, u32> = candidates
        .iter()
        .filter_map(|&c| Some((c, t.sockets.get(&c)?.refs)))
        .collect();
    for c in &candidates {
        for u in queued_at.get(c).into_iter().flatten() {
            if let Some(n) = outside.get_mut(u) {
                *n = n.saturating_sub(1);
            }
        }
    }
    // Reachable: held from outside, or queued at a reachable candidate.
    let mut reachable: BTreeSet<u64> = outside
        .iter()
        .filter(|&(_, &n)| n > 0)
        .map(|(&c, _)| c)
        .collect();
    let mut work: Vec<u64> = reachable.iter().copied().collect();
    while let Some(r) = work.pop() {
        for &u in queued_at.get(&r).into_iter().flatten() {
            if candidates.contains(&u) && reachable.insert(u) {
                work.push(u);
            }
        }
    }
    // The rest are garbage: nothing queued at them will ever be received.
    let mut doomed = Vec::new();
    let mut embryos = Vec::new();
    for g in candidates.difference(&reachable) {
        let Some(s) = t.sockets.get_mut(g) else {
            continue;
        };
        doomed.extend(s.queue.drain(..).filter_map(|d| d.rights));
        s.queued_bytes = 0;
        match &s.state {
            State::Connected { stream, .. } => {
                doomed.extend(stream_socket::take_bundles_toward(*stream));
            }
            State::Listening { backlog, .. } => {
                for p in backlog {
                    match p.link {
                        Link::Stream(stream) => {
                            doomed.extend(stream_socket::take_bundles_toward(stream));
                        }
                        Link::Socket(e) => embryos.push(e),
                    }
                }
            }
            State::Idle | State::Paired { .. } => {}
        }
    }
    for e in embryos {
        if let Some(s) = t.sockets.get_mut(&e) {
            doomed.extend(s.queue.drain(..).filter_map(|d| d.rights));
            s.queued_bytes = 0;
        }
    }
    doomed
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
        let paired = match s.state {
            State::Paired { peer_id, .. } => Some(peer_id),
            _ => None,
        };
        let stream = match (&s.state, s.kind) {
            (State::Connected { stream, .. }, _) => Some(*stream),
            (State::Idle, Kind::Dgram) | (State::Paired { .. }, _) => {
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
        };
        // A sequenced-packet peer reads the end, or finds the pipe broken.
        if let Some(p) = paired.and_then(|id| t.sockets.get_mut(&id)) {
            wakes.append(&mut p.readers.take_all());
            wakes.append(&mut p.room.take_all());
        }
        stream
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
            // An unconnected stream or sequenced-packet socket is writable
            // and hung up, as Linux's unix_poll reports one.
            State::Idle if s.kind.connects() => return 0x04 | 0x10,
            State::Paired { peer_id, .. } => {
                let peer = t.sockets.get(peer_id);
                let mut flags = 0u16;
                let peer_done = peer.is_none_or(|p| p.wr_shut);
                if !s.queue.is_empty() || s.rd_shut || peer_done {
                    flags |= 0x01;
                }
                if peer_done {
                    flags |= 0x10;
                }
                let broken = s.wr_shut || peer.is_none_or(|p| p.rd_shut);
                if broken {
                    flags |= 0x04 | 0x08;
                } else if peer.is_some_and(|p| p.has_room(0)) {
                    flags |= 0x04;
                }
                return flags;
            }
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
        let (stream, paired) = match s.state {
            State::Connected { stream, .. } => (Some(stream), None),
            State::Paired { peer_id, .. } => (None, Some(peer_id)),
            _ => (None, None),
        };
        // A sequenced-packet socket's room is its peer's to make: the peer's
        // receives wake its `room`.
        if let Some(p) = paired.and_then(|id| t.sockets.get_mut(&id)) {
            p.room.insert(task);
        }
        stream
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
        let (stream, paired) = match s.state {
            State::Connected { stream, .. } => (Some(stream), None),
            State::Paired { peer_id, .. } => (None, Some(peer_id)),
            _ => (None, None),
        };
        if let Some(p) = paired.and_then(|id| t.sockets.get_mut(&id)) {
            p.room.remove(task);
        }
        stream
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
/// abstract and path names, a node's rename and removal, credentials a
/// sender states and who may state them, `SO_PASSCRED` and its inheritance
/// by an accepted connection, the backlog's limit, refusals, `socketpair`,
/// readiness, and what closing ends.
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

    // --- credentials a sender states (SCM_CREDENTIALS on a send) ---
    let claim = |pid, uid, gid| PeerCred { pid, uid, gid };
    let alive = |pid: crate::proc::pcb::ProcessId| pid == 7 || pid == 9;
    let me = claim(7, 1000, 100);
    if check_stated_cred_for(Some(me), me, alive) != Ok(me) {
        return Err("a process may not state its own credentials");
    }
    for (stated, why) in [
        (
            claim(9, 1000, 100),
            "a process claimed another process's pid",
        ),
        (claim(7, 0, 100), "a process claimed another uid"),
        (claim(7, 1000, 0), "a process claimed another gid"),
    ] {
        if check_stated_cred_for(Some(me), stated, alive) != Err(KernelError::NotPermitted) {
            return Err(why);
        }
    }
    let root = claim(7, 0, 0);
    if check_stated_cred_for(Some(root), claim(9, 1000, 100), alive) != Ok(claim(9, 1000, 100)) {
        return Err("root may not state another live process's credentials");
    }
    if check_stated_cred_for(Some(root), claim(8, 0, 0), alive) != Err(KernelError::NoSuchProcess) {
        return Err("a claim naming no process was not NoSuchProcess");
    }
    if check_stated_cred_for(Some(root), claim(7, u32::MAX, 0), alive)
        != Err(KernelError::InvalidArgument)
        || check_stated_cred_for(Some(me), claim(7, 1000, u32::MAX), alive)
            != Err(KernelError::InvalidArgument)
    {
        return Err("a uid or gid of -1 was not InvalidArgument");
    }
    if check_stated_cred_for(None, claim(9, 5, 5), alive) != Ok(claim(9, 5, 5)) {
        return Err("kernel context may not state credentials");
    }
    let stated = claim(9, 1000, 100);
    if send_to_as(client, b"s", &name, Some(stated), None, true) != Ok(1) {
        return Err("send_to_as failed");
    }
    let carried = recv(server, &mut buf, true, false).map_err(|_| "recv failed")?;
    if carried.cred != Some(stated) {
        return Err("a datagram did not carry the credentials its sender stated");
    }
    if send_as(client, b"t", Some(stated), None, true) != Ok(1)
        || recv(server, &mut buf, true, false).map(|r| r.cred) != Ok(Some(stated))
    {
        return Err("a datagram to the connected destination did not carry stated credentials");
    }
    // SO_PASSCRED: off on a new socket, and settable.
    if passcred(server) {
        return Err("a new socket asks for credentials");
    }
    set_passcred(server, true).map_err(|_| "set_passcred failed")?;
    if !passcred(server) {
        return Err("set_passcred did not take");
    }
    set_passcred(server, false).map_err(|_| "set_passcred failed")?;

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
    set_passcred(listener, true).map_err(|_| "set_passcred on the listener failed")?;
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
    if !passcred(s1) || passcred(c1) {
        return Err(
            "the accepted socket did not take its listener's SO_PASSCRED, or the client did",
        );
    }
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

    rights_checks(opened)?;
    seqpacket_checks(opened)
}

/// The timeout checks ([`timeout_checks`]), apart from [`self_test`]: a
/// blocking wait's limit is an hrtimer, which fires only once interrupts are
/// on, and [`self_test`] runs before `cpu::sti()`. `main.rs` runs this one
/// after it, beside the timerfd's blocking test, for the same reason. Run
/// with interrupts off it refuses rather than waiting: on 2026-10-02 the
/// checks, then inside [`self_test`], parked the boot thread on a timer that
/// could not fire, and the boot sat halted until its 2400 s limit (rq39).
///
/// # Errors
///
/// `InternalError` naming the first check that failed, or that interrupts
/// were off.
pub fn self_test_timeouts() -> KernelResult<()> {
    crate::serial_println!("[unix_socket] Running timeout self-test...");
    if !crate::cpu::interrupts_enabled() {
        crate::serial_println!(
            "[unix_socket]   FAIL: run with interrupts off, where no timeout can fire"
        );
        return Err(KernelError::InternalError);
    }
    let mut opened: Vec<UnixHandle> = Vec::new();
    let result = timeout_checks(&mut opened);
    for h in opened {
        close(h);
    }
    if let Err(why) = result {
        crate::serial_println!("[unix_socket]   FAIL: {}", why);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[unix_socket] Timeout self-test PASSED");
    Ok(())
}

/// `SO_RCVTIMEO` / `SO_SNDTIMEO`: read back as set, and only the direction
/// set; a blocking receive that waits its whole limit and then answers
/// `WouldBlock`; a zero limit that does not wait; `accept` likewise; an
/// accepted socket starting with its listener's limits; a connected
/// stream's receive giving up the same way.
fn timeout_checks(opened: &mut Vec<UnixHandle>) -> Result<(), &'static str> {
    const LIMIT_NS: u64 = 30_000_000;
    let mut buf = [0u8; 16];
    let quiet = opened_socket(opened, Kind::Dgram)?;
    if timeout(quiet, Direction::Receive) != Ok(None) {
        return Err("a new socket has a receive timeout");
    }
    set_timeout(quiet, Direction::Receive, Some(LIMIT_NS)).map_err(|_| "set_timeout failed")?;
    if timeout(quiet, Direction::Receive) != Ok(Some(LIMIT_NS))
        || timeout(quiet, Direction::Send) != Ok(None)
    {
        return Err("a timeout did not read back, or set the other direction too");
    }
    let began = crate::hrtimer::now_ns();
    if recv(quiet, &mut buf, false, false).map(|r| r.len) != Err(KernelError::WouldBlock) {
        return Err("a blocking receive past its timeout was not WouldBlock");
    }
    if crate::hrtimer::now_ns().saturating_sub(began) < LIMIT_NS {
        return Err("a receive with a 30 ms timeout gave up early");
    }
    set_timeout(quiet, Direction::Receive, Some(0)).map_err(|_| "set_timeout failed")?;
    if recv(quiet, &mut buf, false, false).map(|r| r.len) != Err(KernelError::WouldBlock) {
        return Err("a receive with a zero timeout did not answer at once");
    }

    let listener = opened_socket(opened, Kind::Stream)?;
    let name = Name::Abstract(b"slate-selftest-timeo".to_vec());
    bind_abstract(listener, b"slate-selftest-timeo").map_err(|_| "bind failed")?;
    listen(listener, 1).map_err(|_| "listen failed")?;
    set_timeout(listener, Direction::Receive, Some(LIMIT_NS)).map_err(|_| "set_timeout failed")?;
    if accept(listener, false).map(|a| a.handle) != Err(KernelError::WouldBlock) {
        return Err("an accept past its timeout was not WouldBlock");
    }
    let client = opened_socket(opened, Kind::Stream)?;
    connect(client, &name, true).map_err(|_| "connect failed")?;
    let accepted = accept(listener, false).map_err(|_| "accept failed")?;
    opened.push(accepted.handle);
    if timeout(accepted.handle, Direction::Receive) != Ok(Some(LIMIT_NS)) {
        return Err("an accepted socket did not start with its listener's timeout");
    }
    if recv(accepted.handle, &mut buf, false, false).map(|r| r.len) != Err(KernelError::WouldBlock)
    {
        return Err("a stream receive past its timeout was not WouldBlock");
    }
    Ok(())
}

/// `SCM_RIGHTS`, with Unix sockets as the descriptors carried (their holder
/// counts are this module's to read): a datagram carrying one, received and
/// released; one never received, released when its socket closes; a socket
/// carried by itself, and two carried by each other, collected as garbage;
/// on a stream, a receive that reaches the carried bytes takes the
/// descriptors and stops at their end, one that starts before them reads into
/// them, a peek hands up a copy, a short read takes them at the first touch,
/// and closing the reading end releases what was not read.
#[allow(clippy::too_many_lines)] // one linear script; splitting it hides the order
fn rights_checks(opened: &mut Vec<UnixHandle>) -> Result<(), &'static str> {
    use crate::proc::linux_fd::FdEntry;
    let mut buf = [0u8; 16];
    let refs = |h: UnixHandle| TABLE.lock().sockets.get(&h.0).map(|s| s.refs);
    // A bundle carrying one more reference to `h`.
    let carrying = |h: UnixHandle| -> Result<Option<Bundle>, &'static str> {
        let p = passed::Passed::take(FdEntry::unix_socket(h.raw(), 0, 0))
            .map_err(|_| "a reference to a live socket could not be taken")?;
        Ok(Bundle::new(alloc::vec![p]))
    };
    // Received rights, as one Unix socket's id.
    let landed = |r: Option<Bundle>| -> Option<u64> {
        let mut ps = r?.into_passed();
        let id = ps.first().and_then(passed::Passed::unix_socket);
        ps.clear();
        passed::drain();
        id
    };

    // --- a datagram carries a socket; received, then released ---
    let (d1, d2) = pair(Kind::Dgram).map_err(|_| "a datagram pair failed")?;
    opened.push(d1);
    opened.push(d2);
    let x = create(Kind::Dgram).map_err(|_| "create failed")?;
    if send_as(d1, b"r", None, carrying(x)?, true) != Ok(1) {
        close(x);
        return Err("a datagram carrying a descriptor was not sent");
    }
    close(x);
    if refs(x) != Some(1) {
        return Err("a socket in flight did not outlive its last descriptor");
    }
    let got = recv(d2, &mut buf, true, false).map_err(|_| "recv failed")?;
    if got.len != 1 || got.rights.as_ref().map(|b| b.unix_sockets().count()) != Some(1) {
        return Err("a datagram's descriptors did not arrive with it");
    }
    if landed(got.rights) != Some(x.raw()) || kind(x).is_some() {
        return Err("a received descriptor dropped was not released");
    }

    // --- never received: released when the receiving socket closes ---
    let (e1, e2) = pair(Kind::Dgram).map_err(|_| "a datagram pair failed")?;
    opened.push(e1);
    let y = create(Kind::Dgram).map_err(|_| "create failed")?;
    let sent = send_as(e1, b"u", None, carrying(y)?, true);
    close(y);
    if sent != Ok(1) || refs(y) != Some(1) {
        return Err("a second datagram carrying a descriptor was not sent");
    }
    close(e2);
    if kind(y).is_some() {
        return Err("a descriptor queued at a closed socket was not released");
    }

    // --- garbage: a socket carried by itself, two carried by each other ---
    let lone = create(Kind::Dgram).map_err(|_| "create failed")?;
    if send_dgram(lone, lone.0, b"o", None, carrying(lone)?, true) != Ok(1) {
        close(lone);
        return Err("a socket could not send itself to itself");
    }
    passed::drain();
    close(lone);
    if kind(lone).is_some() {
        return Err("a socket carried only by itself was not collected");
    }
    let (g1, g2) = pair(Kind::Dgram).map_err(|_| "a datagram pair failed")?;
    // A pair's datagram goes to the other end: g1's own reference is queued
    // at g2, and g2's at g1.
    let a = send_as(g1, b"1", None, carrying(g1)?, true);
    let b = send_as(g2, b"2", None, carrying(g2)?, true);
    close(g1);
    if a != Ok(1) || b != Ok(1) || kind(g1).is_none() {
        close(g2);
        return Err("two sockets carrying each other were not both kept while one was held");
    }
    close(g2);
    if kind(g1).is_some() || kind(g2).is_some() {
        return Err("two sockets carried only by each other were not collected");
    }

    // --- a stream: the descriptors ride on the bytes their send wrote ---
    let (s1, s2) = pair(Kind::Stream).map_err(|_| "a stream pair failed")?;
    opened.push(s1);
    opened.push(s2);
    let z = create(Kind::Dgram).map_err(|_| "create failed")?;
    opened.push(z);
    let base = refs(z).unwrap_or(0);
    let plain_then_marked = send(s1, b"ab", true) == Ok(2)
        && send_as(s1, b"cd", None, carrying(z)?, true) == Ok(2)
        && send(s1, b"ef", true) == Ok(2);
    if !plain_then_marked {
        return Err("stream sends with and without descriptors failed");
    }
    let r = recv(s2, &mut buf, true, false).map_err(|_| "stream recv failed")?;
    if r.len != 4 || buf.get(..4) != Some(&b"abcd"[..]) || landed(r.rights) != Some(z.raw()) {
        return Err(
            "a stream receive did not read into the marked bytes and take their descriptors",
        );
    }
    let r = recv(s2, &mut buf, true, false).map_err(|_| "stream recv failed")?;
    if r.len != 2 || r.rights.is_some() {
        return Err("the bytes after a marked stretch came with descriptors");
    }
    if send_as(s1, b"gh", None, carrying(z)?, true) != Ok(2) || send(s1, b"ij", true) != Ok(2) {
        return Err("stream sends failed");
    }
    let r = recv(s2, &mut buf, true, false).map_err(|_| "stream recv failed")?;
    if r.len != 2 || buf.get(..2) != Some(&b"gh"[..]) || landed(r.rights) != Some(z.raw()) {
        return Err("a stream receive read past the end of the marked bytes");
    }
    if recv(s2, &mut buf, true, false).map(|r| r.len) != Ok(2) {
        return Err("the bytes after a marked stretch were lost");
    }
    // A peek hands up a copy, and the descriptors stay for the receive.
    if send_as(s1, b"kl", None, carrying(z)?, true) != Ok(2) {
        return Err("a stream send failed");
    }
    let peeked = recv(s2, &mut buf, true, true).map_err(|_| "stream peek failed")?;
    if peeked.len != 2 || landed(peeked.rights) != Some(z.raw()) {
        return Err("a peek at marked bytes did not hand up their descriptors");
    }
    let r = recv(s2, &mut buf, true, false).map_err(|_| "stream recv failed")?;
    if r.len != 2 || landed(r.rights) != Some(z.raw()) {
        return Err("a peek took the descriptors the receive after it should have");
    }
    // A short read takes them at the first touch; the rest is plain.
    if send_as(s1, b"mnop", None, carrying(z)?, true) != Ok(4) {
        return Err("a stream send failed");
    }
    let mut two = [0u8; 2];
    let r = recv(s2, &mut two, true, false).map_err(|_| "stream recv failed")?;
    if r.len != 2 || landed(r.rights) != Some(z.raw()) {
        return Err("a short read of marked bytes did not take their descriptors");
    }
    let r = recv(s2, &mut buf, true, false).map_err(|_| "stream recv failed")?;
    if r.len != 2 || r.rights.is_some() {
        return Err("the rest of a marked stretch came with its descriptors again");
    }
    // Not read before the reading end closes: released.
    if send_as(s1, b"q", None, carrying(z)?, true) != Ok(1) {
        return Err("a stream send failed");
    }
    close_early(opened, s2);
    // Each check above shows its own sockets' references given back; a
    // global count of what is in flight would race any other task passing
    // sockets meanwhile.
    if refs(z) != Some(base) {
        return Err("descriptors on bytes a closed end never read were not released");
    }
    Ok(())
}

/// `SOCK_SEQPACKET`: a pair's sends arriving whole and in order, cut to the
/// buffer with the whole length reported; a listener's connection that sends
/// before it is accepted, its message waiting for the accepted socket; the
/// peer's close read as end of file after its messages, and a send to it
/// refused; descriptors riding on a message; a stream kind refused.
fn seqpacket_checks(opened: &mut Vec<UnixHandle>) -> Result<(), &'static str> {
    use crate::proc::linux_fd::FdEntry;
    let mut buf = [0u8; 16];
    let (a, b) = pair(Kind::SeqPacket).map_err(|_| "a sequenced-packet pair failed")?;
    opened.push(a);
    opened.push(b);
    if send(a, b"one", true) != Ok(3) || send(a, b"second", true) != Ok(6) {
        return Err("sends on a sequenced-packet pair failed");
    }
    let mut two = [0u8; 2];
    let first = recv(b, &mut two, true, false).map_err(|_| "recv failed")?;
    let second = recv(b, &mut buf, true, false).map_err(|_| "recv failed")?;
    if first.len != 2
        || first.full_len != 3
        || second.len != 6
        || buf.get(..6) != Some(&b"second"[..])
    {
        return Err("sequenced packets did not arrive whole, in order, cut to the buffer");
    }
    if poll_status(a) & 0x04 == 0 || poll_status(b) & 0x01 != 0 {
        return Err("a sequenced-packet pair's readiness is wrong");
    }
    // Descriptors ride on a message.
    let x = create(Kind::Dgram).map_err(|_| "create failed")?;
    let carried = passed::Passed::take(FdEntry::unix_socket(x.raw(), 0, 0))
        .map_err(|_| "a reference to a live socket could not be taken")?;
    let sent = send_as(a, b"r", None, Bundle::new(alloc::vec![carried]), true);
    close(x);
    let got = recv(b, &mut buf, true, false).map_err(|_| "recv failed")?;
    let came = got.rights.map(Bundle::into_passed).unwrap_or_default();
    let ok = sent == Ok(1) && came.len() == 1;
    drop(came);
    passed::drain();
    if !ok || kind(x).is_some() {
        return Err("descriptors did not ride on a sequenced packet, or were not released");
    }
    // The peer's close: what it sent is read, then end of file; a send to it
    // is refused.
    if send(a, b"last", true) != Ok(4) {
        return Err("a send before the close failed");
    }
    close_early(opened, a);
    if recv(b, &mut buf, true, false).map(|r| r.len) != Ok(4)
        || recv(b, &mut buf, true, false).map(|r| r.len) != Ok(0)
    {
        return Err("a closed peer's message, then end of file, did not arrive");
    }
    if send(b, b"x", true) != Err(KernelError::BrokenPipe) {
        return Err("a send to a closed peer was not BrokenPipe");
    }

    // --- by name: a connection sends before it is accepted ---
    let listener = opened_socket(opened, Kind::SeqPacket)?;
    bind_abstract(listener, b"slate-selftest-seqpacket").map_err(|_| "bind failed")?;
    listen(listener, 2).map_err(|_| "listen failed")?;
    let name = Name::Abstract(b"slate-selftest-seqpacket".to_vec());
    let stream = opened_socket(opened, Kind::Stream)?;
    if connect(stream, &name, true) != Err(KernelError::WrongSocketType) {
        return Err("a stream connecting to a sequenced-packet listener was not refused");
    }
    let client = opened_socket(opened, Kind::SeqPacket)?;
    connect(client, &name, true).map_err(|_| "connect failed")?;
    if send(client, b"early", true) != Ok(5) {
        return Err("a connection could not send before it was accepted");
    }
    let accepted = accept(listener, true).map_err(|_| "accept failed")?;
    opened.push(accepted.handle);
    let early = recv(accepted.handle, &mut buf, true, false).map_err(|_| "recv failed")?;
    if early.len != 5 || buf.get(..5) != Some(&b"early"[..]) {
        return Err("what a connection sent before accept did not reach the accepted socket");
    }
    if send(accepted.handle, b"back", true) != Ok(4)
        || recv(client, &mut buf, true, false).map(|r| r.len) != Ok(4)
    {
        return Err("the accepted socket's reply did not arrive");
    }
    // A connection still in the backlog when the listener closes: its client
    // reads the end.
    let waiting = opened_socket(opened, Kind::SeqPacket)?;
    connect(waiting, &name, true).map_err(|_| "a second connect failed")?;
    close_early(opened, listener);
    if recv(waiting, &mut buf, true, false).map(|r| r.len) != Ok(0) {
        return Err("a connection in a closed listener's backlog did not read the end");
    }
    Ok(())
}

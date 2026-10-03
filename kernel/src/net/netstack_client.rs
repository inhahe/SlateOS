//! Reusable in-kernel client for the userspace `net.stack` daemon.
//!
//! This is the kernel side of the Path B userspace-netstack migration
//! (`design-decisions.md` §63, cutover strategy §66). It wraps the shared-memory
//! control-ring protocol — `netipc::ring` opcodes driven over an `OP_RING_TCP`
//! control channel to the persistent daemon session — that was previously
//! hand-inlined in the `spawn.rs` boot self-tests, into a single reusable
//! [`NetstackConn`] type: **one** connection id on the daemon, driven
//! `connect → send → recv → close`, where each operation is a control
//! round-trip against the daemon's *persistent* session.
//!
//! ## One ring for every socket, or one each
//!
//! The operator's question A-Q15 (`design-decisions.md` §972) asks for two
//! designs behind one switch, [`RingMode`], so they can be measured:
//! - **A, the default:** every [`NetstackConn`] submits on **one**
//!   shared-memory ring, `SHARED_RING`.
//! - **B:** each `NetstackConn` gets a ring of its own, which the daemon keeps
//!   as a session of its own.
//!
//! Either way, connections are addressed by ids unique system-wide
//! ([`alloc_conn_id`]), and the daemon's tables are daemon-wide, so a frame for
//! any socket is routed whichever ring is being served. Until 2026-09-27 every
//! `NetstackConn` allocated a ring of its own while the daemon kept a single
//! session and reset it whenever a different ring arrived: opening a second
//! socket destroyed the first.
//!
//! ## Persistence
//!
//! Each operation opens a fresh `net.stack` service channel, hands the daemon the
//! ring handle (`OP_RING_TCP`), the daemon drains the queued SQE(s) against its
//! persistent session, and posts one completion each. The kernel keeps the ring
//! mapped for the whole boot. Because a connection is opened in one round and
//! driven (send/recv) in later rounds, a successful send after a connect *is
//! itself* proof that the daemon's session survived between submissions --
//! exactly the property the persistent socket daemon needs for the staged
//! cutover (§66, Q22b).
//!
//! ## Data window layout
//!
//! A single fixed ring region carries both directions:
//!
//! ```text
//! [ SND_OFF .. SND_OFF+SND_CAP )   send staging   (SND_CAP = 1024 = daemon TCP_SND_BUF)
//! [ RCV_OFF .. RCV_OFF+RCV_CAP )   recv landing   (RCV_CAP =  512 = daemon MSG_CAP)
//! ```
//!
//! The caps match the daemon's per-op limits (`services/netstack/src/main.rs`):
//! `OP_SEND` rejects `data_len > 1024`, and `OP_RECV` returns at most 512 bytes
//! per call. [`NetstackConn::send`] therefore chunks the caller's buffer into
//! ≤`SND_CAP` pieces (one round-trip each) and [`NetstackConn::recv`] returns a
//! single ≤`RCV_CAP` slice per call.
//!
//! ## Scope (increment 5.4)
//!
//! This module only provides the reusable client and the `net.userspace` boot
//! switch (default off). It does **not** wire the AF_INET Linux socket syscalls
//! yet — that is increment 5.5, which layers a socket-fd object over this client.

use crate::error::{KernelError, KernelResult};
use crate::ipc::{channel, service, shm};
use crate::sched::kmutex::{KMutex, KMutexGuard};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

/// Send-staging window offset within the ring data area.
const SND_OFF: u32 = 0;
/// Send-staging window capacity. Matches the daemon's `TCP_SND_BUF` — a single
/// `OP_SEND` with `data_len` above this is rejected, so we chunk to it.
const SND_CAP: u32 = 1024;
/// Recv-landing window offset (immediately after the send window).
const RCV_OFF: u32 = SND_CAP;
/// Recv-landing window capacity. Matches the daemon's `MSG_CAP` — a single
/// `OP_RECV` returns at most this many bytes.
const RCV_CAP: u32 = 512;
/// Total ring data-area length: send window + recv window.
const DATA_LEN: u32 = SND_CAP + RCV_CAP;

/// SQ / CQ depth. Each round-trip queues a single SQE, but a little headroom
/// keeps the geometry identical to the original self-tests.
const SQ_ENTRIES: u32 = 8;
const CQ_ENTRIES: u32 = 8;

/// `user_data` base ("NSCL" = net-stack-client). Every SQE gets a distinct,
/// monotonically increasing tag so completions can be matched 1:1 in FIFO order.
const UD_BASE: u64 = 0x4e53_434c_0000_0000;

/// Per-round control-channel reply timeout (ns). Generous: the daemon may be
/// blocking on the wire (connect handshake, receive) while it drains our SQE.
const RECV_TIMEOUT_NS: u64 = 12_000_000_000;

/// Whether AF_INET/AF_INET6 stream sockets route to the userspace netstack
/// daemon rather than the in-kernel resident stack.
///
/// **Default ON since 2026-09-12** (increment 5.7). The operator answered A-Q9
/// with option C -- flip the default, but fix the server rough edge first -- and
/// that prerequisite is done: no kernel recv path asks the daemon to withhold a
/// reply any more, so a listener's accepted connections no longer serialise
/// behind one quiet peer (`D-NETSOCK-SYNC`). See `design-decisions.md` 934, and
/// §66 for the staged-cutover plan this completes the middle step of.
///
/// `net.userspace=0` (or `off`/`false`/`no`) on the kernel cmdline opts back out
/// to the resident stack. That escape hatch is deliberate and temporary: A-Q9's
/// option D deletes the resident stack, and when that lands there will be
/// nothing to fall back *to*, so this function and its parameter go with it.
/// Until then the flip stays reversible without a revert.
///
/// The truthiness of an explicit value matches `kernparam::is_set`; only the
/// absent case differs, and that is the whole of the flip.
#[must_use]
pub fn userspace_enabled() -> bool {
    crate::fs::kernparam::get("net.userspace")
        .is_none_or(|v| v.is_empty() || v == "1" || v == "yes" || v == "true")
}

// ---------------------------------------------------------------------------
// The one ring (A-Q15 design A)
// ---------------------------------------------------------------------------

/// The one shared-memory ring every socket's daemon traffic goes through,
/// created on first use and never torn down.
///
/// One ring means one daemon session, and a session holds every socket's
/// connections, listeners and datagram sockets side by side. Until 2026-09-27
/// each [`NetstackConn`] allocated a ring of its own, while the daemon kept one
/// session and reset it whenever a different ring arrived. Opening a second
/// socket therefore destroyed the first: the operator's question A-Q15.
/// `design-decisions.md` §972 asks for this design (A) and the per-socket one
/// (B) behind a switch ([`RingMode`]), and for a measurement between them.
/// This is A, and the default.
///
/// A sleeping lock, because it is held across a daemon round-trip and a
/// round-trip blocks on the daemon's reply. It is always the innermost lock a
/// socket operation takes: a socket's own lock, then this.
static SHARED_RING: KMutex<Option<RingHandle>> = KMutex::new(None);

/// Which ring a newly opened socket submits on: design A or B of A-Q15
/// (`design-decisions.md` §972), which asks for both behind one switch so the
/// two can be measured against each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingMode {
    /// Design A: every socket on [`SHARED_RING`], one daemon session in all.
    /// Sockets share one queue, so one socket's slow round-trip delays the
    /// next socket's.
    Shared,
    /// Design B: each socket on a ring of its own, which the daemon keeps as a
    /// session of its own. Costs a region per socket, and the daemon's session
    /// table (64) caps the number of sockets.
    PerSocket,
}

/// [`set_ring_mode`]'s override: 0 none, 1 shared, 2 per-socket.
static RING_MODE_OVERRIDE: AtomicU8 = AtomicU8::new(0);

/// Rings alive now: the shared one once created, plus one per socket opened in
/// per-socket mode and not yet closed. See [`rings_live`].
static RINGS_LIVE: AtomicU32 = AtomicU32::new(0);

/// Bytes of shared memory one ring takes, once a ring has been created.
static RING_BYTES: AtomicU32 = AtomicU32::new(0);

/// How many rings are alive, and the shared memory they hold in all.
///
/// The memory half of A-Q15's measurement is exact rather than sampled: under
/// design A this stays at one ring whatever the number of sockets, under B it
/// grows by one ring per socket. A ring is mapped by the daemon too, but it is
/// the same pages, so the bytes are counted once.
#[must_use]
pub fn rings_live() -> (u32, u64) {
    let n = RINGS_LIVE.load(Ordering::Acquire);
    let each = RING_BYTES.load(Ordering::Acquire);
    (n, u64::from(n).saturating_mul(u64::from(each)))
}

/// The mode a socket opened now gets.
///
/// [`set_ring_mode`]'s override if one is set, else the `net.ring` boot switch
/// (`per-socket` for design B; anything else, or absent, for A).
#[must_use]
pub fn ring_mode() -> RingMode {
    match RING_MODE_OVERRIDE.load(Ordering::Acquire) {
        1 => RingMode::Shared,
        2 => RingMode::PerSocket,
        _ => {
            if crate::fs::kernparam::get("net.ring").is_some_and(|v| v == "per-socket") {
                RingMode::PerSocket
            } else {
                RingMode::Shared
            }
        }
    }
}

/// Override the ring mode for sockets opened from now on (`None` returns the
/// choice to the boot switch). Sockets already open keep the ring they have.
/// For the load harness and the design-B self-tests, which run both designs in
/// one boot.
pub fn set_ring_mode(mode: Option<RingMode>) {
    let v = match mode {
        None => 0,
        Some(RingMode::Shared) => 1,
        Some(RingMode::PerSocket) => 2,
    };
    RING_MODE_OVERRIDE.store(v, Ordering::Release);
}

/// Next id [`alloc_conn_id`] hands out.
static NEXT_CONN_ID: AtomicU32 = AtomicU32::new(1);

/// A connection id no other live socket holds.
///
/// Every socket shares one daemon session, so the daemon's ids are one
/// namespace for the whole system: a connection, a listener, an accepted
/// connection and a datagram socket each need an id nothing else is using. The
/// counter is 32 bits and never reuses an id before it wraps, which at one
/// allocation per socket does not happen within a boot. 0 is never returned.
#[must_use]
pub fn alloc_conn_id() -> u32 {
    loop {
        let id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

/// A shared-memory ring to the daemon: the region, its size, and the
/// `user_data` tags handed out on it.
struct RingHandle {
    /// Shared-memory region backing the ring, shared with the daemon.
    handle: shm::ShmHandle,
    /// Region size in bytes (as passed to the daemon in each `OP_RING_TCP`).
    size: u32,
    /// Next `user_data` tag to hand out.
    next_ud: u64,
    /// Whether the daemon has served a round for this ring, and so may hold a
    /// session for it: an own ring's teardown stops that session only if so.
    served: bool,
}

impl Drop for RingHandle {
    /// Free the region. Only an own ring (design B) is ever dropped -- the
    /// shared ring lives in a static for the whole boot -- and its teardown has
    /// already stopped the daemon's session for it.
    fn drop(&mut self) {
        shm::close(self.handle);
        // Saturating: an unbalanced decrement must not wrap the count.
        let _ = RINGS_LIVE.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            Some(n.saturating_sub(1))
        });
    }
}

impl RingHandle {
    /// Allocate the region and initialise the ring header in it.
    fn create() -> KernelResult<Self> {
        let need = netipc::ring::region_size(SQ_ENTRIES, CQ_ENTRIES, DATA_LEN);
        let handle = shm::create(need)?;
        let size = match shm::size(handle) {
            Ok(s) => s,
            Err(e) => {
                shm::close(handle);
                return Err(e);
            }
        };
        let kaddr = match shm::kernel_addr(handle) {
            Ok(p) => p,
            Err(e) => {
                shm::close(handle);
                return Err(e);
            }
        };
        // SAFETY: `kaddr` is valid and writable for `size` (>= need) bytes and is
        // exclusively ours until the daemon attaches during a submit round. The
        // ring header is published with a release fence inside `init`. Only the
        // header is needed here -- the driver view is re-`attach`ed per round-trip
        // (see `attach`) -- so the `Ring` value is discarded.
        if unsafe { netring::Ring::init(kaddr, size, SQ_ENTRIES, CQ_ENTRIES, DATA_LEN) }.is_none() {
            shm::close(handle);
            return Err(KernelError::InternalError);
        }
        let Ok(size) = u32::try_from(size) else {
            shm::close(handle);
            return Err(KernelError::InternalError);
        };
        RINGS_LIVE.fetch_add(1, Ordering::AcqRel);
        RING_BYTES.store(size, Ordering::Release);
        Ok(Self {
            handle,
            size,
            next_ud: UD_BASE,
            served: false,
        })
    }

    /// Re-attach the ring driver view from the shared-memory handle.
    ///
    /// Attaching is stateless -- the free-running SQ/CQ indices live in the
    /// shared region, so re-deriving the view each round-trip is correct, and
    /// keeps a non-`Send` raw pointer out of any struct that outlives it.
    fn attach(&self) -> KernelResult<netring::Ring> {
        let kaddr = shm::kernel_addr(self.handle)?;
        let len = self.size as usize;
        // SAFETY: `kaddr` is the stable kernel VA of our shm region, valid and
        // aligned for `len` bytes for the region's lifetime (closed only when
        // this `RingHandle` drops, and `&self` keeps it alive meanwhile); `attach`
        // only reads the header (published by `create`) and bounds-checks the
        // geometry, so it can never read or write outside the region.
        unsafe { netring::Ring::attach(kaddr, len) }.ok_or(KernelError::InternalError)
    }
}

/// Where a socket's round-trips go.
enum RingRef {
    /// [`SHARED_RING`] (design A).
    Shared,
    /// A ring of the socket's own (design B), guarded by whatever guards the
    /// `NetstackConn`: the socket's own lock.
    Own(RingHandle),
}

/// How a [`RingGuard`] holds its ring.
enum RingLock<'a> {
    /// The shared ring's lock.
    Shared(KMutexGuard<'static, Option<RingHandle>>),
    /// A socket's own ring, borrowed from its `NetstackConn`.
    Own(&'a mut RingHandle),
}

/// Exclusive use of a ring for one daemon round-trip -- the shared ring's lock,
/// or a socket's own ring -- and an attached view of it.
///
/// Everything a round-trip touches -- the data window written before it, the
/// completion, the window read after it -- happens while one of these is alive,
/// so no other socket's round-trip can use the window in between. Operations
/// that make several round-trips take one guard per round-trip, so other
/// sockets interleave between them rather than waiting for the whole call.
struct RingGuard<'a> {
    lock: RingLock<'a>,
    view: netring::Ring,
}

impl<'a> RingGuard<'a> {
    /// Take `ring`: the shared ring's lock, created on first use, or the
    /// socket's own ring.
    fn acquire(ring: &'a mut RingRef) -> KernelResult<Self> {
        match ring {
            RingRef::Shared => {
                let mut lock = SHARED_RING.lock();
                if lock.is_none() {
                    *lock = Some(RingHandle::create()?);
                }
                let view = lock.as_ref().ok_or(KernelError::InternalError)?.attach()?;
                Ok(Self {
                    lock: RingLock::Shared(lock),
                    view,
                })
            }
            RingRef::Own(h) => {
                let view = h.attach()?;
                Ok(Self {
                    lock: RingLock::Own(h),
                    view,
                })
            }
        }
    }

    /// The ring handle. Always there once `acquire` returned.
    fn handle(&self) -> KernelResult<&RingHandle> {
        match &self.lock {
            RingLock::Shared(g) => g.as_ref().ok_or(KernelError::InternalError),
            RingLock::Own(h) => Ok(h),
        }
    }

    /// The ring handle, mutably.
    fn handle_mut(&mut self) -> KernelResult<&mut RingHandle> {
        match &mut self.lock {
            RingLock::Shared(g) => g.as_mut().ok_or(KernelError::InternalError),
            RingLock::Own(h) => Ok(h),
        }
    }

    /// Hand out the next `user_data` tag.
    fn next_ud(&mut self) -> u64 {
        match self.handle_mut() {
            Ok(h) => {
                let ud = h.next_ud;
                h.next_ud = h.next_ud.wrapping_add(1);
                ud
            }
            Err(_) => UD_BASE,
        }
    }

    /// Stage bytes in the ring's data window at `off`.
    fn write_data(&self, off: usize, data: &[u8]) -> bool {
        self.view.write_data(off, data)
    }

    /// Read bytes back from the ring's data window at `off`.
    fn read_data(&self, off: usize, out: &mut [u8]) -> bool {
        self.view.read_data(off, out)
    }

    /// Push one SQE, run control round-trips until its completion arrives,
    /// and reap exactly that one, checking that no extra completion is posted
    /// after it. Late completions of earlier round-trips that gave up are
    /// discarded on the way. Returns the completion result.
    fn submit_and_reap(&mut self, sqe: &netipc::ring::Sqe) -> KernelResult<i32> {
        let want_ud = sqe.user_data;
        if !self.view.sq_push(sqe) {
            return Err(KernelError::ResourceExhausted);
        }
        // Bounded poll, matching the eight loops elsewhere in this file and
        // for the reason they all state: a round drives the daemon's pump
        // once, so one round is not guaranteed to produce the completion.
        //
        // Re-rounding is safe and is NOT a re-submission: `sq_push` above ran
        // once, and a round carries no SQE -- its contract is "the daemon
        // drains whatever SQEs are queued against its persistent session". A
        // later round on an already-drained queue finds nothing to do and the
        // completion is already in the CQ.
        //
        // Reaped by tag. Tags are handed out in increasing order and every SQE
        // gets exactly one completion, so one with a lower tag answers an SQE
        // that an earlier round-trip gave up on -- the daemon got to it late.
        // On the shared ring that completion would otherwise fail whichever
        // socket's round-trip came next, so it is discarded, and said so. A
        // higher tag cannot exist: nothing later has been submitted.
        let mut got = None;
        let mut rounds = 0u32;
        let mut stale = 0u32;
        'rounds: for _ in 0..8u32 {
            rounds = rounds.saturating_add(1);
            self.submit_round()?;
            if let Ok(h) = self.handle_mut() {
                h.served = true;
            }
            while let Some(c) = self.view.cq_pop() {
                if c.user_data == want_ud {
                    got = Some(c);
                    break 'rounds;
                }
                if c.user_data > want_ud {
                    return Err(KernelError::InternalError);
                }
                stale = stale.saturating_add(1);
            }
        }
        if stale > 0 {
            crate::serial_println!(
                "[netstack-client]   discarded {} late completion(s) of round-trips that \
                 had already given up",
                stale
            );
        }
        // Say so when one round was not enough: if this line never appears the
        // loop is insurance, and if it does, the number says how close a
        // single poll would have come to failing.
        if rounds > 1 {
            crate::serial_println!(
                "[netstack-client]   submit_and_reap needed {} rounds for one \
                 completion -- a single poll would have failed here",
                rounds
            );
        }
        // `TimedOut` rather than `InternalError`: "the daemon never answered"
        // and "the daemon answered wrongly" want different words.
        let cqe = got.ok_or(KernelError::TimedOut)?;
        // No SQE should ever produce more than one completion.
        if self.view.cq_pop().is_some() {
            return Err(KernelError::InternalError);
        }
        Ok(cqe.result)
    }

    /// One `OP_RING_TCP` control round-trip: open a fresh `net.stack` channel,
    /// hand the daemon the ring's handle and size, wait for the
    /// acknowledgement. The daemon drains whatever SQEs are queued.
    fn submit_round(&self) -> KernelResult<()> {
        let client = service::connect(b"net.stack")?;
        // Everything from here is fallible; make sure the channel is always
        // closed regardless of which step fails.
        let outcome = self.submit_round_on(client);
        channel::close(client);
        outcome
    }

    fn submit_round_on(&self, client: channel::ChannelHandle) -> KernelResult<()> {
        let ring = self.handle()?;
        // Authorize the daemon backing `net.stack` to `SYS_SHM_MAP` the ring
        // region before we hand it the handle. Idempotent, so re-doing it every
        // round is cheap. Skipped when the service is kernel-provided (PID 0):
        // the kernel is the TCB and never needs a grant. `authorize` can only
        // fail with `InvalidHandle`, which is impossible here -- the region
        // lives for the whole boot -- so the result is ignorable.
        if let Some(pid) = service::provider_pid(b"net.stack")
            && pid != 0
        {
            let _ = shm::authorize(ring.handle, pid);
        }
        let mut req = [0u8; 16];
        let n = netipc::encode_ring_tcp(&mut req, ring.handle.raw(), ring.size)
            .ok_or(KernelError::InternalError)?;
        let encoded = req.get(..n).ok_or(KernelError::InternalError)?;
        let msg = channel::Message::from_bytes(encoded)?;
        channel::send(client, msg)?;
        let reply = channel::recv_timeout(client, RECV_TIMEOUT_NS)?;
        match netipc::parse_bytes_reply(reply.data()) {
            netipc::BytesReply::Ok(_) => Ok(()),
            netipc::BytesReply::Fail | netipc::BytesReply::Malformed => {
                Err(KernelError::InternalError)
            }
        }
    }
}

/// A connection's local endpoint, as reported by the daemon for `getsockname`.
///
/// The address family is carried by the variant (the daemon distinguishes them by
/// the length it writes to the ring: 6 bytes for v4, 18 for v6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalEndpoint {
    /// IPv4 local address `(ip, port)`.
    V4([u8; 4], u16),
    /// IPv6 local address `(ip6, port)`.
    V6([u8; 16], u16),
}

/// A client of the userspace `net.stack` daemon: one connection id of its own
/// on the shared ring, plus any listener or accepted-connection ids it installs.
///
/// Drive it with [`connect`](Self::connect) → [`send`](Self::send) →
/// [`recv`](Self::recv) → [`close`](Self::close). Dropping it without `close`
/// still closes, best effort, everything it installed in the daemon.
pub struct NetstackConn {
    /// This client's own connection id, from [`alloc_conn_id`].
    conn_id: u32,
    /// The ring its round-trips go through, fixed at [`open`](Self::open) by
    /// [`ring_mode`].
    ring: RingRef,
    /// Every id this client has installed in the daemon's session and not yet
    /// closed: its own connection or datagram socket, a listener, connections
    /// it accepted. Teardown closes each one.
    ///
    /// The shared session is never stopped, so nothing is cleaned up for this
    /// client unless it names it. Each id's space is reserved *before* the
    /// round-trip that installs it, so recording it cannot fail afterwards and
    /// leak a daemon slot.
    installed: Vec<u32>,
}

// Note: every field is plain data (an id and a vector of ids), so
// `NetstackConn` is automatically `Send + Sync`. Crucially it does *not* hold a
// `Ring` view (whose raw `*mut u8` would make it `!Send`): each round-trip takes
// a `RingGuard` for the shared ring and drops it again. This is what lets a
// `NetstackConn` live in the global socket table. Callers that share one must
// still serialize access to it with their own lock, because `installed` is
// state the round-trips update.

impl NetstackConn {
    /// Prepare a client with a fresh connection id, on the ring [`ring_mode`]
    /// chooses now: the shared ring (design A), or a new ring of its own
    /// (design B). Does **not** contact the daemon yet — the first round-trip
    /// happens on [`connect`](Self::connect).
    ///
    /// # Errors
    ///
    /// In per-socket mode, an error creating the ring's shared memory.
    pub fn open() -> KernelResult<Self> {
        let ring = match ring_mode() {
            RingMode::Shared => RingRef::Shared,
            RingMode::PerSocket => RingRef::Own(RingHandle::create()?),
        };
        Ok(Self {
            conn_id: alloc_conn_id(),
            ring,
            installed: Vec::new(),
        })
    }

    /// This client's own connection id.
    ///
    /// A client stream/datagram socket drives this id; a server-side accepted
    /// connection is addressed by its own id via the `*_on` methods. Exposed so the
    /// socket object layer ([`crate::net::socket`]) can record the effective id for a
    /// socket that shares a listener's `NetstackConn`.
    #[must_use]
    pub fn conn_id(&self) -> u32 {
        self.conn_id
    }

    // The three below take the `installed` field, not `self`: a `RingGuard`
    // borrows `self.ring`, and these run while one is alive.

    /// Make room to record one more installed id, before the round-trip that
    /// installs it (see the `installed` field).
    fn reserve_install(installed: &mut Vec<u32>) -> KernelResult<()> {
        installed
            .try_reserve(1)
            .map_err(|_| KernelError::OutOfMemory)
    }

    /// Record that `id` now names something in the daemon's session.
    fn install(installed: &mut Vec<u32>, id: u32) {
        if !installed.contains(&id) {
            // Cannot reallocate: `reserve_install` made room before the
            // round-trip that installed `id`.
            installed.push(id);
        }
    }

    /// Record that `id` no longer names anything in the daemon's session.
    fn uninstall(installed: &mut Vec<u32>, id: u32) {
        installed.retain(|&i| i != id);
    }

    /// Open the TCP connection to `ip:port`.
    ///
    /// When `nonblock` is clear, this performs a **blocking** connect: the daemon
    /// completes the TCP handshake synchronously and the result is `>= 0` on success
    /// (connection now live) or `< 0` if it failed (no upstream / refused).
    ///
    /// When `nonblock` is set, the [`netipc::ring::CONNECT_NONBLOCK`] flag is passed
    /// to the daemon, which transmits the SYN and returns immediately:
    /// - `0` — the handshake already completed (a fast/loopback peer answered within
    ///   the one RX pump the daemon does before replying); the socket is established.
    /// - [`netipc::ring::ERR_IN_PROGRESS`] — the handshake is still pending; the
    ///   caller should `poll(POLLOUT)` and then check
    ///   [`take_so_error`](Self::poll_ready)-style readiness / `getsockopt(SO_ERROR)`.
    /// - `< 0` (other) — the connect could not even be started.
    ///
    /// A non-negative result *or* `ERR_IN_PROGRESS` marks the client connected so a
    /// later [`send`](Self::send) / [`poll_ready`](Self::poll_ready) drives the same
    /// persisted connection.
    ///
    /// # Errors
    ///
    /// Returns an error on a control-protocol fault (ring full, missing/misordered
    /// completion, service-channel failure) — distinct from a `< 0` connect
    /// result, which is a normal "no upstream" outcome.
    pub fn connect(&mut self, ip: &[u8; 4], port: u16, nonblock: bool) -> KernelResult<i32> {
        Self::reserve_install(&mut self.installed)?;
        // Recorded before the round-trip, dropped only on a definite refusal:
        // if the round-trip itself fails, the daemon may still have installed
        // the connection, and a teardown OP_CLOSE for an unknown id is harmless.
        Self::install(&mut self.installed, self.conn_id);
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let ud = ring.next_ud();
        let mut aux = netipc::ring::Sqe::pack_endpoint(ip, port);
        if nonblock {
            aux |= netipc::ring::CONNECT_NONBLOCK;
        }
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_CONNECT,
            conn_id: self.conn_id,
            user_data: ud,
            aux,
            ..netipc::ring::Sqe::default()
        };
        let res = ring.submit_and_reap(&sqe)?;
        // Both an established (`res >= 0`) and an in-progress non-blocking connect
        // leave a live connection installed in the daemon session; anything else
        // installed nothing.
        if res < 0 && res != netipc::ring::ERR_IN_PROGRESS {
            Self::uninstall(&mut self.installed, self.conn_id);
        }
        Ok(res)
    }

    /// IPv6 sibling of [`connect`](Self::connect): open a connection to
    /// `ip6:port` (daemon [`OP_CONNECT6`](netipc::ring::OP_CONNECT6)).
    ///
    /// The 16-byte peer address does not fit in the SQE's 64-bit `aux`, so it
    /// travels in the ring data window (`SND_OFF`, 16 bytes) and the port occupies
    /// the low 16 bits of `aux`; the daemon resolves the next hop via NDP (or, for
    /// a loopback self-connect, diverts by IP without NDP). Result semantics match
    /// [`connect`](Self::connect): `>= 0` established, `ERR_IN_PROGRESS` a started
    /// non-blocking handshake, other `< 0` a failure to start.
    ///
    /// # Errors
    ///
    /// Returns an error on a control-protocol fault (see [`connect`](Self::connect)).
    pub fn connect6(&mut self, ip6: &[u8; 16], port: u16, nonblock: bool) -> KernelResult<i32> {
        Self::reserve_install(&mut self.installed)?;
        // Recorded before the round-trip, as in `connect`.
        Self::install(&mut self.installed, self.conn_id);
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        if !ring.write_data(SND_OFF as usize, ip6) {
            return Err(KernelError::InternalError);
        }
        let ud = ring.next_ud();
        let mut aux = u64::from(port);
        if nonblock {
            aux |= netipc::ring::CONNECT_NONBLOCK;
        }
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_CONNECT6,
            conn_id: self.conn_id,
            data_off: SND_OFF,
            data_len: 16,
            user_data: ud,
            aux,
        };
        let res = ring.submit_and_reap(&sqe)?;
        if res < 0 && res != netipc::ring::ERR_IN_PROGRESS {
            Self::uninstall(&mut self.installed, self.conn_id);
        }
        Ok(res)
    }

    /// Send `buf` to the connected peer, chunking into ≤`SND_CAP` pieces (one
    /// daemon round-trip each).
    ///
    /// Returns the total number of bytes the daemon accepted. Stops early (and
    /// returns the partial total) if the daemon accepts a short/zero write or
    /// returns a negative result mid-stream after some bytes were already queued;
    /// if the very first chunk fails, the negative daemon result is returned as-is
    /// so the caller can distinguish "peer gone" from a protocol fault.
    ///
    /// When `nonblock` is set, the [`netipc::ring::SEND_NONBLOCK`] flag is passed to
    /// the daemon: if the send window is full (a prior segment still unacknowledged)
    /// the daemon returns [`netipc::ring::ERR_WOULD_BLOCK`] rather than waiting for
    /// the peer's ACK. If that happens on the *first* chunk (nothing queued yet)
    /// this method surfaces [`KernelError::WouldBlock`] (→ `EAGAIN`); if it happens
    /// mid-stream after some bytes were accepted, it returns the partial total
    /// (matching Linux `send(2)`, which returns the short count rather than EAGAIN
    /// once it has made progress). When `nonblock` is clear, the daemon blocks
    /// (polls) up to its send deadline for the window to drain.
    ///
    /// # Errors
    ///
    /// - [`KernelError::WouldBlock`] — `nonblock` was set and the window was full
    ///   before any bytes were accepted.
    /// - a control-protocol fault (see [`connect`](Self::connect)).
    pub fn send(&mut self, buf: &[u8], nonblock: bool) -> KernelResult<i32> {
        let cid = self.conn_id;
        self.send_on(cid, buf, nonblock)
    }

    /// Send `buf` on an explicit connection id (see [`send`](Self::send)).
    ///
    /// Used to drive a *server-side* accepted connection whose id differs from the
    /// client's own [`conn_id`](Self::conn_id) on the shared ring (the listen/accept
    /// loopback self-test). The public [`send`](Self::send) is the `self.conn_id`
    /// specialization.
    ///
    /// # Errors
    ///
    /// Same as [`send`](Self::send).
    pub fn send_on(&mut self, conn_id: u32, buf: &[u8], nonblock: bool) -> KernelResult<i32> {
        let send_aux = if nonblock {
            netipc::ring::SEND_NONBLOCK
        } else {
            0
        };
        let mut total: i32 = 0;
        let mut off = 0usize;
        while off < buf.len() {
            // The ring per chunk, not for the whole buffer: a large send by one
            // socket lets other sockets' round-trips in between its chunks.
            let mut ring = RingGuard::acquire(&mut self.ring)?;
            let end = off.saturating_add(SND_CAP as usize).min(buf.len());
            let chunk = buf.get(off..end).ok_or(KernelError::InternalError)?;
            if !ring.write_data(SND_OFF as usize, chunk) {
                return Err(KernelError::InternalError);
            }
            let chunk_len = u32::try_from(chunk.len()).map_err(|_| KernelError::InternalError)?;
            let ud = ring.next_ud();
            let sqe = netipc::ring::Sqe {
                op: netipc::ring::OP_SEND,
                conn_id,
                data_off: SND_OFF,
                data_len: chunk_len,
                user_data: ud,
                aux: send_aux,
            };
            let res = ring.submit_and_reap(&sqe)?;
            if res == netipc::ring::ERR_WOULD_BLOCK {
                // Non-blocking send hit a full window. Report progress if any bytes
                // were already accepted (Linux returns the short count); otherwise
                // surface EAGAIN so the caller's O_NONBLOCK write retries later.
                if total > 0 {
                    return Ok(total);
                }
                return Err(KernelError::WouldBlock);
            }
            if res == netipc::ring::ERR_BROKEN_PIPE {
                // Write side was shut down (`shutdown(SHUT_WR)`). Report any bytes
                // already accepted (Linux returns the short count); otherwise EPIPE.
                if total > 0 {
                    return Ok(total);
                }
                return Err(KernelError::BrokenPipe);
            }
            if res == netipc::ring::ERR_TIMED_OUT {
                // The connection timed out (every resend unanswered). As above:
                // the short count if any bytes were accepted, else ETIMEDOUT.
                if total > 0 {
                    return Ok(total);
                }
                return Err(KernelError::TimedOut);
            }
            if res < 0 {
                // Peer gone mid-stream: report bytes already queued, or the raw
                // negative result if nothing has been sent yet.
                if total > 0 {
                    return Ok(total);
                }
                return Ok(res);
            }
            total = total.saturating_add(res);
            let accepted = usize::try_from(res).unwrap_or(0);
            if accepted == 0 {
                // Daemon accepted nothing this round — avoid an infinite loop.
                break;
            }
            off = off.saturating_add(accepted);
        }
        Ok(total)
    }

    /// Receive up to `min(buf.len(), RCV_CAP)` bytes from the connected peer into
    /// `buf` in a single daemon round-trip.
    ///
    /// Returns the byte count copied into `buf` (`0` means no data this call —
    /// peer idle or closed; the caller decides whether to retry). Negative daemon
    /// results are passed through unchanged.
    ///
    /// When `nonblock` is set, the [`netipc::ring::RECV_NONBLOCK`] flag is passed
    /// to the daemon: if no data has arrived yet and the stream is still open, the
    /// daemon returns [`netipc::ring::ERR_WOULD_BLOCK`] instead of polling, which
    /// this method surfaces as [`KernelError::WouldBlock`] (→ `EAGAIN`). This is
    /// how a caller honours `O_NONBLOCK` on a daemon-backed stream socket. When
    /// `nonblock` is clear, the daemon blocks (polls) up to its receive deadline.
    ///
    /// When `peek` is set, the [`netipc::ring::RECV_PEEK`] flag is passed to the
    /// daemon: buffered bytes are copied out **without** being consumed, so a
    /// subsequent `recv` returns the same data. This is how a caller honours
    /// `MSG_PEEK` on a daemon-backed stream socket.
    ///
    /// # Errors
    ///
    /// - [`KernelError::WouldBlock`] — `nonblock` was set and no data was ready.
    /// - a control-protocol fault (see [`connect`](Self::connect)), or a failure to
    ///   read back the ring data window.
    pub fn recv(&mut self, buf: &mut [u8], nonblock: bool, peek: bool) -> KernelResult<i32> {
        let cid = self.conn_id;
        self.recv_on(cid, buf, nonblock, peek)
    }

    /// Receive on an explicit connection id (see [`recv`](Self::recv)).
    ///
    /// The server-side counterpart to [`send_on`](Self::send_on): reads from an
    /// accepted connection whose id differs from the client's own [`conn_id`](Self::conn_id)
    /// on the shared ring. The public [`recv`](Self::recv) is the
    /// `self.conn_id` specialization.
    ///
    /// # Errors
    ///
    /// Same as [`recv`](Self::recv).
    pub fn recv_on(
        &mut self,
        conn_id: u32,
        buf: &mut [u8],
        nonblock: bool,
        peek: bool,
    ) -> KernelResult<i32> {
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let want = buf.len().min(RCV_CAP as usize);
        let want_u32 = u32::try_from(want).map_err(|_| KernelError::InternalError)?;
        let ud = ring.next_ud();
        let mut aux = if nonblock {
            netipc::ring::RECV_NONBLOCK
        } else {
            0
        };
        if peek {
            aux |= netipc::ring::RECV_PEEK;
        }
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_RECV,
            conn_id,
            data_off: RCV_OFF,
            data_len: want_u32,
            user_data: ud,
            aux,
        };
        let res = ring.submit_and_reap(&sqe)?;
        if res == netipc::ring::ERR_WOULD_BLOCK {
            // Nothing ready. `net::socket` asks this way on purpose and waits
            // itself (`wait_until`); an O_NONBLOCK caller gets EAGAIN.
            return Err(KernelError::WouldBlock);
        }
        if res == netipc::ring::ERR_TIMED_OUT {
            // Every resend of our last segment went unanswered: the peer is
            // gone. ETIMEDOUT -- never the 0 that would read as its EOF.
            return Err(KernelError::TimedOut);
        }
        if res <= 0 {
            return Ok(res);
        }
        let n = usize::try_from(res).unwrap_or(0).min(want);
        let window = buf.get_mut(..n).ok_or(KernelError::InternalError)?;
        if !ring.read_data(RCV_OFF as usize, window) {
            return Err(KernelError::InternalError);
        }
        Ok(res)
    }

    /// Bind a connectionless UDP datagram socket on `port`
    /// (daemon [`OP_UDP_BIND`](netipc::ring::OP_UDP_BIND)).
    ///
    /// `port == 0` asks the daemon to pick an unused ephemeral port. Returns the
    /// bound local port (`>= 0`), which the caller records for `getsockname`.
    /// Marks the client "connected" so teardown emits the `OP_CLOSE` that unbinds
    /// the daemon-side socket.
    ///
    /// # Errors
    ///
    /// - [`KernelError::AddrInUse`] — the daemon reports the port is already bound
    ///   ([`ERR_ADDR_IN_USE`](netipc::ring::ERR_ADDR_IN_USE)).
    /// - [`KernelError::ResourceExhausted`] — the daemon's socket table is full.
    /// - a control-protocol fault (see [`connect`](Self::connect)).
    pub fn udp_bind(&mut self, port: u16) -> KernelResult<u16> {
        // An unbound socket `udp_open` made is in the daemon already: a refused
        // bind leaves it there, so must leave it recorded for teardown too.
        let opened = self.installed.contains(&self.conn_id);
        Self::reserve_install(&mut self.installed)?;
        // Recorded before the round-trip, as in `connect`.
        Self::install(&mut self.installed, self.conn_id);
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let ud = ring.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_UDP_BIND,
            conn_id: self.conn_id,
            user_data: ud,
            aux: u64::from(port),
            ..netipc::ring::Sqe::default()
        };
        let res = ring.submit_and_reap(&sqe)?;
        if res < 0 && !opened {
            Self::uninstall(&mut self.installed, self.conn_id);
        }
        if res == netipc::ring::ERR_ADDR_IN_USE {
            return Err(KernelError::AddrInUse);
        }
        if res < 0 {
            // Table full (daemon `-1`) or any other bind failure.
            return Err(KernelError::ResourceExhausted);
        }
        // A bound UDP socket holds a daemon-side slot, recorded above so teardown
        // emits the OP_CLOSE that unbinds it (the daemon routes OP_CLOSE to
        // `udp.take` when the id isn't a TCP connection).
        u16::try_from(res).map_err(|_| KernelError::InternalError)
    }

    /// Create the daemon-side UDP socket **without a port**
    /// (daemon [`OP_UDP_BIND`](netipc::ring::OP_UDP_BIND) with
    /// [`UDP_BIND_UNBOUND`](netipc::ring::UDP_BIND_UNBOUND)): what a socket
    /// needs to take multicast options before `bind(2)`, as Linux's does. A
    /// later [`udp_bind`](Self::udp_bind) gives it its port, keeping its
    /// options and groups. Teardown closes it like a bound one.
    ///
    /// # Errors
    ///
    /// - [`KernelError::ResourceExhausted`] — the daemon's socket table is full.
    /// - [`KernelError::AddrInUse`] — the daemon already holds a socket under
    ///   this id (a caller bug: open it once).
    /// - a control-protocol fault (see [`connect`](Self::connect)).
    pub fn udp_open(&mut self) -> KernelResult<()> {
        Self::reserve_install(&mut self.installed)?;
        Self::install(&mut self.installed, self.conn_id);
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let ud = ring.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_UDP_BIND,
            conn_id: self.conn_id,
            user_data: ud,
            aux: netipc::ring::UDP_BIND_UNBOUND,
            ..netipc::ring::Sqe::default()
        };
        let res = ring.submit_and_reap(&sqe)?;
        match res {
            0 => Ok(()),
            // Whatever is under this id was there before: not ours to forget.
            netipc::ring::ERR_ADDR_IN_USE => Err(KernelError::AddrInUse),
            _ => {
                Self::uninstall(&mut self.installed, self.conn_id);
                Err(KernelError::ResourceExhausted)
            }
        }
    }

    /// Set one of the UDP socket's multicast options
    /// (daemon [`OP_UDP_SETOPT`](netipc::ring::OP_UDP_SETOPT)): the scalar
    /// `value`, or the group in `window` for a join or leave
    /// ([`netipc::sockopt::Set`]). The socket must exist in the daemon —
    /// bound, or [opened](Self::udp_open).
    ///
    /// Returns `Ok(Ok(()))`, or `Ok(Err(errno))` with the positive Linux errno
    /// the daemon refused it with (`EINVAL`, `EADDRINUSE`, `EADDRNOTAVAIL`,
    /// `ENOBUFS`, `ENODEV`), which the socket layer hands to the caller as
    /// it is: these are Linux's own answers to the same call, and have no
    /// [`KernelError`] of their own.
    ///
    /// # Errors
    ///
    /// - [`KernelError::MsgSize`] — `window` is larger than the ring's
    ///   send window (a caller bug: a group window is at most 16 bytes).
    /// - [`KernelError::InternalError`] — the daemon holds no socket under
    ///   this id, or answered something no setsockopt can.
    /// - a control-protocol fault (see [`connect`](Self::connect)).
    pub fn udp_setopt(
        &mut self,
        option: u16,
        value: u32,
        window: &[u8],
    ) -> KernelResult<Result<(), i32>> {
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        if window.len() > SND_CAP as usize {
            return Err(KernelError::MsgSize);
        }
        if !window.is_empty() && !ring.write_data(SND_OFF as usize, window) {
            return Err(KernelError::InternalError);
        }
        let data_len = u32::try_from(window.len()).map_err(|_| KernelError::InternalError)?;
        let ud = ring.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_UDP_SETOPT,
            conn_id: self.conn_id,
            data_off: SND_OFF,
            data_len,
            user_data: ud,
            aux: netipc::ring::Sqe::pack_udp_opt(option, value),
        };
        let res = ring.submit_and_reap(&sqe)?;
        match res {
            0 => Ok(Ok(())),
            netipc::ring::ERR_INVALID
            | netipc::ring::ERR_ADDR_IN_USE
            | netipc::ring::ERR_ADDR_NOT_AVAIL
            | netipc::ring::ERR_NO_BUFS
            | netipc::ring::ERR_NO_DEVICE => Ok(Err(res.saturating_neg())),
            _ => Err(KernelError::InternalError),
        }
    }

    /// Read one of the UDP socket's scalar multicast options back
    /// (daemon [`OP_UDP_GETOPT`](netipc::ring::OP_UDP_GETOPT)). The socket
    /// must exist in the daemon, as for [`udp_setopt`](Self::udp_setopt).
    ///
    /// # Errors
    ///
    /// - [`KernelError::InvalidArgument`] — `option` has no value to read.
    /// - [`KernelError::InternalError`] — the daemon holds no socket under
    ///   this id.
    /// - a control-protocol fault (see [`connect`](Self::connect)).
    pub fn udp_getopt(&mut self, option: u16) -> KernelResult<i32> {
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let ud = ring.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_UDP_GETOPT,
            conn_id: self.conn_id,
            user_data: ud,
            aux: netipc::ring::Sqe::pack_udp_opt(option, 0),
            ..netipc::ring::Sqe::default()
        };
        let res = ring.submit_and_reap(&sqe)?;
        match res {
            v if v >= 0 => Ok(v),
            netipc::ring::ERR_INVALID => Err(KernelError::InvalidArgument),
            _ => Err(KernelError::InternalError),
        }
    }

    /// Send one UDP datagram from the bound socket to `ip:port`
    /// (daemon [`OP_UDP_SEND`](netipc::ring::OP_UDP_SEND)).
    ///
    /// The payload travels in the ring send window; the destination address rides
    /// in the SQE `aux` ([`pack_endpoint`](netipc::ring::Sqe::pack_endpoint)).
    /// Returns the number of payload bytes accepted (the whole datagram on success).
    /// A datagram larger than the daemon's per-datagram limit is rejected.
    ///
    /// # Errors
    ///
    /// - [`KernelError::MsgSize`] — the payload exceeds the daemon's datagram limit
    ///   ([`ERR_MSG_SIZE`](netipc::ring::ERR_MSG_SIZE)).
    /// - [`KernelError::NotConnected`] — the socket is not bound (daemon `-1`).
    /// - a control-protocol fault (see [`connect`](Self::connect)).
    pub fn udp_send_to(&mut self, ip: &[u8; 4], port: u16, buf: &[u8]) -> KernelResult<i32> {
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        // A single datagram must fit the ring send window (the daemon also caps it
        // to its per-datagram maximum and returns ERR_MSG_SIZE). Rather than
        // silently truncate a payload larger than the window, reject it as EMSGSIZE
        // — a datagram is all-or-nothing, so a partial send would corrupt it.
        if buf.len() > SND_CAP as usize {
            return Err(KernelError::MsgSize);
        }
        let want = buf.len();
        let payload = buf.get(..want).ok_or(KernelError::InternalError)?;
        if want > 0 && !ring.write_data(SND_OFF as usize, payload) {
            return Err(KernelError::InternalError);
        }
        let want_u32 = u32::try_from(want).map_err(|_| KernelError::InternalError)?;
        let ud = ring.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_UDP_SEND,
            conn_id: self.conn_id,
            data_off: SND_OFF,
            data_len: want_u32,
            user_data: ud,
            aux: netipc::ring::Sqe::pack_endpoint(ip, port),
        };
        let res = ring.submit_and_reap(&sqe)?;
        if res == netipc::ring::ERR_MSG_SIZE {
            return Err(KernelError::MsgSize);
        }
        if res < 0 {
            // Not bound, or a TX failure — surface as not-connected.
            return Err(KernelError::NotConnected);
        }
        Ok(res)
    }

    /// Send one UDP datagram over **IPv6** from the bound socket to `[ip6]:port`
    /// (daemon [`OP_UDP_SEND6`](netipc::ring::OP_UDP_SEND6)).
    ///
    /// The 16-byte IPv6 destination does not fit in the SQE `aux`, so it rides at
    /// the front of the ring send window (`[dst_ip16:16][payload...]`); the port
    /// travels in the low 16 bits of `aux`. The IPv6 sibling of
    /// [`udp_send_to`](Self::udp_send_to); the same completion semantics apply.
    ///
    /// # Errors
    ///
    /// - [`KernelError::MsgSize`] — the payload exceeds the daemon's datagram limit
    ///   ([`ERR_MSG_SIZE`](netipc::ring::ERR_MSG_SIZE)), or `16 + payload` overruns
    ///   the ring send window.
    /// - [`KernelError::NotConnected`] — the socket is not bound (daemon `-1`).
    /// - a control-protocol fault (see [`connect`](Self::connect)).
    pub fn udp_send_to6(&mut self, ip6: &[u8; 16], port: u16, buf: &[u8]) -> KernelResult<i32> {
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        // The 16-byte destination address plus the payload must fit the ring send
        // window; reject an oversized datagram as EMSGSIZE rather than truncating
        // (a datagram is all-or-nothing).
        let want = buf.len();
        if want.saturating_add(16) > SND_CAP as usize {
            return Err(KernelError::MsgSize);
        }
        // Prepend the destination address, then the payload.
        if !ring.write_data(SND_OFF as usize, ip6) {
            return Err(KernelError::InternalError);
        }
        if want > 0 {
            let payload = buf.get(..want).ok_or(KernelError::InternalError)?;
            if !ring.write_data(SND_OFF as usize + 16, payload) {
                return Err(KernelError::InternalError);
            }
        }
        let data_len = u32::try_from(want + 16).map_err(|_| KernelError::InternalError)?;
        let ud = ring.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_UDP_SEND6,
            conn_id: self.conn_id,
            data_off: SND_OFF,
            data_len,
            user_data: ud,
            aux: u64::from(port),
        };
        let res = ring.submit_and_reap(&sqe)?;
        if res == netipc::ring::ERR_MSG_SIZE {
            return Err(KernelError::MsgSize);
        }
        if res < 0 {
            return Err(KernelError::NotConnected);
        }
        Ok(res)
    }

    /// Receive one UDP datagram into `buf`, reporting the source address
    /// (daemon [`OP_UDP_RECV`](netipc::ring::OP_UDP_RECV)).
    ///
    /// The daemon drains the NIC, routes datagrams to their bound sockets, and
    /// dequeues the oldest datagram for this socket, prepending a 24-byte in-band
    /// source-address header ([`pack_udp_addr`](netipc::ring::Sqe::pack_udp_addr))
    /// to the ring window. Returns `(payload_len, src_ip, src_port)`. The payload
    /// is truncated to `buf.len()` (excess bytes are lost, matching UDP `recvfrom`
    /// without `MSG_TRUNC`).
    ///
    /// When `nonblock` is set (or the daemon has no queued datagram), an empty
    /// queue surfaces as [`KernelError::WouldBlock`] (→ `EAGAIN`); the caller's
    /// poll/retry loop drives blocking semantics.
    ///
    /// # Errors
    ///
    /// - [`KernelError::WouldBlock`] — no datagram is queued.
    /// - [`KernelError::NotConnected`] — the socket is not bound (daemon `-1`).
    /// - a control-protocol fault (see [`connect`](Self::connect)), or a failure to
    ///   read back the ring window.
    pub fn udp_recv_from(
        &mut self,
        buf: &mut [u8],
        nonblock: bool,
    ) -> KernelResult<(i32, [u8; 4], u16)> {
        let (res, _family, ip16, port) = self.udp_recv_any(buf, nonblock)?;
        let mut src_ip = [0u8; 4];
        src_ip.copy_from_slice(ip16.get(..4).ok_or(KernelError::InternalError)?);
        Ok((res, src_ip, port))
    }

    /// Receive one UDP datagram into `buf`, reporting the source address **with its
    /// family** — the family-aware core behind [`udp_recv_from`](Self::udp_recv_from)
    /// (which drops the family and truncates the address to 4 bytes for IPv4).
    ///
    /// Returns `(payload_len, family, src_ip16, src_port)` where `family` is
    /// [`UDP_AF_INET`](netipc::ring::UDP_AF_INET) or
    /// [`UDP_AF_INET6`](netipc::ring::UDP_AF_INET6) and `src_ip16` is the fixed
    /// 16-byte address (IPv4 in `src_ip16[0..4]`, the rest zero). The socket layer
    /// uses the reported family to write back the matching `sockaddr_in` /
    /// `sockaddr_in6`, since a datagram's family is a property of the packet, not
    /// the socket. See [`udp_recv_from`](Self::udp_recv_from) for the error cases.
    pub fn udp_recv_any(
        &mut self,
        buf: &mut [u8],
        nonblock: bool,
    ) -> KernelResult<(i32, u16, [u8; 16], u16)> {
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let hdr_len = netipc::ring::UDP_ADDR_HDR_LEN;
        // The daemon writes a 24-byte address header + payload into the recv
        // window, so the window must be at least the header plus whatever payload
        // the caller can take (capped to the recv window).
        let room = buf.len().min((RCV_CAP as usize).saturating_sub(hdr_len));
        let cap = hdr_len.saturating_add(room);
        let cap_u32 = u32::try_from(cap).map_err(|_| KernelError::InternalError)?;
        let ud = ring.next_ud();
        let aux = if nonblock {
            netipc::ring::RECV_NONBLOCK
        } else {
            0
        };
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_UDP_RECV,
            conn_id: self.conn_id,
            data_off: RCV_OFF,
            data_len: cap_u32,
            user_data: ud,
            aux,
        };
        let res = ring.submit_and_reap(&sqe)?;
        if res == netipc::ring::ERR_WOULD_BLOCK {
            return Err(KernelError::WouldBlock);
        }
        if res < 0 {
            // -1 = not bound (or window too small, already avoided above).
            return Err(KernelError::NotConnected);
        }
        // Read back the address header, then the payload.
        let mut hdr = [0u8; netipc::ring::UDP_ADDR_HDR_LEN];
        if !ring.read_data(RCV_OFF as usize, &mut hdr) {
            return Err(KernelError::InternalError);
        }
        let (family, ip16, src_port) =
            netipc::ring::Sqe::unpack_udp_addr(&hdr).ok_or(KernelError::InternalError)?;
        let n = usize::try_from(res).unwrap_or(0).min(room).min(buf.len());
        if n > 0 {
            let window = buf.get_mut(..n).ok_or(KernelError::InternalError)?;
            if !ring.read_data(RCV_OFF as usize + hdr_len, window) {
                return Err(KernelError::InternalError);
            }
        }
        Ok((res, family, ip16, src_port))
    }

    /// Probe the connection's readiness **without consuming any buffered data**
    /// (a non-destructive peek).
    ///
    /// Issues an [`OP_POLL`](netipc::ring::OP_POLL) round-trip: the daemon drains
    /// arrived frames once and reports a readiness bitmask. Returns
    /// `(readable, writable, error)`:
    /// - `readable` — the socket has buffered bytes or the peer has closed, so a
    ///   subsequent `recv`/`read` returns data (or `0`/EOF) promptly.
    /// - `writable` — the connection is established and can accept a send. A
    ///   non-blocking connect still in its handshake reports *not* writable until it
    ///   completes (so `poll(POLLOUT)` waits for the connect to resolve).
    /// - `error` — the connection has an error condition (a non-blocking connect
    ///   that was refused / timed out). Linux wakes `POLLOUT` **and** `POLLERR` in
    ///   this case; `getsockopt(SO_ERROR)` then reports `ECONNREFUSED`.
    ///
    /// A subsequent [`recv`](Self::recv) still returns the same bytes — this only
    /// reports readiness, it does not move data. Used by the poll/epoll engine to
    /// report an honest `POLLIN`/`POLLOUT`/`POLLERR` for a daemon-backed socket.
    ///
    /// # Errors
    ///
    /// Returns a control-protocol fault (see [`connect`](Self::connect)), or
    /// [`KernelError::NotConnected`] if the daemon reports no such connection
    /// (the socket was never connected or has been torn down).
    pub fn poll_ready(&mut self) -> KernelResult<(bool, bool, bool)> {
        let cid = self.conn_id;
        self.poll_on(cid)
    }

    /// Non-destructively poll an explicit connection (or listener) id
    /// (see [`poll_ready`](Self::poll_ready)).
    ///
    /// The server-side counterpart to [`poll_ready`](Self::poll_ready): reports the
    /// readiness of an accepted connection — or a listener, for which `readable`
    /// signals a pending connection in the backlog — whose id differs from the
    /// client's own [`conn_id`](Self::conn_id) on the shared ring.
    ///
    /// # Errors
    ///
    /// Same as [`poll_ready`](Self::poll_ready).
    pub fn poll_on(&mut self, conn_id: u32) -> KernelResult<(bool, bool, bool)> {
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let ud = ring.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_POLL,
            conn_id,
            user_data: ud,
            ..netipc::ring::Sqe::default()
        };
        let res = ring.submit_and_reap(&sqe)?;
        if res < 0 {
            // Daemon reports no such connection (`-1`).
            return Err(KernelError::NotConnected);
        }
        let readable = res & netipc::ring::POLL_READABLE != 0;
        let writable = res & netipc::ring::POLL_WRITABLE != 0;
        let error = res & netipc::ring::POLL_ERR != 0;
        Ok((readable, writable, error))
    }

    /// Register a passive TCP listener on `port` under `listener_id`
    /// (daemon [`OP_LISTEN`](netipc::ring::OP_LISTEN)).
    ///
    /// `listener_id` is a session-local id distinct from any connection id; the
    /// daemon keys the listener table by it. The low 16 bits of the SQE `aux`
    /// carry the local port (host byte order). Returns `0` on success or `-1` if
    /// the listener table is full / the id is already in use.
    ///
    /// This is the server-side entry point for the userspace-netstack cutover:
    /// a `bind`+`listen` on an AF_INET socket maps to one `OP_LISTEN`.
    ///
    /// # Errors
    ///
    /// Returns a control-protocol fault (see [`connect`](Self::connect)).
    pub fn listen(&mut self, listener_id: u32, port: u16) -> KernelResult<i32> {
        Self::reserve_install(&mut self.installed)?;
        // Recorded before the round-trip, as in `connect`.
        Self::install(&mut self.installed, listener_id);
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let ud = ring.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_LISTEN,
            conn_id: listener_id,
            user_data: ud,
            aux: u64::from(port),
            ..netipc::ring::Sqe::default()
        };
        let res = ring.submit_and_reap(&sqe)?;
        if res != 0 {
            Self::uninstall(&mut self.installed, listener_id);
        }
        Ok(res)
    }

    /// Dequeue one established connection from `listener_id`'s backlog into
    /// `new_conn_id` (daemon [`OP_ACCEPT`](netipc::ring::OP_ACCEPT)).
    ///
    /// The low 32 bits of the SQE `aux` carry `new_conn_id` — the id under which
    /// the daemon installs the accepted connection so later `send`/`recv` can
    /// address it. On success (`0`) the 6-byte peer address `[ip:4][port_be:2]`
    /// is written into `peer`. Returns:
    /// - `0` — an established connection was accepted (`peer` filled).
    /// - [`netipc::ring::ERR_WOULD_BLOCK`] — the backlog is empty (no completed
    ///   handshake waiting); a non-blocking `accept(2)` maps this to `EAGAIN`.
    /// - `-1` — unknown listener id, or the accepted-conn id could not be
    ///   installed (id already in use / table full).
    ///
    /// # Errors
    ///
    /// Returns a control-protocol fault (see [`connect`](Self::connect)), or a
    /// failure to read back the peer-address window.
    pub fn accept(
        &mut self,
        listener_id: u32,
        new_conn_id: u32,
        peer: &mut [u8; 6],
    ) -> KernelResult<i32> {
        Self::reserve_install(&mut self.installed)?;
        // Recorded before the round-trip, as in `connect`.
        Self::install(&mut self.installed, new_conn_id);
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let ud = ring.next_ud();
        // Reuse the recv-landing window for the 6-byte peer address: the guard
        // holds the ring for this whole round-trip, so no other receive can land
        // in the window before the address is read back.
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_ACCEPT,
            conn_id: listener_id,
            data_off: RCV_OFF,
            data_len: 6,
            user_data: ud,
            aux: u64::from(new_conn_id),
        };
        let res = ring.submit_and_reap(&sqe)?;
        if res != 0 {
            // Nothing accepted (an empty backlog, or a refusal): nothing to close.
            Self::uninstall(&mut self.installed, new_conn_id);
        } else if !ring.read_data(RCV_OFF as usize, peer) {
            // Accepted, so it stays recorded for teardown to close.
            return Err(KernelError::InternalError);
        }
        Ok(res)
    }

    /// IPv6 sibling of [`accept`](Self::accept): dequeue one established connection
    /// and read back an 18-byte peer address `[ip6:16][port_be:2]`.
    ///
    /// The daemon writes the IPv6 form when the accepted connection is IPv6 and the
    /// window is at least 18 bytes; a listener is family-agnostic, so this is the
    /// variant to call when the accepted connection is expected to be IPv6. Result
    /// semantics match [`accept`](Self::accept).
    ///
    /// # Errors
    ///
    /// Returns a control-protocol fault (see [`connect`](Self::connect)), or a
    /// failure to read back the peer-address window.
    pub fn accept6(
        &mut self,
        listener_id: u32,
        new_conn_id: u32,
        peer: &mut [u8; 18],
    ) -> KernelResult<i32> {
        Self::reserve_install(&mut self.installed)?;
        // Recorded before the round-trip, as in `connect`.
        Self::install(&mut self.installed, new_conn_id);
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let ud = ring.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_ACCEPT,
            conn_id: listener_id,
            data_off: RCV_OFF,
            data_len: 18,
            user_data: ud,
            aux: u64::from(new_conn_id),
        };
        let res = ring.submit_and_reap(&sqe)?;
        if res != 0 {
            // Nothing accepted (an empty backlog, or a refusal): nothing to close.
            Self::uninstall(&mut self.installed, new_conn_id);
        } else if !ring.read_data(RCV_OFF as usize, peer) {
            // Accepted, so it stays recorded for teardown to close.
            return Err(KernelError::InternalError);
        }
        Ok(res)
    }

    /// Query this connection's **local** endpoint for `getsockname`
    /// (daemon [`OP_LOCALADDR`](netipc::ring::OP_LOCALADDR)).
    ///
    /// The daemon owns the local address — the NIC's configured IP and the
    /// ephemeral source port it chose when it built the SYN — so it is asked
    /// directly rather than tracked kernel-side. The daemon writes the endpoint
    /// into the recv window and returns its length: `6` = IPv4 (`[ip:4][port:2]`),
    /// `18` = IPv6 (`[ip6:16][port:2]`); the family is recovered from that length.
    ///
    /// # Errors
    ///
    /// - [`KernelError::NotConnected`] — the daemon reports no such connection
    ///   (`-1`), i.e. the socket was never connected or has been torn down.
    /// - a control-protocol fault (see [`connect`](Self::connect)), or a failure to
    ///   read back the address window / an unexpected length.
    pub fn local_addr(&mut self) -> KernelResult<LocalEndpoint> {
        let cid = self.conn_id;
        self.local_addr_on(cid)
    }

    /// Query an explicit connection id's **local** endpoint for `getsockname`
    /// (see [`local_addr`](Self::local_addr)).
    ///
    /// The server-side counterpart to [`local_addr`](Self::local_addr): reports the
    /// local address of an accepted connection whose id differs from the client's
    /// own [`conn_id`](Self::conn_id) on the shared ring.
    ///
    /// # Errors
    ///
    /// Same as [`local_addr`](Self::local_addr).
    pub fn local_addr_on(&mut self, conn_id: u32) -> KernelResult<LocalEndpoint> {
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let ud = ring.next_ud();
        // Ask for the 18-byte (v6) form; the daemon writes 6 for a v4 connection.
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_LOCALADDR,
            conn_id,
            data_off: RCV_OFF,
            data_len: 18,
            user_data: ud,
            aux: 0,
        };
        let res = ring.submit_and_reap(&sqe)?;
        match res {
            6 => {
                let mut buf = [0u8; 6];
                if !ring.read_data(RCV_OFF as usize, &mut buf) {
                    return Err(KernelError::InternalError);
                }
                let [a0, a1, a2, a3, p0, p1] = buf;
                Ok(LocalEndpoint::V4(
                    [a0, a1, a2, a3],
                    u16::from_be_bytes([p0, p1]),
                ))
            }
            18 => {
                let mut buf = [0u8; 18];
                if !ring.read_data(RCV_OFF as usize, &mut buf) {
                    return Err(KernelError::InternalError);
                }
                let ip6: [u8; 16] = buf
                    .get(..16)
                    .and_then(|s| <[u8; 16]>::try_from(s).ok())
                    .ok_or(KernelError::InternalError)?;
                let port = buf
                    .get(16..18)
                    .and_then(|s| <[u8; 2]>::try_from(s).ok())
                    .map(u16::from_be_bytes)
                    .ok_or(KernelError::InternalError)?;
                Ok(LocalEndpoint::V6(ip6, port))
            }
            _ => Err(KernelError::NotConnected),
        }
    }

    /// Half- or full-close the connection per `shutdown(2)`.
    ///
    /// `how` is the Linux value: [`netipc::ring::SHUT_RD`] (0),
    /// [`netipc::ring::SHUT_WR`] (1), or [`netipc::ring::SHUT_RDWR`] (2). Unlike
    /// [`close`](Self::close) this keeps the connection (and its ring session)
    /// alive — the still-open direction continues to work. After `SHUT_WR` a
    /// subsequent [`send`](Self::send) fails with [`KernelError::BrokenPipe`]; after
    /// `SHUT_RD` a subsequent [`recv`](Self::recv) reports EOF (0 bytes).
    ///
    /// # Errors
    ///
    /// - [`KernelError::NotConnected`] — the daemon has no such connection.
    /// - transport/resource faults from the underlying ring round-trip.
    pub fn shutdown(&mut self, how: u64) -> KernelResult<()> {
        let cid = self.conn_id;
        self.shutdown_on(cid, how)
    }

    /// Half- or full-close an explicit connection id per `shutdown(2)`
    /// (see [`shutdown`](Self::shutdown)).
    ///
    /// The server-side counterpart to [`shutdown`](Self::shutdown): shuts down an
    /// accepted connection whose id differs from the client's own [`conn_id`](Self::conn_id)
    /// on the shared ring.
    ///
    /// # Errors
    ///
    /// Same as [`shutdown`](Self::shutdown).
    pub fn shutdown_on(&mut self, conn_id: u32, how: u64) -> KernelResult<()> {
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let ud = ring.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_SHUTDOWN,
            conn_id,
            data_off: 0,
            data_len: 0,
            user_data: ud,
            aux: how,
        };
        match ring.submit_and_reap(&sqe)? {
            0 => Ok(()),
            _ => Err(KernelError::NotConnected), // -1 → unknown connection
        }
    }

    /// Close a single id this client installed (daemon
    /// [`OP_CLOSE`](netipc::ring::OP_CLOSE)): a connection, an accepted
    /// connection, a datagram socket or a listener. Everything else on the
    /// shared ring stays alive.
    ///
    /// This is how a server releases one accepted connection when its socket
    /// closes while the listener stays up. A no-op for an id this client never
    /// installed.
    ///
    /// # Errors
    ///
    /// Returns a control-protocol fault (see [`connect`](Self::connect)). A daemon
    /// `-1` (unknown id) is treated as success — the id is already gone, which is
    /// the desired end state.
    pub fn close_conn(&mut self, conn_id: u32) -> KernelResult<()> {
        if !self.installed.contains(&conn_id) {
            return Ok(());
        }
        let mut ring = RingGuard::acquire(&mut self.ring)?;
        let ud = ring.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_CLOSE,
            conn_id,
            user_data: ud,
            ..netipc::ring::Sqe::default()
        };
        // A `-1` (unknown id) is fine: it is already gone.
        let _ = ring.submit_and_reap(&sqe)?;
        Self::uninstall(&mut self.installed, conn_id);
        Ok(())
    }

    /// Close everything this client installed in the daemon.
    ///
    /// Consumes the client. Best effort: a failure closing one id does not stop
    /// the others from being closed.
    ///
    /// # Errors
    ///
    /// Currently always `Ok(())`; the signature is fallible to allow future
    /// teardown validation without a breaking change.
    pub fn close(mut self) -> KernelResult<()> {
        self.teardown();
        Ok(())
    }

    // ---- internals --------------------------------------------------------

    /// Close every installed id, one round-trip each. Idempotent: the list is
    /// emptied as it goes. The shared session itself is never stopped -- other
    /// sockets are using it.
    fn teardown(&mut self) {
        let ids = core::mem::take(&mut self.installed);
        for id in ids {
            // A guard per id, so other sockets interleave with a long teardown.
            let Ok(mut ring) = RingGuard::acquire(&mut self.ring) else {
                // No ring to reach the daemon through: nothing can be closed, and
                // the daemon's own table is what outlives this.
                return;
            };
            let ud = ring.next_ud();
            let sqe = netipc::ring::Sqe {
                op: netipc::ring::OP_CLOSE,
                conn_id: id,
                user_data: ud,
                ..netipc::ring::Sqe::default()
            };
            // Best effort, per id: a daemon that restarted has nothing to close.
            let _ = ring.submit_and_reap(&sqe);
        }
        // A ring of our own is a daemon session of its own (design B): stop it,
        // so the daemon unmaps the region before `RingHandle`'s drop frees it.
        // Only if the daemon ever served it -- a never-used ring has no session
        // to stop, and must not contact the daemon at all.
        if matches!(&self.ring, RingRef::Own(h) if h.served) {
            if let Ok(mut ring) = RingGuard::acquire(&mut self.ring) {
                let ud = ring.next_ud();
                let sqe = netipc::ring::Sqe {
                    op: netipc::ring::OP_STOP,
                    user_data: ud,
                    ..netipc::ring::Sqe::default()
                };
                // Best effort: the region is freed either way.
                let _ = ring.submit_and_reap(&sqe);
            }
            if let RingRef::Own(h) = &mut self.ring {
                h.served = false;
            }
        }
    }
}

impl Drop for NetstackConn {
    fn drop(&mut self) {
        // Close whatever the caller did not close, and stop an own ring's
        // session; the `RingHandle` field's own drop then frees its region.
        self.teardown();
    }
}

/// Boot self-test: fetch an HTTP response through the reusable [`NetstackConn`]
/// client (connect → send → recv → close, each a separate daemon round-trip).
///
/// This replaces the hand-inlined `netstack_ring_tcp_persist_roundtrip` in
/// `spawn.rs`: because the client drives connect and send in *separate* control
/// round-trips, a successful send after connect proves the daemon's session
/// persisted across submissions — the exact property that test validated.
///
/// Returns `Ok(Some(()))` if the connection returned an HTTP response,
/// `Ok(None)` if there was no upstream / a short response (network variance — the
/// client path still ran end to end), and `Err` on a real protocol fault.
///
/// # Errors
///
/// Propagates control-protocol faults from the client, and reports a send that
/// fails on a freshly-connected session (which would mean the daemon session did
/// *not* survive between the connect and send rounds) as an error.
pub fn self_test_http(ip: &[u8; 4], port: u16) -> KernelResult<Option<()>> {
    const HTTP_REQ: &[u8] = b"HEAD / HTTP/1.0\r\nHost: example.com\r\nConnection: close\r\n\r\n";

    let mut conn = NetstackConn::open()?;

    let connect_res = conn.connect(ip, port, false)?;
    if connect_res < 0 {
        // No upstream — the client round-trip path still ran; report cleanly.
        conn.close()?;
        return Ok(None);
    }

    let send_res = conn.send(HTTP_REQ, false)?;
    if send_res < 0 {
        crate::serial_println!(
            "[netstack-client]   persisted-conn send failed (result {}) — session did not \
             survive across submissions",
            send_res
        );
        conn.close()?;
        return Err(KernelError::InternalError);
    }

    let mut body = [0u8; RCV_CAP as usize];
    let recv_res = conn.recv(&mut body, false, false)?;
    conn.close()?;

    if recv_res < 5 {
        // Connected + sent, but nothing came back (slirp variance). The
        // persistence path is proven regardless.
        return Ok(None);
    }

    #[allow(clippy::cast_sign_loss)]
    let n = (recv_res as usize).min(body.len());
    let window = body.get(..n).unwrap_or(&[]);
    if window.len() >= 5 && window.get(..5) == Some(b"HTTP/".as_slice()) {
        let line_end = window
            .iter()
            .position(|&b| b == b'\r' || b == b'\n')
            .unwrap_or(window.len().min(64));
        let show = window.get(..line_end).unwrap_or(&[]);
        crate::serial_print!("[netstack-client]   client HTTP status = ");
        for &b in show {
            let c = if (0x20..0x7f).contains(&b) { b } else { b'.' };
            crate::serial_print!("{}", c as char);
        }
        crate::serial_println!("");
        Ok(Some(()))
    } else {
        Ok(None)
    }
}

/// Boot self-test: prove the **UDP `SOCK_DGRAM`** path end-to-end over the daemon
/// (`D-NETSOCK-SYNC`, UDP increment).
///
/// Binds a connectionless datagram socket on an ephemeral port
/// ([`udp_bind`](NetstackConn::udp_bind)), sends a real DNS `A`-record query for
/// `example.com` to `dns_ip:53` ([`udp_send_to`](NetstackConn::udp_send_to)), then
/// polls ([`udp_recv_from`](NetstackConn::udp_recv_from)) for the reply. A datagram
/// that arrives **from source port 53** with the matching transaction id and the
/// DNS response bit set proves the full round-trip: kernel client → ring
/// `OP_UDP_BIND`/`OP_UDP_SEND` → daemon TX on the raw NIC → wire → daemon RX pump →
/// `OP_UDP_RECV` with the in-band source-address header → kernel client.
///
/// Returns `Ok(Some(()))` when a valid DNS reply was received, `Ok(None)` when no
/// reply arrived (slirp/network variance — the bind/send/recv path still ran), and
/// `Err` only on a real control-protocol fault. `dns_ip == 0.0.0.0` (no configured
/// resolver) returns `Ok(None)`.
///
/// # Errors
///
/// Propagates control-protocol faults from the client. A `bind` or `send` failure
/// on a fresh session is surfaced as an error (it would mean the UDP ring path is
/// broken, not mere network variance).
pub fn self_test_udp_dns(dns_ip: &[u8; 4]) -> KernelResult<Option<()>> {
    if *dns_ip == [0, 0, 0, 0] {
        return Ok(None); // No resolver configured — nothing to query.
    }
    // A minimal DNS A-record query for "example.com" (txid 0x1234, RD set).
    const TXID: u16 = 0x1234;
    #[rustfmt::skip]
    const QUERY: [u8; 29] = [
        0x12, 0x34,             // transaction id
        0x01, 0x00,             // flags: RD (recursion desired)
        0x00, 0x01,             // qdcount = 1
        0x00, 0x00,             // ancount = 0
        0x00, 0x00,             // nscount = 0
        0x00, 0x00,             // arcount = 0
        7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', // label "example"
        3, b'c', b'o', b'm',    // label "com"
        0,                      // root label
        0x00, 0x01,             // qtype  = A
        0x00, 0x01,             // qclass = IN
    ];

    let mut conn = NetstackConn::open()?;

    // Bind an ephemeral local port for the datagram socket.
    let local_port = conn.udp_bind(0)?;
    crate::serial_println!(
        "[netstack-client]   UDP bound ephemeral port {} for DNS query",
        local_port
    );

    let sent = conn.udp_send_to(dns_ip, 53, &QUERY)?;
    if sent < QUERY.len() as i32 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   UDP DNS query short-sent ({} of {} bytes)",
            sent,
            QUERY.len()
        );
        return Err(KernelError::InternalError);
    }

    // Poll for the reply. Each non-blocking recv drives the daemon's RX pump once,
    // so a bounded loop is enough under slirp's fast local DNS.
    let mut resp = [0u8; 512];
    for _ in 0..64u32 {
        match conn.udp_recv_from(&mut resp, true) {
            Ok((n, src_ip, src_port)) => {
                let len = usize::try_from(n).unwrap_or(0).min(resp.len());
                // A DNS reply comes from port 53, echoes the txid, and has the
                // response (QR) bit set in the flags high byte.
                let dg = resp.get(..len).unwrap_or(&[]);
                let txid_ok = dg.get(..2) == Some(TXID.to_be_bytes().as_slice());
                let is_response = dg.get(2).is_some_and(|&f| f & 0x80 != 0);
                if src_port == 53 && txid_ok && is_response {
                    conn.close()?;
                    crate::serial_println!(
                        "[netstack-client]   UDP DNS reply: {} bytes from {:?}:53 \
                         (txid matched, QR set) — SOCK_DGRAM round-trip proven over the daemon",
                        len,
                        src_ip
                    );
                    return Ok(Some(()));
                }
                // A stray datagram (not our DNS reply) — keep polling.
            }
            Err(KernelError::WouldBlock) => {
                // Nothing queued yet — retry (the daemon pumped the NIC this round).
            }
            Err(e) => {
                conn.close()?;
                return Err(e);
            }
        }
    }

    conn.close()?;
    // No reply came back (network variance); the bind/send/recv path still ran.
    Ok(None)
}

/// Boot self-test: prove the **AF_INET6 UDP datagram** path (`OP_UDP_SEND6` +
/// v6-aware `OP_UDP_RECV`) end to end over the daemon's in-process software
/// loopback — the datagram sibling of [`self_test_connect6`].
///
/// Like the v6 TCP test, slirp offers no IPv6 peer under QEMU, so this uses the
/// daemon's loopback divert: a frame addressed to `me.ip6` (the daemon's
/// EUI-64 link-local, derived from the NIC MAC) is routed into the daemon's RX
/// FIFO instead of the wire. The test binds a UDP socket on a fixed port, sends a
/// datagram to `[me.ip6]:port`, then polls for it to come back. Because
/// [`NetstackConn::udp_send_to6`] uses the socket's own local port as the UDP source, the
/// looped-back datagram is delivered to the very socket that sent it. A received
/// datagram whose source header reports `AF_INET6`, the local link-local address,
/// and the sent payload proves: kernel client → `OP_UDP_SEND6` → daemon v6 TX →
/// loopback → daemon v6 RX classify (`parse_udp` IPv6 arm) → `OP_UDP_RECV` with
/// the `UDP_AF_INET6` in-band header → kernel client.
///
/// The EUI-64 link-local derivation is inlined (the kernel crate cannot depend on
/// `netproto`), matching [`self_test_connect6`] and `icmpv6::link_local_from_mac`.
///
/// Returns `Ok(Some(()))` on a proven v6 datagram round-trip, `Ok(None)` if there
/// is no NIC yet (no MAC → the daemon has no `me.ip6` to loop back to), and `Err`
/// on a real control-protocol fault or a parity break (bind/send failure, or the
/// looped-back datagram never arriving).
///
/// # Errors
///
/// Propagates control-protocol faults from the client; a bind/send failure or a
/// missing loopback datagram is reported as an error (the v6 UDP path is broken,
/// not mere network variance — the loopback is deterministic and in-process).
pub fn self_test_udp6_loopback() -> KernelResult<Option<()>> {
    /// Loopback datagram port (arbitrary, distinct from the other self-tests').
    const PORT: u16 = 9201;
    const PAYLOAD: &[u8] = b"slate-udp6:datagram-loopback";

    let mac = crate::net::interface::mac().0;
    if mac == [0u8; 6] {
        // No NIC → the daemon has no me.ip6 to loop back to.
        return Ok(None);
    }

    // Inline EUI-64 link-local (RFC 4291 App. A), matching the daemon's
    // `icmpv6::link_local_from_mac(mac)` used to seed `me.ip6`.
    let ll: [u8; 16] = [
        0xFE,
        0x80,
        0,
        0,
        0,
        0,
        0,
        0,
        mac[0] ^ 0x02,
        mac[1],
        mac[2],
        0xFF,
        0xFE,
        mac[3],
        mac[4],
        mac[5],
    ];

    let mut conn = NetstackConn::open()?;

    // Bind the datagram socket to the fixed loopback port so the looped-back
    // datagram (dst_port == PORT) routes back to this same socket.
    let bound = conn.udp_bind(PORT)?;
    if bound != PORT {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   v6 UDP bind(port={}) returned {} — IPv6 datagram parity broken",
            PORT,
            bound
        );
        return Err(KernelError::InternalError);
    }

    // Send the datagram to our own link-local (loops back in-process).
    let sent = conn.udp_send_to6(&ll, PORT, PAYLOAD)?;
    if sent < PAYLOAD.len() as i32 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   v6 UDP short-sent ({} of {} bytes) — IPv6 datagram parity broken",
            sent,
            PAYLOAD.len()
        );
        return Err(KernelError::InternalError);
    }

    // Poll for the looped-back datagram. Each non-blocking recv drives the daemon's
    // UDP pump once, so a bounded loop suffices for the deterministic loopback.
    let mut resp = [0u8; 128];
    for _ in 0..32u32 {
        match conn.udp_recv_any(&mut resp, true) {
            Ok((n, family, src_ip16, src_port)) => {
                let len = usize::try_from(n).unwrap_or(0).min(resp.len());
                let dg = resp.get(..len).unwrap_or(&[]);
                if family == netipc::ring::UDP_AF_INET6
                    && src_ip16 == ll
                    && src_port == PORT
                    && dg == PAYLOAD
                {
                    conn.close()?;
                    crate::serial_println!(
                        "[netstack-client]   v6 UDP datagram looped back: {} bytes from \
                         fe80::…:{} (AF_INET6, payload matched) — AF_INET6 SOCK_DGRAM proven",
                        len,
                        src_port
                    );
                    return Ok(Some(()));
                }
                if family != netipc::ring::UDP_AF_INET6 && len > 0 {
                    conn.close()?;
                    crate::serial_println!(
                        "[netstack-client]   v6 UDP recv reported family {} (want AF_INET6) — v6 demux broken",
                        family
                    );
                    return Err(KernelError::InternalError);
                }
                // A stray/partial datagram — keep polling.
            }
            Err(KernelError::WouldBlock) => {
                // Nothing queued yet — retry (the daemon pumped the NIC this round).
            }
            Err(e) => {
                conn.close()?;
                return Err(e);
            }
        }
    }

    conn.close()?;
    crate::serial_println!(
        "[netstack-client]   v6 UDP loopback datagram never arrived — IPv6 datagram parity broken"
    );
    Err(KernelError::InternalError)
}

/// Boot self-test: prove UDP `connect(2)` **default-peer** semantics on a
/// daemon-backed `SOCK_DGRAM` socket, end-to-end through the
/// [`crate::net::socket`] object layer the Linux syscalls use.
///
/// Two phases over the in-process IPv6 loopback (a frame to our own EUI-64
/// link-local is diverted into the daemon RX FIFO, bypassing NDP):
///
/// 1. **Connected send + filter-pass.** Bind a datagram socket to a fixed port,
///    `dgram_connect` it to `[me.ip6]:port` (itself), then a *connected* send
///    ([`dgram_send_connected`](crate::net::socket::dgram_send_connected) — no
///    explicit destination) must loop back and be delivered, because the source
///    (`[me.ip6]:port`) equals the connected peer. `getpeername`
///    ([`dgram_peer`](crate::net::socket::dgram_peer)) must report that peer.
/// 2. **Filter-drop.** Re-`connect` the same socket to a *different* peer port,
///    then inject a datagram from our own port via an explicit
///    [`dgram_send_to6`](crate::net::socket::dgram_send_to6) to ourselves. The
///    daemon delivers it to our bound port, but a *connected* receive must **drop**
///    it (its source port ≠ the connected peer port) — Linux filters a connected
///    UDP socket to its peer. A receive that returns any datagram means the filter
///    leaked.
///
/// Returns `Ok(Some(()))` on both phases proven, `Ok(None)` if there is no NIC yet
/// (no MAC → the daemon has no `me.ip6`), and `Err` on a control-protocol fault or
/// a parity break.
///
/// # Errors
///
/// Propagates control-protocol faults; a bind/connect/send failure, a missing
/// loopback datagram (phase 1), or a leaked filter (phase 2) is an error.
pub fn self_test_udp_connect() -> KernelResult<Option<()>> {
    use crate::net::socket::{self, DgramPeer};
    /// Bound/connected port for phase 1 (self-connect).
    const PORT: u16 = 9310;
    /// A different peer port for phase 2 (so an injected datagram from `PORT` is
    /// filtered out).
    const OTHER: u16 = 9312;
    const PAYLOAD: &[u8] = b"slate-udp:connect-default-peer";

    let mac = crate::net::interface::mac().0;
    if mac == [0u8; 6] {
        // No NIC → the daemon has no me.ip6 to loop back to.
        return Ok(None);
    }
    // Inline EUI-64 link-local (RFC 4291 App. A), matching the daemon's
    // `icmpv6::link_local_from_mac(mac)` used to seed `me.ip6`.
    let ll: [u8; 16] = [
        0xFE,
        0x80,
        0,
        0,
        0,
        0,
        0,
        0,
        mac[0] ^ 0x02,
        mac[1],
        mac[2],
        0xFF,
        0xFE,
        mac[3],
        mac[4],
        mac[5],
    ];

    let h = socket::create_dgram(10)?;
    let bound = match socket::dgram_bind(h, PORT) {
        Ok(b) => b,
        Err(e) => {
            socket::close(h);
            return Err(e);
        }
    };
    if bound != PORT {
        socket::close(h);
        crate::serial_println!(
            "[netstack-client]   UDP connect: bind(port={PORT}) returned {bound} — parity broken"
        );
        return Err(KernelError::InternalError);
    }

    // Phase 1 — connect to ourselves, connected send must loop back and pass the
    // peer filter.
    if let Err(e) = socket::dgram_connect(h, DgramPeer::V6(ll, PORT)) {
        socket::close(h);
        return Err(e);
    }
    match socket::dgram_peer(h) {
        Ok(Some(DgramPeer::V6(p, pp))) if p == ll && pp == PORT => {}
        other => {
            socket::close(h);
            crate::serial_println!(
                "[netstack-client]   UDP connect: getpeername mismatch ({other:?}) — parity broken"
            );
            return Err(KernelError::InternalError);
        }
    }
    let sent = match socket::dgram_send_connected(h, PAYLOAD) {
        Ok(n) => n,
        Err(e) => {
            socket::close(h);
            return Err(e);
        }
    };
    if usize::try_from(sent).unwrap_or(0) < PAYLOAD.len() {
        socket::close(h);
        crate::serial_println!(
            "[netstack-client]   UDP connect: short connected send ({sent} bytes) — parity broken"
        );
        return Err(KernelError::InternalError);
    }
    let mut resp = [0u8; 128];
    let mut got = false;
    for _ in 0..32u32 {
        match socket::dgram_recv_from(h, &mut resp, true) {
            Ok((n, family, ip16, port)) => {
                let len = usize::try_from(n).unwrap_or(0).min(resp.len());
                let dg = resp.get(..len).unwrap_or(&[]);
                if family == netipc::ring::UDP_AF_INET6
                    && ip16 == ll
                    && port == PORT
                    && dg == PAYLOAD
                {
                    got = true;
                    break;
                }
            }
            Err(KernelError::WouldBlock) => {}
            Err(e) => {
                socket::close(h);
                return Err(e);
            }
        }
    }
    if !got {
        socket::close(h);
        crate::serial_println!(
            "[netstack-client]   UDP connect: connected send never looped back — parity broken"
        );
        return Err(KernelError::InternalError);
    }

    // Phase 2 — re-connect to a different peer, then inject a datagram from our own
    // port (source == PORT, peer == OTHER). The connected recv must drop it.
    if let Err(e) = socket::dgram_connect(h, DgramPeer::V6(ll, OTHER)) {
        socket::close(h);
        return Err(e);
    }
    if let Err(e) = socket::dgram_send_to6(h, &ll, PORT, b"stray-should-be-filtered") {
        socket::close(h);
        return Err(e);
    }
    let mut leaked = false;
    for _ in 0..16u32 {
        match socket::dgram_recv_from(h, &mut resp, true) {
            // Any delivered datagram means the peer filter leaked (the injected one
            // came from PORT, not the connected peer OTHER).
            Ok(_) => {
                leaked = true;
                break;
            }
            Err(KernelError::WouldBlock) => {}
            Err(e) => {
                socket::close(h);
                return Err(e);
            }
        }
    }
    socket::close(h);
    if leaked {
        crate::serial_println!(
            "[netstack-client]   UDP connect: connected recv delivered a non-peer datagram — filter broken"
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!(
        "[netstack-client]   UDP connect() default-peer proven: connected send looped back \
         (filter-pass), getpeername ok, non-peer datagram dropped (filter-drop) — \
         SOCK_DGRAM connect parity"
    );
    Ok(Some(()))
}

/// Boot self-test: UDP **multicast** end to end through the
/// [`crate::net::socket`] layer the Linux `setsockopt`/`getsockopt` use, and
/// the daemon's group membership, delivery and send options behind it.
///
/// Every send uses TTL / hop limit 0, so nothing leaves the machine: what
/// comes back is the daemon's loop to its own members (`IP_MULTICAST_LOOP`,
/// on by default), which makes the test deterministic. In order, for IPv4:
///
/// 1. An unbound socket reads its TTL as the default (1) without the daemon
///    creating anything; setting it to 0 opens the daemon's socket
///    *unbound*, and reads back 0.
/// 2. Joining the group before `bind(2)` succeeds (as Linux allows); joining
///    again is `EADDRINUSE`; joining on an interface address that is not the
///    host's is `ENODEV`; a unicast "group" is `EINVAL`.
/// 3. `bind` gives the open socket its port, keeping the membership: a send
///    to the group comes back to it, from its own port.
/// 4. A second socket, opened by a join, refused a `bind` to the same port
///    (`EADDRINUSE`), still has its daemon socket -- the `udp_bind`
///    bookkeeping must not forget it -- and closing it leaves the first
///    socket's membership intact.
/// 5. With the loop off, the send does not come back; after leaving the
///    group, nor does it with the loop on, and leaving again is
///    `EADDRNOTAVAIL`.
///
/// Then for IPv6: join before bind, hop limit 0 read back, bind, and the
/// send to the group comes back as an `AF_INET6` datagram.
///
/// Returns `Ok(Some(()))` when every step held, `Ok(None)` with no NIC (the
/// daemon then has no interface to be a member on), `Err` on any break.
///
/// # Errors
///
/// A step that answered wrongly is [`KernelError::InternalError`], after a
/// serial line naming it; a control-protocol fault propagates as itself.
pub fn self_test_udp_multicast() -> KernelResult<Option<()>> {
    use crate::net::socket;
    if crate::net::interface::mac().0 == [0u8; 6] {
        return Ok(None);
    }
    let h = socket::create_dgram(2)?;
    let v4 = multicast_v4_steps(h);
    socket::close(h);
    v4?;
    let h6 = socket::create_dgram(10)?;
    let v6 = multicast_v6_steps(h6);
    socket::close(h6);
    v6?;
    crate::serial_println!(
        "[netstack-client]   UDP multicast proven: options before bind, join refusals \
         (EADDRINUSE/ENODEV/EINVAL), group send looped back on v4 and v6, loop-off and \
         leave stop it, a refused bind keeps its open socket"
    );
    Ok(Some(()))
}

/// Ports and groups of [`self_test_udp_multicast`], apart from every other
/// self-test's.
const MCAST_PORT: u16 = 9330;
const MCAST_PORT6: u16 = 9332;
const MCAST_GROUP4: [u8; 4] = [239, 255, 77, 1];
const MCAST_GROUP6: [u8; 16] = [0xFF, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x51, 0x57];
const MCAST_PAYLOAD: &[u8] = b"slate-udp:multicast-loop";

/// An IPv4 join (or leave) of `group` on the interface with address `iface`.
fn mcast_group4(group: [u8; 4], iface: [u8; 4], join: bool) -> netipc::sockopt::Set {
    let mut window = [0u8; 16];
    window[..4].copy_from_slice(&group);
    window[4..8].copy_from_slice(&iface);
    netipc::sockopt::Set::Group {
        option: if join {
            netipc::ring::UDP_OPT_MCAST_JOIN4
        } else {
            netipc::ring::UDP_OPT_MCAST_LEAVE4
        },
        window,
        len: netipc::ring::UDP_MREQ4_LEN,
    }
}

/// A scalar multicast option.
fn mcast_scalar(option: u16, value: u32) -> netipc::sockopt::Set {
    netipc::sockopt::Set::Scalar { option, value }
}

/// Set `set` on `h` and check the daemon's answer is `want` (`Err` holding
/// the positive errno).
fn mcast_expect_set(
    h: crate::net::socket::SocketHandle,
    what: &str,
    set: netipc::sockopt::Set,
    want: Result<(), i32>,
) -> KernelResult<()> {
    match crate::net::socket::dgram_setopt(h, set) {
        Ok(got) if got == want => Ok(()),
        Ok(got) => {
            crate::serial_println!(
                "[netstack-client]   multicast: {} answered {:?}, want {:?}",
                what,
                got,
                want
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            crate::serial_println!("[netstack-client]   multicast: {} failed: {:?}", what, e);
            Err(e)
        }
    }
}

/// Read option `option` of `h` and check it is `want`.
fn mcast_expect_get(
    h: crate::net::socket::SocketHandle,
    what: &str,
    option: u16,
    want: i32,
) -> KernelResult<()> {
    let got = crate::net::socket::dgram_getopt(h, option)?;
    if got == want {
        return Ok(());
    }
    crate::serial_println!("[netstack-client]   multicast: {} read {}, want {}", what, got, want);
    Err(KernelError::InternalError)
}

/// Whether the datagram [`MCAST_PAYLOAD`] from port `port` of family `family`
/// is (`true`) or is not (`false`) waiting on `h`, polling a few times: the
/// daemon queues a looped-back datagram while it serves the send, so the
/// first poll finds it, and the rest only guard against a slow pump.
fn mcast_arrives(h: crate::net::socket::SocketHandle, family: u16, port: u16) -> KernelResult<bool> {
    let mut buf = [0u8; 64];
    for _ in 0..8u32 {
        match crate::net::socket::dgram_recv_from(h, &mut buf, true) {
            Ok((n, fam, _src, src_port)) => {
                let len = usize::try_from(n).unwrap_or(0).min(buf.len());
                if fam == family && src_port == port && buf.get(..len) == Some(MCAST_PAYLOAD) {
                    return Ok(true);
                }
                crate::serial_println!(
                    "[netstack-client]   multicast: a stray datagram ({} bytes, family {}, \
                     port {}) -- ignored",
                    len,
                    fam,
                    src_port
                );
            }
            Err(KernelError::WouldBlock) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}

/// Fail with `what` unless `got == want`.
fn mcast_check(what: &str, got: bool, want: bool) -> KernelResult<()> {
    if got == want {
        return Ok(());
    }
    crate::serial_println!(
        "[netstack-client]   multicast: {} -- the datagram {} arrive",
        what,
        if want { "did not" } else { "did" }
    );
    Err(KernelError::InternalError)
}

/// The IPv4 steps of [`self_test_udp_multicast`] on the fresh socket `h`.
fn multicast_v4_steps(h: crate::net::socket::SocketHandle) -> KernelResult<()> {
    use crate::net::socket;
    use netipc::ring as r;
    let af = netipc::ring::UDP_AF_INET;
    // 1. Options on an unbound socket.
    mcast_expect_get(h, "default TTL", r::UDP_OPT_MCAST_TTL, 1)?;
    mcast_expect_set(h, "TTL 0", mcast_scalar(r::UDP_OPT_MCAST_TTL, 0), Ok(()))?;
    mcast_expect_get(h, "TTL set unbound", r::UDP_OPT_MCAST_TTL, 0)?;
    // 2. Joins before bind, and the daemon's refusals.
    let join = mcast_group4(MCAST_GROUP4, [0; 4], true);
    mcast_expect_set(h, "join", join, Ok(()))?;
    mcast_expect_set(h, "join twice", join, Err(r::ERR_ADDR_IN_USE.saturating_neg()))?;
    mcast_expect_set(
        h,
        "join on a foreign address",
        mcast_group4([239, 255, 77, 2], [192, 0, 2, 1], true),
        Err(r::ERR_NO_DEVICE.saturating_neg()),
    )?;
    mcast_expect_set(
        h,
        "join a unicast address",
        mcast_group4([10, 0, 0, 1], [0; 4], true),
        Err(r::ERR_INVALID.saturating_neg()),
    )?;
    // 3. Bind keeps the membership; the send loops back.
    let port = socket::dgram_bind(h, MCAST_PORT)?;
    if port != MCAST_PORT {
        crate::serial_println!("[netstack-client]   multicast: bind gave port {}", port);
        return Err(KernelError::InternalError);
    }
    socket::dgram_send_to(h, &MCAST_GROUP4, MCAST_PORT, MCAST_PAYLOAD)?;
    mcast_check("send to the group", mcast_arrives(h, af, MCAST_PORT)?, true)?;
    // 4. A refused bind keeps its open socket.
    let h2 = socket::create_dgram(2)?;
    let second = multicast_refused_bind(h2);
    socket::close(h2);
    second?;
    socket::dgram_send_to(h, &MCAST_GROUP4, MCAST_PORT, MCAST_PAYLOAD)?;
    mcast_check("send after the second socket closed", mcast_arrives(h, af, MCAST_PORT)?, true)?;
    // 5. Loop off, then leave.
    mcast_expect_set(h, "loop off", mcast_scalar(r::UDP_OPT_MCAST_LOOP4, 0), Ok(()))?;
    mcast_expect_get(h, "loop read back", r::UDP_OPT_MCAST_LOOP4, 0)?;
    socket::dgram_send_to(h, &MCAST_GROUP4, MCAST_PORT, MCAST_PAYLOAD)?;
    mcast_check("send with the loop off", mcast_arrives(h, af, MCAST_PORT)?, false)?;
    mcast_expect_set(h, "loop on", mcast_scalar(r::UDP_OPT_MCAST_LOOP4, 1), Ok(()))?;
    let leave = mcast_group4(MCAST_GROUP4, [0; 4], false);
    mcast_expect_set(h, "leave", leave, Ok(()))?;
    mcast_expect_set(h, "leave twice", leave, Err(r::ERR_ADDR_NOT_AVAIL.saturating_neg()))?;
    socket::dgram_send_to(h, &MCAST_GROUP4, MCAST_PORT, MCAST_PAYLOAD)?;
    mcast_check("send after leaving", mcast_arrives(h, af, MCAST_PORT)?, false)
}

/// Step 4 of [`self_test_udp_multicast`]: `h2` joins the group (opening its
/// daemon socket unbound), is refused [`MCAST_PORT`], and must still have
/// that socket -- reading an option from it answers from the daemon.
fn multicast_refused_bind(h2: crate::net::socket::SocketHandle) -> KernelResult<()> {
    use crate::net::socket;
    let join = mcast_group4(MCAST_GROUP4, [0; 4], true);
    mcast_expect_set(h2, "second socket's join", join, Ok(()))?;
    match socket::dgram_bind(h2, MCAST_PORT) {
        Err(KernelError::AddrInUse) => {}
        other => {
            crate::serial_println!(
                "[netstack-client]   multicast: second bind to a taken port answered {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }
    // The daemon still holds the socket, membership and all: a second join
    // is refused as a duplicate, which only a live daemon socket can say.
    mcast_expect_set(
        h2,
        "second socket's join after the refused bind",
        join,
        Err(netipc::ring::ERR_ADDR_IN_USE.saturating_neg()),
    )?;
    mcast_expect_get(h2, "second socket's TTL", netipc::ring::UDP_OPT_MCAST_TTL, 1)
}

/// The IPv6 steps of [`self_test_udp_multicast`] on the fresh `AF_INET6`
/// socket `h`.
fn multicast_v6_steps(h: crate::net::socket::SocketHandle) -> KernelResult<()> {
    use crate::net::socket;
    use netipc::ring as r;
    let join = netipc::sockopt::Set::Group {
        option: r::UDP_OPT_MCAST_JOIN6,
        window: MCAST_GROUP6,
        len: r::UDP_MREQ6_LEN,
    };
    mcast_expect_set(h, "v6 join", join, Ok(()))?;
    mcast_expect_set(h, "hop limit 0", mcast_scalar(r::UDP_OPT_MCAST_HOPS, 0), Ok(()))?;
    mcast_expect_get(h, "hop limit read back", r::UDP_OPT_MCAST_HOPS, 0)?;
    mcast_expect_get(h, "default v6 loop", r::UDP_OPT_MCAST_LOOP6, 1)?;
    let port = socket::dgram_bind(h, MCAST_PORT6)?;
    if port != MCAST_PORT6 {
        crate::serial_println!("[netstack-client]   multicast: v6 bind gave port {}", port);
        return Err(KernelError::InternalError);
    }
    socket::dgram_send_to6(h, &MCAST_GROUP6, MCAST_PORT6, MCAST_PAYLOAD)?;
    mcast_check(
        "v6 send to the group",
        mcast_arrives(h, netipc::ring::UDP_AF_INET6, MCAST_PORT6)?,
        true,
    )?;
    let leave = netipc::sockopt::Set::Group {
        option: r::UDP_OPT_MCAST_LEAVE6,
        window: MCAST_GROUP6,
        len: r::UDP_MREQ6_LEN,
    };
    mcast_expect_set(h, "v6 leave", leave, Ok(()))
}

/// Boot self-test: prove that a **non-blocking** receive on a freshly-connected
/// daemon socket returns "would block" rather than stalling for the full receive
/// deadline — the `O_NONBLOCK` parity property (`D-NETSOCK-SYNC`).
///
/// The sequence: `connect` to `ip:port`, then *before sending any request*, issue
/// a non-blocking `recv`. A well-behaved server sends nothing unsolicited, so no
/// data is buffered and the stream is open — the daemon must answer
/// [`netipc::ring::ERR_WOULD_BLOCK`], which the client surfaces as
/// [`KernelError::WouldBlock`]. (If the peer *did* immediately deliver data or a
/// FIN, a non-negative result is also acceptable — the point is only that the
/// call returned promptly with a decisive answer instead of blocking.)
///
/// Returns `Ok(Some(()))` if the non-blocking semantics were exercised (either a
/// `WouldBlock` or an immediate data/EOF result), `Ok(None)` if there was no
/// upstream to connect to (network variance — nothing to assert), and `Err` on a
/// real control-protocol fault.
///
/// # Errors
///
/// Propagates control-protocol faults from the client.
pub fn self_test_nonblock_recv(ip: &[u8; 4], port: u16) -> KernelResult<Option<()>> {
    let mut conn = NetstackConn::open()?;

    let connect_res = conn.connect(ip, port, false)?;
    if connect_res < 0 {
        // No upstream — nothing to assert; the client path still ran.
        conn.close()?;
        return Ok(None);
    }

    let mut body = [0u8; RCV_CAP as usize];
    let outcome = conn.recv(&mut body, true, false);
    conn.close()?;

    match outcome {
        Err(KernelError::WouldBlock) => {
            crate::serial_println!(
                "[netstack-client]   non-blocking recv on idle socket returned WouldBlock (EAGAIN) \
                 as expected"
            );
            Ok(Some(()))
        }
        Ok(n) => {
            // Peer delivered something immediately (data or EOF). Still a prompt,
            // decisive non-blocking answer — the property under test.
            crate::serial_println!(
                "[netstack-client]   non-blocking recv returned promptly with {} byte(s) \
                 (peer had data/EOF ready)",
                n
            );
            Ok(Some(()))
        }
        Err(e) => Err(e),
    }
}

/// Boot self-test: prove that the poll/epoll readiness probe
/// ([`NetstackConn::poll_ready`], daemon `OP_POLL`) reports an **honest** state —
/// an idle connected socket is writable but not readable, and it *becomes*
/// readable only once the peer's response actually arrives. This is the parity
/// property behind honest `POLLIN`/`POLLOUT` for daemon-backed sockets
/// (`D-NETSOCK-SYNC`): the former placeholder always reported readable, which
/// would spin a poller that then read `EAGAIN`.
///
/// Sequence: `connect`; poll (expect writable, and — for a well-behaved server —
/// not-yet-readable); send a `HEAD` request; poll in a bounded loop until the
/// socket reports readable. The receive buffer is never consumed by the probe, so
/// a later `recv` would still return the response.
///
/// Returns `Ok(Some(()))` if the readiness path was exercised (the socket was
/// writable and either started readable or transitioned to readable once data
/// arrived), `Ok(None)` if there was no upstream / no response came back (network
/// variance — the probe path still ran honestly), and `Err` on a real
/// control-protocol fault or a connected socket that reports not-writable.
///
/// # Errors
///
/// Propagates control-protocol faults from the client; reports a connected socket
/// that is not writable as an error (that would break `POLLOUT` parity).
pub fn self_test_poll_ready(ip: &[u8; 4], port: u16) -> KernelResult<Option<()>> {
    const HTTP_REQ: &[u8] = b"HEAD / HTTP/1.0\r\nHost: example.com\r\nConnection: close\r\n\r\n";

    let mut conn = NetstackConn::open()?;

    let connect_res = conn.connect(ip, port, false)?;
    if connect_res < 0 {
        conn.close()?;
        return Ok(None);
    }

    // Idle connected socket: must be writable; a well-behaved server has sent
    // nothing yet, so it should not (yet) be readable.
    let (readable0, writable0, _err0) = conn.poll_ready()?;
    if !writable0 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   poll: connected socket reported NOT writable — POLLOUT parity broken"
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[netstack-client]   poll on idle socket: readable={} writable={}",
        readable0,
        writable0
    );

    // Solicit a response, then poll until the socket honestly reports readable.
    let send_res = conn.send(HTTP_REQ, false)?;
    if send_res < 0 {
        conn.close()?;
        return Ok(None);
    }

    let mut became_readable = readable0;
    for _ in 0..64u32 {
        let (readable, _writable, _err) = conn.poll_ready()?;
        if readable {
            became_readable = true;
            break;
        }
    }
    conn.close()?;

    if became_readable {
        crate::serial_println!(
            "[netstack-client]   poll reported POLLIN once the HTTP response arrived (honest readiness)"
        );
        Ok(Some(()))
    } else {
        // No response came back (slirp variance); the poll path still ran honestly.
        Ok(None)
    }
}

/// Boot self-test: prove the **non-blocking connect** path (`connect` with
/// `O_NONBLOCK` → `EINPROGRESS` → `poll(POLLOUT)` → `getsockopt(SO_ERROR)`),
/// mirroring Linux (`D-NETSOCK-SYNC`).
///
/// Sequence: open a client, issue a non-blocking [`connect`](NetstackConn::connect)
/// (which returns `0` if the handshake already completed within the daemon's single
/// post-SYN pump, or [`netipc::ring::ERR_IN_PROGRESS`] if it is still pending), then
/// drive [`poll_ready`](NetstackConn::poll_ready) -- [`NONBLOCK_CONNECT_BURST`]
/// times back to back, then every 10 ms up to [`NONBLOCK_CONNECT_DEADLINE_MS`] --
/// until the socket reports **writable** (the connect resolved), checking that it
/// never reports the error bit for a good endpoint. A writable, error-free result
/// is the parity property: a `poll(POLLOUT)` waiter is woken exactly when the
/// connect completes, however often it asked before then.
///
/// Returns `Ok(Some(()))` if the non-blocking-connect readiness path was exercised
/// (connect started and the socket became writable without error), `Ok(None)` if
/// there was no upstream (the connect could not start or never resolved — network
/// variance, nothing to assert), and `Err` on a real control-protocol fault or a
/// socket that reported the error bit against a known-good endpoint.
///
/// # Errors
///
/// Propagates control-protocol faults from the client; reports an unexpected
/// `POLL_ERR` on a good endpoint as an error (that would break connect parity).
pub fn self_test_nonblock_connect(ip: &[u8; 4], port: u16) -> KernelResult<Option<()>> {
    let mut conn = NetstackConn::open()?;

    let connect_res = conn.connect(ip, port, true)?;
    if connect_res < 0 && connect_res != netipc::ring::ERR_IN_PROGRESS {
        // Could not even start the connect (no route / no upstream). Nothing to
        // assert; the non-blocking-connect path still ran end to end.
        conn.close()?;
        return Ok(None);
    }

    if connect_res == 0 {
        crate::serial_println!(
            "[netstack-client]   non-blocking connect completed synchronously (fast peer)"
        );
    } else {
        crate::serial_println!(
            "[netstack-client]   non-blocking connect returned EINPROGRESS; polling for POLLOUT"
        );
    }

    // Poll for writable (POLLOUT), as a userspace non-blocking connect would:
    // first a burst of back-to-back polls, then on a clock.
    //
    // The burst is the regression test for known-issues
    // `A-NONBLOCK-CONNECT-GAVE-UP-AFTER-FIVE-POLLS`. The daemon used to resend
    // the SYN on every poll and refuse the connect after five, so five polls
    // inside the server's round trip refused a connect to a live server -- this
    // test's own endpoint, which the blocking tests just before it reached. A
    // pending connect must survive any number of polls; only time may end it.
    let start = crate::hrtimer::now_ns();
    let mut polls = 0u32;
    let mut writable = false;
    let mut waited_ms;
    loop {
        let (_readable, w, error) = conn.poll_ready()?;
        polls = polls.saturating_add(1);
        waited_ms = crate::hrtimer::now_ns().saturating_sub(start) / 1_000_000;
        if error {
            conn.close()?;
            crate::serial_println!(
                "[netstack-client]   non-blocking connect reported POLL_ERR against a good \
                 endpoint after {} poll(s) and {} ms",
                polls,
                waited_ms
            );
            return Err(KernelError::InternalError);
        }
        if w {
            writable = true;
            break;
        }
        if waited_ms >= NONBLOCK_CONNECT_DEADLINE_MS {
            break;
        }
        if polls >= NONBLOCK_CONNECT_BURST {
            crate::sched::sleep_ms(10);
        }
    }
    conn.close()?;

    if writable {
        crate::serial_println!(
            "[netstack-client]   non-blocking connect resolved to writable (POLLOUT) after {} \
             poll(s), the first {} back to back, in {} ms — connect parity ok",
            polls,
            polls.min(NONBLOCK_CONNECT_BURST),
            waited_ms
        );
        Ok(Some(()))
    } else {
        // Neither connected nor refused inside the deadline (slirp variance);
        // the path still ran.
        crate::serial_println!(
            "[netstack-client]   non-blocking connect: no answer from the server in {} ms \
             ({} polls) -- not refused, not connected",
            waited_ms,
            polls
        );
        Ok(None)
    }
}

/// Back-to-back polls [`self_test_nonblock_connect`] makes before it starts
/// pausing between them: well past the five that used to refuse a pending
/// connect.
const NONBLOCK_CONNECT_BURST: u32 = 16;

/// How long [`self_test_nonblock_connect`] waits for the handshake, in ms: past
/// the daemon's 12.6 s give-up (`netproto::tcp_rtx`), so a server that never
/// answers is seen as refused rather than as a test that stopped looking.
const NONBLOCK_CONNECT_DEADLINE_MS: u64 = 15_000;

/// Boot self-test: prove the **non-blocking send** path (`send`/`write` with
/// `O_NONBLOCK`), mirroring Linux (`D-NETSOCK-SYNC`).
///
/// On a socket whose send window has room, a non-blocking send must accept the
/// bytes and return the count — exactly like a blocking send — rather than
/// spuriously reporting `EAGAIN`. Only a *full* window (a prior segment still
/// unacknowledged) yields [`KernelError::WouldBlock`]. This test connects, checks
/// the socket is writable, then issues a non-blocking [`send`](NetstackConn::send)
/// of a request and asserts the daemon accepted it (the window was empty, so the
/// `SEND_NONBLOCK` flag must not have blocked it).
///
/// Returns `Ok(Some(()))` if the non-blocking send accepted the request, `Ok(None)`
/// if there was no upstream (connect could not complete — network variance), and
/// `Err` on a real control-protocol fault or an unexpected `WouldBlock` on an
/// empty window (that would break `O_NONBLOCK` send parity).
///
/// # Errors
///
/// Propagates control-protocol faults from the client; reports a `WouldBlock` on a
/// known-writable (empty-window) socket as an error.
pub fn self_test_nonblock_send(ip: &[u8; 4], port: u16) -> KernelResult<Option<()>> {
    const HTTP_REQ: &[u8] = b"HEAD / HTTP/1.0\r\nHost: example.com\r\nConnection: close\r\n\r\n";

    let mut conn = NetstackConn::open()?;

    let connect_res = conn.connect(ip, port, false)?;
    if connect_res < 0 {
        conn.close()?;
        return Ok(None); // no upstream — nothing to assert
    }

    // Fresh connection: the send window is empty, so a non-blocking send must
    // succeed (accept the bytes), not return EAGAIN.
    let send_res = match conn.send(HTTP_REQ, true) {
        Ok(n) => n,
        Err(KernelError::WouldBlock) => {
            conn.close()?;
            crate::serial_println!(
                "[netstack-client]   non-blocking send on an empty window returned EAGAIN — send parity broken"
            );
            return Err(KernelError::InternalError);
        }
        Err(e) => {
            conn.close()?;
            return Err(e);
        }
    };
    conn.close()?;

    if send_res > 0 {
        crate::serial_println!(
            "[netstack-client]   non-blocking send accepted {} bytes on a writable socket (no spurious EAGAIN) — send parity ok",
            send_res
        );
        Ok(Some(()))
    } else {
        // Peer gone between connect and send (slirp variance); path still ran.
        Ok(None)
    }
}

/// Boot self-test: prove the **server-socket** path (`listen`/`accept`) over the
/// daemon, closing the last `D-NETSOCK-SYNC` parity gap before the `net.userspace`
/// default can be flipped (increment 5.7).
///
/// Unlike the client self-tests above, this needs *both* ends of a TCP connection
/// — a listener and a connecting peer — but there is no external server to talk
/// to under slirp (which drops host-to-self packets). It therefore drives the
/// daemon's **in-process software loopback**: a connection opened to the daemon's
/// own `me.ip` is diverted into an internal RX FIFO and delivered to a listener in
/// the *same* daemon session. Because a blocking connect cannot pump the listener
/// (its tight RX loop only reads its own 4-tuple), the connect here is
/// **non-blocking** — a single `OP_CONNECT` pump drives the entire 3-way handshake
/// for *both* ends, leaving the client established and the passive server
/// connection established and queued in the listener's backlog. `OP_ACCEPT` then
/// dequeues it, and a bidirectional data exchange proves the accepted connection
/// is a real, addressable socket within the one ring session.
///
/// The whole exchange happens over one [`NetstackConn`] ring: the listener id and
/// the accepted-connection id are allocated ids distinct from the client's
/// own ([`alloc_conn_id`]), all demuxed by the daemon by 4-tuple.
///
/// Returns `Ok(Some(()))` if the listen→connect→accept→data round-trip completed,
/// `Ok(None)` if the interface has no IPv4 address yet (no DHCP lease — loopback
/// needs a non-zero `me.ip`, nothing to assert), and `Err` on a real
/// control-protocol fault or a parity break (listen/accept/connect failing over
/// loopback, or the echoed data mismatching).
///
/// # Errors
///
/// Propagates control-protocol faults from the client; reports a failed
/// listen/connect/accept over loopback, or a data mismatch, as an error.
pub fn self_test_listen_accept() -> KernelResult<Option<()>> {
    /// Poll listener `id` and judge the answer against what a listener may say:
    /// readable when a connection waits, otherwise nothing -- never writable,
    /// never in error, and never "no such id". `Some(true)`: the answer is
    /// `want_readable`'s. `Some(false)`: merely quiet while readable was wanted
    /// (poll again). `None`: wrong outright, and already reported.
    fn listener_polls(
        conn: &mut NetstackConn,
        id: u32,
        want_readable: bool,
        when: &str,
    ) -> KernelResult<Option<bool>> {
        let (readable, writable, error) = match conn.poll_on(id) {
            Ok(bits) => bits,
            Err(KernelError::NotConnected) => {
                crate::serial_println!(
                    "[netstack-client]   OP_POLL does not know listener {} ({}) — poll on a listening socket can never report a connection",
                    id,
                    when
                );
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        if writable || error || (readable && !want_readable) {
            crate::serial_println!(
                "[netstack-client]   listener {} polled readable={} writable={} error={} {} — want {}",
                id,
                readable,
                writable,
                error,
                when,
                if want_readable {
                    "readable only"
                } else {
                    "no events"
                }
            );
            return Ok(None);
        }
        Ok(Some(readable == want_readable))
    }

    /// Loopback listen port (arbitrary, unused elsewhere).
    const PORT: u16 = 9099;
    const CLIENT_MSG: &[u8] = b"slate-listen-accept:ping";
    const SERVER_MSG: &[u8] = b"slate-listen-accept:pong";

    let me_ip = crate::net::interface::ip().0;
    if me_ip == [0, 0, 0, 0] {
        // No lease yet — the loopback divert keys on a non-zero me.ip.
        return Ok(None);
    }

    let mut conn = NetstackConn::open()?;
    // The listener's and the accepted connection's ids come from the one
    // namespace every socket shares on the ring, like the client's own.
    let listener_id = alloc_conn_id();
    let accepted_id = alloc_conn_id();
    let client_id = conn.conn_id();

    // 1. Register the passive listener BEFORE connecting, so the SYN routed over
    //    the loopback FIFO finds a listening port.
    let listen_res = conn.listen(listener_id, PORT)?;
    if listen_res != 0 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   listen(port={}) failed on a fresh session (result {}) — server-socket parity broken",
            PORT,
            listen_res
        );
        return Err(KernelError::InternalError);
    }

    // 1b. An idle listener polls as no events -- and as *that*, not as an id the
    //     daemon does not know. Until 2026-09-26 the daemon's OP_POLL looked only
    //     at connections and answered `-1` for every listener, which reads as
    //     "not connected", so no poll/select/epoll caller was ever woken by an
    //     incoming connection (lane F's requests/f-a-poll-never-reports-a-
    //     connection-waiting-on-a-listening-socket.md).
    if listener_polls(&mut conn, listener_id, false, "before any connection")? != Some(true) {
        conn.close()?;
        return Err(KernelError::InternalError);
    }

    // 2. Non-blocking connect to our own IP: one OP_CONNECT pump drives the full
    //    handshake for both ends over the software loopback.
    let connect_res = conn.connect(&me_ip, PORT, true)?;
    if connect_res != 0 && connect_res != netipc::ring::ERR_IN_PROGRESS {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   loopback connect failed (result {}) — server-socket parity broken",
            connect_res
        );
        return Err(KernelError::InternalError);
    }
    // If the handshake did not complete inside the connect's single pump, drive it
    // to writable (each poll pumps once). Loopback normally completes immediately.
    if connect_res == netipc::ring::ERR_IN_PROGRESS {
        let mut writable = false;
        for _ in 0..16u32 {
            let (_r, w, err) = conn.poll_ready()?;
            if err {
                conn.close()?;
                crate::serial_println!(
                    "[netstack-client]   loopback connect reported POLL_ERR — server-socket parity broken"
                );
                return Err(KernelError::InternalError);
            }
            if w {
                writable = true;
                break;
            }
        }
        if !writable {
            conn.close()?;
            crate::serial_println!(
                "[netstack-client]   loopback connect never resolved to writable — server-socket parity broken"
            );
            return Err(KernelError::InternalError);
        }
    }

    // 2b. The completed connection now waits in the backlog, so the listener must
    //     poll readable: POLLIN on a listening socket means "accept will not
    //     block". Each poll pumps once, so a final ACK still in flight gets a
    //     few chances to land.
    let mut listener_readable = false;
    for _ in 0..16u32 {
        match listener_polls(&mut conn, listener_id, true, "once a connection completed")? {
            Some(true) => {
                listener_readable = true;
                break;
            }
            Some(false) => {}
            None => {
                conn.close()?;
                return Err(KernelError::InternalError);
            }
        }
    }
    if !listener_readable {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   a loopback connection completed but its listener never polled readable — a server waiting in poll/select/epoll would sleep through it"
        );
        return Err(KernelError::InternalError);
    }

    // 3. Accept the passive connection queued in the backlog. Once, not in a
    //    retry loop: the listener has just polled readable, and the daemon
    //    answers poll and accept from one predicate (`Listener::acceptable`), so
    //    the first accept must succeed. A retry would hide exactly the
    //    disagreement that shared predicate exists to prevent.
    let mut peer = [0u8; 6];
    let ares = conn.accept(listener_id, accepted_id, &mut peer)?;
    if ares != 0 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   the listener polled readable but accept returned {} — poll and accept disagree",
            ares
        );
        return Err(KernelError::InternalError);
    }

    // 3b. Its one connection taken, the listener is quiet again.
    if listener_polls(
        &mut conn,
        listener_id,
        false,
        "after its connection was accepted",
    )? != Some(true)
    {
        conn.close()?;
        return Err(KernelError::InternalError);
    }

    // The accepted peer's source IP must be our own (the loopback client's src).
    let peer_ip = peer.get(..4).unwrap_or(&[]);
    if peer_ip != me_ip {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   accepted peer ip {}.{}.{}.{} != local ip — demux broken",
            peer.first().copied().unwrap_or(0),
            peer.get(1).copied().unwrap_or(0),
            peer.get(2).copied().unwrap_or(0),
            peer.get(3).copied().unwrap_or(0),
        );
        return Err(KernelError::InternalError);
    }
    let peer_port = u16::from_be_bytes([
        peer.get(4).copied().unwrap_or(0),
        peer.get(5).copied().unwrap_or(0),
    ]);
    crate::serial_println!(
        "[netstack-client]   accepted loopback connection from {}.{}.{}.{}:{}",
        me_ip[0],
        me_ip[1],
        me_ip[2],
        me_ip[3],
        peer_port
    );

    // 4. Client → server: send on the client's id, receive on the accepted id.
    let sent = conn.send(CLIENT_MSG, false)?;
    if sent <= 0 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   loopback client send returned {} — server-socket parity broken",
            sent
        );
        return Err(KernelError::InternalError);
    }
    if !recv_exact(&mut conn, accepted_id, CLIENT_MSG)? {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   server did not receive the client's message intact — parity broken"
        );
        return Err(KernelError::InternalError);
    }

    // 5. Server → client: send on the accepted id, receive on the client's id.
    let sent2 = conn.send_on(accepted_id, SERVER_MSG, false)?;
    if sent2 <= 0 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   loopback server send returned {} — server-socket parity broken",
            sent2
        );
        return Err(KernelError::InternalError);
    }
    if !recv_exact(&mut conn, client_id, SERVER_MSG)? {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   client did not receive the server's message intact — parity broken"
        );
        return Err(KernelError::InternalError);
    }

    // 6. shutdown(2) parity on the established client stream (the client's id):
    //    SHUT_WR closes the write side (a later send must fail with EPIPE), then
    //    SHUT_RD closes the read side (a later recv must report EOF = 0 bytes).
    conn.shutdown(netipc::ring::SHUT_WR)?;
    match conn.send(b"post-shutdown", false) {
        Err(KernelError::BrokenPipe) => {}
        other => {
            conn.close()?;
            crate::serial_println!(
                "[netstack-client]   send after shutdown(SHUT_WR) did not return EPIPE ({other:?}) — shutdown parity broken"
            );
            return Err(KernelError::InternalError);
        }
    }
    conn.shutdown(netipc::ring::SHUT_RD)?;
    match conn.recv(&mut [0u8; 16], false, false) {
        Ok(0) => {}
        other => {
            conn.close()?;
            crate::serial_println!(
                "[netstack-client]   recv after shutdown(SHUT_RD) did not report EOF ({other:?}) — shutdown parity broken"
            );
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!(
        "[netstack-client]   shutdown(SHUT_WR)→EPIPE + shutdown(SHUT_RD)→EOF ok"
    );

    conn.close()?;
    crate::serial_println!(
        "[netstack-client]   listen/poll/accept + bidirectional data + shutdown over loopback ok — server-socket parity ok"
    );
    Ok(Some(()))
}

/// Boot self-test: prove the **IPv6 connect** path (`OP_CONNECT6`) over the daemon,
/// closing the final `D-NETSOCK-SYNC` parity gap before the `net.userspace` default
/// can be flipped (increment 5.7). The kernel-resident stack could open AF_INET6
/// connections; the daemon was IPv4-only until this increment, so this test asserts
/// that a full TCP-over-IPv6 handshake and bidirectional data exchange now work
/// through the userspace daemon.
///
/// Like [`self_test_listen_accept`], it exercises the daemon's **in-process software
/// loopback** because slirp offers no IPv6 peer (and no IPv6 router, so real NDP
/// cannot be driven end-to-end under QEMU). The connect target is the daemon's own
/// **link-local address** `fe80::/64 + EUI-64(mac)` — the same value the daemon
/// derives from its NIC MAC for `me.ip6`. A frame addressed to `me.ip6` is diverted
/// into the daemon's internal RX FIFO (bypassing NDP entirely, since the loopback
/// demuxes by destination IP), so the v6 handshake completes in-process exactly as
/// the v4 loopback does. A single non-blocking `OP_CONNECT6` pump drives the 3-way
/// handshake for both ends; `OP_ACCEPT` (18-byte peer window: `[ip6:16][port:2]`)
/// dequeues the passive side; then a bidirectional exchange proves the accepted
/// IPv6 connection is a real, addressable socket.
///
/// The kernel crate cannot depend on `netproto`, so the EUI-64 link-local
/// derivation is inlined here (RFC 4291 App. A: flip the U/L bit of the first MAC
/// octet, insert `FF:FE` in the middle) to reproduce `icmpv6::link_local_from_mac`.
///
/// Returns `Ok(Some(()))` if the v6 connect→accept→data round-trip completed,
/// `Ok(None)` if the interface has no MAC yet (no NIC — the loopback v6 divert keys
/// on a non-zero `me.ip6` derived from the MAC), and `Err` on a real
/// control-protocol fault or a parity break.
///
/// # Errors
///
/// Propagates control-protocol faults from the client; reports a failed
/// connect6/accept over loopback, or a data mismatch, as an error.
pub fn self_test_connect6() -> KernelResult<Option<()>> {
    /// Loopback listen port (arbitrary, distinct from the v4 self-test's).
    const PORT: u16 = 9101;
    const CLIENT_MSG: &[u8] = b"slate-connect6:ping";
    const SERVER_MSG: &[u8] = b"slate-connect6:pong";

    let mac = crate::net::interface::mac().0;
    if mac == [0u8; 6] {
        // No NIC → the daemon has no me.ip6 to loop back to.
        return Ok(None);
    }

    // Inline EUI-64 link-local (RFC 4291 App. A), matching the daemon's
    // `icmpv6::link_local_from_mac(mac)` used to seed `me.ip6`.
    let ll: [u8; 16] = [
        0xFE,
        0x80,
        0,
        0,
        0,
        0,
        0,
        0,
        mac[0] ^ 0x02,
        mac[1],
        mac[2],
        0xFF,
        0xFE,
        mac[3],
        mac[4],
        mac[5],
    ];

    let mut conn = NetstackConn::open()?;
    // The listener's and the accepted connection's ids come from the one
    // namespace every socket shares on the ring, like the client's own.
    let listener_id = alloc_conn_id();
    let accepted_id = alloc_conn_id();
    let client_id = conn.conn_id();

    // 1. Register the passive listener BEFORE connecting (family-agnostic port).
    let listen_res = conn.listen(listener_id, PORT)?;
    if listen_res != 0 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   v6 listen(port={}) failed (result {}) — IPv6 parity broken",
            PORT,
            listen_res
        );
        return Err(KernelError::InternalError);
    }

    // 2. Non-blocking IPv6 connect to our own link-local: one OP_CONNECT6 pump
    //    drives the full handshake for both ends over the software loopback.
    let connect_res = conn.connect6(&ll, PORT, true)?;
    if connect_res != 0 && connect_res != netipc::ring::ERR_IN_PROGRESS {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   v6 loopback connect failed (result {}) — IPv6 parity broken",
            connect_res
        );
        return Err(KernelError::InternalError);
    }
    if connect_res == netipc::ring::ERR_IN_PROGRESS {
        let mut writable = false;
        for _ in 0..16u32 {
            let (_r, w, err) = conn.poll_ready()?;
            if err {
                conn.close()?;
                crate::serial_println!(
                    "[netstack-client]   v6 loopback connect reported POLL_ERR — IPv6 parity broken"
                );
                return Err(KernelError::InternalError);
            }
            if w {
                writable = true;
                break;
            }
        }
        if !writable {
            conn.close()?;
            crate::serial_println!(
                "[netstack-client]   v6 loopback connect never resolved to writable — IPv6 parity broken"
            );
            return Err(KernelError::InternalError);
        }
    }

    // 3. Accept the passive connection (18-byte v6 peer window).
    let mut peer = [0u8; 18];
    let mut accepted = false;
    for _ in 0..16u32 {
        let ares = conn.accept6(listener_id, accepted_id, &mut peer)?;
        if ares == 0 {
            accepted = true;
            break;
        }
        if ares != netipc::ring::ERR_WOULD_BLOCK {
            conn.close()?;
            crate::serial_println!(
                "[netstack-client]   v6 accept failed (result {}) — IPv6 parity broken",
                ares
            );
            return Err(KernelError::InternalError);
        }
    }
    if !accepted {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   v6 accept never dequeued the loopback connection — IPv6 parity broken"
        );
        return Err(KernelError::InternalError);
    }

    // The accepted peer's source IPv6 must be our own link-local.
    let peer_ip6 = peer.get(..16).unwrap_or(&[]);
    if peer_ip6 != ll {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   accepted v6 peer address != local link-local — v6 demux broken"
        );
        return Err(KernelError::InternalError);
    }
    let peer_port = u16::from_be_bytes([
        peer.get(16).copied().unwrap_or(0),
        peer.get(17).copied().unwrap_or(0),
    ]);
    crate::serial_println!(
        "[netstack-client]   accepted IPv6 loopback connection from fe80::…:{} on port {}",
        peer_port,
        PORT
    );

    // 4. Client → server.
    let sent = conn.send(CLIENT_MSG, false)?;
    if sent <= 0 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   v6 loopback client send returned {} — IPv6 parity broken",
            sent
        );
        return Err(KernelError::InternalError);
    }
    if !recv_exact(&mut conn, accepted_id, CLIENT_MSG)? {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   v6 server did not receive the client's message intact — IPv6 parity broken"
        );
        return Err(KernelError::InternalError);
    }

    // 5. Server → client.
    let sent2 = conn.send_on(accepted_id, SERVER_MSG, false)?;
    if sent2 <= 0 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   v6 loopback server send returned {} — IPv6 parity broken",
            sent2
        );
        return Err(KernelError::InternalError);
    }
    if !recv_exact(&mut conn, client_id, SERVER_MSG)? {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   v6 client did not receive the server's message intact — IPv6 parity broken"
        );
        return Err(KernelError::InternalError);
    }

    // 6. getsockname: the client connection's local endpoint must be our own
    //    link-local (this is a self-connect) with a non-zero ephemeral port.
    match conn.local_addr()? {
        LocalEndpoint::V6(ip6, lport) => {
            if ip6 != ll || lport == 0 {
                conn.close()?;
                crate::serial_println!(
                    "[netstack-client]   v6 getsockname wrong (ip6 mismatch or port 0) — parity broken"
                );
                return Err(KernelError::InternalError);
            }
            crate::serial_println!(
                "[netstack-client]   v6 getsockname: local fe80::…:{} ok",
                lport
            );
        }
        LocalEndpoint::V4(..) => {
            conn.close()?;
            crate::serial_println!(
                "[netstack-client]   v6 getsockname returned an IPv4 endpoint — parity broken"
            );
            return Err(KernelError::InternalError);
        }
    }

    conn.close()?;
    crate::serial_println!(
        "[netstack-client]   IPv6 connect/accept + bidirectional data + getsockname over loopback ok — IPv6 parity ok"
    );
    Ok(Some(()))
}

/// Blocking-receive helper for the listen/accept self-test: read from `conn_id`
/// (looping over ≤`RCV_CAP` chunks) until `expect.len()` bytes have arrived, then
/// verify they match `expect` byte-for-byte. Returns `Ok(true)` on an exact match,
/// `Ok(false)` on a mismatch, short read, or premature EOF.
fn recv_exact(conn: &mut NetstackConn, conn_id: u32, expect: &[u8]) -> KernelResult<bool> {
    let mut got = [0u8; 128];
    let cap = got.len().min(expect.len());
    let mut filled = 0usize;
    for _ in 0..32u32 {
        if filled >= cap {
            break;
        }
        let slot = match got.get_mut(filled..cap) {
            Some(s) => s,
            None => break,
        };
        let n = conn.recv_on(conn_id, slot, false, false)?;
        if n < 0 {
            return Ok(false);
        }
        if n == 0 {
            // No data this round and stream still open, or EOF; try a couple more
            // pumps then give up.
            continue;
        }
        let added = usize::try_from(n)
            .unwrap_or(0)
            .min(cap.saturating_sub(filled));
        filled = filled.saturating_add(added);
    }
    if filled != expect.len() {
        return Ok(false);
    }
    Ok(got.get(..filled) == Some(expect))
}

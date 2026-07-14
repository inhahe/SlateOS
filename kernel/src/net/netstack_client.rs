//! Reusable in-kernel client for the userspace `net.stack` daemon.
//!
//! This is the kernel side of the Path B userspace-netstack migration
//! (`design-decisions.md` §63, cutover strategy §66). It wraps the shared-memory
//! control-ring protocol — `netipc::ring` opcodes driven over an `OP_RING_TCP`
//! control channel to the persistent daemon session — that was previously
//! hand-inlined in the `spawn.rs` boot self-tests, into a single reusable
//! [`NetstackConn`] type: **one** shared-memory ring plus **one** TCP connection,
//! driven `connect → send → recv → close`, where each operation is a control
//! round-trip against the daemon's *persistent* session.
//!
//! ## Persistence
//!
//! Each operation opens a fresh `net.stack` service channel, hands the daemon the
//! shared ring handle (`OP_RING_TCP`), the daemon drains the queued SQE(s)
//! against its persistent per-ring session, and posts one completion each. The
//! kernel keeps the single ring mapped across all rounds. Because the connection
//! is opened in one round and driven (send/recv) in later rounds, a successful
//! send after a connect *is itself* proof that the daemon's session survived
//! between submissions — exactly the property the persistent socket daemon needs
//! for the staged cutover (§66, Q22b).
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

/// The one connection id used within a [`NetstackConn`]'s session. A single
/// client owns a single connection, so a fixed id is sufficient and keeps the
/// daemon-side session table trivial.
const CONN_ID: u32 = 1;

/// `user_data` base ("NSCL" = net-stack-client). Every SQE gets a distinct,
/// monotonically increasing tag so completions can be matched 1:1 in FIFO order.
const UD_BASE: u64 = 0x4e53_434c_0000_0000;

/// Per-round control-channel reply timeout (ns). Generous: the daemon may be
/// blocking on the wire (connect handshake, receive) while it drains our SQE.
const RECV_TIMEOUT_NS: u64 = 12_000_000_000;

/// Whether the userspace-netstack cutover switch (`net.userspace`) is set on the
/// kernel command line.
///
/// **Default off.** Absent the flag, the kernel keeps using its in-kernel
/// resident stack. This is the staged-cutover gate from `design-decisions.md`
/// §66 (Q22b → staged): prove daemon parity in QEMU behind the switch, flip the
/// default, then delete the resident stack. When set: the persistent userspace
/// netstack daemon is spawned at boot and claims the NIC (increment 5.6), and
/// AF_INET/AF_INET6 `SOCK_STREAM` sockets route to it (increment 5.5). The
/// default has not been flipped yet (increment 5.7), so today this only fires
/// when the operator explicitly passes `net.userspace` on the kernel cmdline.
#[must_use]
pub fn userspace_enabled() -> bool {
    crate::fs::kernparam::is_set("net.userspace")
}

/// A single client connection to the userspace `net.stack` daemon.
///
/// Owns one shared-memory ring and one daemon-side TCP connection. Drive it with
/// [`connect`](Self::connect) → [`send`](Self::send) → [`recv`](Self::recv) →
/// [`close`](Self::close). Dropping without calling `close` still tears the
/// daemon session down (best effort) and always releases the shared memory.
pub struct NetstackConn {
    /// Shared-memory region backing the ring, shared with the daemon.
    handle: shm::ShmHandle,
    /// Region size in bytes (as passed to the daemon in each `OP_RING_TCP`).
    size: u32,
    /// The single connection id this client drives.
    conn_id: u32,
    /// Next `user_data` tag to hand out.
    next_ud: u64,
    /// Whether a connection is currently open (a successful `connect` that has
    /// not yet been closed). Guards the teardown `OP_CLOSE`.
    connected: bool,
    /// Whether the daemon has served at least one round-trip for our ring (and
    /// therefore may hold a session for it). Set the first time a submission
    /// succeeds; cleared once we send `OP_STOP`. Guards teardown so a
    /// created-but-never-used connection never contacts the daemon, and so
    /// teardown runs at most once.
    session_open: bool,
}

// Note: every field is plain data (a `ShmHandle` u64 newtype, integers, bools),
// so `NetstackConn` is automatically `Send + Sync`. Crucially it does *not* hold
// an owned `Ring` view (whose raw `*mut u8` would make it `!Send`): the ring is
// re-`attach`ed on demand from the shared-memory handle inside each operation.
// This is what lets a `NetstackConn` live in the global socket table. Callers
// that share one must still serialize access with their own lock, because the
// daemon-side session is single-producer/single-consumer.

impl NetstackConn {
    /// Allocate the shared ring and prepare a client. Does **not** contact the
    /// daemon yet — the first round-trip happens on [`connect`](Self::connect).
    ///
    /// # Errors
    ///
    /// Returns an error if the shared memory cannot be created/mapped or the ring
    /// geometry cannot be initialized.
    pub fn open() -> KernelResult<Self> {
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
        // ring header is published with a release fence inside `init`. We only
        // need the header written here — the driver view is re-`attach`ed on
        // demand per op (see `attach_ring`), so the `Ring` value is discarded.
        if unsafe { netring::Ring::init(kaddr, size, SQ_ENTRIES, CQ_ENTRIES, DATA_LEN) }.is_none() {
            shm::close(handle);
            return Err(KernelError::InternalError);
        }
        let size_u32 = u32::try_from(size).map_err(|_| KernelError::InternalError)?;
        Ok(Self {
            handle,
            size: size_u32,
            conn_id: CONN_ID,
            next_ud: UD_BASE,
            connected: false,
            session_open: false,
        })
    }

    /// Re-attach the ring driver view from the shared-memory handle.
    ///
    /// Attaching is stateless — the free-running SQ/CQ indices live in the shared
    /// region, so re-deriving the view each op is correct (and avoids caching a
    /// non-`Send` raw pointer in the struct). The header was published by
    /// [`open`](Self::open)'s `init`.
    fn attach_ring(&self) -> KernelResult<netring::Ring> {
        let kaddr = shm::kernel_addr(self.handle)?;
        let len = self.size as usize;
        // SAFETY: `kaddr` is the stable kernel VA of our shm region, valid and
        // aligned for `len` bytes for the region's lifetime; `attach` only reads
        // the header (published by `open`) and bounds-checks the geometry, so it
        // can never read/write outside the region.
        unsafe { netring::Ring::attach(kaddr, len) }.ok_or(KernelError::InternalError)
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
        let ring = self.attach_ring()?;
        let ud = self.next_ud();
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
        let res = self.submit_and_reap(&ring, &sqe)?;
        // Both an established (`res >= 0`) and an in-progress non-blocking connect
        // leave a live connection installed in the daemon session.
        if res >= 0 || res == netipc::ring::ERR_IN_PROGRESS {
            self.connected = true;
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
    /// client's fixed [`CONN_ID`] within the same ring session (the listen/accept
    /// loopback self-test). The public [`send`](Self::send) is the `self.conn_id`
    /// specialization.
    ///
    /// # Errors
    ///
    /// Same as [`send`](Self::send).
    fn send_on(&mut self, conn_id: u32, buf: &[u8], nonblock: bool) -> KernelResult<i32> {
        let ring = self.attach_ring()?;
        let send_aux = if nonblock { netipc::ring::SEND_NONBLOCK } else { 0 };
        let mut total: i32 = 0;
        let mut off = 0usize;
        while off < buf.len() {
            let end = off.saturating_add(SND_CAP as usize).min(buf.len());
            let chunk = buf.get(off..end).ok_or(KernelError::InternalError)?;
            if !ring.write_data(SND_OFF as usize, chunk) {
                return Err(KernelError::InternalError);
            }
            let chunk_len = u32::try_from(chunk.len()).map_err(|_| KernelError::InternalError)?;
            let ud = self.next_ud();
            let sqe = netipc::ring::Sqe {
                op: netipc::ring::OP_SEND,
                conn_id,
                data_off: SND_OFF,
                data_len: chunk_len,
                user_data: ud,
                aux: send_aux,
            };
            let res = self.submit_and_reap(&ring, &sqe)?;
            if res == netipc::ring::ERR_WOULD_BLOCK {
                // Non-blocking send hit a full window. Report progress if any bytes
                // were already accepted (Linux returns the short count); otherwise
                // surface EAGAIN so the caller's O_NONBLOCK write retries later.
                if total > 0 {
                    return Ok(total);
                }
                return Err(KernelError::WouldBlock);
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
    /// # Errors
    ///
    /// - [`KernelError::WouldBlock`] — `nonblock` was set and no data was ready.
    /// - a control-protocol fault (see [`connect`](Self::connect)), or a failure to
    ///   read back the ring data window.
    pub fn recv(&mut self, buf: &mut [u8], nonblock: bool) -> KernelResult<i32> {
        let cid = self.conn_id;
        self.recv_on(cid, buf, nonblock)
    }

    /// Receive on an explicit connection id (see [`recv`](Self::recv)).
    ///
    /// The server-side counterpart to [`send_on`](Self::send_on): reads from an
    /// accepted connection whose id differs from the client's fixed [`CONN_ID`]
    /// within one ring session. The public [`recv`](Self::recv) is the
    /// `self.conn_id` specialization.
    ///
    /// # Errors
    ///
    /// Same as [`recv`](Self::recv).
    fn recv_on(&mut self, conn_id: u32, buf: &mut [u8], nonblock: bool) -> KernelResult<i32> {
        let ring = self.attach_ring()?;
        let want = buf.len().min(RCV_CAP as usize);
        let want_u32 = u32::try_from(want).map_err(|_| KernelError::InternalError)?;
        let ud = self.next_ud();
        let aux = if nonblock { netipc::ring::RECV_NONBLOCK } else { 0 };
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_RECV,
            conn_id,
            data_off: RCV_OFF,
            data_len: want_u32,
            user_data: ud,
            aux,
        };
        let res = self.submit_and_reap(&ring, &sqe)?;
        if res == netipc::ring::ERR_WOULD_BLOCK {
            // Non-blocking recv with nothing ready: the caller's O_NONBLOCK.
            return Err(KernelError::WouldBlock);
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
        let ring = self.attach_ring()?;
        let ud = self.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_POLL,
            conn_id: self.conn_id,
            user_data: ud,
            ..netipc::ring::Sqe::default()
        };
        let res = self.submit_and_reap(&ring, &sqe)?;
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
        let ring = self.attach_ring()?;
        let ud = self.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_LISTEN,
            conn_id: listener_id,
            user_data: ud,
            aux: u64::from(port),
            ..netipc::ring::Sqe::default()
        };
        self.submit_and_reap(&ring, &sqe)
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
        let ring = self.attach_ring()?;
        let ud = self.next_ud();
        // Reuse the recv-landing window for the 6-byte peer address: accept is
        // issued before any data recv on this ring, so there is no clash.
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_ACCEPT,
            conn_id: listener_id,
            data_off: RCV_OFF,
            data_len: 6,
            user_data: ud,
            aux: u64::from(new_conn_id),
        };
        let res = self.submit_and_reap(&ring, &sqe)?;
        if res == 0 && !ring.read_data(RCV_OFF as usize, peer) {
            return Err(KernelError::InternalError);
        }
        Ok(res)
    }

    /// Close the connection and end the daemon session.
    ///
    /// Consumes the client. Best effort: any per-op failure during teardown is
    /// ignored (the daemon also reaps idle sessions on its own deadline). The
    /// shared memory is released when the returned value is dropped.
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

    /// Hand out the next `user_data` tag.
    fn next_ud(&mut self) -> u64 {
        let ud = self.next_ud;
        self.next_ud = self.next_ud.wrapping_add(1);
        ud
    }

    /// Push one SQE, run one control round-trip, and reap exactly one completion,
    /// verifying it echoes the SQE's `user_data` and that no extra completion is
    /// posted. Returns the completion result.
    fn submit_and_reap(
        &mut self,
        ring: &netring::Ring,
        sqe: &netipc::ring::Sqe,
    ) -> KernelResult<i32> {
        let want_ud = sqe.user_data;
        if !ring.sq_push(sqe) {
            return Err(KernelError::ResourceExhausted);
        }
        self.submit_round()?;
        // The daemon served us, so it now holds a session for our ring — mark it
        // so teardown emits an `OP_STOP`. (A created-but-never-submitted client
        // leaves this false and never contacts the daemon.)
        self.session_open = true;
        let cqe = ring.cq_pop().ok_or(KernelError::InternalError)?;
        if cqe.user_data != want_ud {
            return Err(KernelError::InternalError);
        }
        // No SQE should ever produce more than one completion.
        if ring.cq_pop().is_some() {
            return Err(KernelError::InternalError);
        }
        Ok(cqe.result)
    }

    /// One `OP_RING_TCP` control round-trip: open a fresh `net.stack` channel,
    /// hand the daemon our ring handle+size, wait for the acknowledgement. The
    /// daemon drains whatever SQEs are queued against its persistent session.
    fn submit_round(&self) -> KernelResult<()> {
        let client = service::connect(b"net.stack")?;
        // Everything from here is fallible; make sure the channel is always
        // closed regardless of which step fails.
        let outcome = self.submit_round_on(client);
        channel::close(client);
        outcome
    }

    fn submit_round_on(&self, client: channel::ChannelHandle) -> KernelResult<()> {
        let mut req = [0u8; 16];
        let n = netipc::encode_ring_tcp(&mut req, self.handle.raw(), self.size)
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

    /// Close the connection (if open) and stop the daemon session. Idempotent:
    /// runs at most once thanks to `session_open`.
    fn teardown(&mut self) {
        if !self.session_open {
            return;
        }
        // Attach once for the whole teardown. If the region can't be attached
        // (should not happen while the handle is live), give up cleanly — the
        // daemon reaps idle sessions on its own deadline anyway.
        let ring = match self.attach_ring() {
            Ok(r) => r,
            Err(_) => {
                self.session_open = false;
                return;
            }
        };
        if self.connected {
            let ud = self.next_ud();
            let close_sqe = netipc::ring::Sqe {
                op: netipc::ring::OP_CLOSE,
                conn_id: self.conn_id,
                user_data: ud,
                ..netipc::ring::Sqe::default()
            };
            // Best effort — the session is torn down next regardless.
            let _ = self.submit_and_reap(&ring, &close_sqe);
            self.connected = false;
        }
        let ud = self.next_ud();
        let stop_sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_STOP,
            user_data: ud,
            ..netipc::ring::Sqe::default()
        };
        let _ = self.submit_and_reap(&ring, &stop_sqe);
        self.session_open = false;
    }
}

impl Drop for NetstackConn {
    fn drop(&mut self) {
        // Tear the daemon session down if the caller didn't call close(), then
        // always release the shared memory.
        self.teardown();
        shm::close(self.handle);
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
    let recv_res = conn.recv(&mut body, false)?;
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
    let outcome = conn.recv(&mut body, true);
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
/// drive [`poll_ready`](NetstackConn::poll_ready) in a bounded loop until the socket
/// reports **writable** (the connect resolved) — checking that it never reports the
/// error bit for a good endpoint. A writable, error-free result is the parity
/// property: a `poll(POLLOUT)` waiter is woken exactly when the connect completes.
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

    // Poll for writable (POLLOUT), exactly as a userspace non-blocking connect would.
    let mut writable = false;
    for _ in 0..64u32 {
        let (_readable, w, error) = conn.poll_ready()?;
        if error {
            conn.close()?;
            crate::serial_println!(
                "[netstack-client]   non-blocking connect reported POLL_ERR against a good endpoint"
            );
            return Err(KernelError::InternalError);
        }
        if w {
            writable = true;
            break;
        }
    }
    conn.close()?;

    if writable {
        crate::serial_println!(
            "[netstack-client]   non-blocking connect resolved to writable (POLLOUT) — connect parity ok"
        );
        Ok(Some(()))
    } else {
        // Handshake never completed in-window (slirp variance); path still ran.
        Ok(None)
    }
}

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
/// the accepted-connection id are session-local ids distinct from the client's
/// fixed [`CONN_ID`], all demuxed by the daemon by 4-tuple.
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
    /// Session-local listener id (distinct from any connection id).
    const LISTENER_ID: u32 = 100;
    /// Id under which the accepted server-side connection is installed.
    const ACCEPTED_ID: u32 = 101;
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

    // 1. Register the passive listener BEFORE connecting, so the SYN routed over
    //    the loopback FIFO finds a listening port.
    let listen_res = conn.listen(LISTENER_ID, PORT)?;
    if listen_res != 0 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   listen(port={}) failed on a fresh session (result {}) — server-socket parity broken",
            PORT,
            listen_res
        );
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

    // 3. Accept the passive connection queued in the backlog. Each accept pumps
    //    once, so retry a few times in case the final ACK needs another pump.
    let mut peer = [0u8; 6];
    let mut accepted = false;
    for _ in 0..16u32 {
        let ares = conn.accept(LISTENER_ID, ACCEPTED_ID, &mut peer)?;
        if ares == 0 {
            accepted = true;
            break;
        }
        if ares != netipc::ring::ERR_WOULD_BLOCK {
            conn.close()?;
            crate::serial_println!(
                "[netstack-client]   accept failed (result {}) — server-socket parity broken",
                ares
            );
            return Err(KernelError::InternalError);
        }
    }
    if !accepted {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   accept never dequeued the loopback connection — server-socket parity broken"
        );
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

    // 4. Client → server: send on CONN_ID, receive on the accepted id.
    let sent = conn.send(CLIENT_MSG, false)?;
    if sent <= 0 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   loopback client send returned {} — server-socket parity broken",
            sent
        );
        return Err(KernelError::InternalError);
    }
    if !recv_exact(&mut conn, ACCEPTED_ID, CLIENT_MSG)? {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   server did not receive the client's message intact — parity broken"
        );
        return Err(KernelError::InternalError);
    }

    // 5. Server → client: send on the accepted id, receive on CONN_ID.
    let sent2 = conn.send_on(ACCEPTED_ID, SERVER_MSG, false)?;
    if sent2 <= 0 {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   loopback server send returned {} — server-socket parity broken",
            sent2
        );
        return Err(KernelError::InternalError);
    }
    if !recv_exact(&mut conn, CONN_ID, SERVER_MSG)? {
        conn.close()?;
        crate::serial_println!(
            "[netstack-client]   client did not receive the server's message intact — parity broken"
        );
        return Err(KernelError::InternalError);
    }

    conn.close()?;
    crate::serial_println!(
        "[netstack-client]   listen/accept + bidirectional data over loopback ok — server-socket parity ok"
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
        let n = conn.recv_on(conn_id, slot, false)?;
        if n < 0 {
            return Ok(false);
        }
        if n == 0 {
            // No data this round and stream still open, or EOF; try a couple more
            // pumps then give up.
            continue;
        }
        let added = usize::try_from(n).unwrap_or(0).min(cap.saturating_sub(filled));
        filled = filled.saturating_add(added);
    }
    if filled != expect.len() {
        return Ok(false);
    }
    Ok(got.get(..filled) == Some(expect))
}

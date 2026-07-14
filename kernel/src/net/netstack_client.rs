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
/// default, then delete the resident stack. Nothing routes real socket traffic
/// on this yet (that is increment 5.5); today it only records boot-time intent
/// and is surfaced by the boot self-test.
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
    /// The mapped ring driver over `handle`. Re-attaching to the same VA is
    /// stateless (the free-running indices live in the shared region), so this
    /// owned struct is valid across every round-trip.
    ring: netring::Ring,
    /// The single connection id this client drives.
    conn_id: u32,
    /// Next `user_data` tag to hand out.
    next_ud: u64,
    /// Whether a connection is currently open (a successful `connect` that has
    /// not yet been closed). Guards the teardown `OP_CLOSE`.
    connected: bool,
    /// Whether the daemon still holds a session for our ring. Cleared once we
    /// send `OP_STOP`, so teardown runs at most once.
    session_open: bool,
}

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
        // ring header is published with a release fence inside `init`.
        let ring = match unsafe { netring::Ring::init(kaddr, size, SQ_ENTRIES, CQ_ENTRIES, DATA_LEN) }
        {
            Some(r) => r,
            None => {
                shm::close(handle);
                return Err(KernelError::InternalError);
            }
        };
        let size_u32 = u32::try_from(size).map_err(|_| KernelError::InternalError)?;
        Ok(Self {
            handle,
            size: size_u32,
            ring,
            conn_id: CONN_ID,
            next_ud: UD_BASE,
            connected: false,
            session_open: true,
        })
    }

    /// Open the TCP connection to `ip:port`.
    ///
    /// Returns the daemon's connect result: `>= 0` on success (connection now
    /// live), `< 0` if the connect failed (no upstream / refused). A non-negative
    /// result marks the client connected so a later [`send`](Self::send) drives
    /// the same persisted connection.
    ///
    /// # Errors
    ///
    /// Returns an error on a control-protocol fault (ring full, missing/misordered
    /// completion, service-channel failure) — distinct from a `< 0` connect
    /// result, which is a normal "no upstream" outcome.
    pub fn connect(&mut self, ip: &[u8; 4], port: u16) -> KernelResult<i32> {
        let ud = self.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_CONNECT,
            conn_id: self.conn_id,
            user_data: ud,
            aux: netipc::ring::Sqe::pack_endpoint(ip, port),
            ..netipc::ring::Sqe::default()
        };
        let res = self.submit_and_reap(&sqe)?;
        if res >= 0 {
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
    /// # Errors
    ///
    /// Returns an error on a control-protocol fault (see [`connect`](Self::connect)).
    pub fn send(&mut self, buf: &[u8]) -> KernelResult<i32> {
        let mut total: i32 = 0;
        let mut off = 0usize;
        while off < buf.len() {
            let end = off.saturating_add(SND_CAP as usize).min(buf.len());
            let chunk = buf.get(off..end).ok_or(KernelError::InternalError)?;
            if !self.ring.write_data(SND_OFF as usize, chunk) {
                return Err(KernelError::InternalError);
            }
            let chunk_len = u32::try_from(chunk.len()).map_err(|_| KernelError::InternalError)?;
            let ud = self.next_ud();
            let sqe = netipc::ring::Sqe {
                op: netipc::ring::OP_SEND,
                conn_id: self.conn_id,
                data_off: SND_OFF,
                data_len: chunk_len,
                user_data: ud,
                aux: 0,
            };
            let res = self.submit_and_reap(&sqe)?;
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
    /// # Errors
    ///
    /// Returns an error on a control-protocol fault (see [`connect`](Self::connect))
    /// or if the ring data window cannot be read back.
    pub fn recv(&mut self, buf: &mut [u8]) -> KernelResult<i32> {
        let want = buf.len().min(RCV_CAP as usize);
        let want_u32 = u32::try_from(want).map_err(|_| KernelError::InternalError)?;
        let ud = self.next_ud();
        let sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_RECV,
            conn_id: self.conn_id,
            data_off: RCV_OFF,
            data_len: want_u32,
            user_data: ud,
            aux: 0,
        };
        let res = self.submit_and_reap(&sqe)?;
        if res <= 0 {
            return Ok(res);
        }
        let n = usize::try_from(res).unwrap_or(0).min(want);
        let window = buf.get_mut(..n).ok_or(KernelError::InternalError)?;
        if !self.ring.read_data(RCV_OFF as usize, window) {
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
    fn submit_and_reap(&mut self, sqe: &netipc::ring::Sqe) -> KernelResult<i32> {
        let want_ud = sqe.user_data;
        if !self.ring.sq_push(sqe) {
            return Err(KernelError::ResourceExhausted);
        }
        self.submit_round()?;
        let cqe = self.ring.cq_pop().ok_or(KernelError::InternalError)?;
        if cqe.user_data != want_ud {
            return Err(KernelError::InternalError);
        }
        // No SQE should ever produce more than one completion.
        if self.ring.cq_pop().is_some() {
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
        if self.connected {
            let ud = self.next_ud();
            let close_sqe = netipc::ring::Sqe {
                op: netipc::ring::OP_CLOSE,
                conn_id: self.conn_id,
                user_data: ud,
                ..netipc::ring::Sqe::default()
            };
            // Best effort — the session is torn down next regardless.
            let _ = self.submit_and_reap(&close_sqe);
            self.connected = false;
        }
        let ud = self.next_ud();
        let stop_sqe = netipc::ring::Sqe {
            op: netipc::ring::OP_STOP,
            user_data: ud,
            ..netipc::ring::Sqe::default()
        };
        let _ = self.submit_and_reap(&stop_sqe);
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

    let connect_res = conn.connect(ip, port)?;
    if connect_res < 0 {
        // No upstream — the client round-trip path still ran; report cleanly.
        conn.close()?;
        return Ok(None);
    }

    let send_res = conn.send(HTTP_REQ)?;
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
    let recv_res = conn.recv(&mut body)?;
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

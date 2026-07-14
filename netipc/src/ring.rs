//! Shared-memory data-ring ABI for the `netstack` bulk data path (Phase 4/5).
//!
//! The one-shot control path (opcodes in the crate root) rides
//! `channel::Message` byte payloads — fine for request/reply ops (DNS resolve,
//! TCP fetch), but a per-byte kernel↔daemon copy for streaming `send`/`recv`
//! would blow the < 2 µs IPC round-trip budget. This module defines the
//! **zero-copy alternative**: an io_uring-style pair of single-producer/
//! single-consumer (SPSC) rings living in one shared-memory region that the
//! kernel `SYS_SHM_CREATE`s and both sides map.
//!
//! # Model
//!
//! Two rings share one region:
//! - **SQ (submission queue)** — kernel is the *producer*, daemon the
//!   *consumer*. The kernel pushes [`Sqe`]s ("send these bytes on connection C",
//!   "recv into C", "connect", "close").
//! - **CQ (completion queue)** — daemon is the *producer*, kernel the
//!   *consumer*. The daemon pushes [`Cqe`]s echoing the submission's
//!   `user_data` with a result (bytes transferred, or a negative errno).
//!
//! Bulk payloads live in a separate **data area**; SQE/CQE carry only a
//! `(data_off, data_len)` window into it, so no message bytes are copied across
//! the channel — the kernel writes send-data straight into the shared data area
//! and the daemon reads it there (and vice-versa for recv).
//!
//! # Concurrency
//!
//! Each ring is SPSC, so it needs no locks: the producer owns the *tail* index,
//! the consumer owns the *head* index, and each reads the other's index. The
//! indices are free-running `u32`s (monotonic, wrapping); the physical slot is
//! `index & (entries - 1)` (entries is a power of two). Empty ⇔ `head == tail`;
//! full ⇔ `tail - head == entries`. The four indices sit on separate cache
//! lines in the header to avoid producer/consumer false sharing.
//!
//! This module is deliberately **pure and mapping-agnostic**: it defines the
//! byte layout, entry (de)serialization, and index arithmetic only. The atomic
//! acquire/release accesses to the shared indices (and the SHM mapping) live at
//! the kernel and daemon integration sites — keeping this crate `no_std`,
//! dependency-free, and `#![forbid(unsafe_code)]`.

// ---------------------------------------------------------------------------
// Region header
// ---------------------------------------------------------------------------

/// Magic in the region header: ASCII "NRNG" (netstack ring).
pub const RING_MAGIC: u32 = 0x4E52_4E47;

/// ABI version. Bump on any incompatible layout change.
pub const RING_VERSION: u32 = 1;

/// Cache-line size assumed for false-sharing separation of the indices.
pub const CACHE_LINE: usize = 64;

// Header field byte offsets. The scalar descriptors pack into the first cache
// line; the four hot indices then get a cache line each.
/// Offset of the `magic` field.
pub const OFF_MAGIC: usize = 0;
/// Offset of the `version` field.
pub const OFF_VERSION: usize = 4;
/// Offset of the `sq_entries` field (power of two).
pub const OFF_SQ_ENTRIES: usize = 8;
/// Offset of the `cq_entries` field (power of two).
pub const OFF_CQ_ENTRIES: usize = 12;
/// Offset of the `sqe_off` field (start of the SQE array).
pub const OFF_SQE_OFF: usize = 16;
/// Offset of the `cqe_off` field (start of the CQE array).
pub const OFF_CQE_OFF: usize = 20;
/// Offset of the `data_off` field (start of the bulk data area).
pub const OFF_DATA_OFF: usize = 24;
/// Offset of the `data_len` field (length of the bulk data area).
pub const OFF_DATA_LEN: usize = 28;

/// SQ head index (daemon = SQ consumer writes it). Own cache line.
pub const OFF_SQ_HEAD: usize = CACHE_LINE;
/// SQ tail index (kernel = SQ producer writes it). Own cache line.
pub const OFF_SQ_TAIL: usize = 2 * CACHE_LINE;
/// CQ head index (kernel = CQ consumer writes it). Own cache line.
pub const OFF_CQ_HEAD: usize = 3 * CACHE_LINE;
/// CQ tail index (daemon = CQ producer writes it). Own cache line.
pub const OFF_CQ_TAIL: usize = 4 * CACHE_LINE;

/// Total header length: five cache lines (scalars + four separated indices).
pub const HEADER_LEN: usize = 5 * CACHE_LINE;

/// Size of one submission-queue entry, in bytes.
pub const SQE_SIZE: usize = 32;
/// Size of one completion-queue entry, in bytes.
pub const CQE_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Submission / completion opcodes
// ---------------------------------------------------------------------------

/// No-op submission (used by tests / keepalive); completes with result 0.
pub const OP_NOP: u8 = 0x00;
/// Open a TCP connection to `aux` (`[ip:4][port_be:2]` in the low 6 bytes).
/// Completion `result` is `0` on success (connection established), or a negative
/// errno. If [`CONNECT_NONBLOCK`] is set in `aux` (bit 48, above the endpoint
/// bytes), the daemon starts the handshake and returns immediately: `0` if it
/// completed synchronously, else [`ERR_IN_PROGRESS`] (the caller then polls the
/// connection via [`OP_POLL`] for `POLL_WRITABLE`/`POLL_ERR`).
pub const OP_CONNECT: u8 = 0x01;
/// Send `data_len` bytes (at `data_off`) on connection `conn_id`.
/// Completion `result` is bytes accepted, or a negative errno.
pub const OP_SEND: u8 = 0x02;
/// Receive up to `data_len` bytes into the data window at `data_off` on
/// connection `conn_id`. Completion `result` is bytes received (0 = peer EOF),
/// or a negative errno.
pub const OP_RECV: u8 = 0x03;
/// Close connection `conn_id`. Completion `result` is 0, or a negative errno.
pub const OP_CLOSE: u8 = 0x04;
/// End the persistent ring session: after draining the remaining SQEs the daemon
/// closes any still-live connections, drops its per-session connection table, and
/// unmaps the ring. Used by the client to tear a session down explicitly (the
/// daemon otherwise keeps the mapping + connection state alive across separate
/// submissions so `OP_CONNECT` in one round and `OP_SEND`/`OP_RECV` in a later
/// round drive the *same* connection). Completes with `result = 0`.
pub const OP_STOP: u8 = 0x05;
/// Report the readiness of connection `conn_id` **without consuming any data**
/// (a non-destructive peek). The daemon drains any already-arrived frames into
/// the connection's receive buffer exactly once (like the non-blocking `OP_RECV`
/// probe) — which also drives an in-flight non-blocking connect's handshake to
/// completion — and then completes with a non-negative readiness bitmask built
/// from [`POLL_READABLE`] / [`POLL_WRITABLE`] / [`POLL_ERR`], or a negative errno
/// (`-1` = unknown connection). No bytes are moved into the data area and the
/// receive buffer is left intact, so a subsequent `OP_RECV` still returns the
/// same bytes. This is
/// what lets the kernel report an *honest* `POLLIN`/`POLLOUT` for a daemon-backed
/// socket instead of the old "always ready" placeholder — migration Phase 5,
/// closing part of the `D-NETSOCK-SYNC` parity gap.
pub const OP_POLL: u8 = 0x06;
/// Begin **passively listening** for inbound TCP connections. `conn_id` is the
/// caller-chosen id for the *listener* object (distinct from the ids of the
/// connections it later yields via [`OP_ACCEPT`]); the low 16 bits of `aux` carry
/// the local port to bind (host byte order). The daemon registers a listener on
/// that port so that inbound SYNs are demultiplexed to it (a passive open:
/// `LISTEN` → `SYN_RCVD` → `ESTABLISHED`), queuing each completed connection on
/// the listener's accept queue. Completion `result` is `0` on success, or a
/// negative errno (`-1` = no slot / port already bound). Server-socket support —
/// migration Phase 5, closing the listen/accept part of the `D-NETSOCK-SYNC`
/// parity gap.
pub const OP_LISTEN: u8 = 0x07;
/// **Accept** the next queued inbound connection from the listener named by
/// `conn_id`. The low 32 bits of `aux` carry the caller-chosen id to assign to the
/// newly-accepted connection (it becomes an ordinary connection id usable with
/// [`OP_SEND`]/[`OP_RECV`]/[`OP_POLL`]/[`OP_CLOSE`]). On success the peer address is
/// written to the data window at `data_off` as `[ip:4][port_be:2]` (6 bytes; the
/// window must be at least that large) and completion `result` is `0`. If the
/// accept queue is empty the daemon completes with [`ERR_WOULD_BLOCK`] (the
/// listener socket is treated as non-blocking at the ring layer; the kernel blocks
/// by re-submitting / polling). `-1` = unknown listener or data window too small.
pub const OP_ACCEPT: u8 = 0x08;

// ---------------------------------------------------------------------------
// Op flags (carried in [`Sqe::aux`]) and result sentinels
// ---------------------------------------------------------------------------

/// [`OP_RECV`] `aux` flag: perform a **non-blocking** receive.
///
/// When set, the daemon drains any already-arrived frames exactly once and then
/// returns immediately: if the connection has buffered in-order bytes (or has hit
/// EOF) it completes normally (bytes received, or `0` on EOF); otherwise — no data
/// yet, stream still open — it completes with [`ERR_WOULD_BLOCK`] instead of
/// polling for the full receive deadline. When clear, `OP_RECV` blocks (polls) up
/// to the daemon's receive deadline as before.
///
/// This is what lets the kernel honour `O_NONBLOCK` on a daemon-backed stream
/// socket (`recv`/`read` return `EAGAIN` rather than stalling the caller) —
/// migration Phase 5, closing part of the `D-NETSOCK-SYNC` parity gap.
pub const RECV_NONBLOCK: u64 = 1 << 0;

/// [`OP_SEND`] `aux` flag: perform a **non-blocking** send.
///
/// The daemon is a single-outstanding-segment sender: only one unacknowledged
/// segment may be in flight at a time (its retransmit buffer holds exactly one
/// segment). When a prior segment is still unacknowledged the send window is
/// *full*. With this flag set, the daemon drains any pending ACKs exactly once
/// and, if the window is still full, completes with [`ERR_WOULD_BLOCK`] instead
/// of waiting for the peer to ACK — this is how the kernel honours `O_NONBLOCK`
/// on `send`/`write`. When clear, `OP_SEND` blocks (polls) until the window
/// drains (or the send deadline elapses) as before.
///
/// Distinct bit from [`RECV_NONBLOCK`] purely for clarity; the two never share
/// an SQE (a send and a recv are different opcodes), but keeping them on
/// separate bits avoids any accidental aliasing if flags are ever combined.
pub const SEND_NONBLOCK: u64 = 1 << 1;

/// [`OP_CONNECT`] `aux` flag: perform a **non-blocking** connect.
///
/// The endpoint is packed into the low 48 bits of `aux` (`[ip:4][port_be:2]`, see
/// [`Sqe::pack_endpoint`]), leaving the high 16 bits free for flags. This flag
/// lives in bit 48. When set, the daemon transmits the SYN, registers the
/// connection in the `SYN_SENT` state, and completes immediately with either `0`
/// (handshake already finished) or [`ERR_IN_PROGRESS`]; the handshake is then
/// driven to completion by the RX pump on subsequent [`OP_POLL`] / [`OP_RECV`]
/// submissions. When clear, `OP_CONNECT` blocks until the handshake resolves (or
/// fails) as before.
///
/// This is what lets the kernel honour `O_NONBLOCK` on `connect(2)` for a
/// daemon-backed stream socket (returning `EINPROGRESS`, then `POLLOUT` +
/// `getsockopt(SO_ERROR)`) — migration Phase 5, closing part of the
/// `D-NETSOCK-SYNC` parity gap.
pub const CONNECT_NONBLOCK: u64 = 1 << 48;

/// Completion `result` sentinel: the operation would have blocked and the SQE
/// requested non-blocking behaviour (e.g. [`OP_RECV`] with [`RECV_NONBLOCK`] set
/// and no data available). Numerically mirrors Linux `-EAGAIN`; the kernel client
/// maps it to `KernelError::WouldBlock`, which the `recv(2)`/`read(2)` path
/// reports as the `EAGAIN` errno. Distinct from `0` (peer EOF) and from the
/// generic `-1` transport error so the three cases never alias.
pub const ERR_WOULD_BLOCK: i32 = -11;

/// Completion `result` sentinel: a non-blocking [`OP_CONNECT`]
/// ([`CONNECT_NONBLOCK`]) has started the handshake but it is not yet complete.
/// Numerically mirrors Linux `-EINPROGRESS`; the kernel maps it to
/// `KernelError::InProgress` → the `connect(2)` `EINPROGRESS` errno. Distinct
/// from `0` (established) and `-1` (could not start / no slot).
pub const ERR_IN_PROGRESS: i32 = -115;

/// [`OP_POLL`] readiness bit: the connection is **readable** — it has buffered
/// in-order bytes waiting, or the peer has closed (so a `recv` would return `0`
/// / EOF promptly). Mirrors the sense of Linux `POLLIN`.
pub const POLL_READABLE: i32 = 1 << 0;

/// [`OP_POLL`] readiness bit: the connection is **writable** — it is connected
/// and has room to accept at least some bytes for sending. Mirrors the sense of
/// Linux `POLLOUT`. Also set once a non-blocking connect has *resolved* (whether
/// it succeeded or failed) so a `POLLOUT`-waiting `connect(2)` wakes up.
pub const POLL_WRITABLE: i32 = 1 << 1;

/// [`OP_POLL`] readiness bit: the connection has an **error** condition — most
/// commonly a non-blocking connect that was refused (RST) or timed out. Mirrors
/// the sense of Linux `POLLERR`; the kernel surfaces the concrete error via
/// `getsockopt(SO_ERROR)`. A `SYN_SENT` connection whose handshake is still in
/// flight reports *neither* [`POLL_WRITABLE`] nor this bit (not yet ready).
pub const POLL_ERR: i32 = 1 << 2;

// ---------------------------------------------------------------------------
// Entry structs
// ---------------------------------------------------------------------------

/// A submission-queue entry (kernel → daemon). 32 bytes on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sqe {
    /// Operation (`OP_*`).
    pub op: u8,
    /// Target connection id (meaning is op-specific; ignored by `OP_CONNECT`).
    pub conn_id: u32,
    /// Offset into the data area for this op's payload window.
    pub data_off: u32,
    /// Length of the data window.
    pub data_len: u32,
    /// Opaque token echoed verbatim in the matching [`Cqe::user_data`].
    pub user_data: u64,
    /// Auxiliary operand (e.g. `[ip:4][port_be:2]` for `OP_CONNECT`).
    pub aux: u64,
}

impl Sqe {
    /// Serialize into a fixed 32-byte little-endian slot.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; SQE_SIZE] {
        let mut b = [0u8; SQE_SIZE];
        b[0] = self.op;
        // bytes 1..4 reserved (padding)
        b[4..8].copy_from_slice(&self.conn_id.to_le_bytes());
        b[8..12].copy_from_slice(&self.data_off.to_le_bytes());
        b[12..16].copy_from_slice(&self.data_len.to_le_bytes());
        b[16..24].copy_from_slice(&self.user_data.to_le_bytes());
        b[24..32].copy_from_slice(&self.aux.to_le_bytes());
        b
    }

    /// Deserialize from a 32-byte slot. Returns `None` if `b` is too short.
    #[must_use]
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let s = b.get(..SQE_SIZE)?;
        Some(Sqe {
            op: s[0],
            conn_id: u32::from_le_bytes([s[4], s[5], s[6], s[7]]),
            data_off: u32::from_le_bytes([s[8], s[9], s[10], s[11]]),
            data_len: u32::from_le_bytes([s[12], s[13], s[14], s[15]]),
            user_data: u64::from_le_bytes([s[16], s[17], s[18], s[19], s[20], s[21], s[22], s[23]]),
            aux: u64::from_le_bytes([s[24], s[25], s[26], s[27], s[28], s[29], s[30], s[31]]),
        })
    }

    /// Pack an `[ip:4][port_be:2]` endpoint into [`Sqe::aux`] for `OP_CONNECT`.
    #[must_use]
    pub fn pack_endpoint(ip: &[u8; 4], port: u16) -> u64 {
        let p = port.to_be_bytes();
        u64::from_le_bytes([ip[0], ip[1], ip[2], ip[3], p[0], p[1], 0, 0])
    }

    /// Unpack an `[ip:4][port_be:2]` endpoint previously packed with
    /// [`Sqe::pack_endpoint`].
    #[must_use]
    pub fn unpack_endpoint(aux: u64) -> ([u8; 4], u16) {
        let b = aux.to_le_bytes();
        ([b[0], b[1], b[2], b[3]], u16::from_be_bytes([b[4], b[5]]))
    }
}

/// A completion-queue entry (daemon → kernel). 16 bytes on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cqe {
    /// Echo of the submission's [`Sqe::user_data`].
    pub user_data: u64,
    /// Op result: bytes transferred / new conn id (≥ 0), or a negative errno.
    pub result: i32,
    /// Op-specific completion flags (reserved; 0 for now).
    pub flags: u32,
}

impl Cqe {
    /// Serialize into a fixed 16-byte little-endian slot.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; CQE_SIZE] {
        let mut b = [0u8; CQE_SIZE];
        b[0..8].copy_from_slice(&self.user_data.to_le_bytes());
        b[8..12].copy_from_slice(&self.result.to_le_bytes());
        b[12..16].copy_from_slice(&self.flags.to_le_bytes());
        b
    }

    /// Deserialize from a 16-byte slot. Returns `None` if `b` is too short.
    #[must_use]
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let s = b.get(..CQE_SIZE)?;
        Some(Cqe {
            user_data: u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]),
            result: i32::from_le_bytes([s[8], s[9], s[10], s[11]]),
            flags: u32::from_le_bytes([s[12], s[13], s[14], s[15]]),
        })
    }
}

// ---------------------------------------------------------------------------
// SPSC index arithmetic (pure)
// ---------------------------------------------------------------------------
//
// Indices are free-running u32 counters. Callers load/store them atomically on
// the shared header; these helpers do only the (overflow-safe) math.

/// Physical slot for a free-running `index` in a ring of `entries` slots.
/// `entries` MUST be a power of two (enforced by [`is_power_of_two`]).
#[must_use]
pub fn slot(index: u32, entries: u32) -> u32 {
    index & entries.wrapping_sub(1)
}

/// Number of occupied slots given the producer `tail` and consumer `head`.
/// Wrapping subtraction makes this correct across the u32 wrap boundary.
#[must_use]
pub fn used(head: u32, tail: u32) -> u32 {
    tail.wrapping_sub(head)
}

/// True if the ring is empty (`head == tail`).
#[must_use]
pub fn is_empty(head: u32, tail: u32) -> bool {
    head == tail
}

/// True if the ring is full (`entries` slots occupied).
#[must_use]
pub fn is_full(head: u32, tail: u32, entries: u32) -> bool {
    used(head, tail) >= entries
}

/// Free slots available to the producer.
#[must_use]
pub fn free(head: u32, tail: u32, entries: u32) -> u32 {
    entries.wrapping_sub(used(head, tail))
}

/// True if `n` is a power of two (and non-zero) — required for the `& (n-1)`
/// slot mask to be a valid modulo.
#[must_use]
pub fn is_power_of_two(n: u32) -> bool {
    n != 0 && (n & n.wrapping_sub(1)) == 0
}

// ---------------------------------------------------------------------------
// Region sizing / layout helpers
// ---------------------------------------------------------------------------

/// Byte offset of the SQE array (immediately after the header).
#[must_use]
pub const fn sqe_array_off() -> usize {
    HEADER_LEN
}

/// Byte offset of the CQE array (after the SQE array).
#[must_use]
pub fn cqe_array_off(sq_entries: u32) -> usize {
    HEADER_LEN + (sq_entries as usize) * SQE_SIZE
}

/// Byte offset of the bulk data area (after the CQE array).
#[must_use]
pub fn data_area_off(sq_entries: u32, cq_entries: u32) -> usize {
    cqe_array_off(sq_entries) + (cq_entries as usize) * CQE_SIZE
}

/// Total shared-memory region size needed for the given ring geometry.
#[must_use]
pub fn region_size(sq_entries: u32, cq_entries: u32, data_len: u32) -> usize {
    data_area_off(sq_entries, cq_entries) + data_len as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqe_round_trip() {
        let sqe = Sqe {
            op: OP_SEND,
            conn_id: 0xDEAD_BEEF,
            data_off: 0x1000,
            data_len: 1460,
            user_data: 0x0102_0304_0506_0708,
            aux: Sqe::pack_endpoint(&[93, 184, 216, 34], 443),
        };
        let bytes = sqe.to_bytes();
        assert_eq!(bytes.len(), SQE_SIZE);
        let back = Sqe::from_bytes(&bytes).unwrap();
        assert_eq!(sqe, back);
        let (ip, port) = Sqe::unpack_endpoint(back.aux);
        assert_eq!(ip, [93, 184, 216, 34]);
        assert_eq!(port, 443);
    }

    #[test]
    fn sqe_from_short_is_none() {
        assert!(Sqe::from_bytes(&[0u8; SQE_SIZE - 1]).is_none());
    }

    #[test]
    fn recv_nonblock_flag_round_trips_and_is_distinct() {
        // The non-blocking flag rides in `aux`, orthogonal to the endpoint
        // packing used by OP_CONNECT, and must survive serialization.
        let sqe = Sqe {
            op: OP_RECV,
            conn_id: 1,
            data_off: 1024,
            data_len: 512,
            user_data: 0x4e53_434c_0000_0007,
            aux: RECV_NONBLOCK,
        };
        let back = Sqe::from_bytes(&sqe.to_bytes()).unwrap();
        assert_eq!(back.aux & RECV_NONBLOCK, RECV_NONBLOCK);
        assert_eq!(sqe, back);
        // A cleared flag must not read as set (blocking recv path).
        let blocking = Sqe { aux: 0, ..sqe };
        assert_eq!(blocking.aux & RECV_NONBLOCK, 0);
        // The would-block sentinel must not collide with EOF or the generic error.
        assert_ne!(ERR_WOULD_BLOCK, 0);
        assert_ne!(ERR_WOULD_BLOCK, -1);
    }

    #[test]
    fn send_nonblock_flag_round_trips_and_is_distinct() {
        // The non-blocking send flag rides in `aux` on an OP_SEND SQE and must
        // survive serialization without disturbing the data window fields.
        let sqe = Sqe {
            op: OP_SEND,
            conn_id: 2,
            data_off: 2048,
            data_len: 256,
            user_data: 0x4e53_434c_0000_000a,
            aux: SEND_NONBLOCK,
        };
        let back = Sqe::from_bytes(&sqe.to_bytes()).unwrap();
        assert_eq!(back.aux & SEND_NONBLOCK, SEND_NONBLOCK);
        assert_eq!(sqe, back);
        // A cleared flag must not read as set (blocking send path).
        let blocking = Sqe { aux: 0, ..sqe };
        assert_eq!(blocking.aux & SEND_NONBLOCK, 0);
        // SEND_NONBLOCK and RECV_NONBLOCK occupy different bits.
        assert_ne!(SEND_NONBLOCK, RECV_NONBLOCK);
        assert_eq!(SEND_NONBLOCK & RECV_NONBLOCK, 0);
        // The would-block sentinel it produces is distinct from EOF / generic error.
        assert_ne!(ERR_WOULD_BLOCK, 0);
        assert_ne!(ERR_WOULD_BLOCK, -1);
    }

    #[test]
    fn poll_opcode_and_readiness_bits_are_distinct() {
        // OP_POLL must not collide with any other opcode.
        for other in [OP_NOP, OP_CONNECT, OP_SEND, OP_RECV, OP_CLOSE, OP_STOP] {
            assert_ne!(OP_POLL, other, "OP_POLL aliases another opcode");
        }
        // An OP_POLL SQE round-trips through serialization unchanged.
        let sqe = Sqe {
            op: OP_POLL,
            conn_id: 1,
            user_data: 0x4e53_434c_0000_0009,
            ..Sqe::default()
        };
        assert_eq!(Sqe::from_bytes(&sqe.to_bytes()).unwrap(), sqe);
        // Readiness bits are single, distinct, non-overlapping, and positive so a
        // combined mask never looks like a negative errno.
        assert_ne!(POLL_READABLE, POLL_WRITABLE);
        assert_eq!(POLL_READABLE & POLL_WRITABLE, 0);
        assert!(POLL_READABLE > 0 && POLL_WRITABLE > 0);
        assert!(POLL_READABLE | POLL_WRITABLE > 0);
        // A daemon readiness bitmask is non-negative, unlike the -1 error / EAGAIN.
        assert_ne!(POLL_READABLE | POLL_WRITABLE, ERR_WOULD_BLOCK);
        assert_ne!(POLL_READABLE | POLL_WRITABLE, -1);
    }

    #[test]
    fn connect_nonblock_flag_and_progress_sentinel_are_distinct() {
        // The CONNECT_NONBLOCK flag lives above the packed endpoint bits, so it
        // must not disturb the endpoint round-trip and must survive serialization.
        let ip = [93, 184, 216, 34];
        let port = 443u16;
        let aux = Sqe::pack_endpoint(&ip, port) | CONNECT_NONBLOCK;
        let sqe = Sqe { op: OP_CONNECT, conn_id: 3, user_data: 7, aux, ..Sqe::default() };
        let back = Sqe::from_bytes(&sqe.to_bytes()).unwrap();
        assert_eq!(back.aux & CONNECT_NONBLOCK, CONNECT_NONBLOCK);
        // The endpoint still unpacks correctly with the flag ORed in.
        let (uip, uport) = Sqe::unpack_endpoint(back.aux);
        assert_eq!(uip, ip);
        assert_eq!(uport, port);
        // The flag occupies a bit above the 48-bit endpoint window.
        assert_eq!(CONNECT_NONBLOCK & 0x0000_FFFF_FFFF_FFFF, 0);
        // The in-progress sentinel is distinct from all other result codes.
        assert_ne!(ERR_IN_PROGRESS, 0);
        assert_ne!(ERR_IN_PROGRESS, -1);
        assert_ne!(ERR_IN_PROGRESS, ERR_WOULD_BLOCK);
        // POLL_ERR is a distinct, positive, non-overlapping readiness bit.
        assert!(POLL_ERR > 0);
        assert_eq!(POLL_ERR & (POLL_READABLE | POLL_WRITABLE), 0);
    }

    #[test]
    fn listen_accept_opcodes_round_trip_and_are_distinct() {
        // OP_LISTEN / OP_ACCEPT must not collide with any prior opcode or each other.
        let existing = [OP_NOP, OP_CONNECT, OP_SEND, OP_RECV, OP_CLOSE, OP_STOP, OP_POLL];
        for other in existing {
            assert_ne!(OP_LISTEN, other, "OP_LISTEN aliases another opcode");
            assert_ne!(OP_ACCEPT, other, "OP_ACCEPT aliases another opcode");
        }
        assert_ne!(OP_LISTEN, OP_ACCEPT);

        // An OP_LISTEN SQE carries the local port in the low 16 bits of `aux`.
        let port = 8080u16;
        let listen = Sqe {
            op: OP_LISTEN,
            conn_id: 100,
            user_data: 0x4e53_434c_0000_000b,
            aux: u64::from(port),
            ..Sqe::default()
        };
        let back = Sqe::from_bytes(&listen.to_bytes()).unwrap();
        assert_eq!(listen, back);
        assert_eq!((back.aux & 0xFFFF) as u16, port);

        // An OP_ACCEPT SQE carries the desired new conn id in the low 32 bits of
        // `aux` and points its data window where the peer address will be written.
        let new_id = 101u32;
        let accept = Sqe {
            op: OP_ACCEPT,
            conn_id: 100,
            data_off: 4096,
            data_len: 6,
            user_data: 0x4e53_434c_0000_000c,
            aux: u64::from(new_id),
        };
        let ab = Sqe::from_bytes(&accept.to_bytes()).unwrap();
        assert_eq!(accept, ab);
        assert_eq!((ab.aux & 0xFFFF_FFFF) as u32, new_id);
        // An empty accept queue completes with the would-block sentinel, distinct
        // from the generic -1 (unknown listener / window too small) and success 0.
        assert_ne!(ERR_WOULD_BLOCK, 0);
        assert_ne!(ERR_WOULD_BLOCK, -1);
    }

    #[test]
    fn cqe_round_trip() {
        let cqe = Cqe { user_data: 0xAABB_CCDD_1122_3344, result: -11, flags: 0 };
        let bytes = cqe.to_bytes();
        assert_eq!(bytes.len(), CQE_SIZE);
        let back = Cqe::from_bytes(&bytes).unwrap();
        assert_eq!(cqe, back);
        assert_eq!(back.result, -11);
    }

    #[test]
    fn cqe_from_short_is_none() {
        assert!(Cqe::from_bytes(&[0u8; CQE_SIZE - 1]).is_none());
    }

    #[test]
    fn spsc_empty_full_wrap() {
        let entries = 8u32;
        assert!(is_power_of_two(entries));
        // Fresh ring: empty.
        let (mut head, mut tail) = (0u32, 0u32);
        assert!(is_empty(head, tail));
        assert!(!is_full(head, tail, entries));
        assert_eq!(free(head, tail, entries), entries);
        // Fill it.
        for _ in 0..entries {
            assert!(!is_full(head, tail, entries));
            tail = tail.wrapping_add(1);
        }
        assert!(is_full(head, tail, entries));
        assert_eq!(used(head, tail), entries);
        assert_eq!(free(head, tail, entries), 0);
        // Drain it.
        for _ in 0..entries {
            assert!(!is_empty(head, tail));
            head = head.wrapping_add(1);
        }
        assert!(is_empty(head, tail));
    }

    #[test]
    fn spsc_wraps_across_u32_boundary() {
        let entries = 4u32;
        // Start near the u32 max so the counters wrap mid-test.
        let (mut head, mut tail) = (u32::MAX - 1, u32::MAX - 1);
        assert!(is_empty(head, tail));
        for i in 0..entries {
            tail = tail.wrapping_add(1);
            assert_eq!(used(head, tail), i + 1);
        }
        assert!(is_full(head, tail, entries));
        // Slot indices stay in range across the wrap.
        for idx in [u32::MAX - 1, u32::MAX, 0, 1] {
            assert!(slot(idx, entries) < entries);
        }
        // Drain.
        for _ in 0..entries {
            head = head.wrapping_add(1);
        }
        assert!(is_empty(head, tail));
    }

    #[test]
    fn slot_is_modulo_for_pow2() {
        let entries = 16u32;
        for idx in 0..64u32 {
            assert_eq!(slot(idx, entries), idx % entries);
        }
    }

    #[test]
    fn power_of_two_detection() {
        for p in [1u32, 2, 4, 8, 16, 1024, 1 << 20] {
            assert!(is_power_of_two(p));
        }
        for np in [0u32, 3, 5, 6, 12, 100, 1000] {
            assert!(!is_power_of_two(np));
        }
    }

    #[test]
    fn region_layout_is_contiguous_and_sized() {
        let (sq, cq, data) = (64u32, 64u32, 65536u32);
        assert_eq!(sqe_array_off(), HEADER_LEN);
        assert_eq!(cqe_array_off(sq), HEADER_LEN + 64 * SQE_SIZE);
        assert_eq!(data_area_off(sq, cq), HEADER_LEN + 64 * SQE_SIZE + 64 * CQE_SIZE);
        assert_eq!(
            region_size(sq, cq, data),
            HEADER_LEN + 64 * SQE_SIZE + 64 * CQE_SIZE + 65536
        );
    }

    #[test]
    fn index_fields_are_on_separate_cache_lines() {
        // Each hot index must occupy its own cache line to avoid producer/
        // consumer false sharing.
        let offs = [OFF_SQ_HEAD, OFF_SQ_TAIL, OFF_CQ_HEAD, OFF_CQ_TAIL];
        for (i, &a) in offs.iter().enumerate() {
            assert_eq!(a % CACHE_LINE, 0, "index {i} not cache-line aligned");
            for &b in &offs[i + 1..] {
                assert!(a.abs_diff(b) >= CACHE_LINE, "indices share a cache line");
            }
        }
        // Scalars fit within the first cache line.
        assert!(OFF_DATA_LEN + 4 <= CACHE_LINE);
    }
}

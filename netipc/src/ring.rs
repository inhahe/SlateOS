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
/// Completion `result` is the new connection id, or a negative errno.
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

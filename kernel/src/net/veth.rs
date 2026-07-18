//! Virtual Ethernet (veth) pairs — connected virtual network interfaces.
//!
//! Veth pairs provide isolated virtual links between network namespaces.
//! Each pair consists of two endpoints (A and B): a frame sent on one
//! endpoint's TX path arrives on the other endpoint's RX queue, and
//! vice versa.  This is the primary mechanism for connecting containers
//! to the host network or to each other.
//!
//! ## Design
//!
//! Based on Linux's veth implementation (`drivers/net/veth.c`):
//! - Pairs are created atomically (both endpoints or neither)
//! - Each endpoint has its own MAC address (locally-administered)
//! - Endpoints can be moved between network namespaces
//! - Frame delivery is synchronous: `send()` directly enqueues on the
//!   peer's RX queue (no interrupt or DMA simulation needed)
//! - Per-endpoint bounded RX queue prevents memory exhaustion
//!
//! ## Integration
//!
//! - `netns.rs`: namespaces reference veth endpoints by pair/end ID
//! - `interface.rs`: `ns_info()` returns the veth endpoint's MAC when
//!   a namespace owns a veth end
//! - `net::poll()`: calls `poll_all()` to drain veth RX queues into
//!   the protocol stack via `ethernet::process_frame()`
//!
//! ## References
//!
//! - Linux `drivers/net/veth.c`
//! - `man 4 veth`

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::netns::NetNsId;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of veth pairs.
const MAX_VETH_PAIRS: usize = 32;

/// Maximum frames buffered per endpoint before drops.
///
/// Sized for typical container networking: a burst of 256 frames at
/// 1500 bytes each is ~384 KiB — acceptable for kernel memory.
const VETH_QUEUE_DEPTH: usize = 256;

/// Standard Ethernet MTU.
const VETH_MTU: usize = 1500;

/// Ethernet header size (dst + src + ethertype).
const ETH_HEADER_SIZE: usize = 14;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Identifies which end of a veth pair (A or B).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VethEndId {
    /// The "A" side of the pair.
    A,
    /// The "B" side of the pair.
    B,
}

/// Unique identifier for a veth pair (index into the global table).
pub type VethPairId = usize;

/// One endpoint of a veth pair.
struct VethEnd {
    /// MAC address for this endpoint (locally-administered).
    mac: [u8; 6],
    /// Network namespace this endpoint belongs to.
    ns_id: NetNsId,
    /// Whether this endpoint is administratively up.
    up: bool,
    /// Whether this endpoint is attached to an L2 bridge (a container-network
    /// bridge port). When set, [`poll_all`] does **not** drain this endpoint's
    /// RX queue into the global protocol stack — the owning bridge drains it
    /// instead (see `net::bridge::forward`). This is how a container attached
    /// to a user-defined network has its host-end frames routed to same-network
    /// peers at layer 2 rather than terminating at the host stack.
    bridged: bool,
    /// Inbound frame queue (frames sent by the peer land here).
    rx_queue: VecDeque<Vec<u8>>,
    /// Total bytes transmitted (to peer).
    tx_bytes: u64,
    /// Total bytes received (from peer).
    rx_bytes: u64,
    /// Total frames transmitted.
    tx_packets: u64,
    /// Total frames received.
    rx_packets: u64,
    /// Frames dropped because the RX queue was full.
    rx_drops: u64,
}

impl VethEnd {
    /// Create a new endpoint with the given MAC and namespace.
    fn new(mac: [u8; 6], ns_id: NetNsId) -> Self {
        Self {
            mac,
            ns_id,
            up: false,
            bridged: false,
            rx_queue: VecDeque::with_capacity(16),
            tx_bytes: 0,
            rx_bytes: 0,
            tx_packets: 0,
            rx_packets: 0,
            rx_drops: 0,
        }
    }

    /// Enqueue a frame into this endpoint's RX buffer.
    ///
    /// Returns `Err(ChannelFull)` if the queue is at capacity.
    fn enqueue_rx(&mut self, frame: Vec<u8>) -> KernelResult<()> {
        if self.rx_queue.len() >= VETH_QUEUE_DEPTH {
            self.rx_drops = self.rx_drops.saturating_add(1);
            return Err(KernelError::ChannelFull);
        }
        let len = frame.len() as u64;
        self.rx_queue.push_back(frame);
        self.rx_bytes = self.rx_bytes.saturating_add(len);
        self.rx_packets = self.rx_packets.saturating_add(1);
        Ok(())
    }

    /// Dequeue the next frame from the RX buffer.
    fn dequeue_rx(&mut self) -> Option<Vec<u8>> {
        self.rx_queue.pop_front()
    }

    /// Number of frames waiting in the RX queue.
    fn rx_pending(&self) -> usize {
        self.rx_queue.len()
    }
}

/// A veth pair: two connected virtual Ethernet endpoints.
struct VethPair {
    /// Whether this pair slot is active.
    active: bool,
    /// Endpoint A.
    end_a: VethEnd,
    /// Endpoint B.
    end_b: VethEnd,
}

impl VethPair {
    /// An empty/inactive pair (used to initialise the table).
    fn empty() -> Self {
        Self {
            active: false,
            end_a: VethEnd::new([0; 6], 0),
            end_b: VethEnd::new([0; 6], 0),
        }
    }

    /// Get a reference to the specified end.
    fn end_ref(&self, id: VethEndId) -> &VethEnd {
        match id {
            VethEndId::A => &self.end_a,
            VethEndId::B => &self.end_b,
        }
    }

    /// Get a mutable reference to the specified end.
    fn end_mut(&mut self, id: VethEndId) -> &mut VethEnd {
        match id {
            VethEndId::A => &mut self.end_a,
            VethEndId::B => &mut self.end_b,
        }
    }

    /// Get the peer of the given end.
    fn peer_id(id: VethEndId) -> VethEndId {
        match id {
            VethEndId::A => VethEndId::B,
            VethEndId::B => VethEndId::A,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct VethTable {
    pairs: Vec<VethPair>,
    /// Monotonic counter for generating unique MAC addresses.
    mac_counter: u32,
}

impl VethTable {
    fn new() -> Self {
        let mut pairs = Vec::with_capacity(MAX_VETH_PAIRS);
        for _ in 0..MAX_VETH_PAIRS {
            pairs.push(VethPair::empty());
        }
        Self {
            pairs,
            mac_counter: 0,
        }
    }
}

static TABLE: Mutex<Option<VethTable>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the veth subsystem.
///
/// Must be called after the heap is available (needs `Vec`/`VecDeque`).
pub fn init() {
    let mut table = TABLE.lock();
    *table = Some(VethTable::new());
    crate::serial_println!("[veth] Initialized ({} max pairs)", MAX_VETH_PAIRS);
}

/// Check if the veth subsystem is initialized.
#[must_use]
pub fn is_initialized() -> bool {
    TABLE.lock().is_some()
}

fn with_table<F, R>(f: F) -> R
where
    F: FnOnce(&mut VethTable) -> R,
{
    let mut guard = TABLE.lock();
    let table = guard.as_mut().expect("[veth] not initialized");
    f(table)
}

fn with_table_ref<F, R>(f: F) -> R
where
    F: FnOnce(&VethTable) -> R,
{
    let guard = TABLE.lock();
    let table = guard.as_ref().expect("[veth] not initialized");
    f(table)
}

// ---------------------------------------------------------------------------
// MAC address generation
// ---------------------------------------------------------------------------

/// Generate a locally-administered unicast MAC address for a veth endpoint.
///
/// Format: `02:FE:xx:pp:ss:SS`
/// - `02` = locally-administered unicast (bit 1 set, bit 0 clear)
/// - `FE` = "FE" for "virtual FErry"
/// - `xx` = end identifier (0A for A, 0B for B)
/// - `pp` = pair index (low byte)
/// - `ss:SS` = sequence counter (low 16 bits)
#[allow(clippy::cast_possible_truncation)]
fn generate_mac(pair_idx: usize, end: VethEndId, seq: u32) -> [u8; 6] {
    let end_byte: u8 = match end {
        VethEndId::A => 0x0A,
        VethEndId::B => 0x0B,
    };
    [
        0x02,                          // Locally administered, unicast
        0xFE,                          // Identifier byte
        end_byte,                      // End identifier
        (pair_idx & 0xFF) as u8,       // Pair index low byte
        (seq & 0xFF) as u8,            // Sequence low byte
        ((seq >> 8) & 0xFF) as u8,     // Sequence high byte
    ]
}

// ---------------------------------------------------------------------------
// Public API: pair lifecycle
// ---------------------------------------------------------------------------

/// Create a new veth pair.
///
/// Both endpoints start in the root namespace (ID 0) and in the "down"
/// state.  Use [`set_up`] to bring them up and [`move_end`] to assign
/// an endpoint to a different namespace.
///
/// Returns the pair ID on success.
///
/// # Errors
///
/// - [`KernelError::ResourceExhausted`] if all pair slots are in use.
pub fn create_pair() -> KernelResult<VethPairId> {
    with_table(|table| {
        // Find an empty slot.
        let idx = table.pairs.iter().position(|p| !p.active)
            .ok_or(KernelError::ResourceExhausted)?;

        let seq_a = table.mac_counter;
        table.mac_counter = table.mac_counter.wrapping_add(1);
        let seq_b = table.mac_counter;
        table.mac_counter = table.mac_counter.wrapping_add(1);

        let mac_a = generate_mac(idx, VethEndId::A, seq_a);
        let mac_b = generate_mac(idx, VethEndId::B, seq_b);

        let pair = &mut table.pairs[idx];
        pair.active = true;
        pair.end_a = VethEnd::new(mac_a, crate::netns::ROOT_NS);
        pair.end_b = VethEnd::new(mac_b, crate::netns::ROOT_NS);

        Ok(idx)
    })
}

/// Destroy a veth pair, releasing both endpoints.
///
/// Any frames remaining in the RX queues are discarded.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the pair ID is invalid or
///   the pair is not active.
pub fn destroy_pair(id: VethPairId) -> KernelResult<()> {
    with_table(|table| {
        let pair = table.pairs.get_mut(id)
            .ok_or(KernelError::InvalidArgument)?;
        if !pair.active {
            return Err(KernelError::InvalidArgument);
        }

        // Clear queues to release memory.
        pair.end_a.rx_queue.clear();
        pair.end_b.rx_queue.clear();
        pair.active = false;

        Ok(())
    })
}

/// Move a veth endpoint to a different network namespace.
///
/// The endpoint must be in the "down" state to be moved (following
/// Linux convention: `ip link set dev vethX netns <ns>`).
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the pair/end is invalid or
///   the target namespace does not exist.
/// - [`KernelError::DeviceBusy`] if the endpoint is up.
pub fn move_end(pair_id: VethPairId, end: VethEndId, ns_id: NetNsId) -> KernelResult<()> {
    // Verify the target namespace exists (without holding the veth lock).
    if !crate::netns::exists(ns_id) {
        return Err(KernelError::InvalidArgument);
    }

    with_table(|table| {
        let pair = table.pairs.get_mut(pair_id)
            .ok_or(KernelError::InvalidArgument)?;
        if !pair.active {
            return Err(KernelError::InvalidArgument);
        }

        let endpoint = pair.end_mut(end);
        if endpoint.up {
            return Err(KernelError::DeviceBusy);
        }

        endpoint.ns_id = ns_id;
        Ok(())
    })
}

/// Set an endpoint's administrative state (up or down).
///
/// An endpoint must be up to send or receive frames.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the pair/end is invalid.
pub fn set_up(pair_id: VethPairId, end: VethEndId, up: bool) -> KernelResult<()> {
    with_table(|table| {
        let pair = table.pairs.get_mut(pair_id)
            .ok_or(KernelError::InvalidArgument)?;
        if !pair.active {
            return Err(KernelError::InvalidArgument);
        }
        pair.end_mut(end).up = up;
        Ok(())
    })
}

/// Mark (or unmark) an endpoint as attached to an L2 bridge.
///
/// A bridged endpoint is skipped by [`poll_all`]: its RX frames are drained and
/// switched by the owning bridge (`net::bridge::forward`) rather than delivered
/// to the global host protocol stack. Used when a container joins a
/// user-defined network so its host-end participates in L2 forwarding to
/// same-network peers.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the pair/end is invalid or inactive.
pub fn set_bridged(pair_id: VethPairId, end: VethEndId, bridged: bool) -> KernelResult<()> {
    with_table(|table| {
        let pair = table.pairs.get_mut(pair_id)
            .ok_or(KernelError::InvalidArgument)?;
        if !pair.active {
            return Err(KernelError::InvalidArgument);
        }
        pair.end_mut(end).bridged = bridged;
        Ok(())
    })
}

/// Whether an endpoint is currently marked bridged.
///
/// Returns `false` for an invalid or inactive pair.
#[must_use]
pub fn is_bridged(pair_id: VethPairId, end: VethEndId) -> bool {
    with_table_ref(|table| {
        table
            .pairs
            .get(pair_id)
            .is_some_and(|p| p.active && p.end_ref(end).bridged)
    })
}

// ---------------------------------------------------------------------------
// Public API: frame I/O
// ---------------------------------------------------------------------------

/// Send a frame from the specified endpoint to its peer.
///
/// The frame is an Ethernet frame (header + payload).  It is enqueued
/// directly into the peer endpoint's RX queue — there is no wire or
/// DMA simulation.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the pair/end is invalid.
/// - [`KernelError::NotSupported`] if the sending endpoint is down.
/// - [`KernelError::ChannelFull`] if the peer's RX queue is at capacity
///   (the frame is dropped; counted in the peer's `rx_drops`).
#[allow(clippy::arithmetic_side_effects)]
pub fn send(pair_id: VethPairId, end: VethEndId, frame: Vec<u8>) -> KernelResult<()> {
    with_table(|table| {
        let pair = table.pairs.get_mut(pair_id)
            .ok_or(KernelError::InvalidArgument)?;
        if !pair.active {
            return Err(KernelError::InvalidArgument);
        }

        // Check that the sending endpoint is up.
        let sender_up = pair.end_ref(end).up;
        if !sender_up {
            return Err(KernelError::NotSupported);
        }

        // Check that the peer endpoint is up.
        let peer_end = VethPair::peer_id(end);
        let peer_up = pair.end_ref(peer_end).up;
        if !peer_up {
            // Peer is down — silently drop (like unplugged cable).
            return Ok(());
        }

        // Record TX stats on the sender.
        let frame_len = frame.len() as u64;
        {
            let sender = pair.end_mut(end);
            sender.tx_bytes = sender.tx_bytes.saturating_add(frame_len);
            sender.tx_packets = sender.tx_packets.saturating_add(1);
        }

        // Enqueue on the peer's RX queue.
        let peer = pair.end_mut(peer_end);
        peer.enqueue_rx(frame)
    })
}

/// Receive the next frame from an endpoint's RX queue.
///
/// Returns `None` if the queue is empty.
pub fn recv(pair_id: VethPairId, end: VethEndId) -> Option<Vec<u8>> {
    with_table(|table| {
        let pair = table.pairs.get_mut(pair_id)?;
        if !pair.active {
            return None;
        }
        pair.end_mut(end).dequeue_rx()
    })
}

// ---------------------------------------------------------------------------
// Public API: polling (integration with net::poll)
// ---------------------------------------------------------------------------

/// Drain all veth endpoints' RX queues and feed frames into the
/// protocol stack.
///
/// Called from `net::poll()` on each network poll cycle.  For each
/// active, up endpoint with pending frames, dequeues frames and
/// passes them through `ethernet::process_frame()`.
///
/// Frames are processed in the context of the endpoint's network
/// namespace (the namespace the endpoint is assigned to).
pub fn poll_all() {
    if !is_initialized() {
        return;
    }

    // Collect pending frames under the lock, then process outside it
    // to avoid holding the veth lock during protocol processing
    // (which may need to send response frames).
    let mut pending: Vec<(Vec<u8>, NetNsId)> = Vec::new();

    with_table(|table| {
        for pair in table.pairs.iter_mut() {
            if !pair.active {
                continue;
            }
            // Drain both endpoints.
            for end_id in &[VethEndId::A, VethEndId::B] {
                let endpoint = pair.end_mut(*end_id);
                if !endpoint.up {
                    continue;
                }
                // A bridged endpoint's frames are owned by its L2 bridge, which
                // drains and forwards them itself (net::bridge::forward). Leave
                // them here so the bridge — not the global host stack — sees
                // them; otherwise same-network peer traffic would be swallowed
                // by the host stack instead of switched to the peer.
                if endpoint.bridged {
                    continue;
                }
                while let Some(frame) = endpoint.dequeue_rx() {
                    pending.push((frame, endpoint.ns_id));
                }
            }
        }
    });

    // Process collected frames outside the lock.
    for (frame, ns_id) in &pending {
        // Record stats via the interface module.
        super::interface::record_rx(frame.len());

        // Feed into the protocol stack in the endpoint's own network
        // namespace so socket lookup and "addressed to us" checks are
        // scoped correctly (a frame drained from a container-side veth
        // endpoint is processed as that container's namespace, not root).
        if let Err(e) = super::ethernet::process_frame(frame, *ns_id) {
            super::interface::record_rx_drop();
            crate::serial_println!("[veth] Error processing frame: {:?}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// Public API: queries
// ---------------------------------------------------------------------------

/// Statistics for a single veth endpoint.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API — fields read by kshell and syscall handlers.
pub struct VethEndStats {
    /// MAC address.
    pub mac: [u8; 6],
    /// Network namespace the endpoint belongs to.
    pub ns_id: NetNsId,
    /// Whether the endpoint is up.
    pub up: bool,
    /// Total bytes transmitted (to peer).
    pub tx_bytes: u64,
    /// Total bytes received (from peer).
    pub rx_bytes: u64,
    /// Total frames transmitted.
    pub tx_packets: u64,
    /// Total frames received.
    pub rx_packets: u64,
    /// Frames dropped due to full RX queue.
    pub rx_drops: u64,
    /// Frames currently pending in the RX queue.
    pub rx_pending: usize,
}

/// Statistics for a veth pair.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API.
pub struct VethPairStats {
    /// Pair ID (index).
    pub id: VethPairId,
    /// Endpoint A statistics.
    pub end_a: VethEndStats,
    /// Endpoint B statistics.
    pub end_b: VethEndStats,
}

fn end_stats(e: &VethEnd) -> VethEndStats {
    VethEndStats {
        mac: e.mac,
        ns_id: e.ns_id,
        up: e.up,
        tx_bytes: e.tx_bytes,
        rx_bytes: e.rx_bytes,
        tx_packets: e.tx_packets,
        rx_packets: e.rx_packets,
        rx_drops: e.rx_drops,
        rx_pending: e.rx_pending(),
    }
}

/// Get statistics for a veth pair.
///
/// Returns `None` if the pair ID is invalid or inactive.
#[must_use]
pub fn pair_stats(id: VethPairId) -> Option<VethPairStats> {
    with_table_ref(|table| {
        let pair = table.pairs.get(id)?;
        if !pair.active {
            return None;
        }
        Some(VethPairStats {
            id,
            end_a: end_stats(&pair.end_a),
            end_b: end_stats(&pair.end_b),
        })
    })
}

/// Get the MAC address for a veth endpoint.
///
/// Returns `None` if the pair is invalid or inactive.
#[must_use]
pub fn mac(pair_id: VethPairId, end: VethEndId) -> Option<[u8; 6]> {
    with_table_ref(|table| {
        let pair = table.pairs.get(pair_id)?;
        if !pair.active {
            return None;
        }
        Some(pair.end_ref(end).mac)
    })
}

/// Count active veth pairs.
#[must_use]
pub fn active_count() -> usize {
    with_table_ref(|table| {
        table.pairs.iter().filter(|p| p.active).count()
    })
}

/// List all active veth pairs' statistics.
#[must_use]
pub fn list_all() -> Vec<VethPairStats> {
    with_table_ref(|table| {
        let mut result = Vec::new();
        for (i, pair) in table.pairs.iter().enumerate() {
            if pair.active {
                result.push(VethPairStats {
                    id: i,
                    end_a: end_stats(&pair.end_a),
                    end_b: end_stats(&pair.end_b),
                });
            }
        }
        result
    })
}

/// Find a veth endpoint assigned to a given namespace.
///
/// Returns the pair ID and end ID of the first endpoint found in
/// the specified namespace.  Used by `interface.rs` to look up the
/// veth MAC for a namespace.
///
/// Returns `None` if no endpoint is assigned to the namespace.
#[must_use]
pub fn find_endpoint_for_ns(ns_id: NetNsId) -> Option<(VethPairId, VethEndId)> {
    with_table_ref(|table| {
        for (i, pair) in table.pairs.iter().enumerate() {
            if !pair.active {
                continue;
            }
            if pair.end_a.ns_id == ns_id {
                return Some((i, VethEndId::A));
            }
            if pair.end_b.ns_id == ns_id {
                return Some((i, VethEndId::B));
            }
        }
        None
    })
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Comprehensive self-test for the veth subsystem.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[veth] Running self-test...");

    test_create_destroy()?;
    test_mac_generation()?;
    test_frame_loopback()?;
    test_endpoint_down_drop()?;
    test_queue_full_drop()?;
    test_move_endpoint()?;
    test_move_requires_down()?;
    test_stats_tracking()?;
    test_multiple_pairs()?;
    test_bidirectional()?;
    test_send_frame_ns_veth_egress()?;

    crate::serial_println!("[veth] Self-test PASSED (11 tests)");
    Ok(())
}

/// Test 1: Create and destroy a veth pair.
fn test_create_destroy() -> KernelResult<()> {
    let initial = active_count();
    let id = create_pair()?;

    // Pair should be active.
    if active_count() != initial.saturating_add(1) {
        crate::serial_println!("[veth]   FAIL: active_count after create");
        return Err(KernelError::InternalError);
    }

    // Should have stats.
    if pair_stats(id).is_none() {
        crate::serial_println!("[veth]   FAIL: pair_stats after create");
        return Err(KernelError::InternalError);
    }

    destroy_pair(id)?;

    if active_count() != initial {
        crate::serial_println!("[veth]   FAIL: active_count after destroy");
        return Err(KernelError::InternalError);
    }

    // Stats should be gone.
    if pair_stats(id).is_some() {
        crate::serial_println!("[veth]   FAIL: pair_stats after destroy");
        return Err(KernelError::InternalError);
    }

    // Double-destroy should fail.
    if destroy_pair(id).is_ok() {
        crate::serial_println!("[veth]   FAIL: double destroy should fail");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[veth]   test 1 (create/destroy): OK");
    Ok(())
}

/// Test 2: MAC addresses are locally-administered and unique.
fn test_mac_generation() -> KernelResult<()> {
    let id = create_pair()?;

    let mac_a = mac(id, VethEndId::A).ok_or(KernelError::InternalError)?;
    let mac_b = mac(id, VethEndId::B).ok_or(KernelError::InternalError)?;

    // Bit 1 of first octet set (locally administered).
    if mac_a[0] & 0x02 == 0 {
        crate::serial_println!("[veth]   FAIL: MAC A not locally-administered");
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }
    if mac_b[0] & 0x02 == 0 {
        crate::serial_println!("[veth]   FAIL: MAC B not locally-administered");
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    // Bit 0 clear (unicast).
    if mac_a[0] & 0x01 != 0 {
        crate::serial_println!("[veth]   FAIL: MAC A not unicast");
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }
    if mac_b[0] & 0x01 != 0 {
        crate::serial_println!("[veth]   FAIL: MAC B not unicast");
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    // A and B MACs are different.
    if mac_a == mac_b {
        crate::serial_println!("[veth]   FAIL: MAC A == MAC B");
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    destroy_pair(id)?;
    crate::serial_println!("[veth]   test 2 (MAC generation): OK");
    Ok(())
}

/// Test 3: Frame sent on A appears on B's RX queue.
fn test_frame_loopback() -> KernelResult<()> {
    let id = create_pair()?;

    // Both ends must be up for delivery.
    set_up(id, VethEndId::A, true)?;
    set_up(id, VethEndId::B, true)?;

    // Build a minimal Ethernet frame (14-byte header + 4-byte payload).
    let mut frame = alloc::vec![0u8; ETH_HEADER_SIZE + 4];
    // Dst MAC = endpoint B's MAC.
    let mac_b = mac(id, VethEndId::B).ok_or(KernelError::InternalError)?;
    frame[..6].copy_from_slice(&mac_b);
    // Src MAC = endpoint A's MAC.
    let mac_a = mac(id, VethEndId::A).ok_or(KernelError::InternalError)?;
    frame[6..12].copy_from_slice(&mac_a);
    // EtherType = 0x0800 (IPv4, arbitrary for test).
    frame[12] = 0x08;
    frame[13] = 0x00;
    // Payload.
    frame[14] = 0xDE;
    frame[15] = 0xAD;
    frame[16] = 0xBE;
    frame[17] = 0xEF;

    // Send from A.
    send(id, VethEndId::A, frame.clone())?;

    // Receive on B.
    let received = recv(id, VethEndId::B);
    match received {
        Some(ref data) if data.as_slice() == frame.as_slice() => {}
        Some(ref data) => {
            crate::serial_println!(
                "[veth]   FAIL: received frame mismatch (got {} bytes, expected {})",
                data.len(), frame.len()
            );
            destroy_pair(id)?;
            return Err(KernelError::InternalError);
        }
        None => {
            crate::serial_println!("[veth]   FAIL: no frame received on B");
            destroy_pair(id)?;
            return Err(KernelError::InternalError);
        }
    }

    // Queue should now be empty.
    if recv(id, VethEndId::B).is_some() {
        crate::serial_println!("[veth]   FAIL: extra frame on B");
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    destroy_pair(id)?;
    crate::serial_println!("[veth]   test 3 (frame loopback A→B): OK");
    Ok(())
}

/// Test 4: Frames to a down peer are silently dropped.
fn test_endpoint_down_drop() -> KernelResult<()> {
    let id = create_pair()?;

    // Only bring A up, leave B down.
    set_up(id, VethEndId::A, true)?;

    let frame = alloc::vec![0u8; ETH_HEADER_SIZE + 4];
    // Send from A — should succeed silently (peer down = dropped).
    send(id, VethEndId::A, frame)?;

    // B should have nothing.
    if recv(id, VethEndId::B).is_some() {
        crate::serial_println!("[veth]   FAIL: frame reached down endpoint");
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    // Sending from a down endpoint should error.
    let frame2 = alloc::vec![0u8; ETH_HEADER_SIZE + 4];
    set_up(id, VethEndId::A, false)?;
    match send(id, VethEndId::A, frame2) {
        Err(KernelError::NotSupported) => {}
        other => {
            crate::serial_println!("[veth]   FAIL: send from down endpoint: {:?}", other);
            destroy_pair(id)?;
            return Err(KernelError::InternalError);
        }
    }

    destroy_pair(id)?;
    crate::serial_println!("[veth]   test 4 (down endpoint drop): OK");
    Ok(())
}

/// Test 5: Queue full drops are counted.
fn test_queue_full_drop() -> KernelResult<()> {
    let id = create_pair()?;
    set_up(id, VethEndId::A, true)?;
    set_up(id, VethEndId::B, true)?;

    // Fill B's RX queue to capacity.
    for _ in 0..VETH_QUEUE_DEPTH {
        let frame = alloc::vec![0u8; ETH_HEADER_SIZE];
        send(id, VethEndId::A, frame)?;
    }

    // Next send should fail with ChannelFull.
    let overflow_frame = alloc::vec![0u8; ETH_HEADER_SIZE];
    match send(id, VethEndId::A, overflow_frame) {
        Err(KernelError::ChannelFull) => {}
        other => {
            crate::serial_println!("[veth]   FAIL: expected ChannelFull, got {:?}", other);
            destroy_pair(id)?;
            return Err(KernelError::InternalError);
        }
    }

    // Check stats show the drop.
    let stats = pair_stats(id).ok_or(KernelError::InternalError)?;
    if stats.end_b.rx_drops != 1 {
        crate::serial_println!(
            "[veth]   FAIL: expected 1 drop, got {}",
            stats.end_b.rx_drops
        );
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }
    if stats.end_b.rx_pending != VETH_QUEUE_DEPTH {
        crate::serial_println!(
            "[veth]   FAIL: expected {} pending, got {}",
            VETH_QUEUE_DEPTH, stats.end_b.rx_pending
        );
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    // Drain the queue.
    let mut drained = 0usize;
    while recv(id, VethEndId::B).is_some() {
        drained = drained.saturating_add(1);
    }
    if drained != VETH_QUEUE_DEPTH {
        crate::serial_println!(
            "[veth]   FAIL: drained {} frames, expected {}",
            drained, VETH_QUEUE_DEPTH
        );
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    destroy_pair(id)?;
    crate::serial_println!("[veth]   test 5 (queue full drop): OK");
    Ok(())
}

/// Test 6: Move endpoint to another namespace.
fn test_move_endpoint() -> KernelResult<()> {
    let id = create_pair()?;

    // Both ends start in root namespace.
    let stats = pair_stats(id).ok_or(KernelError::InternalError)?;
    if stats.end_a.ns_id != crate::netns::ROOT_NS {
        crate::serial_println!("[veth]   FAIL: end A not in root ns");
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    // Create a child namespace and move end B into it.
    let ns = crate::netns::create()?;
    move_end(id, VethEndId::B, ns)?;

    let stats = pair_stats(id).ok_or(KernelError::InternalError)?;
    if stats.end_b.ns_id != ns {
        crate::serial_println!("[veth]   FAIL: end B not moved to ns {}", ns);
        destroy_pair(id)?;
        crate::netns::delete(ns)?;
        return Err(KernelError::InternalError);
    }

    // find_endpoint_for_ns should find end B in the child namespace.
    match find_endpoint_for_ns(ns) {
        Some((pid, eid)) if pid == id && eid == VethEndId::B => {}
        other => {
            crate::serial_println!("[veth]   FAIL: find_endpoint_for_ns = {:?}", other);
            destroy_pair(id)?;
            crate::netns::delete(ns)?;
            return Err(KernelError::InternalError);
        }
    }

    // Move to non-existent namespace should fail.
    if move_end(id, VethEndId::B, 99).is_ok() {
        crate::serial_println!("[veth]   FAIL: move to bad namespace succeeded");
        destroy_pair(id)?;
        crate::netns::delete(ns)?;
        return Err(KernelError::InternalError);
    }

    destroy_pair(id)?;
    crate::netns::delete(ns)?;
    crate::serial_println!("[veth]   test 6 (move endpoint): OK");
    Ok(())
}

/// Test 7: Cannot move an endpoint that is up.
fn test_move_requires_down() -> KernelResult<()> {
    let id = create_pair()?;
    let ns = crate::netns::create()?;

    set_up(id, VethEndId::B, true)?;

    match move_end(id, VethEndId::B, ns) {
        Err(KernelError::DeviceBusy) => {}
        other => {
            crate::serial_println!("[veth]   FAIL: expected DeviceBusy, got {:?}", other);
            destroy_pair(id)?;
            crate::netns::delete(ns)?;
            return Err(KernelError::InternalError);
        }
    }

    // Bring it down, then move should succeed.
    set_up(id, VethEndId::B, false)?;
    move_end(id, VethEndId::B, ns)?;

    destroy_pair(id)?;
    crate::netns::delete(ns)?;
    crate::serial_println!("[veth]   test 7 (move requires down): OK");
    Ok(())
}

/// Test 8: TX/RX statistics are tracked correctly.
fn test_stats_tracking() -> KernelResult<()> {
    let id = create_pair()?;
    set_up(id, VethEndId::A, true)?;
    set_up(id, VethEndId::B, true)?;

    // Send 3 frames of known sizes from A to B.
    let sizes: [usize; 3] = [64, 128, 256];
    let mut total_bytes: u64 = 0;
    for &sz in &sizes {
        let frame = alloc::vec![0u8; sz];
        total_bytes = total_bytes.saturating_add(sz as u64);
        send(id, VethEndId::A, frame)?;
    }

    let stats = pair_stats(id).ok_or(KernelError::InternalError)?;

    // A should have 3 TX packets.
    if stats.end_a.tx_packets != 3 {
        crate::serial_println!(
            "[veth]   FAIL: end_a.tx_packets = {}, expected 3",
            stats.end_a.tx_packets
        );
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }
    if stats.end_a.tx_bytes != total_bytes {
        crate::serial_println!(
            "[veth]   FAIL: end_a.tx_bytes = {}, expected {}",
            stats.end_a.tx_bytes, total_bytes
        );
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    // B should have 3 RX packets.
    if stats.end_b.rx_packets != 3 {
        crate::serial_println!(
            "[veth]   FAIL: end_b.rx_packets = {}, expected 3",
            stats.end_b.rx_packets
        );
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }
    if stats.end_b.rx_bytes != total_bytes {
        crate::serial_println!(
            "[veth]   FAIL: end_b.rx_bytes = {}, expected {}",
            stats.end_b.rx_bytes, total_bytes
        );
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    // Drain frames.
    while recv(id, VethEndId::B).is_some() {}

    destroy_pair(id)?;
    crate::serial_println!("[veth]   test 8 (stats tracking): OK");
    Ok(())
}

/// Test 9: Multiple pairs coexist independently.
fn test_multiple_pairs() -> KernelResult<()> {
    let id1 = create_pair()?;
    let id2 = create_pair()?;

    if id1 == id2 {
        crate::serial_println!("[veth]   FAIL: duplicate pair IDs");
        destroy_pair(id1)?;
        destroy_pair(id2)?;
        return Err(KernelError::InternalError);
    }

    set_up(id1, VethEndId::A, true)?;
    set_up(id1, VethEndId::B, true)?;
    set_up(id2, VethEndId::A, true)?;
    set_up(id2, VethEndId::B, true)?;

    // Send on pair 1.
    let frame1 = alloc::vec![1u8; ETH_HEADER_SIZE + 2];
    send(id1, VethEndId::A, frame1)?;

    // Send on pair 2.
    let frame2 = alloc::vec![2u8; ETH_HEADER_SIZE + 4];
    send(id2, VethEndId::A, frame2)?;

    // Pair 1's B should have the frame1 data.
    let r1 = recv(id1, VethEndId::B).ok_or(KernelError::InternalError)?;
    if r1[0] != 1 {
        crate::serial_println!("[veth]   FAIL: pair1 got wrong frame");
        destroy_pair(id1)?;
        destroy_pair(id2)?;
        return Err(KernelError::InternalError);
    }

    // Pair 2's B should have the frame2 data.
    let r2 = recv(id2, VethEndId::B).ok_or(KernelError::InternalError)?;
    if r2[0] != 2 {
        crate::serial_println!("[veth]   FAIL: pair2 got wrong frame");
        destroy_pair(id1)?;
        destroy_pair(id2)?;
        return Err(KernelError::InternalError);
    }

    // No cross-contamination.
    if recv(id1, VethEndId::B).is_some() {
        crate::serial_println!("[veth]   FAIL: pair1 has extra frame");
        destroy_pair(id1)?;
        destroy_pair(id2)?;
        return Err(KernelError::InternalError);
    }
    if recv(id2, VethEndId::B).is_some() {
        crate::serial_println!("[veth]   FAIL: pair2 has extra frame");
        destroy_pair(id1)?;
        destroy_pair(id2)?;
        return Err(KernelError::InternalError);
    }

    if active_count() < 2 {
        crate::serial_println!("[veth]   FAIL: active_count < 2");
        destroy_pair(id1)?;
        destroy_pair(id2)?;
        return Err(KernelError::InternalError);
    }

    destroy_pair(id1)?;
    destroy_pair(id2)?;
    crate::serial_println!("[veth]   test 9 (multiple pairs): OK");
    Ok(())
}

/// Test 10: Bidirectional communication (A→B and B→A).
fn test_bidirectional() -> KernelResult<()> {
    let id = create_pair()?;
    set_up(id, VethEndId::A, true)?;
    set_up(id, VethEndId::B, true)?;

    // A → B.
    let frame_ab = alloc::vec![0xAB; ETH_HEADER_SIZE + 2];
    send(id, VethEndId::A, frame_ab)?;

    // B → A.
    let frame_ba = alloc::vec![0xBA; ETH_HEADER_SIZE + 2];
    send(id, VethEndId::B, frame_ba)?;

    // Check B received from A.
    let r_b = recv(id, VethEndId::B).ok_or(KernelError::InternalError)?;
    if r_b[0] != 0xAB {
        crate::serial_println!("[veth]   FAIL: B got wrong data (expected 0xAB)");
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    // Check A received from B.
    let r_a = recv(id, VethEndId::A).ok_or(KernelError::InternalError)?;
    if r_a[0] != 0xBA {
        crate::serial_println!("[veth]   FAIL: A got wrong data (expected 0xBA)");
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    // Stats: both ends should show 1 TX and 1 RX.
    let stats = pair_stats(id).ok_or(KernelError::InternalError)?;
    if stats.end_a.tx_packets != 1 || stats.end_a.rx_packets != 1 {
        crate::serial_println!(
            "[veth]   FAIL: end_a tx={} rx={}, expected 1/1",
            stats.end_a.tx_packets, stats.end_a.rx_packets
        );
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }
    if stats.end_b.tx_packets != 1 || stats.end_b.rx_packets != 1 {
        crate::serial_println!(
            "[veth]   FAIL: end_b tx={} rx={}, expected 1/1",
            stats.end_b.tx_packets, stats.end_b.rx_packets
        );
        destroy_pair(id)?;
        return Err(KernelError::InternalError);
    }

    destroy_pair(id)?;
    crate::serial_println!("[veth]   test 10 (bidirectional): OK");
    Ok(())
}

/// Test 11: `net::send_frame_ns` egresses a container namespace via its veth.
///
/// This exercises the D-CNET-NSRX TX (egress) path: for a non-root namespace
/// with a veth endpoint, the ns-aware send path must route the frame through
/// the container's veth end (B), which enqueues on the peer host end (A)'s RX
/// queue — the point where the bridge picks it up.  Root-namespace traffic
/// (no veth endpoint) continues to the physical NIC via `send_frame`.
fn test_send_frame_ns_veth_egress() -> KernelResult<()> {
    let id = create_pair()?;
    let ns = crate::netns::create()?;

    // Move end B into the container namespace (must be down to move), then
    // bring both ends up so delivery works.
    move_end(id, VethEndId::B, ns)?;
    set_up(id, VethEndId::A, true)?;
    set_up(id, VethEndId::B, true)?;

    // Sanity: the namespace's container endpoint is end B.
    match find_endpoint_for_ns(ns) {
        Some((pid, eid)) if pid == id && eid == VethEndId::B => {}
        other => {
            crate::serial_println!("[veth]   FAIL: find_endpoint_for_ns = {:?}", other);
            destroy_pair(id)?;
            crate::netns::delete(ns)?;
            return Err(KernelError::InternalError);
        }
    }

    // Build a minimal Ethernet frame (dst = host end A, src = container end B).
    let mut frame = alloc::vec![0u8; ETH_HEADER_SIZE + 4];
    let mac_a = mac(id, VethEndId::A).ok_or(KernelError::InternalError)?;
    let mac_b = mac(id, VethEndId::B).ok_or(KernelError::InternalError)?;
    frame[..6].copy_from_slice(&mac_a);
    frame[6..12].copy_from_slice(&mac_b);
    frame[12] = 0x08;
    frame[13] = 0x00;
    frame[14] = 0xC0;
    frame[15] = 0xFF;
    frame[16] = 0xEE;
    frame[17] = 0x00;

    // Egress via the ns-aware send path — must route through the container's
    // veth (end B), enqueuing on the peer end A's RX queue.
    super::send_frame_ns(ns, &frame)?;

    match recv(id, VethEndId::A) {
        Some(ref data) if data.as_slice() == frame.as_slice() => {}
        Some(_) => {
            crate::serial_println!("[veth]   FAIL: send_frame_ns frame mismatch on A");
            destroy_pair(id)?;
            crate::netns::delete(ns)?;
            return Err(KernelError::InternalError);
        }
        None => {
            crate::serial_println!("[veth]   FAIL: send_frame_ns did not egress via veth");
            destroy_pair(id)?;
            crate::netns::delete(ns)?;
            return Err(KernelError::InternalError);
        }
    }

    // A root-namespace frame must NOT be enqueued on this veth (it goes to the
    // physical NIC path instead), so end A's RX queue stays empty.  The physical
    // send itself may succeed or fail with NoSuchDevice depending on whether a
    // NIC is present — irrelevant here; only the veth queue matters.
    let _ = super::send_frame_ns(crate::netns::ROOT_NS, &frame);
    if recv(id, VethEndId::A).is_some() {
        crate::serial_println!("[veth]   FAIL: root-ns frame leaked into veth");
        destroy_pair(id)?;
        crate::netns::delete(ns)?;
        return Err(KernelError::InternalError);
    }

    destroy_pair(id)?;
    crate::netns::delete(ns)?;
    crate::serial_println!("[veth]   test 11 (send_frame_ns veth egress): OK");
    Ok(())
}

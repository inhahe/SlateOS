//! Channel IPC — structured message passing with capability transfer.
//!
//! Channels are the primary IPC mechanism in this microkernel.  A
//! channel is created as a pair of endpoints.  Either endpoint can
//! send messages to the other.  Messages are variable-length byte
//! buffers (structured format is layered on top by userspace).
//!
//! ## Async (Buffered) Mode
//!
//! By default, `send` is non-blocking: the message is placed in the
//! channel's internal queue and the sender continues immediately.
//! If the queue is full, the send fails with [`ChannelFull`].
//!
//! ## Blocking Receive
//!
//! `recv` blocks the calling task until a message is available.
//! `try_recv` returns immediately with `None` if no message is ready.
//!
//! ## Close Detection (Peer Closed)
//!
//! When one endpoint is closed, the other endpoint's subsequent sends
//! fail with [`ChannelClosed`].  A blocking `recv` on a closed
//! channel returns [`ChannelClosed`] once the queue is drained.
//!
//! ## Performance Target
//!
//! Channel round-trip: < 2 µs (Fuchsia: 1–2 µs, L4: 0.5–1 µs).
//! See `bench/baselines.toml`.
//!
//! ## Blocking Send
//!
//! `send_blocking` blocks when the peer's queue is full (unlike `send`
//! which returns [`ChannelFull`] immediately).  `send_timeout` adds a
//! nanosecond deadline to the blocking behavior.  When the peer consumes
//! a message, a blocked sender is woken.
//!
//! ## Synchronous (Rendezvous) Mode
//!
//! `create_sync()` returns a channel pair with no internal message
//! buffer.  A sender parks its message in a rendezvous slot and
//! blocks until a receiver takes it (or uses `send()` / `send_timeout()`
//! for non-blocking / deadline semantics).  This mirrors L4/seL4
//! synchronous IPC where the kernel copies directly from sender to
//! receiver — no intermediate queue allocation.
//!
//! Semantics differences from async channels:
//! - `send()` returns [`ChannelFull`] unless a receiver is already
//!   waiting (the queue is effectively size 0).
//! - `send_blocking()` parks until a receiver takes the message.
//! - `recv()` checks for a parked sender first, then blocks.
//! - `try_recv()` returns `Ok(Some(msg))` only if a sender is parked.
//!
//! ## Future Optimizations (NOT YET IMPLEMENTED)
//!
//! - Page flipping for large messages (zero-copy).
//! - Fast-path register passing for tiny messages (L4-style).
//!
//! [`ChannelFull`]: crate::error::KernelError::ChannelFull
//! [`ChannelClosed`]: crate::error::KernelError::ChannelClosed

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of messages buffered per direction in a channel.
///
/// If a sender fills the queue, subsequent sends fail with
/// [`ChannelFull`](KernelError::ChannelFull) until the receiver
/// drains some messages.
const MAX_QUEUE_DEPTH: usize = 64;

/// Maximum size of a single message payload in bytes.
///
/// Messages larger than this should use shared memory (page
/// flipping will be added later for zero-copy large messages).
const MAX_MESSAGE_SIZE: usize = 64 * 1024; // 64 KiB

// ---------------------------------------------------------------------------
// Channel ID and Handle
// ---------------------------------------------------------------------------

/// Unique identifier for a channel.
type ChannelId = u64;

/// Counter for generating unique channel IDs.
static NEXT_CHANNEL_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_channel_id() -> ChannelId {
    NEXT_CHANNEL_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to one side of a channel.
///
/// Encodes both the channel ID and the side (0 or 1) in a single
/// `u64`.  Bit 0 = side, bits 1–63 = channel ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelHandle(u64);

impl ChannelHandle {
    /// Create a handle for a given channel and side.
    #[allow(clippy::arithmetic_side_effects)]
    fn new(channel_id: ChannelId, side: u8) -> Self {
        Self((channel_id << 1) | u64::from(side & 1))
    }

    /// Reconstruct a handle from its raw u64 representation.
    ///
    /// Used by the syscall layer to convert register values back to
    /// typed handles.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Get the raw u64 representation of this handle.
    ///
    /// Used by the syscall layer to pack handles into return registers.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Extract the channel ID.
    #[allow(clippy::arithmetic_side_effects)]
    fn channel_id(self) -> ChannelId {
        self.0 >> 1
    }

    /// Extract the side (0 or 1).
    #[allow(clippy::cast_possible_truncation)]
    fn side(self) -> usize {
        (self.0 & 1) as usize
    }

    /// The other side's index (0 ↔ 1).
    #[allow(clippy::arithmetic_side_effects)]
    fn peer_side(self) -> usize {
        1 - self.side()
    }
}

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

/// Maximum number of capability handles per message.
///
/// Limits memory pressure from a single message.  Fuchsia channels
/// allow 64 handles per message — we match that.
const MAX_CAPS_PER_MESSAGE: usize = 64;

/// A capability entry in transit (detached from any process table).
///
/// When a sender transfers capabilities through a channel, the kernel
/// removes the entries from the sender's cap table and stores them
/// inside the message until the receiver dequeues it.  At that point,
/// the entries are inserted into the receiver's cap table.
#[derive(Debug, Clone)]
pub struct TransferredCap {
    /// What type of resource this refers to.
    pub resource_type: crate::cap::ResourceType,
    /// The kernel-internal identifier for the resource.
    pub resource_id: u64,
    /// Permitted operations on this resource.
    pub rights: crate::cap::Rights,
}

/// A channel message: a variable-length byte buffer with optional
/// capability transfers.
///
/// Messages carry both data (arbitrary bytes) and optionally one or
/// more capability handles.  Transferred capabilities are moved from
/// the sender's process table into the message and then into the
/// receiver's table on delivery — move semantics, no duplication.
pub struct Message {
    /// Message payload.
    ///
    /// Not `Debug`-derived because messages can be large and printing
    /// their contents in debug output is rarely useful.  Use
    /// `msg.len()` and `msg.data()` for inspection.
    data: Vec<u8>,

    /// Capability entries in transit (transferred from sender to receiver).
    ///
    /// Empty if no capabilities are being transferred.
    caps: Vec<TransferredCap>,
}

impl core::fmt::Debug for Message {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Message({} bytes, {} caps)", self.data.len(), self.caps.len())
    }
}

impl Message {
    /// Create a message from a byte slice (no capabilities).
    ///
    /// The data is copied into a heap-allocated buffer.
    pub fn from_bytes(data: &[u8]) -> KernelResult<Self> {
        if data.len() > MAX_MESSAGE_SIZE {
            return Err(KernelError::MessageTooLarge);
        }
        Ok(Self {
            data: Vec::from(data),
            caps: Vec::new(),
        })
    }

    /// Create a message with both data and capability transfers.
    ///
    /// `caps` contains capability entries that have been detached from
    /// the sender's table and will be inserted into the receiver's
    /// table on delivery.
    pub fn from_bytes_and_caps(data: &[u8], caps: Vec<TransferredCap>) -> KernelResult<Self> {
        if data.len() > MAX_MESSAGE_SIZE {
            return Err(KernelError::MessageTooLarge);
        }
        if caps.len() > MAX_CAPS_PER_MESSAGE {
            return Err(KernelError::InvalidArgument);
        }
        Ok(Self {
            data: Vec::from(data),
            caps,
        })
    }

    /// Get the message payload as a byte slice.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Consume the message and return the owned data.
    #[must_use]
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }

    /// Length of the message payload in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Number of capability entries in transit.
    #[must_use]
    pub fn cap_count(&self) -> usize {
        self.caps.len()
    }

    /// Take the transferred capabilities out of the message.
    ///
    /// After this call, the message no longer carries any capabilities.
    /// The caller (typically `recv_with_caps`) is responsible for
    /// inserting them into the receiver's table.
    pub fn take_caps(&mut self) -> Vec<TransferredCap> {
        core::mem::take(&mut self.caps)
    }

    /// Read-only view of transferred capabilities.
    #[must_use]
    pub fn caps(&self) -> &[TransferredCap] {
        &self.caps
    }
}

// ---------------------------------------------------------------------------
// Channel inner state
// ---------------------------------------------------------------------------

/// Internal state of a channel, shared between both endpoints.
struct Channel {
    /// Message queues, indexed by side (async channels only).
    ///
    /// `queues[0]` holds messages for side 0 to receive (sent by side 1).
    /// `queues[1]` holds messages for side 1 to receive (sent by side 0).
    ///
    /// For sync (rendezvous) channels, queues are unused — messages go
    /// through `rendezvous_slots` instead.
    queues: [VecDeque<Message>; 2],

    /// Whether each side has been closed.
    closed: [bool; 2],

    /// Task blocked on receive for each side (if any).
    ///
    /// When a task calls blocking `recv` and no messages are available,
    /// its ID is stored here.  When the peer sends a message, the
    /// waiter is woken.
    waiters: [Option<TaskId>; 2],

    /// Task blocked on send for each side (if any).
    ///
    /// When a task calls `send_blocking` or `send_timeout` and the
    /// peer's queue is full, its ID is stored here.  When the peer
    /// consumes a message (via recv/try_recv), the sender is woken.
    sender_waiters: [Option<TaskId>; 2],

    /// Whether this is a synchronous (rendezvous) channel.
    ///
    /// Sync channels have no internal message buffer.  A sender parks
    /// its message in `rendezvous_slots[side]` and blocks until the
    /// receiver takes it.
    sync: bool,

    /// Rendezvous slots (sync channels only).
    ///
    /// `rendezvous_slots[side]` holds the message parked by side `side`
    /// waiting for its peer to take it.  The sender blocks until the
    /// peer calls recv/try_recv and takes the message.
    rendezvous_slots: [Option<Message>; 2],
}

impl Channel {
    fn new() -> Self {
        Self {
            queues: [VecDeque::new(), VecDeque::new()],
            closed: [false, false],
            waiters: [None, None],
            sender_waiters: [None, None],
            sync: false,
            rendezvous_slots: [None, None],
        }
    }

    fn new_sync() -> Self {
        Self {
            queues: [VecDeque::new(), VecDeque::new()],
            closed: [false, false],
            waiters: [None, None],
            sender_waiters: [None, None],
            sync: true,
            rendezvous_slots: [None, None],
        }
    }
}

// ---------------------------------------------------------------------------
// Global channel table
// ---------------------------------------------------------------------------

/// All channels, keyed by `ChannelId`.
///
/// Lock ordering: `CHANNELS` → `SCHED` (when waking a blocked task
/// after sending a message, we call `sched::wake()` which acquires
/// the SCHED lock).  No code acquires them in reverse order.
static CHANNELS: Mutex<BTreeMap<ChannelId, Channel>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new channel pair.
///
/// Returns two handles: `(endpoint_0, endpoint_1)`.  Either endpoint
/// can send to the other.  When one endpoint is closed (via [`close`]),
/// the other endpoint's sends fail with [`ChannelClosed`].
///
/// [`ChannelClosed`]: KernelError::ChannelClosed
pub fn create() -> (ChannelHandle, ChannelHandle) {
    let id = alloc_channel_id();
    CHANNELS.lock().insert(id, Channel::new());
    super::stats::channel_created();
    (ChannelHandle::new(id, 0), ChannelHandle::new(id, 1))
}

/// Create a new synchronous (rendezvous) channel pair.
///
/// Returns two handles like [`create`], but the channel has no
/// internal message buffer.  Sends block until a receiver takes
/// the message (L4/seL4-style synchronous IPC).
///
/// # Semantics
///
/// - `send()`: succeeds only if a receiver is already waiting;
///   otherwise returns [`ChannelFull`] (the 0-element queue is full).
/// - `send_blocking()`: parks the message and blocks until a
///   receiver takes it.
/// - `recv()`: if a sender is parked, takes the message immediately
///   and wakes the sender; otherwise blocks for a sender.
/// - `try_recv()`: takes from a parked sender or returns `None`.
///
/// [`ChannelFull`]: KernelError::ChannelFull
pub fn create_sync() -> (ChannelHandle, ChannelHandle) {
    let id = alloc_channel_id();
    CHANNELS.lock().insert(id, Channel::new_sync());
    super::stats::channel_created();
    (ChannelHandle::new(id, 0), ChannelHandle::new(id, 1))
}

/// Query whether a channel is synchronous (rendezvous).
///
/// Returns `None` if the handle is invalid.
#[allow(dead_code)]
pub fn is_sync(handle: ChannelHandle) -> Option<bool> {
    CHANNELS.lock().get(&handle.channel_id()).map(|ch| ch.sync)
}

/// Send a message to the peer endpoint.
///
/// **Async channels:** the message is placed in the peer's receive
/// queue.  If a task is blocked on `recv` on the peer side, it is
/// woken.
///
/// **Sync (rendezvous) channels:** succeeds only if a receiver is
/// already blocked waiting.  The message is placed in the rendezvous
/// slot and the receiver is woken immediately.  If no receiver is
/// waiting, returns [`ChannelFull`] (use `send_blocking()` for
/// blocking send on sync channels).
///
/// # Errors
///
/// - [`ChannelClosed`] — the peer endpoint has been closed.
/// - [`ChannelFull`] — the peer's queue is full (async) or no
///   receiver waiting (sync).
/// - [`InvalidHandle`] — the channel does not exist.
///
/// [`ChannelClosed`]: KernelError::ChannelClosed
/// [`ChannelFull`]: KernelError::ChannelFull
/// [`InvalidHandle`]: KernelError::InvalidHandle
pub fn send(handle: ChannelHandle, msg: Message) -> KernelResult<()> {
    let msg_len = msg.len() as u64;
    let wake_task: Option<TaskId>;

    {
        let mut channels = CHANNELS.lock();
        let ch = channels.get_mut(&handle.channel_id())
            .ok_or(KernelError::InvalidHandle)?;

        // Check if OUR side is closed (can't send from a closed endpoint).
        let our_side = handle.side();
        if ch.closed[our_side] {
            return Err(KernelError::ChannelClosed);
        }

        // Check if the PEER side is closed.
        let peer = handle.peer_side();
        if ch.closed[peer] {
            return Err(KernelError::ChannelClosed);
        }

        if ch.sync {
            // Sync channel: succeed only if a receiver is waiting.
            if ch.waiters[peer].is_some() {
                // Receiver is blocked — place message in rendezvous
                // slot and wake receiver.
                ch.rendezvous_slots[our_side] = Some(msg);
                wake_task = ch.waiters[peer].take();
            } else {
                // No receiver waiting — cannot buffer.
                return Err(KernelError::ChannelFull);
            }
        } else {
            // Async channel: enqueue in peer's queue.
            if ch.queues[peer].len() >= MAX_QUEUE_DEPTH {
                return Err(KernelError::ChannelFull);
            }
            ch.queues[peer].push_back(msg);
            wake_task = ch.waiters[peer].take();
        }

        // Lock is dropped here.
    }

    // Wake the blocked task outside the CHANNELS lock to respect
    // lock ordering (CHANNELS → SCHED).
    if let Some(task_id) = wake_task {
        sched::wake(task_id);
    }

    crate::ktrace::record(
        crate::ktrace::Category::Ipc,
        crate::ktrace::event::CHANNEL_SEND,
        handle.channel_id(),
        msg_len,
    );
    super::stats::channel_send(msg_len);
    Ok(())
}

/// Send a message (blocking when queue is full).
///
/// Unlike [`send`], which returns [`ChannelFull`] immediately when the
/// peer's queue is at capacity, this variant blocks the calling task
/// until space becomes available.
///
/// # Errors
///
/// - [`ChannelClosed`] — the peer endpoint has been closed.
/// - [`InvalidHandle`] — the channel does not exist.
///
/// [`ChannelFull`]: KernelError::ChannelFull
/// [`ChannelClosed`]: KernelError::ChannelClosed
/// [`InvalidHandle`]: KernelError::InvalidHandle
pub fn send_blocking(handle: ChannelHandle, msg: Message) -> KernelResult<()> {
    let msg_len = msg.len() as u64;
    // Wrap the message in an Option so we can retry without cloning.
    let mut pending = Some(msg);

    loop {
        let wake_task: Option<TaskId>;

        {
            let mut channels = CHANNELS.lock();
            let ch = channels.get_mut(&handle.channel_id())
                .ok_or(KernelError::InvalidHandle)?;

            let our_side = handle.side();
            if ch.closed[our_side] {
                return Err(KernelError::ChannelClosed);
            }

            let peer = handle.peer_side();
            if ch.closed[peer] {
                return Err(KernelError::ChannelClosed);
            }

            if ch.sync {
                // Sync channel: check if receiver is waiting.
                if ch.waiters[peer].is_some() {
                    // Receiver blocked — hand off directly.
                    if let Some(m) = pending.take() {
                        ch.rendezvous_slots[our_side] = Some(m);
                    }
                    wake_task = ch.waiters[peer].take();
                    drop(channels);
                    if let Some(task_id) = wake_task {
                        sched::wake(task_id);
                    }
                    super::stats::channel_send(msg_len);
                    return Ok(());
                }

                // No receiver — check if our slot is already occupied
                // (shouldn't be, but guard against double-park).
                if ch.rendezvous_slots[our_side].is_none() {
                    // Park message in rendezvous slot and block.
                    if let Some(m) = pending.take() {
                        ch.rendezvous_slots[our_side] = Some(m);
                    }
                }
                ch.sender_waiters[our_side] = Some(sched::current_task_id());
            } else {
                // Async channel: enqueue if space available.
                if ch.queues[peer].len() < MAX_QUEUE_DEPTH {
                    if let Some(m) = pending.take() {
                        ch.queues[peer].push_back(m);
                    }
                    wake_task = ch.waiters[peer].take();
                    drop(channels);
                    if let Some(task_id) = wake_task {
                        sched::wake(task_id);
                    }
                    super::stats::channel_send(msg_len);
                    return Ok(());
                }

                // Queue full — register as sender waiter and block.
                ch.sender_waiters[our_side] = Some(sched::current_task_id());
            }
        }

        super::stats::channel_send_block();
        sched::block_current();

        // For sync channels: on wake, check if the receiver took our
        // message (rendezvous slot was cleared).
        {
            let channels = CHANNELS.lock();
            if let Some(ch) = channels.get(&handle.channel_id()) {
                let our_side = handle.side();
                if ch.sync && ch.rendezvous_slots[our_side].is_none() {
                    // Receiver took the message — we're done.
                    super::stats::channel_send(msg_len);
                    return Ok(());
                }
            } else {
                return Err(KernelError::InvalidHandle);
            }
        }
        // If we get here on a sync channel, the wake was spurious
        // (or the channel was closed) — loop back to re-check.
    }
}

/// Send a message with a timeout (nanoseconds).
///
/// Blocks up to `timeout_ns` nanoseconds waiting for queue space (async)
/// or a receiver (sync).  Returns `Err(TimedOut)` if the deadline expires.
///
/// `timeout_ns = 0` is equivalent to `send()` (returns `TimedOut`
/// instead of `ChannelFull` if no space / no receiver).
///
/// # Errors
///
/// - [`TimedOut`] — no space / no receiver within the deadline.
/// - [`ChannelClosed`] — the peer has been closed.
/// - [`InvalidHandle`] — the channel does not exist.
///
/// [`TimedOut`]: KernelError::TimedOut
/// [`ChannelClosed`]: KernelError::ChannelClosed
/// [`InvalidHandle`]: KernelError::InvalidHandle
pub fn send_timeout(handle: ChannelHandle, msg: Message, timeout_ns: u64) -> KernelResult<()> {
    let msg_len = msg.len() as u64;
    // Fast path: try immediately.
    {
        let mut channels = CHANNELS.lock();
        let ch = channels.get_mut(&handle.channel_id())
            .ok_or(KernelError::InvalidHandle)?;

        let our_side = handle.side();
        if ch.closed[our_side] {
            return Err(KernelError::ChannelClosed);
        }

        let peer = handle.peer_side();
        if ch.closed[peer] {
            return Err(KernelError::ChannelClosed);
        }

        if ch.sync {
            // Sync: succeed if receiver waiting.
            if ch.waiters[peer].is_some() {
                ch.rendezvous_slots[our_side] = Some(msg);
                let wake_task = ch.waiters[peer].take();
                drop(channels);
                if let Some(task_id) = wake_task {
                    sched::wake(task_id);
                }
                super::stats::channel_send(msg_len);
                return Ok(());
            }
        } else {
            // Async: succeed if queue has space.
            if ch.queues[peer].len() < MAX_QUEUE_DEPTH {
                ch.queues[peer].push_back(msg);
                let wake_task = ch.waiters[peer].take();
                drop(channels);
                if let Some(task_id) = wake_task {
                    sched::wake(task_id);
                }
                super::stats::channel_send(msg_len);
                return Ok(());
            }
        }
    }

    // Non-blocking mode.
    if timeout_ns == 0 {
        return Err(KernelError::TimedOut);
    }

    // Schedule timer.
    let deadline_ns = crate::hrtimer::now_ns().saturating_add(timeout_ns);

    fn timeout_wake(tid: u64) {
        if !sched::try_wake(tid) {
            sched::defer_wake(tid);
        }
    }

    let timer_handle = crate::hrtimer::schedule_ns(
        timeout_ns,
        timeout_wake,
        sched::current_task_id(),
    );

    // Wrap in Option for retry without clone.
    let mut pending = Some(msg);

    loop {
        {
            let mut channels = CHANNELS.lock();
            let ch = channels.get_mut(&handle.channel_id())
                .ok_or_else(|| {
                    crate::hrtimer::cancel(timer_handle);
                    KernelError::InvalidHandle
                })?;

            let our_side = handle.side();
            if ch.closed[our_side] || ch.closed[handle.peer_side()] {
                // Clean up rendezvous slot if we parked a message.
                if ch.sync {
                    ch.rendezvous_slots[our_side] = None;
                }
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::ChannelClosed);
            }

            let peer = handle.peer_side();

            if ch.sync {
                // Sync: check if our slot was taken (receiver came).
                if ch.rendezvous_slots[our_side].is_none() && pending.is_none() {
                    // Message was taken — success.
                    crate::hrtimer::cancel(timer_handle);
                    super::stats::channel_send(msg_len);
                    return Ok(());
                }
                // Try to place message if receiver is now waiting.
                if ch.waiters[peer].is_some() {
                    if let Some(m) = pending.take() {
                        ch.rendezvous_slots[our_side] = Some(m);
                    }
                    let wake_task = ch.waiters[peer].take();
                    crate::hrtimer::cancel(timer_handle);
                    drop(channels);
                    if let Some(task_id) = wake_task {
                        sched::wake(task_id);
                    }
                    super::stats::channel_send(msg_len);
                    return Ok(());
                }
                // Park message and block.
                if pending.is_some() && ch.rendezvous_slots[our_side].is_none() {
                    if let Some(m) = pending.take() {
                        ch.rendezvous_slots[our_side] = Some(m);
                    }
                }
                ch.sender_waiters[our_side] = Some(sched::current_task_id());
            } else {
                // Async: try to enqueue.
                if ch.queues[peer].len() < MAX_QUEUE_DEPTH {
                    if let Some(m) = pending.take() {
                        ch.queues[peer].push_back(m);
                    }
                    let wake_task = ch.waiters[peer].take();
                    crate::hrtimer::cancel(timer_handle);
                    drop(channels);
                    if let Some(task_id) = wake_task {
                        sched::wake(task_id);
                    }
                    super::stats::channel_send(msg_len);
                    return Ok(());
                }
                ch.sender_waiters[our_side] = Some(sched::current_task_id());
            }

            // Check timeout.
            if crate::hrtimer::now_ns() >= deadline_ns {
                // Clean up any parked message.
                if ch.sync {
                    ch.rendezvous_slots[our_side] = None;
                }
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::TimedOut);
            }
        }

        super::stats::channel_send_block();
        sched::block_current();
    }
}

/// Send a message with capability transfer.
///
/// This is the primary mechanism for passing kernel object access
/// between processes.  The `cap_handles` are removed from the
/// sender's capability table (move semantics) and attached to the
/// message.  When the receiver calls `recv_with_caps`, the capabilities
/// are inserted into their table with new handles.
///
/// # Arguments
///
/// - `handle`: the channel endpoint to send on.
/// - `data`: message payload bytes.
/// - `cap_handles`: raw capability handle values from the sender's table.
/// - `sender_pid`: the PID of the sending process (for cap table access).
///
/// # Errors
///
/// - [`ChannelClosed`] — peer endpoint closed.
/// - [`ChannelFull`] — peer's queue is full.
/// - [`InvalidHandle`] — channel doesn't exist.
/// - [`InvalidCapability`] — one of the cap handles is invalid.
/// - [`InvalidArgument`] — too many caps (> 64).
///
/// On error, no caps are transferred (all-or-nothing).
///
/// [`ChannelClosed`]: KernelError::ChannelClosed
/// [`ChannelFull`]: KernelError::ChannelFull
/// [`InvalidHandle`]: KernelError::InvalidHandle
/// [`InvalidCapability`]: KernelError::InvalidCapability
pub fn send_with_caps(
    handle: ChannelHandle,
    data: &[u8],
    cap_handles: &[u64],
    sender_pid: u64,
) -> KernelResult<()> {
    use crate::proc::pcb;

    if cap_handles.len() > MAX_CAPS_PER_MESSAGE {
        return Err(KernelError::InvalidArgument);
    }
    if data.len() > MAX_MESSAGE_SIZE {
        return Err(KernelError::MessageTooLarge);
    }

    // Phase 1: Extract caps from sender's table (all-or-nothing).
    let transferred: Vec<TransferredCap> = if cap_handles.is_empty() {
        Vec::new()
    } else {
        let entries = pcb::remove_caps(sender_pid, cap_handles)?;
        entries
            .into_iter()
            .map(|e| TransferredCap {
                resource_type: e.resource_type,
                resource_id: e.resource_id,
                rights: e.rights,
            })
            .collect()
    };

    // Phase 2: Build message and enqueue.
    let msg = Message::from_bytes_and_caps(data, transferred)?;
    send(handle, msg)
}

/// Receive a message and extract transferred capabilities.
///
/// Dequeues a message, inserts any transferred capability entries
/// into the receiver's process table, and returns the data + new
/// capability handles.
///
/// # Arguments
///
/// - `handle`: the channel endpoint to receive on.
/// - `receiver_pid`: the PID of the receiving process.
///
/// # Returns
///
/// - `Ok((msg_data, cap_handles))` — the payload and new handle values.
/// - `Err(ChannelClosed)` — peer closed and queue drained.
/// - `Err(InvalidHandle)` — channel doesn't exist.
pub fn recv_with_caps(
    handle: ChannelHandle,
    receiver_pid: u64,
) -> KernelResult<(Vec<u8>, Vec<u64>)> {
    use crate::proc::pcb;

    // Use blocking recv to get the message.
    let mut msg = recv(handle)?;

    // Extract caps and insert into receiver's table.
    let caps = msg.take_caps();
    let new_handles = if caps.is_empty() {
        Vec::new()
    } else {
        let entries: Vec<_> = caps
            .iter()
            .map(|c| (c.resource_type, c.resource_id, c.rights))
            .collect();
        // insert_caps returns only successfully inserted handles;
        // caps dropped due to table-full are silently lost.
        pcb::insert_caps(receiver_pid, &entries)?
    };

    Ok((msg.into_data(), new_handles))
}

/// Try to receive a message with capability extraction (non-blocking).
///
/// Same as [`recv_with_caps`] but returns immediately if no message
/// is available.
pub fn try_recv_with_caps(
    handle: ChannelHandle,
    receiver_pid: u64,
) -> KernelResult<Option<(Vec<u8>, Vec<u64>)>> {
    use crate::proc::pcb;

    let mut msg = match try_recv(handle)? {
        Some(m) => m,
        None => return Ok(None),
    };

    let caps = msg.take_caps();
    let new_handles = if caps.is_empty() {
        Vec::new()
    } else {
        let entries: Vec<_> = caps
            .iter()
            .map(|c| (c.resource_type, c.resource_id, c.rights))
            .collect();
        pcb::insert_caps(receiver_pid, &entries)?
    };

    Ok(Some((msg.into_data(), new_handles)))
}

/// Check if a channel has pending messages (non-consuming).
///
/// Returns `true` if at least one message is queued for this
/// endpoint, `false` otherwise (including if the handle is invalid).
///
/// Used by the completion port to poll channels without consuming
/// messages.
pub fn has_pending(handle: ChannelHandle) -> bool {
    let channels = CHANNELS.lock();
    let Some(ch) = channels.get(&handle.channel_id()) else {
        return false;
    };
    let our_side = handle.side();
    let peer = handle.peer_side();

    if ch.sync {
        // Sync channel: pending if the peer has a parked message.
        ch.rendezvous_slots[peer].is_some()
    } else {
        // Async channel: pending if our receive queue is non-empty.
        // SAFETY: our_side is 0 or 1 (from the handle encoding).
        #[allow(clippy::indexing_slicing)]
        !ch.queues[our_side].is_empty()
    }
}

/// Try to receive a message (non-blocking).
///
/// **Async channels:** returns `Ok(Some(msg))` if a message was in
/// the queue, `Ok(None)` if the queue was empty.
///
/// **Sync channels:** returns `Ok(Some(msg))` if a sender has parked
/// a message in the rendezvous slot, `Ok(None)` if no sender is
/// waiting.  Taking the message wakes the blocked sender.
///
/// # Errors
///
/// - [`ChannelClosed`] — the peer is closed and no messages remain.
/// - [`InvalidHandle`] — the channel does not exist.
///
/// [`ChannelClosed`]: KernelError::ChannelClosed
/// [`InvalidHandle`]: KernelError::InvalidHandle
pub fn try_recv(handle: ChannelHandle) -> KernelResult<Option<Message>> {
    let mut channels = CHANNELS.lock();
    let ch = channels.get_mut(&handle.channel_id())
        .ok_or(KernelError::InvalidHandle)?;

    let our_side = handle.side();
    let peer = handle.peer_side();

    if ch.sync {
        // Sync channel: check if the peer parked a message.
        if let Some(msg) = ch.rendezvous_slots[peer].take() {
            // Wake the sender — its message was taken.
            let wake_sender = ch.sender_waiters[peer].take();
            drop(channels);

            if let Some(task_id) = wake_sender {
                sched::wake(task_id);
            }
            super::stats::channel_recv();
            return Ok(Some(msg));
        }
    } else {
        // Async channel: dequeue from our queue.
        if let Some(msg) = ch.queues[our_side].pop_front() {
            let wake_sender = ch.sender_waiters[peer].take();
            drop(channels);

            if let Some(task_id) = wake_sender {
                sched::wake(task_id);
            }
            super::stats::channel_recv();
            return Ok(Some(msg));
        }
    }

    // No messages available.  If the peer is closed, there will
    // never be more.
    if ch.closed[peer] {
        return Err(KernelError::ChannelClosed);
    }

    // Empty but peer is alive — caller should retry or block.
    Ok(None)
}

/// Receive a message (blocking).
///
/// If no message is available, blocks the calling task until the peer
/// sends a message or closes the channel.
///
/// # Errors
///
/// - [`ChannelClosed`] — the peer closed and no messages remain.
/// - [`InvalidHandle`] — the channel does not exist.
///
/// [`ChannelClosed`]: KernelError::ChannelClosed
/// [`InvalidHandle`]: KernelError::InvalidHandle
pub fn recv(handle: ChannelHandle) -> KernelResult<Message> {
    loop {
        // First, try non-blocking receive.
        {
            let mut channels = CHANNELS.lock();
            let ch = channels.get_mut(&handle.channel_id())
                .ok_or(KernelError::InvalidHandle)?;

            let our_side = handle.side();
            let peer = handle.peer_side();

            if ch.sync {
                // Sync channel: check for a parked message from peer.
                if let Some(msg) = ch.rendezvous_slots[peer].take() {
                    let ch_id = handle.channel_id();
                    let wake_sender = ch.sender_waiters[peer].take();
                    drop(channels);
                    if let Some(task_id) = wake_sender {
                        sched::wake(task_id);
                    }
                    crate::ktrace::record(
                        crate::ktrace::Category::Ipc,
                        crate::ktrace::event::CHANNEL_RECV,
                        ch_id,
                        msg.len() as u64,
                    );
                    super::stats::channel_recv();
                    return Ok(msg);
                }
            } else {
                // Async channel: dequeue from our queue.
                if let Some(msg) = ch.queues[our_side].pop_front() {
                    let ch_id = handle.channel_id();
                    let wake_sender = ch.sender_waiters[peer].take();
                    drop(channels);
                    if let Some(task_id) = wake_sender {
                        sched::wake(task_id);
                    }
                    crate::ktrace::record(
                        crate::ktrace::Category::Ipc,
                        crate::ktrace::event::CHANNEL_RECV,
                        ch_id,
                        msg.len() as u64,
                    );
                    super::stats::channel_recv();
                    return Ok(msg);
                }
            }

            // No message — check if peer is closed.
            if ch.closed[peer] {
                return Err(KernelError::ChannelClosed);
            }

            // Register ourselves as a waiter.
            ch.waiters[our_side] = Some(sched::current_task_id());

            // Lock is dropped here before blocking.
        }

        // Block until woken by a send or close.
        super::stats::channel_recv_block();
        sched::block_current();

        // When we wake up, loop back and try to receive again.
        // (We re-check because the wake could be spurious or the
        // channel could have been closed while we were blocked.)
    }
}

/// Receive a message with a timeout (nanoseconds).
///
/// Blocks up to `timeout_ns` nanoseconds waiting for a message.
/// Returns `Err(TimedOut)` if the timeout expires before a message
/// arrives.  Returns immediately if a message is already queued.
///
/// `timeout_ns = 0` is equivalent to `try_recv()` (immediate check).
///
/// # Errors
///
/// - [`TimedOut`] — no message arrived within the deadline.
/// - [`ChannelClosed`] — the peer closed and no messages remain.
/// - [`InvalidHandle`] — the channel does not exist.
///
/// [`TimedOut`]: KernelError::TimedOut
/// [`ChannelClosed`]: KernelError::ChannelClosed
/// [`InvalidHandle`]: KernelError::InvalidHandle
pub fn recv_timeout(handle: ChannelHandle, timeout_ns: u64) -> KernelResult<Message> {
    // Fast path: try without blocking.
    match try_recv(handle) {
        Ok(Some(msg)) => return Ok(msg),
        Ok(None) => {}
        Err(e) => return Err(e),
    }

    // timeout_ns == 0 means non-blocking.
    if timeout_ns == 0 {
        return Err(KernelError::TimedOut);
    }

    // Schedule a timer to wake us at the deadline.
    let deadline_ns = crate::hrtimer::now_ns().saturating_add(timeout_ns);
    let task_id = sched::current_task_id();

    fn timeout_wake(tid: u64) {
        if !sched::try_wake(tid) {
            sched::defer_wake(tid);
        }
    }

    let timer_handle = crate::hrtimer::schedule_ns(timeout_ns, timeout_wake, task_id);

    // Block loop: try to receive, block if empty, re-check on wake.
    loop {
        {
            let mut channels = CHANNELS.lock();
            let ch = channels.get_mut(&handle.channel_id())
                .ok_or(KernelError::InvalidHandle)?;

            let our_side = handle.side();
            let peer = handle.peer_side();

            // Try to get a message (sync vs async).
            let got_msg = if ch.sync {
                ch.rendezvous_slots[peer].take()
            } else {
                ch.queues[our_side].pop_front()
            };

            if let Some(msg) = got_msg {
                // Got a message — cancel timer, wake blocked sender.
                crate::hrtimer::cancel(timer_handle);
                let wake_sender = ch.sender_waiters[peer].take();
                drop(channels);
                if let Some(sid) = wake_sender {
                    sched::wake(sid);
                }
                super::stats::channel_recv();
                return Ok(msg);
            }

            // No message — check if peer is closed.
            if ch.closed[peer] {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::ChannelClosed);
            }

            // Check timeout.
            if crate::hrtimer::now_ns() >= deadline_ns {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::TimedOut);
            }

            // Register ourselves as a waiter.
            ch.waiters[our_side] = Some(sched::current_task_id());
        }

        super::stats::channel_recv_block();
        sched::block_current();

        // We woke up — either from send/close or from the timer.
        // Loop back to check which.
    }
}

/// Close a channel endpoint.
///
/// Any task blocked on `recv` on the peer side is woken (it will
/// get `Err(ChannelClosed)` on its next recv attempt if the queue
/// is empty).
///
/// Closing an already-closed endpoint or an invalid handle is a
/// no-op.
pub fn close(handle: ChannelHandle) {
    let mut wake_tasks: [Option<TaskId>; 2] = [None, None];

    {
        let mut channels = CHANNELS.lock();
        let Some(ch) = channels.get_mut(&handle.channel_id()) else {
            return;
        };

        let our_side = handle.side();
        ch.closed[our_side] = true;

        // Wake the peer if it's blocked on recv — it will get
        // ChannelClosed on its next attempt.
        let peer = handle.peer_side();
        wake_tasks[0] = ch.waiters[peer].take();

        // Also wake any sender blocked trying to send to the peer's
        // queue — they'll see ChannelClosed on retry.
        wake_tasks[1] = ch.sender_waiters[peer].take();

        // For sync channels: drop any parked message from our side
        // (no one will ever take it).  Also drop the peer's parked
        // message since we're closing — the peer's sender_waiter was
        // already woken above.
        if ch.sync {
            ch.rendezvous_slots[our_side] = None;
        }

        // If both sides are closed, remove the channel entirely.
        if ch.closed[0] && ch.closed[1] {
            channels.remove(&handle.channel_id());
            super::stats::channel_destroyed();
        }
    }

    for task_id in wake_tasks.iter().flatten() {
        sched::wake(*task_id);
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run channel IPC self-tests.
///
/// Tests:
/// 1. Basic send/receive.
/// 2. Message ordering (FIFO).
/// 3. Channel close detection.
/// 4. Blocking receive (with scheduler integration).
/// 5. Backpressure (queue full).
/// 8. Sync channel: non-blocking send fails without receiver.
/// 9. Sync channel: blocking send/recv rendezvous.
/// 10. Sync channel: close detection.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[ipc] Running channel self-test...");

    test_basic_send_recv()?;
    test_message_ordering()?;
    test_close_detection()?;
    test_blocking_recv()?;
    test_backpressure()?;
    test_sync_send_no_receiver()?;
    test_sync_rendezvous()?;
    test_sync_close()?;

    serial_println!("[ipc] Channel self-test PASSED");
    Ok(())
}

/// Run channel timeout tests (requires hrtimer to be initialized).
///
/// Called separately from `self_test()` because it uses `sleep_ms`
/// which depends on the high-resolution timer subsystem.
pub fn self_test_timeout() -> KernelResult<()> {
    test_recv_timeout()?;
    test_cap_transfer_message()?;
    Ok(())
}

/// Test 1: Basic send and receive.
fn test_basic_send_recv() -> KernelResult<()> {
    let (ep0, ep1) = create();

    // Send from ep0, receive on ep1.
    let msg = Message::from_bytes(b"hello")?;
    send(ep0, msg)?;

    let received = try_recv(ep1)?
        .ok_or(KernelError::InternalError)?;
    if received.data() != b"hello" {
        serial_println!("[ipc]   FAIL: basic send/recv data mismatch");
        return Err(KernelError::InternalError);
    }

    // Send from ep1, receive on ep0.
    let msg = Message::from_bytes(b"world")?;
    send(ep1, msg)?;

    let received = try_recv(ep0)?
        .ok_or(KernelError::InternalError)?;
    if received.data() != b"world" {
        serial_println!("[ipc]   FAIL: reverse send/recv data mismatch");
        return Err(KernelError::InternalError);
    }

    close(ep0);
    close(ep1);
    serial_println!("[ipc]   Basic send/recv: OK");
    Ok(())
}

/// Test 2: FIFO message ordering.
fn test_message_ordering() -> KernelResult<()> {
    let (ep0, ep1) = create();

    for i in 0u8..10 {
        let msg = Message::from_bytes(&[i])?;
        send(ep0, msg)?;
    }

    for i in 0u8..10 {
        let received = try_recv(ep1)?
            .ok_or(KernelError::InternalError)?;
        if received.data() != [i] {
            serial_println!("[ipc]   FAIL: ordering — expected {}, got {:?}", i, received.data());
            return Err(KernelError::InternalError);
        }
    }

    close(ep0);
    close(ep1);
    serial_println!("[ipc]   Message ordering (FIFO): OK");
    Ok(())
}

/// Test 3: Peer close detection.
fn test_close_detection() -> KernelResult<()> {
    let (ep0, ep1) = create();

    // Close ep0, then try to send from ep1.
    close(ep0);

    let msg = Message::from_bytes(b"should fail")?;
    match send(ep1, msg) {
        Err(KernelError::ChannelClosed) => {}
        other => {
            serial_println!("[ipc]   FAIL: send to closed peer returned {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // Receive from ep1 on a channel where peer is closed and queue
    // is empty → ChannelClosed.
    match try_recv(ep1) {
        Err(KernelError::ChannelClosed) => {}
        other => {
            serial_println!("[ipc]   FAIL: recv on closed empty channel returned {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    close(ep1);
    serial_println!("[ipc]   Close detection: OK");
    Ok(())
}

/// Counter for blocking recv self-test verification.
static BLOCKING_RESULT: AtomicU64 = AtomicU64::new(0);

/// Receiver task for the blocking recv self-test.
///
/// Blocks on `recv` on the given channel handle, stores 42 to
/// `BLOCKING_RESULT` if the expected message arrives.
extern "C" fn blocking_recv_task(handle_raw: u64) {
    let handle = ChannelHandle(handle_raw);
    match recv(handle) {
        Ok(msg) => {
            if msg.data() == b"wake up" {
                BLOCKING_RESULT.store(42, Ordering::SeqCst);
            }
        }
        Err(_) => {
            BLOCKING_RESULT.store(99, Ordering::SeqCst);
        }
    }
}

/// Test 4: Blocking receive with scheduler.
fn test_blocking_recv() -> KernelResult<()> {
    BLOCKING_RESULT.store(0, Ordering::SeqCst);

    let (ep0, ep1) = create();

    // Pack the handle value into the u64 argument for the receiver task.
    let ep1_raw = ep1.0;

    sched::spawn(b"recv-test", 16, blocking_recv_task, ep1_raw, 0)?;

    // Yield to let the receiver run — it will block on recv.
    sched::yield_now();

    // Now send a message from ep0.  This should wake the receiver.
    let msg = Message::from_bytes(b"wake up")?;
    send(ep0, msg)?;

    // Yield to let the receiver process the message and exit.
    sched::yield_now();
    sched::yield_now(); // Extra yield in case of scheduling delay.

    let result = BLOCKING_RESULT.load(Ordering::SeqCst);
    if result != 42 {
        serial_println!("[ipc]   FAIL: blocking recv result={}, expected 42", result);
        close(ep0);
        close(ep1);
        return Err(KernelError::InternalError);
    }

    close(ep0);
    // ep1 is closed by the receiver task's exit (it goes out of scope).
    // But since we passed the raw handle, we should close it explicitly.
    close(ep1);

    serial_println!("[ipc]   Blocking recv (scheduler integration): OK");
    Ok(())
}

/// Test 5: Backpressure — queue full.
fn test_backpressure() -> KernelResult<()> {
    let (ep0, ep1) = create();

    // Fill the queue to capacity.
    for _ in 0..MAX_QUEUE_DEPTH {
        let msg = Message::from_bytes(b"x")?;
        send(ep0, msg)?;
    }

    // Next send should fail with ChannelFull.
    let msg = Message::from_bytes(b"overflow")?;
    match send(ep0, msg) {
        Err(KernelError::ChannelFull) => {}
        other => {
            serial_println!("[ipc]   FAIL: expected ChannelFull, got {:?}", other);
            close(ep0);
            close(ep1);
            return Err(KernelError::InternalError);
        }
    }

    // Drain one message, then send should succeed again.
    let _ = try_recv(ep1)?;
    let msg = Message::from_bytes(b"after drain")?;
    send(ep0, msg)?;

    close(ep0);
    close(ep1);
    serial_println!("[ipc]   Backpressure (queue full): OK");
    Ok(())
}

/// Result storage for the timeout recv self-test.
static TIMEOUT_RESULT: AtomicU64 = AtomicU64::new(0);

/// Task that calls `recv_timeout` on an empty channel — should time out.
extern "C" fn timeout_recv_task(handle_raw: u64) {
    let handle = ChannelHandle(handle_raw);
    // 5 ms timeout — should expire since nobody sends.
    match recv_timeout(handle, 5_000_000) {
        Err(KernelError::TimedOut) => {
            TIMEOUT_RESULT.store(1, Ordering::SeqCst);
        }
        Ok(_) => {
            TIMEOUT_RESULT.store(2, Ordering::SeqCst);
        }
        Err(_) => {
            TIMEOUT_RESULT.store(3, Ordering::SeqCst);
        }
    }
}

/// Task that calls `recv_timeout` but gets a message before the deadline.
static TIMEOUT_EARLY_RESULT: AtomicU64 = AtomicU64::new(0);

extern "C" fn timeout_recv_early_task(handle_raw: u64) {
    let handle = ChannelHandle(handle_raw);
    // 500 ms timeout — should receive before it expires.
    match recv_timeout(handle, 500_000_000) {
        Ok(msg) => {
            if msg.data() == b"early" {
                TIMEOUT_EARLY_RESULT.store(1, Ordering::SeqCst);
            } else {
                TIMEOUT_EARLY_RESULT.store(4, Ordering::SeqCst);
            }
        }
        Err(KernelError::TimedOut) => {
            TIMEOUT_EARLY_RESULT.store(2, Ordering::SeqCst);
        }
        Err(_) => {
            TIMEOUT_EARLY_RESULT.store(3, Ordering::SeqCst);
        }
    }
}

/// Test 6: `recv_timeout` — both timeout expiry and early-message paths.
fn test_recv_timeout() -> KernelResult<()> {
    // --- Part A: Timeout expires (no sender) ---
    TIMEOUT_RESULT.store(0, Ordering::SeqCst);
    let (ep0, ep1) = create();

    sched::spawn(b"recv-to-test", 16, timeout_recv_task, ep1.0, 0)?;

    // Let the receiver run, block on its 5 ms recv_timeout, and record the
    // outcome. Poll for completion with a generous overall deadline instead of
    // a single fixed sleep: a fixed margin is racy under boot-timing shifts
    // (the receiver may be scheduled late, or its wake hrtimer may fire a few
    // ticks behind), which intermittently tripped this test with result=0. The
    // receiver sets TIMEOUT_RESULT to a non-zero code exactly once it returns,
    // so poll until that happens (up to ~1s) and only then assert it timed out.
    // Cf. B-FUTEX-TOWAKER-LOSTWAKE — timeout self-tests must not depend on a
    // fixed number of ticks/yields.
    let mut result = 0;
    for _ in 0..200 {
        sched::sleep_ms(5);
        result = TIMEOUT_RESULT.load(Ordering::SeqCst);
        if result != 0 {
            break;
        }
    }
    if result != 1 {
        serial_println!("[ipc]   FAIL: recv_timeout didn't time out (result={})", result);
        close(ep0);
        close(ep1);
        return Err(KernelError::InternalError);
    }

    close(ep0);
    close(ep1);

    // --- Part B: Message arrives before timeout ---
    TIMEOUT_EARLY_RESULT.store(0, Ordering::SeqCst);
    let (ep0, ep1) = create();

    sched::spawn(b"recv-to-early", 16, timeout_recv_early_task, ep1.0, 0)?;

    // Yield to let receiver start blocking.
    sched::yield_now();
    sched::yield_now();

    // Send a message — should wake receiver before its 500ms deadline.
    let msg = Message::from_bytes(b"early")?;
    send(ep0, msg)?;

    // Let the receiver process the message. Poll for completion rather than
    // assuming a fixed number of yields is enough (same timing-robustness
    // rationale as Part A). The receiver's deadline is 500 ms, so a ~1s poll
    // window comfortably distinguishes "woke early with the message" (result=1)
    // from a genuine failure.
    let mut result = 0;
    for _ in 0..200 {
        sched::sleep_ms(5);
        result = TIMEOUT_EARLY_RESULT.load(Ordering::SeqCst);
        if result != 0 {
            break;
        }
    }
    if result != 1 {
        serial_println!("[ipc]   FAIL: recv_timeout early-msg (result={})", result);
        close(ep0);
        close(ep1);
        return Err(KernelError::InternalError);
    }

    close(ep0);
    close(ep1);

    // --- Part C: Zero timeout = immediate try ---
    let (ep0, ep1) = create();
    match recv_timeout(ep1, 0) {
        Err(KernelError::TimedOut) => {}
        other => {
            serial_println!("[ipc]   FAIL: recv_timeout(0) returned {:?}", other);
            close(ep0);
            close(ep1);
            return Err(KernelError::InternalError);
        }
    }

    // With a message queued, zero timeout should still succeed.
    let msg = Message::from_bytes(b"instant")?;
    send(ep0, msg)?;
    match recv_timeout(ep1, 0) {
        Ok(m) if m.data() == b"instant" => {}
        other => {
            serial_println!("[ipc]   FAIL: recv_timeout(0) with msg returned {:?}", other);
            close(ep0);
            close(ep1);
            return Err(KernelError::InternalError);
        }
    }

    close(ep0);
    close(ep1);

    serial_println!("[ipc]   Receive with timeout: OK");
    Ok(())
}

/// Test 7: Capability transfer in messages — verifies Message can carry
/// `TransferredCap` entries and they survive send/recv through a channel.
fn test_cap_transfer_message() -> KernelResult<()> {
    use crate::cap::{ResourceType, Rights};

    // Create a channel pair.
    let (ep0, ep1) = create();

    // Build a message with caps attached.
    let caps = alloc::vec![
        TransferredCap {
            resource_type: ResourceType::Channel,
            resource_id: 42,
            rights: Rights::READ,
        },
        TransferredCap {
            resource_type: ResourceType::File,
            resource_id: 100,
            rights: Rights::ALL,
        },
    ];
    let msg = Message::from_bytes_and_caps(b"hello-caps", caps)?;

    // Verify message state before send.
    if msg.cap_count() != 2 {
        serial_println!("[ipc]   FAIL: cap_count before send = {}", msg.cap_count());
        close(ep0);
        close(ep1);
        return Err(KernelError::InternalError);
    }

    // Send the message.
    send(ep0, msg)?;

    // Receive it on the other side.
    let mut received = try_recv(ep1)?
        .ok_or(KernelError::InternalError)?;

    // Verify data.
    if received.data() != b"hello-caps" {
        serial_println!("[ipc]   FAIL: cap transfer data mismatch");
        close(ep0);
        close(ep1);
        return Err(KernelError::InternalError);
    }

    // Verify caps survived the transfer.
    if received.cap_count() != 2 {
        serial_println!("[ipc]   FAIL: cap_count after recv = {}", received.cap_count());
        close(ep0);
        close(ep1);
        return Err(KernelError::InternalError);
    }

    // Take the caps and verify contents.
    let received_caps = received.take_caps();
    if received_caps.len() != 2 {
        serial_println!("[ipc]   FAIL: take_caps len = {}", received_caps.len());
        close(ep0);
        close(ep1);
        return Err(KernelError::InternalError);
    }

    if received_caps[0].resource_id != 42
        || received_caps[1].resource_id != 100
    {
        serial_println!("[ipc]   FAIL: cap resource_id mismatch");
        close(ep0);
        close(ep1);
        return Err(KernelError::InternalError);
    }

    // After take_caps, message should have 0 caps.
    if received.cap_count() != 0 {
        serial_println!("[ipc]   FAIL: cap_count after take = {}", received.cap_count());
        close(ep0);
        close(ep1);
        return Err(KernelError::InternalError);
    }

    close(ep0);
    close(ep1);

    serial_println!("[ipc]   Capability transfer in message: OK");
    Ok(())
}

// ---------------------------------------------------------------------------
// Sync (rendezvous) channel tests
// ---------------------------------------------------------------------------

/// Test 8: Sync send fails without a receiver (ChannelFull).
fn test_sync_send_no_receiver() -> KernelResult<()> {
    let (ep0, ep1) = create_sync();

    // Non-blocking send should fail — no receiver waiting.
    let msg = Message::from_bytes(b"sync-msg")?;
    match send(ep0, msg) {
        Err(KernelError::ChannelFull) => {}
        other => {
            serial_println!("[ipc]   FAIL: sync send without receiver returned {:?}", other);
            close(ep0);
            close(ep1);
            return Err(KernelError::InternalError);
        }
    }

    // try_recv should return None (no sender parked).
    match try_recv(ep1)? {
        None => {}
        Some(_) => {
            serial_println!("[ipc]   FAIL: sync try_recv returned message with no sender");
            close(ep0);
            close(ep1);
            return Err(KernelError::InternalError);
        }
    }

    // Verify is_sync returns true.
    if is_sync(ep0) != Some(true) || is_sync(ep1) != Some(true) {
        serial_println!("[ipc]   FAIL: is_sync() returned unexpected value");
        close(ep0);
        close(ep1);
        return Err(KernelError::InternalError);
    }

    close(ep0);
    close(ep1);
    serial_println!("[ipc]   Sync send no receiver: OK");
    Ok(())
}

/// Result storage for sync rendezvous test.
static SYNC_RECV_RESULT: AtomicU64 = AtomicU64::new(0);

/// Receiver task for sync rendezvous test.
///
/// Calls blocking recv on a sync channel.  Stores 42 if the expected
/// message arrives.
extern "C" fn sync_recv_task(handle_raw: u64) {
    let handle = ChannelHandle(handle_raw);
    match recv(handle) {
        Ok(msg) => {
            if msg.data() == b"rendezvous" {
                SYNC_RECV_RESULT.store(42, Ordering::SeqCst);
            } else {
                SYNC_RECV_RESULT.store(99, Ordering::SeqCst);
            }
        }
        Err(_) => {
            SYNC_RECV_RESULT.store(88, Ordering::SeqCst);
        }
    }
}

/// Test 9: Sync blocking send/recv rendezvous.
///
/// Spawns a receiver task that blocks on recv, then the main task
/// does a send_blocking.  Since the receiver is waiting, the message
/// should be handed off directly.
fn test_sync_rendezvous() -> KernelResult<()> {
    SYNC_RECV_RESULT.store(0, Ordering::SeqCst);

    let (ep0, ep1) = create_sync();

    // Spawn receiver that blocks on recv(ep1).
    sched::spawn(b"sync-recv", 16, sync_recv_task, ep1.0, 0)?;

    // Yield to let receiver block.
    sched::yield_now();
    sched::yield_now();

    // Now send_blocking — receiver is waiting, so should succeed immediately.
    let msg = Message::from_bytes(b"rendezvous")?;
    send_blocking(ep0, msg)?;

    // Yield to let receiver process.
    sched::yield_now();
    sched::yield_now();

    let result = SYNC_RECV_RESULT.load(Ordering::SeqCst);
    if result != 42 {
        serial_println!("[ipc]   FAIL: sync rendezvous result={}, expected 42", result);
        close(ep0);
        close(ep1);
        return Err(KernelError::InternalError);
    }

    close(ep0);
    close(ep1);
    serial_println!("[ipc]   Sync rendezvous: OK");
    Ok(())
}

/// Test 10: Sync close detection.
fn test_sync_close() -> KernelResult<()> {
    let (ep0, ep1) = create_sync();

    // Close ep0, try to send from ep1.
    close(ep0);

    let msg = Message::from_bytes(b"should fail")?;
    match send(ep1, msg) {
        Err(KernelError::ChannelClosed) => {}
        other => {
            serial_println!("[ipc]   FAIL: sync send to closed peer returned {:?}", other);
            close(ep1);
            return Err(KernelError::InternalError);
        }
    }

    // recv on closed channel should return ChannelClosed.
    match try_recv(ep1) {
        Err(KernelError::ChannelClosed) => {}
        other => {
            serial_println!("[ipc]   FAIL: sync recv on closed channel returned {:?}", other);
            close(ep1);
            return Err(KernelError::InternalError);
        }
    }

    close(ep1);
    serial_println!("[ipc]   Sync close detection: OK");
    Ok(())
}

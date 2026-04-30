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
//! ## Future Optimizations (NOT YET IMPLEMENTED)
//!
//! - Capability handle transfer in messages.
//! - Page flipping for large messages (zero-copy).
//! - Fast-path register passing for tiny messages (L4-style).
//! - Synchronous (rendezvous) mode.
//!
//! [`ChannelFull`]: crate::error::KernelError::ChannelFull
//! [`ChannelClosed`]: crate::error::KernelError::ChannelClosed

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

/// A channel message: a variable-length byte buffer.
///
/// In the future, this will also carry capability handles for
/// cross-process capability transfer.
pub struct Message {
    /// Message payload.
    ///
    /// Not `Debug`-derived because messages can be large and printing
    /// their contents in debug output is rarely useful.  Use
    /// `msg.len()` and `msg.data()` for inspection.
    data: Vec<u8>,
}

impl core::fmt::Debug for Message {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Message({} bytes)", self.data.len())
    }
}

impl Message {
    /// Create a message from a byte slice.
    ///
    /// The data is copied into a heap-allocated buffer.
    pub fn from_bytes(data: &[u8]) -> KernelResult<Self> {
        if data.len() > MAX_MESSAGE_SIZE {
            return Err(KernelError::MessageTooLarge);
        }
        Ok(Self {
            data: Vec::from(data),
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
}

// ---------------------------------------------------------------------------
// Channel inner state
// ---------------------------------------------------------------------------

/// Internal state of a channel, shared between both endpoints.
struct Channel {
    /// Message queues, indexed by side.
    ///
    /// `queues[0]` holds messages for side 0 to receive (sent by side 1).
    /// `queues[1]` holds messages for side 1 to receive (sent by side 0).
    queues: [VecDeque<Message>; 2],

    /// Whether each side has been closed.
    closed: [bool; 2],

    /// Task blocked on receive for each side (if any).
    ///
    /// When a task calls blocking `recv` and no messages are available,
    /// its ID is stored here.  When the peer sends a message, the
    /// waiter is woken.
    waiters: [Option<TaskId>; 2],
}

impl Channel {
    fn new() -> Self {
        Self {
            queues: [VecDeque::new(), VecDeque::new()],
            closed: [false, false],
            waiters: [None, None],
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
    (ChannelHandle::new(id, 0), ChannelHandle::new(id, 1))
}

/// Send a message to the peer endpoint.
///
/// The message is placed in the peer's receive queue.  If a task is
/// blocked on `recv` on the peer side, it is woken.
///
/// # Errors
///
/// - [`ChannelClosed`] — the peer endpoint has been closed.
/// - [`ChannelFull`] — the peer's queue is full (backpressure).
/// - [`InvalidHandle`] — the channel does not exist.
///
/// [`ChannelClosed`]: KernelError::ChannelClosed
/// [`ChannelFull`]: KernelError::ChannelFull
/// [`InvalidHandle`]: KernelError::InvalidHandle
pub fn send(handle: ChannelHandle, msg: Message) -> KernelResult<()> {
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

        // Check queue capacity (backpressure).
        if ch.queues[peer].len() >= MAX_QUEUE_DEPTH {
            return Err(KernelError::ChannelFull);
        }

        // Enqueue the message for the peer to receive.
        ch.queues[peer].push_back(msg);

        // If the peer has a task blocked on recv, wake it.
        wake_task = ch.waiters[peer].take();

        // Lock is dropped here.
    }

    // Wake the blocked task outside the CHANNELS lock to respect
    // lock ordering (CHANNELS → SCHED).
    if let Some(task_id) = wake_task {
        sched::wake(task_id);
    }

    Ok(())
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

    // SAFETY: our_side is 0 or 1 (from the handle encoding).
    #[allow(clippy::indexing_slicing)]
    !ch.queues[our_side].is_empty()
}

/// Try to receive a message (non-blocking).
///
/// Returns `Ok(Some(msg))` if a message was available, `Ok(None)` if
/// the queue was empty, or `Err(ChannelClosed)` if the peer is
/// closed and no messages remain.
///
/// # Errors
///
/// - [`ChannelClosed`] — the peer is closed and the queue is empty.
/// - [`InvalidHandle`] — the channel does not exist.
///
/// [`ChannelClosed`]: KernelError::ChannelClosed
/// [`InvalidHandle`]: KernelError::InvalidHandle
pub fn try_recv(handle: ChannelHandle) -> KernelResult<Option<Message>> {
    let mut channels = CHANNELS.lock();
    let ch = channels.get_mut(&handle.channel_id())
        .ok_or(KernelError::InvalidHandle)?;

    let our_side = handle.side();

    // Try to dequeue a message.
    if let Some(msg) = ch.queues[our_side].pop_front() {
        return Ok(Some(msg));
    }

    // Queue is empty.  If the peer is closed, there will never be
    // more messages.
    if ch.closed[handle.peer_side()] {
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

            if let Some(msg) = ch.queues[our_side].pop_front() {
                return Ok(msg);
            }

            // Queue empty — check if peer is closed.
            if ch.closed[handle.peer_side()] {
                return Err(KernelError::ChannelClosed);
            }

            // Register ourselves as a waiter.
            ch.waiters[our_side] = Some(sched::current_task_id());

            // Lock is dropped here before blocking.
        }

        // Block until woken by a send or close.
        sched::block_current();

        // When we wake up, loop back and try to receive again.
        // (We re-check because the wake could be spurious or the
        // channel could have been closed while we were blocked.)
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
    let wake_task: Option<TaskId>;

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
        wake_task = ch.waiters[peer].take();

        // If both sides are closed, remove the channel entirely.
        if ch.closed[0] && ch.closed[1] {
            channels.remove(&handle.channel_id());
        }
    }

    if let Some(task_id) = wake_task {
        sched::wake(task_id);
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
pub fn self_test() -> KernelResult<()> {
    serial_println!("[ipc] Running channel self-test...");

    test_basic_send_recv()?;
    test_message_ordering()?;
    test_close_detection()?;
    test_blocking_recv()?;
    test_backpressure()?;

    serial_println!("[ipc] Channel self-test PASSED");
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

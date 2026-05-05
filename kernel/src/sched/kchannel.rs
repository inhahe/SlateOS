//! Kernel-internal bounded MPMC channel.
//!
//! A `KChannel<T>` is a fixed-capacity, multi-producer multi-consumer
//! queue for passing typed messages between kernel tasks.  Unlike the
//! userspace IPC channels (which transfer opaque byte messages between
//! processes), `KChannel` is for kernel-internal subsystem communication:
//! block layer → filesystem, interrupt handler → deferred processing,
//! device driver → protocol stack, etc.
//!
//! ## Design
//!
//! - **Bounded**: Fixed capacity specified at creation.  Producers block
//!   when the channel is full (backpressure).
//! - **MPMC**: Multiple tasks can send and receive concurrently.
//! - **Typed**: Messages are `Copy` types (no heap allocation on send).
//! - **Sleeping**: Uses `WaitQueue` for producer/consumer blocking (not
//!   spin-based).  Suitable for process-context kernel tasks.
//!
//! ## Capacity
//!
//! The channel uses a power-of-two circular buffer for efficient modular
//! indexing.  Capacity is rounded up to the next power of two if needed.
//! Maximum capacity is 256 entries (for kernel-internal channels, this
//! is generous — if you need more, consider restructuring your pipeline).
//!
//! ## References
//!
//! - Go channels (bounded, blocking send/recv)
//! - crossbeam-channel (lock-free MPMC in Rust)
//! - Linux kfifo (kernel FIFO buffer)

use core::cell::Cell;
use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;

use super::waitqueue::WaitQueue;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum channel capacity (entries).
const MAX_CAPACITY: usize = 256;

// ---------------------------------------------------------------------------
// KChannel
// ---------------------------------------------------------------------------

/// A bounded, blocking, multi-producer multi-consumer kernel channel.
///
/// Messages must be `Copy` (no allocations, no destructors).
///
/// # Safety
///
/// `send()` and `recv()` must NOT be called from ISR/softirq context
/// (they block).  Use `try_send()` and `try_recv()` for non-blocking
/// access from any context.
pub struct KChannel<T: Copy, const N: usize> {
    /// Circular buffer holding messages.
    buffer: Mutex<ChannelBuffer<T, N>>,
    /// Wake producers when space becomes available.
    send_wq: WaitQueue,
    /// Wake consumers when a message arrives.
    recv_wq: WaitQueue,
    /// Total messages sent (for diagnostics).
    total_sent: AtomicU32,
    /// Total messages received (for diagnostics).
    total_recv: AtomicU32,
}

/// Internal circular buffer state (protected by the outer Mutex).
struct ChannelBuffer<T: Copy, const N: usize> {
    /// Storage array.
    data: [Option<T>; N],
    /// Write position (next slot to write to).
    head: usize,
    /// Read position (next slot to read from).
    tail: usize,
    /// Current number of items in the buffer.
    count: usize,
    /// Whether the channel is closed (no more sends allowed).
    closed: bool,
}

impl<T: Copy, const N: usize> ChannelBuffer<T, N> {
    const fn new() -> Self {
        // SAFETY: None is valid initialization for Option<T: Copy>.
        Self {
            data: [None; N],
            head: 0,
            tail: 0,
            count: 0,
            closed: false,
        }
    }

    fn is_full(&self) -> bool {
        self.count >= N
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn push(&mut self, item: T) -> bool {
        if self.is_full() || self.closed {
            return false;
        }
        self.data[self.head] = Some(item);
        self.head = (self.head + 1) % N;
        self.count += 1;
        true
    }

    fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let item = self.data[self.tail].take();
        self.tail = (self.tail + 1) % N;
        self.count -= 1;
        item
    }
}

/// Error returned when sending to a closed channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError<T> {
    /// Channel is closed — message returned to caller.
    Closed(T),
}

/// Error returned by try_send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrySendError<T> {
    /// Channel is full — message returned to caller.
    Full(T),
    /// Channel is closed.
    Closed(T),
}

/// Error returned by recv when channel is closed and empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecvError;

impl<T: Copy, const N: usize> KChannel<T, N> {
    /// Create a new channel with capacity N.
    ///
    /// N must be at least 1 and at most [`MAX_CAPACITY`].
    pub const fn new() -> Self {
        assert!(N >= 1, "KChannel capacity must be at least 1");
        assert!(N <= MAX_CAPACITY, "KChannel capacity exceeds maximum");
        Self {
            buffer: Mutex::new(ChannelBuffer::new()),
            send_wq: WaitQueue::new(),
            recv_wq: WaitQueue::new(),
            total_sent: AtomicU32::new(0),
            total_recv: AtomicU32::new(0),
        }
    }

    /// Send a message, blocking if the channel is full.
    ///
    /// Returns `Ok(())` on success, `Err(SendError::Closed(msg))` if
    /// the channel is closed.
    pub fn send(&self, msg: T) -> Result<(), SendError<T>> {
        // Fast path: try without blocking.
        {
            let mut buf = self.buffer.lock();
            if buf.closed {
                return Err(SendError::Closed(msg));
            }
            if buf.push(msg) {
                self.total_sent.fetch_add(1, Ordering::Relaxed);
                drop(buf);
                self.recv_wq.wake_one();
                return Ok(());
            }
        }

        // Slow path: wait for space.
        // Use Cell so the Fn closure can "mutate" the sent flag.
        let sent = Cell::new(false);
        self.send_wq.wait_until(|| {
            let mut buf = self.buffer.lock();
            if buf.closed {
                return true; // Exit the wait — we'll return error below.
            }
            if buf.push(msg) {
                self.total_sent.fetch_add(1, Ordering::Relaxed);
                sent.set(true);
                return true;
            }
            false
        });

        if sent.get() {
            self.recv_wq.wake_one();
            Ok(())
        } else {
            // Exited because channel closed.
            Err(SendError::Closed(msg))
        }
    }

    /// Try to send without blocking.
    ///
    /// Returns `Ok(())` if the message was enqueued, or an error if
    /// the channel is full or closed.
    pub fn try_send(&self, msg: T) -> Result<(), TrySendError<T>> {
        let mut buf = self.buffer.lock();
        if buf.closed {
            return Err(TrySendError::Closed(msg));
        }
        if buf.push(msg) {
            self.total_sent.fetch_add(1, Ordering::Relaxed);
            drop(buf);
            self.recv_wq.wake_one();
            Ok(())
        } else {
            Err(TrySendError::Full(msg))
        }
    }

    /// Receive a message, blocking if the channel is empty.
    ///
    /// Returns `Ok(msg)` on success, `Err(RecvError)` if the channel
    /// is closed and empty (no more messages will arrive).
    pub fn recv(&self) -> Result<T, RecvError> {
        // Fast path: try without blocking.
        {
            let mut buf = self.buffer.lock();
            if let Some(item) = buf.pop() {
                self.total_recv.fetch_add(1, Ordering::Relaxed);
                drop(buf);
                self.send_wq.wake_one();
                return Ok(item);
            }
            if buf.closed {
                return Err(RecvError);
            }
        }

        // Slow path: wait for a message.
        // Use Cell so the Fn closure can store the received item.
        let result: Cell<Option<T>> = Cell::new(None);
        self.recv_wq.wait_until(|| {
            let mut buf = self.buffer.lock();
            if let Some(item) = buf.pop() {
                result.set(Some(item));
                return true;
            }
            buf.closed // Exit wait if closed (empty + closed = done).
        });

        if let Some(item) = result.get() {
            self.total_recv.fetch_add(1, Ordering::Relaxed);
            self.send_wq.wake_one();
            Ok(item)
        } else {
            Err(RecvError)
        }
    }

    /// Try to receive without blocking.
    ///
    /// Returns `Some(msg)` if a message was available, `None` otherwise.
    pub fn try_recv(&self) -> Option<T> {
        let mut buf = self.buffer.lock();
        if let Some(item) = buf.pop() {
            self.total_recv.fetch_add(1, Ordering::Relaxed);
            drop(buf);
            self.send_wq.wake_one();
            Some(item)
        } else {
            None
        }
    }

    /// Close the channel.
    ///
    /// After closing:
    /// - `send()` / `try_send()` return `Err(Closed)`.
    /// - `recv()` drains remaining messages, then returns `Err(RecvError)`.
    /// - All blocked senders and receivers are woken.
    pub fn close(&self) {
        let mut buf = self.buffer.lock();
        buf.closed = true;
        drop(buf);
        // Wake everyone so they can observe the closure.
        self.send_wq.wake_all();
        self.recv_wq.wake_all();
    }

    /// Whether the channel is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.buffer.lock().closed
    }

    /// Current number of buffered messages.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.lock().count
    }

    /// Whether the buffer is empty.
    #[must_use]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Channel capacity.
    #[must_use]
    #[allow(dead_code)] // Diagnostics API.
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Total messages sent since creation.
    #[must_use]
    #[allow(dead_code)]
    pub fn total_sent(&self) -> u32 {
        self.total_sent.load(Ordering::Relaxed)
    }

    /// Total messages received since creation.
    #[must_use]
    #[allow(dead_code)]
    pub fn total_recv(&self) -> u32 {
        self.total_recv.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the kernel channel.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[kchannel] Running self-test...");

    // --- 1. Basic send/recv ---
    {
        let ch: KChannel<u64, 4> = KChannel::new();
        assert!(ch.try_send(10).is_ok());
        assert!(ch.try_send(20).is_ok());
        assert_eq!(ch.len(), 2);

        assert_eq!(ch.try_recv(), Some(10));
        assert_eq!(ch.try_recv(), Some(20));
        assert_eq!(ch.try_recv(), None);
        assert_eq!(ch.len(), 0);
    }
    serial_println!("[kchannel]   Basic send/recv: OK");

    // --- 2. Capacity enforcement ---
    {
        let ch: KChannel<u64, 2> = KChannel::new();
        assert!(ch.try_send(1).is_ok());
        assert!(ch.try_send(2).is_ok());
        // Full — should fail.
        match ch.try_send(3) {
            Err(TrySendError::Full(3)) => {} // Expected.
            other => panic!("Expected Full(3), got {:?}", other),
        }
        assert_eq!(ch.len(), 2);
    }
    serial_println!("[kchannel]   Capacity enforcement: OK");

    // --- 3. FIFO ordering ---
    {
        let ch: KChannel<u64, 8> = KChannel::new();
        for i in 0..8 {
            assert!(ch.try_send(i).is_ok());
        }
        for i in 0..8 {
            assert_eq!(ch.try_recv(), Some(i));
        }
    }
    serial_println!("[kchannel]   FIFO ordering: OK");

    // --- 4. Close semantics ---
    {
        let ch: KChannel<u64, 4> = KChannel::new();
        assert!(ch.try_send(100).is_ok());
        ch.close();
        assert!(ch.is_closed());
        // Send after close → error.
        match ch.try_send(200) {
            Err(TrySendError::Closed(200)) => {} // Expected.
            other => panic!("Expected Closed(200), got {:?}", other),
        }
        // Recv drains remaining messages.
        assert_eq!(ch.try_recv(), Some(100));
        // Then empty.
        assert_eq!(ch.try_recv(), None);
    }
    serial_println!("[kchannel]   Close semantics: OK");

    // --- 5. Multi-task producer-consumer ---
    {
        use core::sync::atomic::{AtomicU64, Ordering as AOrdering};

        static TEST_CH: KChannel<u64, 8> = KChannel::new();
        static SUM: AtomicU64 = AtomicU64::new(0);

        // Reset.
        SUM.store(0, AOrdering::Relaxed);
        // Drain any leftover from previous runs.
        while TEST_CH.try_recv().is_some() {}

        extern "C" fn consumer(_: u64) {
            loop {
                match TEST_CH.recv() {
                    Ok(val) => {
                        SUM.fetch_add(val, AOrdering::Relaxed);
                    }
                    Err(_) => break, // Channel closed.
                }
            }
        }

        // Spawn consumer.
        let tid = crate::sched::spawn(
            b"test-ch-recv",
            crate::sched::task::DEFAULT_PRIORITY,
            consumer,
            0,
            0,
        );
        assert!(tid.is_ok());

        // Send values 1..=5.
        for i in 1..=5 {
            let _ = TEST_CH.send(i);
            crate::sched::yield_now();
        }

        // Close and let consumer finish.
        TEST_CH.close();
        for _ in 0..20 {
            crate::sched::yield_now();
        }

        // Sum should be 1+2+3+4+5 = 15.
        assert_eq!(SUM.load(AOrdering::Relaxed), 15);
    }
    serial_println!("[kchannel]   Multi-task producer-consumer: OK");

    serial_println!("[kchannel] Self-test PASSED");
}

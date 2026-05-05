//! IPC statistics — per-mechanism usage and performance tracking.
//!
//! Tracks message rates, sizes, and latency across all IPC mechanisms
//! (channels, pipes, shared memory, eventfd, completion ports).  In a
//! microkernel where almost everything communicates via IPC, understanding
//! message flow is critical for performance tuning.
//!
//! ## What is Tracked
//!
//! - **Channels**: messages sent/received, bytes transferred, avg message size.
//! - **Pipes**: bytes written/read, write/read operations.
//! - **Shared memory**: regions created/destroyed, total mapped bytes.
//! - **Eventfd**: signals sent, wakeups delivered.
//! - **Completion ports**: notifications posted, waits completed.
//! - **Futexes**: wait/wake operations, contention events.
//!
//! ## Design
//!
//! Per-mechanism atomic counters with zero allocation overhead.  Stats
//! accumulate since boot (or last reset).  The `ipcstat` kshell command
//! presents a unified view.
//!
//! ## Overhead
//!
//! One atomic increment per IPC operation (~2-5ns).  No allocation,
//! no locking, no contention between CPUs (each counter is updated
//! only from one context at a time in practice).
//!
//! ## References
//!
//! - Fuchsia object diagnostics (`k ipc` commands)
//! - Linux /proc/net/stat — per-protocol statistics
//! - QNX pulse/message statistics

use core::sync::atomic::{AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Per-mechanism counters
// ---------------------------------------------------------------------------

/// Channel IPC statistics.
struct ChannelCounters {
    /// Messages sent (enqueued).
    sends: AtomicU64,
    /// Messages received (dequeued).
    recvs: AtomicU64,
    /// Bytes sent (sum of all message payloads).
    bytes_sent: AtomicU64,
    /// Send operations that blocked (queue full).
    send_blocks: AtomicU64,
    /// Receive operations that blocked (queue empty).
    recv_blocks: AtomicU64,
    /// Channels created.
    channels_created: AtomicU64,
    /// Channels destroyed (both ends closed).
    channels_destroyed: AtomicU64,
}

/// Pipe IPC statistics.
struct PipeCounters {
    /// Write operations.
    writes: AtomicU64,
    /// Read operations.
    reads: AtomicU64,
    /// Bytes written.
    bytes_written: AtomicU64,
    /// Bytes read.
    bytes_read: AtomicU64,
    /// Write operations that blocked (pipe full).
    write_blocks: AtomicU64,
    /// Read operations that blocked (pipe empty).
    read_blocks: AtomicU64,
    /// Pipes created.
    pipes_created: AtomicU64,
}

/// Shared memory statistics.
struct ShmCounters {
    /// Regions created.
    regions_created: AtomicU64,
    /// Regions destroyed.
    regions_destroyed: AtomicU64,
    /// Total bytes mapped (sum across all regions).
    total_bytes_mapped: AtomicU64,
}

/// Eventfd statistics.
struct EventfdCounters {
    /// Signal operations (write to increment counter).
    signals: AtomicU64,
    /// Read operations (consume counter value).
    reads: AtomicU64,
    /// Wakeups delivered (blocked reader unblocked).
    wakeups: AtomicU64,
    /// Eventfds created.
    created: AtomicU64,
}

/// Completion port statistics.
struct CompletionCounters {
    /// Notifications posted.
    posts: AtomicU64,
    /// Wait operations completed (returned results).
    waits: AtomicU64,
    /// Wait operations that blocked (no ready items).
    wait_blocks: AtomicU64,
    /// Completion ports created.
    created: AtomicU64,
}

/// Futex statistics.
struct FutexCounters {
    /// Wait operations.
    waits: AtomicU64,
    /// Wake operations.
    wakes: AtomicU64,
    /// Threads actually woken.
    threads_woken: AtomicU64,
    /// Waits that returned immediately (value mismatch).
    spurious_waits: AtomicU64,
}

// ---------------------------------------------------------------------------
// Static storage
// ---------------------------------------------------------------------------

static CHANNEL: ChannelCounters = ChannelCounters {
    sends: AtomicU64::new(0),
    recvs: AtomicU64::new(0),
    bytes_sent: AtomicU64::new(0),
    send_blocks: AtomicU64::new(0),
    recv_blocks: AtomicU64::new(0),
    channels_created: AtomicU64::new(0),
    channels_destroyed: AtomicU64::new(0),
};

static PIPE: PipeCounters = PipeCounters {
    writes: AtomicU64::new(0),
    reads: AtomicU64::new(0),
    bytes_written: AtomicU64::new(0),
    bytes_read: AtomicU64::new(0),
    write_blocks: AtomicU64::new(0),
    read_blocks: AtomicU64::new(0),
    pipes_created: AtomicU64::new(0),
};

static SHM: ShmCounters = ShmCounters {
    regions_created: AtomicU64::new(0),
    regions_destroyed: AtomicU64::new(0),
    total_bytes_mapped: AtomicU64::new(0),
};

static EVENTFD: EventfdCounters = EventfdCounters {
    signals: AtomicU64::new(0),
    reads: AtomicU64::new(0),
    wakeups: AtomicU64::new(0),
    created: AtomicU64::new(0),
};

static COMPLETION: CompletionCounters = CompletionCounters {
    posts: AtomicU64::new(0),
    waits: AtomicU64::new(0),
    wait_blocks: AtomicU64::new(0),
    created: AtomicU64::new(0),
};

static FUTEX: FutexCounters = FutexCounters {
    waits: AtomicU64::new(0),
    wakes: AtomicU64::new(0),
    threads_woken: AtomicU64::new(0),
    spurious_waits: AtomicU64::new(0),
};

// ---------------------------------------------------------------------------
// Public API — recording (called by IPC subsystems)
// ---------------------------------------------------------------------------

// --- Channel ---

/// Record a channel message send.
#[inline]
pub fn channel_send(bytes: u64) {
    CHANNEL.sends.fetch_add(1, Ordering::Relaxed);
    CHANNEL.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
}

/// Record a channel message receive.
#[inline]
pub fn channel_recv() {
    CHANNEL.recvs.fetch_add(1, Ordering::Relaxed);
}

/// Record a channel send that blocked.
#[inline]
pub fn channel_send_block() {
    CHANNEL.send_blocks.fetch_add(1, Ordering::Relaxed);
}

/// Record a channel recv that blocked.
#[inline]
pub fn channel_recv_block() {
    CHANNEL.recv_blocks.fetch_add(1, Ordering::Relaxed);
}

/// Record channel creation.
#[inline]
pub fn channel_created() {
    CHANNEL.channels_created.fetch_add(1, Ordering::Relaxed);
}

/// Record channel destruction.
#[inline]
pub fn channel_destroyed() {
    CHANNEL.channels_destroyed.fetch_add(1, Ordering::Relaxed);
}

// --- Pipe ---

/// Record a pipe write.
#[inline]
pub fn pipe_write(bytes: u64) {
    PIPE.writes.fetch_add(1, Ordering::Relaxed);
    PIPE.bytes_written.fetch_add(bytes, Ordering::Relaxed);
}

/// Record a pipe read.
#[inline]
pub fn pipe_read(bytes: u64) {
    PIPE.reads.fetch_add(1, Ordering::Relaxed);
    PIPE.bytes_read.fetch_add(bytes, Ordering::Relaxed);
}

/// Record a pipe write that blocked.
#[inline]
pub fn pipe_write_block() {
    PIPE.write_blocks.fetch_add(1, Ordering::Relaxed);
}

/// Record a pipe read that blocked.
#[inline]
pub fn pipe_read_block() {
    PIPE.read_blocks.fetch_add(1, Ordering::Relaxed);
}

/// Record pipe creation.
#[inline]
pub fn pipe_created() {
    PIPE.pipes_created.fetch_add(1, Ordering::Relaxed);
}

// --- Shared memory ---

/// Record shared memory region creation.
#[inline]
pub fn shm_created(bytes: u64) {
    SHM.regions_created.fetch_add(1, Ordering::Relaxed);
    SHM.total_bytes_mapped.fetch_add(bytes, Ordering::Relaxed);
}

/// Record shared memory region destruction.
#[inline]
pub fn shm_destroyed(bytes: u64) {
    SHM.regions_destroyed.fetch_add(1, Ordering::Relaxed);
    SHM.total_bytes_mapped.fetch_sub(bytes, Ordering::Relaxed);
}

// --- Eventfd ---

/// Record an eventfd signal (write).
#[inline]
pub fn eventfd_signal() {
    EVENTFD.signals.fetch_add(1, Ordering::Relaxed);
}

/// Record an eventfd read.
#[inline]
pub fn eventfd_read() {
    EVENTFD.reads.fetch_add(1, Ordering::Relaxed);
}

/// Record an eventfd wakeup (blocked reader unblocked).
#[inline]
pub fn eventfd_wakeup() {
    EVENTFD.wakeups.fetch_add(1, Ordering::Relaxed);
}

/// Record eventfd creation.
#[inline]
pub fn eventfd_created() {
    EVENTFD.created.fetch_add(1, Ordering::Relaxed);
}

// --- Completion port ---

/// Record a completion port notification post.
#[inline]
pub fn completion_post() {
    COMPLETION.posts.fetch_add(1, Ordering::Relaxed);
}

/// Record a completion wait that returned results.
#[inline]
pub fn completion_wait() {
    COMPLETION.waits.fetch_add(1, Ordering::Relaxed);
}

/// Record a completion wait that blocked.
#[inline]
pub fn completion_wait_block() {
    COMPLETION.wait_blocks.fetch_add(1, Ordering::Relaxed);
}

/// Record completion port creation.
#[inline]
pub fn completion_created() {
    COMPLETION.created.fetch_add(1, Ordering::Relaxed);
}

// --- Futex ---

/// Record a futex wait operation.
#[inline]
pub fn futex_wait() {
    FUTEX.waits.fetch_add(1, Ordering::Relaxed);
}

/// Record a futex wake operation.
#[inline]
pub fn futex_wake(threads_woken: u32) {
    FUTEX.wakes.fetch_add(1, Ordering::Relaxed);
    FUTEX.threads_woken.fetch_add(u64::from(threads_woken), Ordering::Relaxed);
}

/// Record a spurious futex wait (value mismatch, returned immediately).
#[inline]
pub fn futex_spurious() {
    FUTEX.spurious_waits.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Public API — querying
// ---------------------------------------------------------------------------

/// Snapshot of all IPC statistics.
#[derive(Debug, Clone)]
pub struct IpcStats {
    // Channels
    pub channel_sends: u64,
    pub channel_recvs: u64,
    pub channel_bytes: u64,
    pub channel_send_blocks: u64,
    pub channel_recv_blocks: u64,
    pub channels_created: u64,
    pub channels_destroyed: u64,
    // Pipes
    pub pipe_writes: u64,
    pub pipe_reads: u64,
    pub pipe_bytes_written: u64,
    pub pipe_bytes_read: u64,
    pub pipe_write_blocks: u64,
    pub pipe_read_blocks: u64,
    pub pipes_created: u64,
    // Shared memory
    pub shm_regions_created: u64,
    pub shm_regions_destroyed: u64,
    pub shm_bytes_mapped: u64,
    // Eventfd
    pub eventfd_signals: u64,
    pub eventfd_reads: u64,
    pub eventfd_wakeups: u64,
    pub eventfd_created: u64,
    // Completion
    pub completion_posts: u64,
    pub completion_waits: u64,
    pub completion_wait_blocks: u64,
    pub completion_created: u64,
    // Futex
    pub futex_waits: u64,
    pub futex_wakes: u64,
    pub futex_threads_woken: u64,
    pub futex_spurious: u64,
}

/// Get a snapshot of all IPC statistics.
#[must_use]
pub fn snapshot() -> IpcStats {
    IpcStats {
        channel_sends: CHANNEL.sends.load(Ordering::Relaxed),
        channel_recvs: CHANNEL.recvs.load(Ordering::Relaxed),
        channel_bytes: CHANNEL.bytes_sent.load(Ordering::Relaxed),
        channel_send_blocks: CHANNEL.send_blocks.load(Ordering::Relaxed),
        channel_recv_blocks: CHANNEL.recv_blocks.load(Ordering::Relaxed),
        channels_created: CHANNEL.channels_created.load(Ordering::Relaxed),
        channels_destroyed: CHANNEL.channels_destroyed.load(Ordering::Relaxed),
        pipe_writes: PIPE.writes.load(Ordering::Relaxed),
        pipe_reads: PIPE.reads.load(Ordering::Relaxed),
        pipe_bytes_written: PIPE.bytes_written.load(Ordering::Relaxed),
        pipe_bytes_read: PIPE.bytes_read.load(Ordering::Relaxed),
        pipe_write_blocks: PIPE.write_blocks.load(Ordering::Relaxed),
        pipe_read_blocks: PIPE.read_blocks.load(Ordering::Relaxed),
        pipes_created: PIPE.pipes_created.load(Ordering::Relaxed),
        shm_regions_created: SHM.regions_created.load(Ordering::Relaxed),
        shm_regions_destroyed: SHM.regions_destroyed.load(Ordering::Relaxed),
        shm_bytes_mapped: SHM.total_bytes_mapped.load(Ordering::Relaxed),
        eventfd_signals: EVENTFD.signals.load(Ordering::Relaxed),
        eventfd_reads: EVENTFD.reads.load(Ordering::Relaxed),
        eventfd_wakeups: EVENTFD.wakeups.load(Ordering::Relaxed),
        eventfd_created: EVENTFD.created.load(Ordering::Relaxed),
        completion_posts: COMPLETION.posts.load(Ordering::Relaxed),
        completion_waits: COMPLETION.waits.load(Ordering::Relaxed),
        completion_wait_blocks: COMPLETION.wait_blocks.load(Ordering::Relaxed),
        completion_created: COMPLETION.created.load(Ordering::Relaxed),
        futex_waits: FUTEX.waits.load(Ordering::Relaxed),
        futex_wakes: FUTEX.wakes.load(Ordering::Relaxed),
        futex_threads_woken: FUTEX.threads_woken.load(Ordering::Relaxed),
        futex_spurious: FUTEX.spurious_waits.load(Ordering::Relaxed),
    }
}

/// Reset all IPC statistics.
pub fn reset() {
    // Channels
    CHANNEL.sends.store(0, Ordering::Relaxed);
    CHANNEL.recvs.store(0, Ordering::Relaxed);
    CHANNEL.bytes_sent.store(0, Ordering::Relaxed);
    CHANNEL.send_blocks.store(0, Ordering::Relaxed);
    CHANNEL.recv_blocks.store(0, Ordering::Relaxed);
    CHANNEL.channels_created.store(0, Ordering::Relaxed);
    CHANNEL.channels_destroyed.store(0, Ordering::Relaxed);
    // Pipes
    PIPE.writes.store(0, Ordering::Relaxed);
    PIPE.reads.store(0, Ordering::Relaxed);
    PIPE.bytes_written.store(0, Ordering::Relaxed);
    PIPE.bytes_read.store(0, Ordering::Relaxed);
    PIPE.write_blocks.store(0, Ordering::Relaxed);
    PIPE.read_blocks.store(0, Ordering::Relaxed);
    PIPE.pipes_created.store(0, Ordering::Relaxed);
    // SHM
    SHM.regions_created.store(0, Ordering::Relaxed);
    SHM.regions_destroyed.store(0, Ordering::Relaxed);
    SHM.total_bytes_mapped.store(0, Ordering::Relaxed);
    // Eventfd
    EVENTFD.signals.store(0, Ordering::Relaxed);
    EVENTFD.reads.store(0, Ordering::Relaxed);
    EVENTFD.wakeups.store(0, Ordering::Relaxed);
    EVENTFD.created.store(0, Ordering::Relaxed);
    // Completion
    COMPLETION.posts.store(0, Ordering::Relaxed);
    COMPLETION.waits.store(0, Ordering::Relaxed);
    COMPLETION.wait_blocks.store(0, Ordering::Relaxed);
    COMPLETION.created.store(0, Ordering::Relaxed);
    // Futex
    FUTEX.waits.store(0, Ordering::Relaxed);
    FUTEX.wakes.store(0, Ordering::Relaxed);
    FUTEX.threads_woken.store(0, Ordering::Relaxed);
    FUTEX.spurious_waits.store(0, Ordering::Relaxed);
}

/// Total IPC operations (sends + recvs + writes + reads + signals + waits).
#[must_use]
pub fn total_operations() -> u64 {
    CHANNEL.sends.load(Ordering::Relaxed)
        .saturating_add(CHANNEL.recvs.load(Ordering::Relaxed))
        .saturating_add(PIPE.writes.load(Ordering::Relaxed))
        .saturating_add(PIPE.reads.load(Ordering::Relaxed))
        .saturating_add(EVENTFD.signals.load(Ordering::Relaxed))
        .saturating_add(EVENTFD.reads.load(Ordering::Relaxed))
        .saturating_add(FUTEX.waits.load(Ordering::Relaxed))
        .saturating_add(FUTEX.wakes.load(Ordering::Relaxed))
        .saturating_add(COMPLETION.posts.load(Ordering::Relaxed))
}

/// Average channel message size (0 if no messages sent).
#[must_use]
pub fn avg_channel_msg_size() -> u64 {
    let sends = CHANNEL.sends.load(Ordering::Relaxed);
    if sends == 0 {
        return 0;
    }
    CHANNEL.bytes_sent.load(Ordering::Relaxed) / sends
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for IPC statistics.
pub fn self_test() {
    serial_println!("[ipc_stats] Running self-test...");

    // Test 1: Reset and verify zeroes.
    reset();
    let s = snapshot();
    assert_eq!(s.channel_sends, 0);
    assert_eq!(s.pipe_writes, 0);
    assert_eq!(s.futex_waits, 0);
    serial_println!("[ipc_stats]   Reset: OK");

    // Test 2: Channel counters.
    channel_send(256);
    channel_send(128);
    channel_recv();
    channel_send_block();
    channel_created();

    let s = snapshot();
    assert_eq!(s.channel_sends, 2);
    assert_eq!(s.channel_bytes, 384);
    assert_eq!(s.channel_recvs, 1);
    assert_eq!(s.channel_send_blocks, 1);
    assert_eq!(s.channels_created, 1);
    serial_println!("[ipc_stats]   Channel: OK (sends=2, bytes=384)");

    // Test 3: Pipe counters.
    pipe_write(1024);
    pipe_read(512);
    pipe_write_block();
    pipe_created();

    let s = snapshot();
    assert_eq!(s.pipe_writes, 1);
    assert_eq!(s.pipe_bytes_written, 1024);
    assert_eq!(s.pipe_reads, 1);
    assert_eq!(s.pipe_bytes_read, 512);
    assert_eq!(s.pipe_write_blocks, 1);
    assert_eq!(s.pipes_created, 1);
    serial_println!("[ipc_stats]   Pipe: OK (writes=1, reads=1)");

    // Test 4: Shared memory counters.
    shm_created(4096);
    shm_created(8192);
    shm_destroyed(4096);

    let s = snapshot();
    assert_eq!(s.shm_regions_created, 2);
    assert_eq!(s.shm_regions_destroyed, 1);
    assert_eq!(s.shm_bytes_mapped, 8192); // 4096 + 8192 - 4096
    serial_println!("[ipc_stats]   SHM: OK (created=2, active_bytes=8192)");

    // Test 5: Eventfd counters.
    eventfd_created();
    eventfd_signal();
    eventfd_signal();
    eventfd_read();
    eventfd_wakeup();

    let s = snapshot();
    assert_eq!(s.eventfd_created, 1);
    assert_eq!(s.eventfd_signals, 2);
    assert_eq!(s.eventfd_reads, 1);
    assert_eq!(s.eventfd_wakeups, 1);
    serial_println!("[ipc_stats]   Eventfd: OK (signals=2, wakeups=1)");

    // Test 6: Completion port counters.
    completion_created();
    completion_post();
    completion_post();
    completion_wait();
    completion_wait_block();

    let s = snapshot();
    assert_eq!(s.completion_created, 1);
    assert_eq!(s.completion_posts, 2);
    assert_eq!(s.completion_waits, 1);
    assert_eq!(s.completion_wait_blocks, 1);
    serial_println!("[ipc_stats]   Completion: OK (posts=2, waits=1)");

    // Test 7: Futex counters.
    futex_wait();
    futex_wait();
    futex_wake(3);
    futex_spurious();

    let s = snapshot();
    assert_eq!(s.futex_waits, 2);
    assert_eq!(s.futex_wakes, 1);
    assert_eq!(s.futex_threads_woken, 3);
    assert_eq!(s.futex_spurious, 1);
    serial_println!("[ipc_stats]   Futex: OK (waits=2, wakes=1, woken=3)");

    // Test 8: Total operations and avg message size.
    let total = total_operations();
    assert!(total > 0);
    let avg = avg_channel_msg_size();
    assert_eq!(avg, 192); // 384 bytes / 2 sends = 192
    serial_println!("[ipc_stats]   Aggregates: OK (total_ops={}, avg_msg={})", total, avg);

    // Cleanup.
    reset();

    serial_println!("[ipc_stats] Self-test PASSED");
}

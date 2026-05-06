//! Wait channel tracking — records what each blocked task is waiting on.
//!
//! When a task blocks (transitions to `Blocked` state), we record the
//! reason it's sleeping.  This allows `ps`/`top` commands to show what
//! each task is stuck on, similar to Unix's WCHAN column.
//!
//! ## Wait Channel Types
//!
//! - **Timer**: sleeping until a specific tick (SYS_SLEEP, ktimer).
//! - **Channel**: waiting to send/receive on an IPC channel.
//! - **Pipe**: blocked on pipe read (empty) or write (full).
//! - **Futex**: waiting on a futex address.
//! - **Mutex**: waiting for a kernel mutex.
//! - **Event**: waiting for a one-shot event or eventfd.
//! - **Join**: waiting for another task to exit.
//! - **Completion**: waiting on a completion port for events.
//! - **IO**: waiting for I/O completion.
//! - **Other**: unspecified reason.
//!
//! ## Design
//!
//! A fixed-size table indexed by task ID (modulo table size).  Each entry
//! stores the wait channel and an optional argument (e.g., the channel
//! handle, futex address, or target tick).  The table is lock-free via
//! atomic operations.
//!
//! ## Integration
//!
//! Subsystems call `wchan::set(task_id, channel, arg)` before blocking
//! and `wchan::clear(task_id)` when the task wakes up.
//!
//! ## References
//!
//! - Linux `/proc/[pid]/wchan` — symbolic wait channel name
//! - FreeBSD `ki_wchan` — wait channel address
//! - ps(1) WCHAN column

use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum tracked tasks (covers task IDs modulo this value).
/// Must be a power of 2.
const TABLE_SIZE: usize = 256;

/// Mask for modular indexing.
const TABLE_MASK: usize = TABLE_SIZE - 1;

// ---------------------------------------------------------------------------
// Wait channel types
// ---------------------------------------------------------------------------

/// The type of resource a task is waiting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WaitChannel {
    /// Not waiting (slot is empty / task is runnable).
    None = 0,
    /// Sleeping until a specific tick.
    Timer = 1,
    /// Waiting on an IPC channel (send or receive).
    Channel = 2,
    /// Blocked on a pipe (read from empty or write to full).
    Pipe = 3,
    /// Waiting on a futex word.
    Futex = 4,
    /// Waiting for a kernel mutex.
    Mutex = 5,
    /// Waiting for an event (eventfd / OnceEvent).
    Event = 6,
    /// Waiting for another task to exit (join).
    Join = 7,
    /// Waiting on a completion port for events.
    Completion = 8,
    /// Waiting for I/O completion.
    Io = 9,
    /// Unspecified wait.
    Other = 10,
}

impl WaitChannel {
    /// Short display name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "-",
            Self::Timer => "timer",
            Self::Channel => "chan",
            Self::Pipe => "pipe",
            Self::Futex => "futex",
            Self::Mutex => "mutex",
            Self::Event => "event",
            Self::Join => "join",
            Self::Completion => "iocp",
            Self::Io => "io",
            Self::Other => "other",
        }
    }

    /// Convert from raw u8.
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Timer,
            2 => Self::Channel,
            3 => Self::Pipe,
            4 => Self::Futex,
            5 => Self::Mutex,
            6 => Self::Event,
            7 => Self::Join,
            8 => Self::Completion,
            9 => Self::Io,
            10 => Self::Other,
            _ => Self::Other,
        }
    }
}

// ---------------------------------------------------------------------------
// Table entry
// ---------------------------------------------------------------------------

/// A single table entry (per-task wait info).
#[repr(C)]
struct WaitEntry {
    /// Wait channel type (atomically written).
    channel: AtomicU8,
    /// Optional argument (address, handle, tick, etc.).
    arg: AtomicU64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Wait channel table indexed by task_id % TABLE_SIZE.
static TABLE: [WaitEntry; TABLE_SIZE] = {
    const ENTRY: WaitEntry = WaitEntry {
        channel: AtomicU8::new(0),
        arg: AtomicU64::new(0),
    };
    [ENTRY; TABLE_SIZE]
};

/// Global counter: total set operations (for stats).
static TOTAL_SETS: AtomicU64 = AtomicU64::new(0);

/// Global counter: total clear operations.
static TOTAL_CLEARS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Record that a task is now waiting on the given channel.
///
/// Called just before the task blocks.  The `arg` provides context:
/// - Timer: the target tick
/// - Channel: the channel handle
/// - Futex: the futex address
/// - Mutex: the mutex address (as u64)
/// - Join: the task ID being waited on
/// - Other: arbitrary context value
#[inline]
pub fn set(task_id: u64, channel: WaitChannel, arg: u64) {
    let idx = task_id as usize & TABLE_MASK;
    TABLE[idx].arg.store(arg, Ordering::Relaxed);
    TABLE[idx].channel.store(channel as u8, Ordering::Release);
    TOTAL_SETS.fetch_add(1, Ordering::Relaxed);
}

/// Clear a task's wait channel (task has woken up).
#[inline]
pub fn clear(task_id: u64) {
    let idx = task_id as usize & TABLE_MASK;
    TABLE[idx].channel.store(WaitChannel::None as u8, Ordering::Release);
    TABLE[idx].arg.store(0, Ordering::Relaxed);
    TOTAL_CLEARS.fetch_add(1, Ordering::Relaxed);
}

/// Query what a task is waiting on.
///
/// Returns `(channel, arg)`.  If the task isn't blocked (or we have
/// no record), returns `(WaitChannel::None, 0)`.
pub fn get(task_id: u64) -> (WaitChannel, u64) {
    let idx = task_id as usize & TABLE_MASK;
    let ch = WaitChannel::from_u8(TABLE[idx].channel.load(Ordering::Acquire));
    let arg = TABLE[idx].arg.load(Ordering::Relaxed);
    (ch, arg)
}

/// Summary statistics.
#[derive(Debug, Clone)]
pub struct WchanStats {
    /// Total set() calls.
    pub total_sets: u64,
    /// Total clear() calls.
    pub total_clears: u64,
    /// Number of tasks currently blocked (channel != None).
    pub currently_blocked: usize,
    /// Breakdown by channel type.
    pub by_channel: [usize; 11],
}

/// Get current wait channel statistics.
pub fn stats() -> WchanStats {
    let mut currently_blocked = 0usize;
    let mut by_channel = [0usize; 11];

    for entry in &TABLE {
        let ch = entry.channel.load(Ordering::Relaxed);
        if ch != 0 {
            currently_blocked += 1;
        }
        let idx = (ch as usize).min(10);
        by_channel[idx] += 1;
    }

    WchanStats {
        total_sets: TOTAL_SETS.load(Ordering::Relaxed),
        total_clears: TOTAL_CLEARS.load(Ordering::Relaxed),
        currently_blocked,
        by_channel,
    }
}

/// Get all currently-blocked entries as a list of (task_id_mod, channel, arg).
///
/// Note: task_id_mod is the slot index (task_id % TABLE_SIZE), not the
/// original task ID.  This is a limitation of the table design — but
/// during normal operation with < 256 total tasks, the slot index == task ID.
pub fn blocked_list(buf: &mut [(u64, WaitChannel, u64)]) -> usize {
    let mut count = 0;
    for (i, entry) in TABLE.iter().enumerate() {
        if count >= buf.len() {
            break;
        }
        let ch = WaitChannel::from_u8(entry.channel.load(Ordering::Relaxed));
        if ch != WaitChannel::None {
            let arg = entry.arg.load(Ordering::Relaxed);
            buf[count] = (i as u64, ch, arg);
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the wait channel tracker.
pub fn self_test() {
    serial_println!("[wchan] Running self-test...");

    // Test 1: Initial state is None.
    let (ch, arg) = get(100);
    assert_eq!(ch, WaitChannel::None);
    assert_eq!(arg, 0);
    serial_println!("[wchan]   Initial state: OK");

    // Test 2: Set a wait channel.
    set(100, WaitChannel::Timer, 5000);
    let (ch, arg) = get(100);
    assert_eq!(ch, WaitChannel::Timer);
    assert_eq!(arg, 5000);
    serial_println!("[wchan]   Set timer: OK");

    // Test 3: Clear.
    clear(100);
    let (ch, _) = get(100);
    assert_eq!(ch, WaitChannel::None);
    serial_println!("[wchan]   Clear: OK");

    // Test 4: Multiple tasks with different channels.
    set(10, WaitChannel::Channel, 42);
    set(11, WaitChannel::Futex, 0xDEAD_BEEF);
    set(12, WaitChannel::Mutex, 0x1234);
    set(13, WaitChannel::Join, 5);

    let (ch10, arg10) = get(10);
    assert_eq!(ch10, WaitChannel::Channel);
    assert_eq!(arg10, 42);

    let (ch11, arg11) = get(11);
    assert_eq!(ch11, WaitChannel::Futex);
    assert_eq!(arg11, 0xDEAD_BEEF);

    let (ch12, _) = get(12);
    assert_eq!(ch12, WaitChannel::Mutex);

    let (ch13, arg13) = get(13);
    assert_eq!(ch13, WaitChannel::Join);
    assert_eq!(arg13, 5);
    serial_println!("[wchan]   Multiple channels: OK");

    // Test 5: Stats.
    let s = stats();
    assert!(s.currently_blocked >= 4, "should have >= 4 blocked tasks");
    assert!(s.by_channel[WaitChannel::Channel as usize] >= 1);
    assert!(s.by_channel[WaitChannel::Futex as usize] >= 1);
    serial_println!("[wchan]   Stats: OK (blocked={})", s.currently_blocked);

    // Test 6: Blocked list.
    let mut buf = [(0u64, WaitChannel::None, 0u64); 16];
    let n = blocked_list(&mut buf);
    assert!(n >= 4);
    serial_println!("[wchan]   Blocked list: OK ({} entries)", n);

    // Cleanup.
    clear(10);
    clear(11);
    clear(12);
    clear(13);

    serial_println!("[wchan] Self-test PASSED");
}

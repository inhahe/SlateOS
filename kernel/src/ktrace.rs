//! Kernel trace buffer — lightweight event recording for debugging.
//!
//! `ktrace` provides a lock-free ring buffer for recording timestamped
//! events from anywhere in the kernel.  Events are stored in a
//! fixed-size circular buffer and can be dumped via the kshell `trace`
//! command for post-mortem analysis.
//!
//! ## Design
//!
//! - **Lock-free**: Uses an atomic write pointer for zero-overhead
//!   recording.  Writers never block — the buffer simply wraps.
//! - **Fixed-size**: 512-entry ring buffer (about 20 KiB).  Old entries
//!   are overwritten when full (most recent events are preserved).
//! - **Low overhead**: Recording a trace event is ~20ns (rdtsc +
//!   atomic increment + one struct write).
//! - **Global enable/disable**: The trace buffer can be paused to
//!   freeze the state for inspection.
//!
//! ## Event Format
//!
//! Each event contains:
//! - Timestamp (TSC cycles since boot)
//! - Category (4-bit enum: sched, mm, ipc, fs, net, irq, etc.)
//! - Event ID (12-bit: specific event within category)
//! - Task ID (which task recorded this event)
//! - Two u64 arguments (event-specific data)
//!
//! ## Usage
//!
//! ```ignore
//! // Record a scheduling event.
//! ktrace::record(Category::Sched, event::CONTEXT_SWITCH, old_tid, new_tid);
//!
//! // Record a page fault.
//! ktrace::record(Category::Mm, event::PAGE_FAULT, fault_addr, flags as u64);
//! ```
//!
//! ## References
//!
//! - Linux ftrace ring buffer (`kernel/trace/ring_buffer.c`)
//! - Windows ETW (Event Tracing for Windows)
//! - FreeBSD KTR (Kernel Trace)

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::bench;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Number of trace entries in the ring buffer.
/// Power of two for efficient modular indexing.
const BUFFER_SIZE: usize = 512;

/// Mask for modular indexing.
const BUFFER_MASK: usize = BUFFER_SIZE - 1;

// ---------------------------------------------------------------------------
// Event categories
// ---------------------------------------------------------------------------

/// Broad category for a trace event.
///
/// Variants not currently emitted (Fs, Net, Sync, Proc, Driver) are
/// reserved for future instrumentation hooks. We define the full
/// classification taxonomy up front so trace consumers can rely on
/// stable category IDs.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Category {
    /// Scheduler: context switches, wake, sleep, spawn, exit.
    Sched = 0,
    /// Memory manager: page faults, alloc, free, reclaim.
    Mm = 1,
    /// IPC: channel send/recv, pipe I/O, completion port.
    Ipc = 2,
    /// Filesystem: open, read, write, close.
    Fs = 3,
    /// Network: packet send/recv, connection state.
    Net = 4,
    /// Interrupts: IRQ entry/exit, softirq.
    Irq = 5,
    /// Syscall: entry/exit.
    Syscall = 6,
    /// Timer: ktimer fire, schedule.
    Timer = 7,
    /// Synchronization: mutex acquire/release, semaphore, barrier.
    Sync = 8,
    /// Process: spawn, exit, signal.
    Proc = 9,
    /// Driver: device I/O, DMA.
    Driver = 10,
    /// General/user-defined.
    General = 15,
}

impl Category {
    /// Short name for display.
    #[allow(dead_code)] // Used by trace dump tooling once wired up.
    pub const fn short_name(self) -> &'static str {
        match self {
            Self::Sched => "sched",
            Self::Mm => "mm",
            Self::Ipc => "ipc",
            Self::Fs => "fs",
            Self::Net => "net",
            Self::Irq => "irq",
            Self::Syscall => "syscall",
            Self::Timer => "timer",
            Self::Sync => "sync",
            Self::Proc => "proc",
            Self::Driver => "driver",
            Self::General => "general",
        }
    }
}

// ---------------------------------------------------------------------------
// Well-known event IDs
// ---------------------------------------------------------------------------

/// Well-known event IDs within each category.
///
/// Constants are defined comprehensively so call sites can opt in over
/// time without revising consumer code. Most aren't yet emitted.
#[allow(dead_code)]
pub mod event {
    // Sched events
    pub const CONTEXT_SWITCH: u16 = 1;
    pub const TASK_SPAWN: u16 = 2;
    pub const TASK_EXIT: u16 = 3;
    pub const TASK_WAKE: u16 = 4;
    pub const TASK_BLOCK: u16 = 5;
    pub const YIELD: u16 = 6;
    pub const PREEMPT: u16 = 7;
    pub const WORK_STEAL: u16 = 8;
    pub const DEFERRED_WAKE: u16 = 9;

    // MM events
    pub const PAGE_FAULT: u16 = 1;
    pub const FRAME_ALLOC: u16 = 2;
    pub const FRAME_FREE: u16 = 3;
    pub const HEAP_ALLOC: u16 = 4;
    pub const HEAP_FREE: u16 = 5;
    pub const SWAP_OUT: u16 = 6;
    pub const SWAP_IN: u16 = 7;
    pub const RECLAIM: u16 = 8;

    // IPC events
    pub const CHANNEL_SEND: u16 = 1;
    pub const CHANNEL_RECV: u16 = 2;
    pub const PIPE_WRITE: u16 = 3;
    pub const PIPE_READ: u16 = 4;

    // IRQ events
    pub const IRQ_ENTER: u16 = 1;
    pub const IRQ_EXIT: u16 = 2;
    pub const SOFTIRQ_ENTER: u16 = 3;
    pub const SOFTIRQ_EXIT: u16 = 4;

    // Syscall events
    pub const SYSCALL_ENTER: u16 = 1;
    pub const SYSCALL_EXIT: u16 = 2;

    // Timer events
    pub const TIMER_FIRE: u16 = 1;
    pub const TIMER_SCHEDULE: u16 = 2;
    pub const TIMER_CANCEL: u16 = 3;
    pub const TIMER_TICK_SHORT: u16 = 4;

    // IPC (continued)
    pub const EVENTFD_SIGNAL: u16 = 5;
    pub const EVENTFD_WAIT: u16 = 6;
    pub const SEM_CREATE: u16 = 7;
    pub const SEM_SIGNAL: u16 = 8;
    pub const SEM_WAIT: u16 = 9;
    pub const SEM_CLOSE: u16 = 10;
    pub const CP_REGISTER: u16 = 11;
    pub const CP_WAIT: u16 = 12;
    pub const CP_NOTIFY: u16 = 13;

    // Sync events
    pub const MUTEX_ACQUIRE: u16 = 1;
    pub const MUTEX_RELEASE: u16 = 2;
    pub const MUTEX_CONTEND: u16 = 3;

    // General
    pub const USER_EVENT: u16 = 1;
}

// ---------------------------------------------------------------------------
// Trace entry
// ---------------------------------------------------------------------------

/// A single trace event entry.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct TraceEntry {
    /// TSC timestamp when the event was recorded.
    pub timestamp: u64,
    /// Task ID that recorded this event.
    pub task_id: u64,
    /// Event category (upper 4 bits) + event ID (lower 12 bits),
    /// packed into a u16.
    pub category_event: u16,
    /// Padding for alignment.
    _pad: u16,
    /// Padding.
    _pad2: u32,
    /// First argument (event-specific).
    pub arg0: u64,
    /// Second argument (event-specific).
    pub arg1: u64,
}

impl TraceEntry {
    pub const fn empty() -> Self {
        Self {
            timestamp: 0,
            task_id: 0,
            category_event: 0,
            _pad: 0,
            _pad2: 0,
            arg0: 0,
            arg1: 0,
        }
    }

    /// Extract the category from the packed field.
    pub fn category(self) -> u8 {
        (self.category_event >> 12) as u8
    }

    /// Extract the event ID from the packed field.
    pub fn event_id(self) -> u16 {
        self.category_event & 0x0FFF
    }

    /// Get the category name.
    pub fn category_name(self) -> &'static str {
        match self.category() {
            0 => "sched",
            1 => "mm",
            2 => "ipc",
            3 => "fs",
            4 => "net",
            5 => "irq",
            6 => "syscall",
            7 => "timer",
            8 => "sync",
            9 => "proc",
            10 => "driver",
            _ => "?",
        }
    }
}

// Ensure the entry is exactly 40 bytes (good cache alignment).
const _: () = assert!(core::mem::size_of::<TraceEntry>() == 40);

// ---------------------------------------------------------------------------
// Global trace buffer
// ---------------------------------------------------------------------------

/// The ring buffer storage.
static mut BUFFER: [TraceEntry; BUFFER_SIZE] = [TraceEntry::empty(); BUFFER_SIZE];

/// Write pointer (next slot to write to).  Wraps modulo BUFFER_SIZE.
static WRITE_POS: AtomicU32 = AtomicU32::new(0);

/// Total events recorded (may exceed BUFFER_SIZE — use modular read).
static TOTAL_EVENTS: AtomicU64 = AtomicU64::new(0);

/// Whether tracing is enabled.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Category filter bitmask: bit N set = category N is enabled.
/// Default: all categories enabled (0xFFFF).
static CATEGORY_MASK: AtomicU32 = AtomicU32::new(0xFFFF);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Record a trace event.
///
/// This is the hot-path recording function.  It's designed to be as
/// fast as possible:
/// - No locks (atomic write pointer)
/// - No allocation
/// - Inline-friendly
///
/// Safe to call from any context (ISR, softirq, task).
#[inline]
pub fn record(category: Category, event_id: u16, arg0: u64, arg1: u64) {
    // Fast-path: check enabled.
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // Category filter.
    let cat_bit = 1u32 << (category as u8);
    if CATEGORY_MASK.load(Ordering::Relaxed) & cat_bit == 0 {
        return;
    }

    let timestamp = bench::rdtsc();
    let task_id = crate::sched::current_task_id();

    // Pack category + event_id.
    let category_event = ((category as u16) << 12) | (event_id & 0x0FFF);

    // Allocate a slot atomically.
    let pos = WRITE_POS.fetch_add(1, Ordering::Relaxed) as usize & BUFFER_MASK;

    // Write the entry.
    // SAFETY: pos is bounded by BUFFER_MASK.  Concurrent writers may
    // interleave slots (acceptable — we're not guaranteeing ordering
    // for overlapping writes).  The worst case is a partially-written
    // entry with mixed fields from two events — the timestamp will
    // reveal the inconsistency.  This is the standard design for
    // lock-free trace buffers (Linux ftrace, FreeBSD KTR).
    unsafe {
        let entry = &mut BUFFER[pos];
        entry.timestamp = timestamp;
        entry.task_id = task_id;
        entry.category_event = category_event;
        entry.arg0 = arg0;
        entry.arg1 = arg1;
    }

    TOTAL_EVENTS.fetch_add(1, Ordering::Relaxed);
}

/// Enable tracing.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Disable tracing (freeze the buffer for inspection).
pub fn disable() {
    ENABLED.store(false, Ordering::Relaxed);
}

/// Whether tracing is currently enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Set the category filter mask.
/// Bit N = 1 means category N is traced.
pub fn set_category_mask(mask: u32) {
    CATEGORY_MASK.store(mask, Ordering::Relaxed);
}

/// Get the current category filter mask.
#[must_use]
pub fn category_mask() -> u32 {
    CATEGORY_MASK.load(Ordering::Relaxed)
}

/// Total events recorded since boot.
#[must_use]
pub fn total_events() -> u64 {
    TOTAL_EVENTS.load(Ordering::Relaxed)
}

/// Number of valid entries currently in the buffer.
#[must_use]
pub fn valid_count() -> usize {
    let total = TOTAL_EVENTS.load(Ordering::Relaxed);
    if total >= BUFFER_SIZE as u64 {
        BUFFER_SIZE
    } else {
        total as usize
    }
}

/// Read the N most recent trace entries into the provided buffer.
///
/// Returns the number of entries written (may be less than `out.len()`
/// if fewer events have been recorded).  Entries are in chronological
/// order (oldest first).
pub fn read_recent(out: &mut [TraceEntry]) -> usize {
    let total = TOTAL_EVENTS.load(Ordering::Relaxed);
    let valid = if total >= BUFFER_SIZE as u64 {
        BUFFER_SIZE
    } else {
        total as usize
    };

    let to_read = out.len().min(valid);
    if to_read == 0 {
        return 0;
    }

    // The write pointer points to the NEXT slot to write.
    // Most recent entry is at (write_pos - 1) & MASK.
    // Oldest entry (in a full buffer) is at write_pos & MASK.
    let write_pos = WRITE_POS.load(Ordering::Relaxed) as usize;

    // Read `to_read` entries starting from (write_pos - to_read).
    for i in 0..to_read {
        let idx = (write_pos.wrapping_sub(to_read).wrapping_add(i)) & BUFFER_MASK;
        // SAFETY: idx is bounded by BUFFER_MASK.  We may read a
        // partially-written entry if a writer is concurrent, but
        // that's acceptable for diagnostic output.
        out[i] = unsafe { BUFFER[idx] };
    }

    to_read
}

/// Clear the trace buffer (reset all counters).
#[allow(dead_code)] // Exposed by the trace control API once a kshell `trace clear` command exists.
pub fn clear() {
    TOTAL_EVENTS.store(0, Ordering::Relaxed);
    WRITE_POS.store(0, Ordering::Relaxed);
    // Zero out the buffer entries.
    for i in 0..BUFFER_SIZE {
        // SAFETY: We're writing to a static array with valid index.
        unsafe {
            BUFFER[i] = TraceEntry::empty();
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the kernel trace buffer.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[ktrace] Running self-test...");

    // Save state.
    let was_enabled = is_enabled();
    let old_total = total_events();

    // --- 1. Record and retrieve ---
    enable();
    record(Category::General, event::USER_EVENT, 0xDEAD, 0xBEEF);
    let new_total = total_events();
    assert!(new_total > old_total);
    serial_println!("[ktrace]   Record event: OK");

    // --- 2. Read back ---
    //
    // We can't assume the entry we just wrote is at the tail of the
    // ring — on SMP, another CPU may have recorded an IRQ / scheduler
    // / timer event between our `record()` and `read_recent()` call.
    // Instead, request a generous window of recent entries and search
    // for the one matching our unique magic argument tuple
    // (USER_EVENT + 0xDEAD + 0xBEEF).  The window size is BUFFER_SIZE
    // so we always find it as long as it hasn't wrapped out.
    let mut entries = [TraceEntry::empty(); BUFFER_SIZE];
    let count = read_recent(&mut entries);
    assert!(count >= 1);
    let ours_slice = &entries[..count];
    let ours = ours_slice
        .iter()
        .rev()
        .find(|e| {
            e.category() == Category::General as u8
                && e.event_id() == event::USER_EVENT
                && e.arg0 == 0xDEAD
                && e.arg1 == 0xBEEF
        })
        .expect("recorded event not found in read_recent window");
    assert!(ours.timestamp > 0);
    serial_println!("[ktrace]   Read back: OK");

    // --- 3. Category filtering ---
    let old_mask = category_mask();
    set_category_mask(0); // Disable all categories.
    let before = total_events();
    record(Category::Sched, event::CONTEXT_SWITCH, 0, 0);
    assert_eq!(total_events(), before); // Should not have recorded.
    set_category_mask(old_mask); // Restore.
    serial_println!("[ktrace]   Category filter: OK");

    // --- 4. Disable/enable ---
    disable();
    let before = total_events();
    record(Category::Mm, event::PAGE_FAULT, 0, 0);
    assert_eq!(total_events(), before); // Should not have recorded.
    enable();
    serial_println!("[ktrace]   Enable/disable: OK");

    // --- 5. Wrap-around (fill buffer beyond capacity) ---
    //
    // Mask everything except General during the loop so timer-driven
    // Sched / Irq events recorded by other CPUs (or by our own timer
    // interrupt firing between iterations) can't inflate the count.
    // Without this mask the assertion is racy — observed 602 vs
    // expected 600 in the wild when boot self-tests run long enough
    // for the periodic timer tick to land inside the 600-iteration
    // window.  Restore the original mask afterwards.
    let saved_mask = category_mask();
    set_category_mask(1u32 << (Category::General as u32));
    let before = total_events();
    for i in 0u64..600 {
        record(Category::General, event::USER_EVENT, i, 0);
    }
    let after = total_events();
    set_category_mask(saved_mask);
    assert_eq!(after - before, 600);
    // valid_count should be capped at BUFFER_SIZE.
    assert_eq!(valid_count(), BUFFER_SIZE);
    serial_println!("[ktrace]   Wrap-around: OK");

    // --- 6. Stats ---
    serial_println!("[ktrace]   Total events: {}", total_events());
    serial_println!("[ktrace]   Buffer size: {}", BUFFER_SIZE);

    // Restore original state.
    if !was_enabled {
        disable();
    }

    serial_println!("[ktrace] Self-test PASSED");
}

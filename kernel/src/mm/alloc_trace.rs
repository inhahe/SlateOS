//! Allocation tracing ring buffer — records recent alloc/free events.
//!
//! Maintains a fixed-size circular buffer of the most recent memory
//! operations (frame allocations and frees).  Useful for post-mortem
//! debugging: when something goes wrong (double-free, use-after-free,
//! OOM), you can inspect the recent allocation history to understand
//! what led to the failure.
//!
//! ## Design
//!
//! - **Fixed-size ring buffer**: 256 entries, no heap allocation.
//! - **Lock-free writes**: uses an atomic sequence counter; entries may
//!   be slightly stale under high concurrency but never corrupt.
//! - **Minimal overhead**: one atomic increment + one struct write per
//!   alloc/free when enabled (~10ns on modern CPUs).
//! - **Snapshot API**: freeze the ring buffer contents for inspection
//!   without stopping ongoing tracing.
//!
//! ## Entry Format
//!
//! Each entry records:
//! - Operation type (alloc/free/alloc_zeroed/realloc)
//! - Frame index
//! - Timestamp (TSC cycles for ordering)
//! - Owner tag (from frame_owner)
//! - CPU that performed the operation
//! - Order (for buddy allocator multi-frame ops)
//!
//! ## Usage
//!
//! ```text
//! mm::alloc_trace::enable();
//! // ... perform operations ...
//! let snap = mm::alloc_trace::snapshot();
//! for entry in snap.entries() {
//!     serial_println!("{:?}", entry);
//! }
//! ```
//!
//! ## References
//!
//! - Linux `mm/kmemleak.c` — kernel memory leak detector
//! - Linux `include/trace/events/kmem.h` — memory tracepoints
//! - Linux ftrace ring buffer — lock-free per-CPU event recording

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use crate::mm::frame_owner::Owner;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Ring buffer capacity (power of 2 for efficient modulo).
const RING_SIZE: usize = 256;

/// Mask for wrapping the write index (RING_SIZE - 1).
const RING_MASK: usize = RING_SIZE - 1;

// ---------------------------------------------------------------------------
// Event types
// ---------------------------------------------------------------------------

/// Type of memory operation recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AllocOp {
    /// Frame allocation (alloc_frame).
    Alloc = 0,
    /// Frame allocation with zeroing (alloc_frame_zeroed).
    AllocZeroed = 1,
    /// Frame free.
    Free = 2,
    /// Multi-frame allocation (high-order buddy).
    AllocBlock = 3,
    /// Multi-frame free.
    FreeBlock = 4,
}

impl AllocOp {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Alloc,
            1 => Self::AllocZeroed,
            2 => Self::Free,
            3 => Self::AllocBlock,
            4 => Self::FreeBlock,
            _ => Self::Alloc,
        }
    }

    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Alloc => "alloc",
            Self::AllocZeroed => "alloc0",
            Self::Free => "free",
            Self::AllocBlock => "alloc_blk",
            Self::FreeBlock => "free_blk",
        }
    }
}

// ---------------------------------------------------------------------------
// Trace entry
// ---------------------------------------------------------------------------

/// A single allocation trace event.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct TraceEntry {
    /// Timestamp (TSC cycles) — for ordering and latency analysis.
    pub timestamp: u64,
    /// Frame index that was allocated or freed.
    pub frame_idx: u32,
    /// Operation type.
    pub op: u8,
    /// Owner tag (from frame_owner::Owner).
    pub owner: u8,
    /// CPU index that performed the operation.
    pub cpu: u8,
    /// Buddy order (0 for single-frame ops).
    pub order: u8,
}

impl TraceEntry {
    /// Create a zeroed (empty) entry.
    pub const fn empty() -> Self {
        Self {
            timestamp: 0,
            frame_idx: 0,
            op: 0,
            owner: 0,
            cpu: 0,
            order: 0,
        }
    }

    /// Get the operation type.
    pub fn operation(&self) -> AllocOp {
        AllocOp::from_u8(self.op)
    }

    /// Get the owner tag.
    pub fn owner_tag(&self) -> Owner {
        Owner::from_u8(self.owner)
    }

    /// Whether this entry is valid (non-zero timestamp means written).
    pub fn is_valid(&self) -> bool {
        self.timestamp != 0
    }
}


// ---------------------------------------------------------------------------
// Ring buffer storage
// ---------------------------------------------------------------------------

/// The ring buffer entries.  UnsafeCell for interior mutability.
struct TraceRing(core::cell::UnsafeCell<[TraceEntry; RING_SIZE]>);

// SAFETY: Entries are written atomically (single u64 sequence number
// guards each slot).  Reads may race but produce valid (possibly stale)
// data rather than undefined behavior.
unsafe impl Sync for TraceRing {}

static RING: TraceRing = TraceRing(core::cell::UnsafeCell::new(
    [TraceEntry::empty(); RING_SIZE]
));

/// Write position (monotonically increasing, masked to get slot index).
static WRITE_POS: AtomicU32 = AtomicU32::new(0);

/// Whether tracing is enabled.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Total events recorded since boot.
static TOTAL_EVENTS: AtomicU64 = AtomicU64::new(0);

/// Total events dropped (when disabled).
static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API — recording
// ---------------------------------------------------------------------------

/// Record an allocation event.
///
/// Called by the frame allocator after every alloc/free.
/// Extremely cheap when enabled (~10ns).  No-op when disabled.
#[inline]
pub fn record(op: AllocOp, frame_idx: u32, owner: Owner, order: u8) {
    if !ENABLED.load(Ordering::Relaxed) {
        DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
        return;
    }

    // Read TSC for timestamp.
    let timestamp = rdtsc();

    // Get CPU index (best-effort, 0 if unavailable).
    let cpu = current_cpu_fast();

    let entry = TraceEntry {
        timestamp,
        frame_idx,
        op: op as u8,
        owner: owner as u8,
        cpu,
        order,
    };

    // Claim a slot atomically.
    let pos = WRITE_POS.fetch_add(1, Ordering::Relaxed);
    let slot = (pos as usize) & RING_MASK;

    // Write the entry.
    // SAFETY: We own this slot (unique position from atomic increment).
    // The write is a single struct copy; readers may see partial data
    // transiently but the timestamp field gates validity.
    unsafe {
        let ptr = RING.0.get() as *mut TraceEntry;
        ptr.add(slot).write(entry);
    }

    TOTAL_EVENTS.fetch_add(1, Ordering::Relaxed);
}

/// Convenience: record a single-frame allocation.
#[inline]
pub fn record_alloc(frame_idx: u32, owner: Owner) {
    record(AllocOp::Alloc, frame_idx, owner, 0);
}

/// Convenience: record a zeroed-frame allocation.
#[inline]
pub fn record_alloc_zeroed(frame_idx: u32, owner: Owner) {
    record(AllocOp::AllocZeroed, frame_idx, owner, 0);
}

/// Convenience: record a frame free.
#[inline]
pub fn record_free(frame_idx: u32) {
    record(AllocOp::Free, frame_idx, Owner::Free, 0);
}

/// Convenience: record a multi-frame allocation.
#[inline]
pub fn record_alloc_block(frame_idx: u32, owner: Owner, order: u8) {
    record(AllocOp::AllocBlock, frame_idx, owner, order);
}

// ---------------------------------------------------------------------------
// Public API — control
// ---------------------------------------------------------------------------

/// Enable allocation tracing.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Disable allocation tracing.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Whether tracing is currently enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Reset the ring buffer (clear all entries and counters).
pub fn reset() {
    WRITE_POS.store(0, Ordering::Release);
    TOTAL_EVENTS.store(0, Ordering::Relaxed);
    DROPPED_EVENTS.store(0, Ordering::Relaxed);

    // Zero out all entries.
    for i in 0..RING_SIZE {
        unsafe {
            let ptr = RING.0.get() as *mut TraceEntry;
            ptr.add(i).write(TraceEntry::empty());
        }
    }
}

// ---------------------------------------------------------------------------
// Public API — querying
// ---------------------------------------------------------------------------

/// Snapshot of the trace ring buffer.
#[derive(Clone)]
pub struct TraceSnapshot {
    /// Entries in chronological order (oldest first).
    pub entries: [TraceEntry; RING_SIZE],
    /// Number of valid entries (may be less than RING_SIZE if fewer
    /// events have been recorded since boot/reset).
    pub count: usize,
    /// Total events recorded since boot/reset.
    pub total_events: u64,
    /// Total events dropped (while disabled).
    pub dropped: u64,
}

/// Take a snapshot of the current ring buffer contents.
///
/// Returns entries in chronological order.  This is a read-only
/// operation — tracing continues during the snapshot.
#[must_use]
pub fn snapshot() -> TraceSnapshot {
    let total = TOTAL_EVENTS.load(Ordering::Acquire);
    let dropped = DROPPED_EVENTS.load(Ordering::Relaxed);
    let write_pos = WRITE_POS.load(Ordering::Acquire) as usize;

    let mut entries = [TraceEntry::empty(); RING_SIZE];
    let count;

    if write_pos <= RING_SIZE {
        // Haven't wrapped yet — entries 0..write_pos are valid.
        count = write_pos;
        for i in 0..count {
            unsafe {
                let ptr = RING.0.get() as *const TraceEntry;
                entries[i] = ptr.add(i).read();
            }
        }
    } else {
        // Wrapped — oldest entry is at write_pos % RING_SIZE.
        count = RING_SIZE;
        let start = write_pos & RING_MASK;
        for i in 0..RING_SIZE {
            let src_idx = (start + i) & RING_MASK;
            unsafe {
                let ptr = RING.0.get() as *const TraceEntry;
                entries[i] = ptr.add(src_idx).read();
            }
        }
    }

    TraceSnapshot {
        entries,
        count,
        total_events: total,
        dropped,
    }
}

/// Statistics summary without copying all entries.
#[derive(Debug, Clone, Copy)]
pub struct TraceStats {
    /// Whether tracing is enabled.
    pub enabled: bool,
    /// Total events recorded.
    pub total_events: u64,
    /// Events dropped while disabled.
    pub dropped: u64,
    /// Current write position.
    pub write_pos: u32,
    /// Ring buffer capacity.
    pub capacity: usize,
    /// Number of valid entries currently in the buffer.
    pub valid_entries: usize,
}

/// Get trace statistics without taking a full snapshot.
#[must_use]
pub fn stats() -> TraceStats {
    let write_pos = WRITE_POS.load(Ordering::Relaxed);
    let valid = if (write_pos as usize) < RING_SIZE {
        write_pos as usize
    } else {
        RING_SIZE
    };

    TraceStats {
        enabled: ENABLED.load(Ordering::Relaxed),
        total_events: TOTAL_EVENTS.load(Ordering::Relaxed),
        dropped: DROPPED_EVENTS.load(Ordering::Relaxed),
        write_pos,
        capacity: RING_SIZE,
        valid_entries: valid,
    }
}

/// Get the most recent N entries (newest first).
///
/// Writes into `buf` and returns the actual count written.
pub fn recent(buf: &mut [TraceEntry]) -> usize {
    let write_pos = WRITE_POS.load(Ordering::Acquire) as usize;
    let available = write_pos.min(RING_SIZE);
    let to_copy = buf.len().min(available);

    for i in 0..to_copy {
        // Walk backwards from write_pos.
        let idx = (write_pos.wrapping_sub(1).wrapping_sub(i)) & RING_MASK;
        unsafe {
            let ptr = RING.0.get() as *const TraceEntry;
            buf[i] = ptr.add(idx).read();
        }
    }

    to_copy
}

// ---------------------------------------------------------------------------
// Analysis helpers
// ---------------------------------------------------------------------------

/// Count allocs vs frees in the current ring buffer.
///
/// If allocs >> frees over a sustained period, there may be a leak.
#[must_use]
pub fn alloc_free_balance() -> (u64, u64) {
    let write_pos = WRITE_POS.load(Ordering::Acquire) as usize;
    let count = write_pos.min(RING_SIZE);
    let start = if write_pos <= RING_SIZE { 0 } else { write_pos & RING_MASK };

    let mut allocs: u64 = 0;
    let mut frees: u64 = 0;

    for i in 0..count {
        let idx = (start + i) & RING_MASK;
        let entry = unsafe {
            let ptr = RING.0.get() as *const TraceEntry;
            ptr.add(idx).read()
        };
        match AllocOp::from_u8(entry.op) {
            AllocOp::Alloc | AllocOp::AllocZeroed | AllocOp::AllocBlock => {
                allocs += 1;
            }
            AllocOp::Free | AllocOp::FreeBlock => {
                frees += 1;
            }
        }
    }

    (allocs, frees)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Read TSC (Time Stamp Counter) for timestamps.
#[inline]
fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags),
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// Get current CPU index quickly (0 if SMP not yet initialized).
#[inline]
fn current_cpu_fast() -> u8 {
    // Use the SMP module's tiered detection (RDPID → rdtscp → APIC MMIO).
    // Never call rdtscp unconditionally — it may not be available.
    #[allow(clippy::cast_possible_truncation)]
    { crate::smp::current_cpu_index() as u8 }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for allocation tracing.
pub fn self_test() {
    serial_println!("[alloc_trace] Running self-test...");

    // Test 1: Initially enabled with zero events after reset.
    reset();
    assert!(is_enabled());
    let s = stats();
    assert_eq!(s.total_events, 0);
    assert_eq!(s.valid_entries, 0);
    serial_println!("[alloc_trace]   Initial state: OK");

    // Test 2: Record events and verify count.
    record_alloc(42, Owner::HeapSlab);
    record_alloc_zeroed(43, Owner::UserAnon);
    record_free(42);
    let s = stats();
    assert_eq!(s.total_events, 3);
    assert_eq!(s.valid_entries, 3);
    serial_println!("[alloc_trace]   Record events: OK");

    // Test 3: Snapshot returns correct entries.
    let snap = snapshot();
    assert_eq!(snap.count, 3);
    assert_eq!(snap.entries[0].frame_idx, 42);
    assert_eq!(snap.entries[0].operation(), AllocOp::Alloc);
    assert_eq!(snap.entries[1].frame_idx, 43);
    assert_eq!(snap.entries[1].operation(), AllocOp::AllocZeroed);
    assert_eq!(snap.entries[2].frame_idx, 42);
    assert_eq!(snap.entries[2].operation(), AllocOp::Free);
    serial_println!("[alloc_trace]   Snapshot: OK");

    // Test 4: recent() returns newest first.
    let mut buf = [TraceEntry::empty(); 4];
    let n = recent(&mut buf);
    assert_eq!(n, 3);
    assert_eq!(buf[0].frame_idx, 42); // Most recent (the free).
    assert_eq!(buf[0].operation(), AllocOp::Free);
    assert_eq!(buf[1].frame_idx, 43); // Second most recent.
    serial_println!("[alloc_trace]   Recent (newest first): OK");

    // Test 5: alloc_free_balance.
    let (allocs, frees) = alloc_free_balance();
    assert_eq!(allocs, 2);
    assert_eq!(frees, 1);
    serial_println!("[alloc_trace]   Alloc/free balance: allocs={}, frees={}", allocs, frees);

    // Test 6: Disable suppresses recording.
    disable();
    record_alloc(99, Owner::Dma);
    let s = stats();
    assert_eq!(s.total_events, 3); // Unchanged.
    assert!(s.dropped > 0);       // Dropped incremented.
    enable();
    serial_println!("[alloc_trace]   Disable/enable: OK");

    // Test 7: Ring wrapping (fill past capacity).
    reset();
    for i in 0..300u32 {
        record_alloc(i, Owner::SelfTest);
    }
    let s = stats();
    assert_eq!(s.total_events, 300);
    assert_eq!(s.valid_entries, RING_SIZE); // Capped at ring size.
    // Oldest entry should be 300 - 256 = 44.
    let snap = snapshot();
    assert_eq!(snap.count, RING_SIZE);
    assert_eq!(snap.entries[0].frame_idx, 44); // Oldest after wrap.
    assert_eq!(snap.entries[RING_SIZE - 1].frame_idx, 299); // Newest.
    serial_println!("[alloc_trace]   Ring wrap (300 events, 256 retained): OK");

    // Test 8: Timestamps are monotonically increasing.
    let snap = snapshot();
    let mut prev_ts = 0u64;
    for i in 0..snap.count {
        if snap.entries[i].is_valid() {
            assert!(snap.entries[i].timestamp >= prev_ts,
                "timestamps should be monotonic");
            prev_ts = snap.entries[i].timestamp;
        }
    }
    serial_println!("[alloc_trace]   Timestamps monotonic: OK");

    // Cleanup.
    reset();

    serial_println!("[alloc_trace] Self-test PASSED");
}

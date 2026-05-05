//! Kernel object lifecycle tracking.
//!
//! Tracks creation and destruction of all kernel object types (tasks,
//! channels, pipes, capabilities, timers, etc.) to detect resource leaks
//! and provide a unified view of system object usage.
//!
//! ## Motivation
//!
//! In a capability-based microkernel, object leaks are a common bug class:
//! - A process creates channels but never closes them.
//! - Capabilities are granted but never revoked after process exit.
//! - Timers are scheduled but never cancelled.
//!
//! By tracking all object types centrally, we can quickly spot imbalances
//! (more creates than destroys = leak) and identify the leaking object type.
//!
//! ## Object Types
//!
//! Tracked object types match kernel resource categories:
//! - Tasks (threads)
//! - Processes
//! - Channels (IPC)
//! - Pipes
//! - Shared memory regions
//! - Eventfd counters
//! - Completion ports
//! - Capabilities
//! - Timers
//! - DMA buffers
//!
//! ## Design
//!
//! Per-type atomic counters for creates, destroys, and current count.
//! Lock-free, zero allocation.  High-water mark tracked per type.
//!
//! ## Usage
//!
//! ```text
//! kshell> kobjects          — show all object counts
//! kshell> kobjects leaks    — show types with creates > destroys
//! ```
//!
//! ## References
//!
//! - Fuchsia `k zx` — kernel object diagnostics
//! - Windows Object Manager — handle/object tracking
//! - seL4 object creation bookkeeping

use core::sync::atomic::{AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Object types
// ---------------------------------------------------------------------------

/// Kernel object categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ObjType {
    Task = 0,
    Process = 1,
    Channel = 2,
    Pipe = 3,
    SharedMemory = 4,
    Eventfd = 5,
    CompletionPort = 6,
    Capability = 7,
    Timer = 8,
    DmaBuffer = 9,
    Futex = 10,
    IoRing = 11,
}

impl ObjType {
    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Task => "Task",
            Self::Process => "Process",
            Self::Channel => "Channel",
            Self::Pipe => "Pipe",
            Self::SharedMemory => "SharedMem",
            Self::Eventfd => "Eventfd",
            Self::CompletionPort => "CompPort",
            Self::Capability => "Capability",
            Self::Timer => "Timer",
            Self::DmaBuffer => "DmaBuf",
            Self::Futex => "Futex",
            Self::IoRing => "IoRing",
        }
    }

    /// Number of object types.
    pub const COUNT: usize = 12;

    /// Iterate all types.
    pub fn all() -> &'static [ObjType; Self::COUNT] {
        &[
            Self::Task, Self::Process, Self::Channel, Self::Pipe,
            Self::SharedMemory, Self::Eventfd, Self::CompletionPort,
            Self::Capability, Self::Timer, Self::DmaBuffer,
            Self::Futex, Self::IoRing,
        ]
    }
}

// ---------------------------------------------------------------------------
// Per-type counters
// ---------------------------------------------------------------------------

/// Counters for a single object type.
struct TypeCounters {
    /// Total objects created since boot.
    created: AtomicU64,
    /// Total objects destroyed since boot.
    destroyed: AtomicU64,
    /// High water mark (max simultaneous active).
    high_water: AtomicU64,
}

impl TypeCounters {
    const fn new() -> Self {
        Self {
            created: AtomicU64::new(0),
            destroyed: AtomicU64::new(0),
            high_water: AtomicU64::new(0),
        }
    }
}

static COUNTERS: [TypeCounters; ObjType::COUNT] = [
    TypeCounters::new(), TypeCounters::new(), TypeCounters::new(),
    TypeCounters::new(), TypeCounters::new(), TypeCounters::new(),
    TypeCounters::new(), TypeCounters::new(), TypeCounters::new(),
    TypeCounters::new(), TypeCounters::new(), TypeCounters::new(),
];

// ---------------------------------------------------------------------------
// Public API — recording
// ---------------------------------------------------------------------------

/// Record creation of a kernel object.
#[inline]
pub fn created(typ: ObjType) {
    let idx = typ as usize;
    if let Some(c) = COUNTERS.get(idx) {
        let total = c.created.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        let destroyed = c.destroyed.load(Ordering::Relaxed);
        let active = total.saturating_sub(destroyed);

        // Update high water mark if needed.
        loop {
            let current_hw = c.high_water.load(Ordering::Relaxed);
            if active <= current_hw {
                break;
            }
            // Try to update (CAS loop for correctness).
            if c.high_water.compare_exchange_weak(
                current_hw, active, Ordering::Relaxed, Ordering::Relaxed
            ).is_ok() {
                break;
            }
        }
    }
}

/// Record destruction of a kernel object.
#[inline]
pub fn destroyed(typ: ObjType) {
    let idx = typ as usize;
    if let Some(c) = COUNTERS.get(idx) {
        c.destroyed.fetch_add(1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Public API — querying
// ---------------------------------------------------------------------------

/// Stats for a single object type.
#[derive(Debug, Clone, Copy)]
pub struct ObjTypeStats {
    pub obj_type: ObjType,
    pub created: u64,
    pub destroyed: u64,
    pub active: u64,
    pub high_water: u64,
}

/// Get stats for a specific object type.
#[must_use]
pub fn type_stats(typ: ObjType) -> ObjTypeStats {
    let idx = typ as usize;
    let (cr, de, hw) = if let Some(c) = COUNTERS.get(idx) {
        (
            c.created.load(Ordering::Relaxed),
            c.destroyed.load(Ordering::Relaxed),
            c.high_water.load(Ordering::Relaxed),
        )
    } else {
        (0, 0, 0)
    };

    ObjTypeStats {
        obj_type: typ,
        created: cr,
        destroyed: de,
        active: cr.saturating_sub(de),
        high_water: hw,
    }
}

/// Get stats for all object types.
pub fn all_stats() -> [ObjTypeStats; ObjType::COUNT] {
    let mut result = [ObjTypeStats {
        obj_type: ObjType::Task,
        created: 0,
        destroyed: 0,
        active: 0,
        high_water: 0,
    }; ObjType::COUNT];

    for (i, typ) in ObjType::all().iter().enumerate() {
        result[i] = type_stats(*typ);
    }
    result
}

/// Total active objects across all types.
#[must_use]
pub fn total_active() -> u64 {
    let mut total: u64 = 0;
    for typ in ObjType::all() {
        let s = type_stats(*typ);
        total = total.saturating_add(s.active);
    }
    total
}

/// Find object types with potential leaks (created > destroyed, active > 0).
pub fn potential_leaks() -> alloc::vec::Vec<ObjTypeStats> {
    let mut leaks = alloc::vec::Vec::new();
    for typ in ObjType::all() {
        let s = type_stats(*typ);
        if s.active > 0 {
            leaks.push(s);
        }
    }
    leaks
}

/// Reset all counters.
pub fn reset() {
    for c in &COUNTERS {
        c.created.store(0, Ordering::Relaxed);
        c.destroyed.store(0, Ordering::Relaxed);
        c.high_water.store(0, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for kernel object tracking.
pub fn self_test() {
    serial_println!("[kobject] Running self-test...");

    // Test 1: Reset state.
    reset();
    let s = type_stats(ObjType::Task);
    assert_eq!(s.created, 0);
    assert_eq!(s.active, 0);
    serial_println!("[kobject]   Reset: OK");

    // Test 2: Create objects.
    created(ObjType::Task);
    created(ObjType::Task);
    created(ObjType::Task);
    created(ObjType::Channel);
    created(ObjType::Channel);
    created(ObjType::Pipe);

    let s = type_stats(ObjType::Task);
    assert_eq!(s.created, 3);
    assert_eq!(s.active, 3);
    assert_eq!(s.high_water, 3);

    let s = type_stats(ObjType::Channel);
    assert_eq!(s.created, 2);
    assert_eq!(s.active, 2);
    serial_println!("[kobject]   Create: OK (tasks=3, channels=2, pipes=1)");

    // Test 3: Destroy objects.
    destroyed(ObjType::Task);
    destroyed(ObjType::Channel);

    let s = type_stats(ObjType::Task);
    assert_eq!(s.created, 3);
    assert_eq!(s.destroyed, 1);
    assert_eq!(s.active, 2);
    assert_eq!(s.high_water, 3); // High water unchanged.

    let s = type_stats(ObjType::Channel);
    assert_eq!(s.active, 1);
    serial_println!("[kobject]   Destroy: OK (tasks active=2, channels active=1)");

    // Test 4: Total active.
    let total = total_active();
    assert_eq!(total, 4); // 2 tasks + 1 channel + 1 pipe
    serial_println!("[kobject]   Total active: OK ({})", total);

    // Test 5: Potential leaks.
    let leaks = potential_leaks();
    assert_eq!(leaks.len(), 3); // Task, Channel, Pipe all have active > 0
    serial_println!("[kobject]   Leak detection: OK ({} types with active objects)", leaks.len());

    // Test 6: High water mark tracks peak.
    created(ObjType::Timer);
    created(ObjType::Timer);
    created(ObjType::Timer);
    destroyed(ObjType::Timer);
    destroyed(ObjType::Timer);
    // Peak was 3, now active = 1.
    let s = type_stats(ObjType::Timer);
    assert_eq!(s.active, 1);
    assert_eq!(s.high_water, 3);
    serial_println!("[kobject]   High water: OK (timer peak=3, active=1)");

    // Test 7: All types reported.
    let all = all_stats();
    assert_eq!(all.len(), ObjType::COUNT);
    serial_println!("[kobject]   All stats: OK ({} types)", all.len());

    // Cleanup.
    reset();

    serial_println!("[kobject] Self-test PASSED");
}

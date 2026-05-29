//! Kernel state snapshot — capture comprehensive system state for diagnostics.
//!
//! Takes a point-in-time snapshot of all major kernel subsystems:
//! - Memory (frames, heap, fragmentation, pressure)
//! - Scheduler (task counts, CPU utilization, switches)
//! - IPC (message rates, blocking events)
//! - Objects (active kernel objects by type)
//! - Capabilities (grants, revocations, denials)
//!
//! Two snapshots can be diffed to see what changed between them.
//! Useful for diagnosing resource leaks, performance regressions,
//! and understanding system behavior over time.
//!
//! ## Design
//!
//! Each snapshot is a struct of ~500 bytes capturing key metrics.
//! Up to 2 snapshots can be stored simultaneously (A and B) for diffing.
//!
//! ## Usage
//!
//! ```text
//! kshell> snapshot save A       — capture current state as A
//! kshell> ... do things ...
//! kshell> snapshot save B       — capture current state as B
//! kshell> snapshot diff A B     — show what changed
//! ```
//!
//! ## References
//!
//! - Linux /proc/meminfo + /proc/stat + /proc/vmstat (combined snapshot)
//! - Fuchsia `k counters` — unified kernel metric dump
//! - DTrace snapshot() — point-in-time capture

// Diagnostic/profiling subsystem — all public API for tooling and kshell
// commands; many helpers may not have call sites in production paths yet.
#![allow(dead_code)]

use crate::serial_println;
use core::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// Snapshot data
// ---------------------------------------------------------------------------

/// A comprehensive kernel state snapshot.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Tick when snapshot was taken.
    pub tick: u64,
    /// Label (b'A' or b'B').
    pub label: u8,
    /// Whether this slot is valid.
    pub valid: bool,

    // --- Memory ---
    /// Free physical frames.
    pub free_frames: u32,
    /// Total physical frames.
    pub total_frames: u32,
    /// Fragmentation index (0-100).
    pub frag_pct: u8,
    /// Heap slab allocs (cumulative).
    pub heap_slab_allocs: u64,
    /// Heap slab frees (cumulative).
    pub heap_slab_frees: u64,
    /// Heap large allocs (cumulative).
    pub heap_large_allocs: u64,
    /// Memory pressure score (0-100).
    pub pressure_score: u8,

    // --- Scheduler ---
    /// Total context switches (all CPUs).
    pub total_ctx_switches: u64,
    /// Total tasks spawned.
    pub tasks_spawned: u64,
    /// Total tasks exited.
    pub tasks_exited: u64,
    /// Load average (×100).
    pub load_avg_x100: u64,

    // --- IPC ---
    /// Total IPC operations.
    pub ipc_total_ops: u64,
    /// Channel messages sent.
    pub channel_sends: u64,
    /// Pipe bytes written.
    pub pipe_bytes: u64,
    /// Futex waits.
    pub futex_waits: u64,

    // --- Objects ---
    /// Total active kernel objects.
    pub total_objects: u64,

    // --- Capabilities ---
    /// Total cap audit events.
    pub cap_events: u64,
    /// Total cap denials.
    pub cap_denials: u64,

    // --- Interrupts ---
    /// Total interrupts (approximate, timer ticks × num_cpus as proxy).
    pub timer_ticks: u64,
}

impl Snapshot {
    const fn empty() -> Self {
        Self {
            tick: 0,
            label: 0,
            valid: false,
            free_frames: 0,
            total_frames: 0,
            frag_pct: 0,
            heap_slab_allocs: 0,
            heap_slab_frees: 0,
            heap_large_allocs: 0,
            pressure_score: 0,
            total_ctx_switches: 0,
            tasks_spawned: 0,
            tasks_exited: 0,
            load_avg_x100: 0,
            ipc_total_ops: 0,
            channel_sends: 0,
            pipe_bytes: 0,
            futex_waits: 0,
            total_objects: 0,
            cap_events: 0,
            cap_denials: 0,
            timer_ticks: 0,
        }
    }
}

/// Diff between two snapshots.
#[derive(Debug, Clone)]
pub struct SnapshotDiff {
    pub tick_delta: u64,
    pub from_label: u8,
    pub to_label: u8,

    // Memory deltas
    pub free_frames_delta: i64,
    pub frag_delta: i8,
    pub heap_net_delta: i64,  // (allocs - frees) delta
    pub pressure_delta: i8,

    // Scheduler deltas
    pub ctx_switches_delta: u64,
    pub tasks_spawned_delta: u64,
    pub tasks_exited_delta: u64,
    pub load_delta: i64,

    // IPC deltas
    pub ipc_ops_delta: u64,
    pub channel_sends_delta: u64,
    pub pipe_bytes_delta: u64,

    // Object delta
    pub objects_delta: i64,

    // Cap deltas
    pub cap_events_delta: u64,
    pub cap_denials_delta: u64,
}

// ---------------------------------------------------------------------------
// Storage (2 slots: A and B)
// ---------------------------------------------------------------------------

struct SnapshotStore(core::cell::UnsafeCell<[Snapshot; 2]>);
unsafe impl Sync for SnapshotStore {}

static STORE: SnapshotStore = SnapshotStore(
    core::cell::UnsafeCell::new([Snapshot::empty(), Snapshot::empty()])
);

static SLOT_A_VALID: AtomicBool = AtomicBool::new(false);
static SLOT_B_VALID: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Capture the current system state.
fn capture(label: u8) -> Snapshot {
    // Memory
    let frame_stats = crate::mm::frame::stats();
    let (free_frames, total_frames) = frame_stats
        .map_or((0, 0), |s| (s.free_frames as u32, s.total_frames as u32));
    let heap_stats = crate::mm::heap::stats();
    let pressure = crate::mm::memory_pressure();

    // Fragmentation
    let frag_pct = crate::mm::frag_history::latest()
        .map_or(0, |s| s.frag_pct);

    // Scheduler
    let sched = crate::sched::sched_stats();

    // IPC
    let ipc = crate::ipc::stats::snapshot();

    // Objects
    let total_objects = crate::kobject::total_active();

    // Capabilities
    let cap = crate::cap::audit::stats();

    // Timer ticks (from APIC)
    let timer_ticks = crate::apic::tick_count();

    Snapshot {
        tick: timer_ticks,
        label,
        valid: true,
        free_frames,
        total_frames,
        frag_pct,
        heap_slab_allocs: heap_stats.slab_allocs,
        heap_slab_frees: heap_stats.slab_frees,
        heap_large_allocs: heap_stats.large_allocs,
        pressure_score: pressure.score,
        total_ctx_switches: sched.total_ctx_switches,
        tasks_spawned: sched.total_tasks_spawned,
        tasks_exited: sched.total_tasks_exited,
        load_avg_x100: sched.load_avg_x100,
        ipc_total_ops: crate::ipc::stats::total_operations(),
        channel_sends: ipc.channel_sends,
        pipe_bytes: ipc.pipe_bytes_written,
        futex_waits: ipc.futex_waits,
        total_objects,
        cap_events: cap.total_events,
        cap_denials: cap.total_denials,
        timer_ticks,
    }
}

/// Save a snapshot with label b'A' or b'B'.
pub fn save(label: u8) {
    let snap = capture(label);
    let slot = if label == b'A' { 0 } else { 1 };

    // SAFETY: slot is 0 or 1, always < STORE_SIZE (2); STORE uses UnsafeCell.
    unsafe {
        let ptr = STORE.0.get() as *mut Snapshot;
        ptr.add(slot).write(snap);
    }

    if slot == 0 {
        SLOT_A_VALID.store(true, Ordering::Release);
    } else {
        SLOT_B_VALID.store(true, Ordering::Release);
    }
}

/// Get a stored snapshot.
pub fn get(label: u8) -> Option<Snapshot> {
    let slot = if label == b'A' { 0 } else { 1 };
    let valid = if slot == 0 {
        SLOT_A_VALID.load(Ordering::Acquire)
    } else {
        SLOT_B_VALID.load(Ordering::Acquire)
    };

    if !valid {
        return None;
    }

    let snap = unsafe {
        let ptr = STORE.0.get() as *const Snapshot;
        ptr.add(slot).read()
    };

    if snap.valid { Some(snap) } else { None }
}

/// Diff two snapshots.
pub fn diff(from_label: u8, to_label: u8) -> Option<SnapshotDiff> {
    let from = get(from_label)?;
    let to = get(to_label)?;

    let from_heap_net = from.heap_slab_allocs as i64 - from.heap_slab_frees as i64;
    let to_heap_net = to.heap_slab_allocs as i64 - to.heap_slab_frees as i64;

    Some(SnapshotDiff {
        tick_delta: to.tick.saturating_sub(from.tick),
        from_label,
        to_label,
        free_frames_delta: to.free_frames as i64 - from.free_frames as i64,
        frag_delta: to.frag_pct as i8 - from.frag_pct as i8,
        heap_net_delta: to_heap_net - from_heap_net,
        pressure_delta: to.pressure_score as i8 - from.pressure_score as i8,
        ctx_switches_delta: to.total_ctx_switches.saturating_sub(from.total_ctx_switches),
        tasks_spawned_delta: to.tasks_spawned.saturating_sub(from.tasks_spawned),
        tasks_exited_delta: to.tasks_exited.saturating_sub(from.tasks_exited),
        load_delta: to.load_avg_x100 as i64 - from.load_avg_x100 as i64,
        ipc_ops_delta: to.ipc_total_ops.saturating_sub(from.ipc_total_ops),
        channel_sends_delta: to.channel_sends.saturating_sub(from.channel_sends),
        pipe_bytes_delta: to.pipe_bytes.saturating_sub(from.pipe_bytes),
        objects_delta: to.total_objects as i64 - from.total_objects as i64,
        cap_events_delta: to.cap_events.saturating_sub(from.cap_events),
        cap_denials_delta: to.cap_denials.saturating_sub(from.cap_denials),
    })
}

/// Clear both snapshot slots.
pub fn clear() {
    unsafe {
        let ptr = STORE.0.get() as *mut Snapshot;
        ptr.add(0).write(Snapshot::empty());
        ptr.add(1).write(Snapshot::empty());
    }
    SLOT_A_VALID.store(false, Ordering::Release);
    SLOT_B_VALID.store(false, Ordering::Release);
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for kernel snapshots.
pub fn self_test() {
    serial_println!("[ksnapshot] Running self-test...");

    // Test 1: Clear state.
    clear();
    assert!(get(b'A').is_none());
    assert!(get(b'B').is_none());
    serial_println!("[ksnapshot]   Clear: OK");

    // Test 2: Save snapshot A.
    save(b'A');
    let a = get(b'A').expect("should have snapshot A");
    assert!(a.valid);
    assert_eq!(a.label, b'A');
    assert!(a.total_frames > 0);
    assert!(a.free_frames > 0);
    serial_println!("[ksnapshot]   Save A: OK (free={}/{}, pressure={})",
        a.free_frames, a.total_frames, a.pressure_score);

    // Test 3: Save snapshot B.
    save(b'B');
    let b = get(b'B').expect("should have snapshot B");
    assert!(b.valid);
    assert!(b.tick >= a.tick);
    serial_println!("[ksnapshot]   Save B: OK (tick delta={})", b.tick - a.tick);

    // Test 4: Diff A → B.
    let d = diff(b'A', b'B').expect("diff should work");
    assert_eq!(d.from_label, b'A');
    assert_eq!(d.to_label, b'B');
    // Context switches should be non-negative (monotonic counter).
    // tick_delta is fine at 0 (both captured very quickly).
    serial_println!("[ksnapshot]   Diff: OK (ctx_sw_delta={}, ipc_ops_delta={}, obj_delta={:+})",
        d.ctx_switches_delta, d.ipc_ops_delta, d.objects_delta);

    // Test 5: Overwrite works.
    save(b'A');
    let a2 = get(b'A').expect("overwritten A");
    assert!(a2.tick >= a.tick);
    serial_println!("[ksnapshot]   Overwrite: OK");

    // Cleanup.
    clear();

    serial_println!("[ksnapshot] Self-test PASSED");
}

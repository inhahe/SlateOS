//! Allocation checkpoints — diff memory state between two points in time.
//!
//! Take a "snapshot" of the current allocation state (free frame count,
//! heap stats, per-owner counts).  Later, take another snapshot and diff
//! them to see what was allocated/freed between the two points.
//!
//! This is the primary tool for finding memory leaks in specific operations:
//! 1. Take checkpoint before the operation.
//! 2. Perform the operation (e.g., create a process, run a test).
//! 3. Undo the operation (e.g., destroy the process).
//! 4. Diff against the checkpoint — any remaining allocations are leaks.
//!
//! ## Design
//!
//! Checkpoints are lightweight (~200 bytes each).  They capture:
//! - Total free frames
//! - Per-owner frame counts (from frame_owner)
//! - Heap slab/large alloc counts
//! - Watermark values
//!
//! Up to 4 named checkpoints can be stored simultaneously.  The diff
//! operation compares any two checkpoints and reports differences.
//!
//! ## Usage
//!
//! ```text
//! kshell> checkpoint save A
//! kshell> ... perform operations ...
//! kshell> checkpoint save B
//! kshell> checkpoint diff A B
//! ```
//!
//! ## References
//!
//! - Valgrind massif — heap profiler snapshots
//! - Linux kmemleak — mark-and-sweep leak detection
//! - Go runtime memory profiling — alloc/free deltas

use crate::mm::{frame, frame_owner};
use crate::serial_println;
use core::sync::atomic::{AtomicU8, Ordering};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of stored checkpoints.
const MAX_CHECKPOINTS: usize = 4;

/// Number of owner tags to track.
const NUM_OWNERS: usize = frame_owner::Owner::COUNT;

// ---------------------------------------------------------------------------
// Checkpoint data
// ---------------------------------------------------------------------------

/// A memory state checkpoint.
#[derive(Debug, Clone, Copy)]
pub struct Checkpoint {
    /// Checkpoint name/label (single letter A-D for simplicity).
    pub label: u8,
    /// Free frames at checkpoint time.
    pub free_frames: u32,
    /// Total frames at checkpoint time.
    pub total_frames: u32,
    /// Per-owner frame counts.
    pub owner_counts: [u32; NUM_OWNERS],
    /// Heap slab allocations (cumulative).
    pub heap_slab_allocs: u64,
    /// Heap slab frees (cumulative).
    pub heap_slab_frees: u64,
    /// Heap large allocations (cumulative).
    pub heap_large_allocs: u64,
    /// APIC tick at checkpoint time.
    pub tick: u64,
    /// Whether this slot is occupied.
    pub valid: bool,
}

impl Checkpoint {
    const fn empty() -> Self {
        Self {
            label: 0,
            free_frames: 0,
            total_frames: 0,
            owner_counts: [0; NUM_OWNERS],
            heap_slab_allocs: 0,
            heap_slab_frees: 0,
            heap_large_allocs: 0,
            tick: 0,
            valid: false,
        }
    }
}

/// Difference between two checkpoints.
#[derive(Debug, Clone)]
pub struct CheckpointDiff {
    /// Change in free frames (positive = more free = memory was freed).
    pub free_delta: i64,
    /// Per-owner count changes (positive = more allocated by this owner).
    pub owner_deltas: [i32; NUM_OWNERS],
    /// Change in net heap slab objects (allocs - frees delta).
    pub heap_slab_delta: i64,
    /// Change in net heap large objects.
    pub heap_large_delta: i64,
    /// Time elapsed (ticks).
    pub tick_delta: u64,
    /// Labels of the two checkpoints compared.
    pub from_label: u8,
    pub to_label: u8,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Stored checkpoints.  UnsafeCell because we access from a single context
/// (kshell commands are serialized).
struct CheckpointStore(core::cell::UnsafeCell<[Checkpoint; MAX_CHECKPOINTS]>);
unsafe impl Sync for CheckpointStore {}

static STORE: CheckpointStore = CheckpointStore(
    core::cell::UnsafeCell::new([Checkpoint::empty(); MAX_CHECKPOINTS])
);

/// Number of checkpoints currently stored.
static STORED_COUNT: AtomicU8 = AtomicU8::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Save a checkpoint with the given label (0-3 or 'A'-'D').
///
/// If a checkpoint with this label already exists, it's overwritten.
/// Returns the slot index used.
pub fn save(label: u8) -> usize {
    let cp = capture(label);

    // Find existing slot with this label, or an empty slot.
    let slot = find_slot(label);

    unsafe {
        let ptr = STORE.0.get() as *mut Checkpoint;
        ptr.add(slot).write(cp);
    }

    // Update count if this is a new slot.
    let current = STORED_COUNT.load(Ordering::Relaxed);
    if slot >= current as usize {
        STORED_COUNT.store((slot + 1) as u8, Ordering::Relaxed);
    }

    slot
}

/// Take a checkpoint without storing it (for programmatic use).
#[must_use]
pub fn capture(label: u8) -> Checkpoint {
    let frame_stats = frame::stats();
    let (free_frames, total_frames) = frame_stats
        .map_or((0, 0), |s| (s.free_frames as u32, s.total_frames as u32));

    let owner_summary = frame_owner::summary();
    let mut owner_counts = [0u32; NUM_OWNERS];
    for i in 0..NUM_OWNERS {
        owner_counts[i] = owner_summary.counts[i];
    }

    let heap_stats = crate::mm::heap::stats();

    Checkpoint {
        label,
        free_frames,
        total_frames,
        owner_counts,
        heap_slab_allocs: heap_stats.slab_allocs,
        heap_slab_frees: heap_stats.slab_frees,
        heap_large_allocs: heap_stats.large_allocs,
        tick: crate::apic::tick_count(),
        valid: true,
    }
}

/// Diff two checkpoints by label.
///
/// Returns `None` if either checkpoint doesn't exist.
pub fn diff(from_label: u8, to_label: u8) -> Option<CheckpointDiff> {
    let from = get(from_label)?;
    let to = get(to_label)?;

    let free_delta = to.free_frames as i64 - from.free_frames as i64;

    let mut owner_deltas = [0i32; NUM_OWNERS];
    for i in 0..NUM_OWNERS {
        owner_deltas[i] = to.owner_counts[i] as i32 - from.owner_counts[i] as i32;
    }

    let from_net_slab = from.heap_slab_allocs as i64 - from.heap_slab_frees as i64;
    let to_net_slab = to.heap_slab_allocs as i64 - to.heap_slab_frees as i64;
    let heap_slab_delta = to_net_slab - from_net_slab;

    let heap_large_delta = to.heap_large_allocs as i64 - from.heap_large_allocs as i64;

    Some(CheckpointDiff {
        free_delta,
        owner_deltas,
        heap_slab_delta,
        heap_large_delta,
        tick_delta: to.tick.saturating_sub(from.tick),
        from_label,
        to_label,
    })
}

/// Get a stored checkpoint by label.
pub fn get(label: u8) -> Option<Checkpoint> {
    for i in 0..MAX_CHECKPOINTS {
        let cp = unsafe {
            let ptr = STORE.0.get() as *const Checkpoint;
            ptr.add(i).read()
        };
        if cp.valid && cp.label == label {
            return Some(cp);
        }
    }
    None
}

/// Clear all stored checkpoints.
pub fn clear() {
    for i in 0..MAX_CHECKPOINTS {
        unsafe {
            let ptr = STORE.0.get() as *mut Checkpoint;
            ptr.add(i).write(Checkpoint::empty());
        }
    }
    STORED_COUNT.store(0, Ordering::Relaxed);
}

/// List all stored checkpoints.
pub fn list() -> [(u8, bool); MAX_CHECKPOINTS] {
    let mut result = [(0u8, false); MAX_CHECKPOINTS];
    for i in 0..MAX_CHECKPOINTS {
        let cp = unsafe {
            let ptr = STORE.0.get() as *const Checkpoint;
            ptr.add(i).read()
        };
        result[i] = (cp.label, cp.valid);
    }
    result
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

/// Find a slot for the given label (existing or first empty).
fn find_slot(label: u8) -> usize {
    // First, look for existing entry with this label.
    for i in 0..MAX_CHECKPOINTS {
        let cp = unsafe {
            let ptr = STORE.0.get() as *const Checkpoint;
            ptr.add(i).read()
        };
        if cp.valid && cp.label == label {
            return i;
        }
    }
    // Then, find first empty slot.
    for i in 0..MAX_CHECKPOINTS {
        let cp = unsafe {
            let ptr = STORE.0.get() as *const Checkpoint;
            ptr.add(i).read()
        };
        if !cp.valid {
            return i;
        }
    }
    // All full — overwrite slot 0.
    0
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for allocation checkpoints.
pub fn self_test() {
    serial_println!("[alloc_checkpoint] Running self-test...");

    // Test 1: Clear state.
    clear();
    assert!(get(b'A').is_none());
    serial_println!("[alloc_checkpoint]   Clear: OK");

    // Test 2: Save and retrieve a checkpoint.
    save(b'A');
    let cp = get(b'A').expect("checkpoint A should exist");
    assert!(cp.valid);
    assert_eq!(cp.label, b'A');
    assert!(cp.total_frames > 0);
    assert!(cp.free_frames > 0);
    serial_println!("[alloc_checkpoint]   Save/get: OK (free={}, total={})",
        cp.free_frames, cp.total_frames);

    // Test 3: Save a second checkpoint and diff.
    // Allocate a frame to create a difference.
    let test_frame = frame::alloc_frame_zeroed();
    save(b'B');

    let d = diff(b'A', b'B').expect("diff should work");
    // We allocated one frame between A and B, so free_delta should be negative.
    // (fewer free frames in B than in A).
    if test_frame.is_ok() {
        assert!(d.free_delta <= 0, "should have fewer free frames: delta={}",
            d.free_delta);
    }
    serial_println!("[alloc_checkpoint]   Diff A→B: free_delta={}, slab_delta={}",
        d.free_delta, d.heap_slab_delta);

    // Free the test frame.
    if let Ok(f) = test_frame {
        unsafe { let _ = frame::free_frame(f); }
    }

    // Test 4: Overwrite existing checkpoint.
    save(b'A'); // Overwrite A.
    let cp2 = get(b'A').expect("overwritten A");
    assert!(cp2.tick >= cp.tick, "new checkpoint should be >= old");
    serial_println!("[alloc_checkpoint]   Overwrite: OK");

    // Test 5: Multiple checkpoints coexist.
    save(b'C');
    save(b'D');
    assert!(get(b'A').is_some());
    assert!(get(b'B').is_some());
    assert!(get(b'C').is_some());
    assert!(get(b'D').is_some());
    serial_println!("[alloc_checkpoint]   4 checkpoints coexist: OK");

    // Test 6: List shows all.
    let ls = list();
    let valid_count = ls.iter().filter(|(_, v)| *v).count();
    assert_eq!(valid_count, 4);
    serial_println!("[alloc_checkpoint]   List: {} valid entries", valid_count);

    // Cleanup.
    clear();

    serial_println!("[alloc_checkpoint] Self-test PASSED");
}

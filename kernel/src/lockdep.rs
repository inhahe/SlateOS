//! Runtime lock order validator (lockdep).
//!
//! Detects potential deadlocks by tracking the order in which locks are
//! acquired.  If lock A is ever held while acquiring lock B, and later
//! lock B is held while acquiring lock A, this module reports a potential
//! deadlock (AB/BA inversion) regardless of whether the threads actually
//! deadlocked.
//!
//! ## How it works
//!
//! Each lock has a *class* identified by its static address (or a caller-
//! provided ID).  When a lock is acquired, we record the ordering edge
//! (held → acquired) in a global dependency graph.  A cycle in this graph
//! means a deadlock is *possible* under some scheduling.
//!
//! ## Performance
//!
//! Lock order checking adds ~50-200ns per lock acquisition (hash lookups,
//! cycle check).  It can be disabled at boot via `lockdep::disable()` or
//! compiled out in production builds by not calling the hooks.
//!
//! ## Limitations
//!
//! - Fixed-size tables (configurable).  If a system uses more lock classes
//!   or deeper nesting than the tables support, new acquisitions are
//!   silently ignored (no false positives, just missed detections).
//! - Only tracks lock *classes* (by address), not individual lock instances.
//!   Two locks at the same address are considered the same class.
//! - Does not detect deadlocks involving wait queues or other non-lock
//!   blocking (e.g., channel send that blocks on a full queue held by a
//!   task waiting on the sender's lock).

use crate::serial_println;
use crate::smp;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of distinct lock classes tracked.
const MAX_CLASSES: usize = 128;

/// Maximum nesting depth per CPU (locks held simultaneously).
const MAX_DEPTH: usize = 16;

/// Maximum number of dependency edges in the graph.
const MAX_EDGES: usize = 512;

/// Maximum CPUs.
const MAX_CPUS: usize = 16;

// ---------------------------------------------------------------------------
// Lock class registry
// ---------------------------------------------------------------------------

/// A lock class: uniquely identifies a "type" of lock by its address.
#[derive(Clone, Copy)]
struct LockClass {
    /// Address used to identify this lock class (typically &SpinLock as usize).
    id: usize,
    /// Name for diagnostic output (e.g., "SCHED", "HEAP").
    name: [u8; 16],
    /// Length of the name.
    name_len: u8,
}

impl LockClass {
    const fn empty() -> Self {
        Self {
            id: 0,
            name: [0; 16],
            name_len: 0,
        }
    }
}

/// Global registry of known lock classes.
static mut CLASSES: [LockClass; MAX_CLASSES] = [LockClass::empty(); MAX_CLASSES];

/// Number of registered classes.
static CLASS_COUNT: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Dependency graph (edges: "class A was held when class B was acquired")
// ---------------------------------------------------------------------------

/// A dependency edge: class_a was held while class_b was acquired.
#[derive(Clone, Copy)]
struct DepEdge {
    from: u16, // class index of the lock that was HELD
    to: u16,   // class index of the lock being ACQUIRED
}

impl DepEdge {
    const fn empty() -> Self {
        Self { from: 0, to: 0 }
    }
}

/// Global dependency graph (append-only during normal operation).
static mut EDGES: [DepEdge; MAX_EDGES] = [DepEdge::empty(); MAX_EDGES];

/// Number of recorded edges.
static EDGE_COUNT: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Per-CPU held-lock stack
// ---------------------------------------------------------------------------

/// Per-CPU stack of currently held locks (class indices).
#[repr(align(64))]
struct HeldStack {
    /// Class indices of locks currently held (bottom → top).
    stack: [u16; MAX_DEPTH],
    /// Current depth (number of locks held).
    depth: u8,
}

impl HeldStack {
    const fn new() -> Self {
        Self {
            stack: [0; MAX_DEPTH],
            depth: 0,
        }
    }
}

static mut HELD: [HeldStack; MAX_CPUS] = {
    const INIT: HeldStack = HeldStack::new();
    [INIT; MAX_CPUS]
};

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether lockdep checking is enabled.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Total violations detected.
static VIOLATIONS: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the lock order validator.
///
/// Call during boot after SMP init (needs `current_cpu_index()`).
pub fn init() {
    ENABLED.store(true, Ordering::Release);
    serial_println!("[lockdep] Lock order validator enabled (max {} classes, {} edges)",
        MAX_CLASSES, MAX_EDGES);
}

/// Disable lock order checking (e.g., during shutdown or panic).
#[allow(dead_code)]
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Notify lockdep that a lock is being acquired.
///
/// Call this BEFORE the actual lock acquisition (while we still know
/// what locks are held — after acquisition we'd need to handle the
/// case where we're blocked on the lock).
///
/// `lock_addr`: address of the lock (e.g., `&spinlock as *const _ as usize`).
/// `name`: short human-readable name for diagnostics.
#[inline]
pub fn lock_acquire(lock_addr: usize, name: &[u8]) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let cpu = smp::current_cpu_index();
    if cpu >= MAX_CPUS {
        return;
    }

    // Find or register the lock class.
    let class_idx = find_or_register_class(lock_addr, name);
    let Some(class_idx) = class_idx else {
        return; // Table full — silently skip.
    };

    // SAFETY: Only this CPU accesses its held stack (called with lock
    // not yet acquired, so no preemption concern for the stack itself).
    let held = unsafe { &mut HELD[cpu] };

    // Check all currently-held locks for ordering violations.
    for i in 0..held.depth as usize {
        let held_class = held.stack[i];
        if held_class == class_idx {
            // Re-entrant acquisition of same class — skip (might be
            // recursive lock or nested lock of same type).
            continue;
        }

        // Record the dependency edge: held_class → class_idx.
        let is_new = record_edge(held_class, class_idx);

        if is_new {
            // New edge — check for cycles (potential deadlock).
            if has_cycle(class_idx, held_class) {
                report_violation(held_class, class_idx, cpu);
            }
        }
    }

    // Push this lock onto the held stack.
    if (held.depth as usize) < MAX_DEPTH {
        held.stack[held.depth as usize] = class_idx;
        held.depth += 1;
    }
}

/// Notify lockdep that a lock has been released.
///
/// `lock_addr`: same address passed to `lock_acquire`.
#[inline]
pub fn lock_release(lock_addr: usize) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let cpu = smp::current_cpu_index();
    if cpu >= MAX_CPUS {
        return;
    }

    let class_idx = find_class(lock_addr);
    let Some(class_idx) = class_idx else {
        return; // Unknown lock — nothing to do.
    };

    // SAFETY: Only this CPU accesses its held stack.
    let held = unsafe { &mut HELD[cpu] };

    // Find and remove from the stack.  Locks may be released out of
    // order (e.g., trylock acquired in different order), so we search
    // the entire stack rather than just popping the top.
    for i in 0..held.depth as usize {
        if held.stack[i] == class_idx {
            // Shift remaining entries down.
            #[allow(clippy::arithmetic_side_effects)]
            for j in i..(held.depth as usize - 1) {
                held.stack[j] = held.stack[j + 1];
            }
            held.depth -= 1;
            return;
        }
    }
    // Lock not found in held stack — benign (might have been acquired
    // before lockdep was enabled, or table was full at acquire time).
}

/// Return the number of violations detected so far.
#[allow(dead_code)]
pub fn violation_count() -> u32 {
    VIOLATIONS.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Find an existing class by lock address, or register a new one.
fn find_or_register_class(lock_addr: usize, name: &[u8]) -> Option<u16> {
    let count = CLASS_COUNT.load(Ordering::Relaxed) as usize;

    // Search existing classes.
    for i in 0..count.min(MAX_CLASSES) {
        // SAFETY: Reading from the class array is safe — entries are
        // append-only and we only read up to the current count.
        if unsafe { CLASSES[i].id } == lock_addr {
            return Some(i as u16);
        }
    }

    // Register new class.
    let idx = CLASS_COUNT.fetch_add(1, Ordering::Relaxed) as usize;
    if idx >= MAX_CLASSES {
        // Table full.  Undo the increment (best-effort).
        CLASS_COUNT.fetch_sub(1, Ordering::Relaxed);
        return None;
    }

    // SAFETY: We "own" slot `idx` because fetch_add gave us a unique index.
    // No other CPU will write to this slot.
    unsafe {
        CLASSES[idx].id = lock_addr;
        let copy_len = name.len().min(16);
        CLASSES[idx].name[..copy_len].copy_from_slice(&name[..copy_len]);
        CLASSES[idx].name_len = copy_len as u8;
    }
    Some(idx as u16)
}

/// Find an existing class by lock address.
fn find_class(lock_addr: usize) -> Option<u16> {
    let count = CLASS_COUNT.load(Ordering::Relaxed) as usize;
    for i in 0..count.min(MAX_CLASSES) {
        if unsafe { CLASSES[i].id } == lock_addr {
            return Some(i as u16);
        }
    }
    None
}

/// Record a dependency edge (from → to).  Returns true if this is a NEW edge.
fn record_edge(from: u16, to: u16) -> bool {
    let count = EDGE_COUNT.load(Ordering::Relaxed) as usize;

    // Check if edge already exists.
    for i in 0..count.min(MAX_EDGES) {
        // SAFETY: Reading from append-only edge array.
        let e = unsafe { EDGES[i] };
        if e.from == from && e.to == to {
            return false; // Already recorded.
        }
    }

    // Add new edge.
    let idx = EDGE_COUNT.fetch_add(1, Ordering::Relaxed) as usize;
    if idx >= MAX_EDGES {
        EDGE_COUNT.fetch_sub(1, Ordering::Relaxed);
        return false; // Table full.
    }

    // SAFETY: We "own" this slot via fetch_add.
    unsafe {
        EDGES[idx] = DepEdge { from, to };
    }
    true
}

/// Check if there's a path from `start` back to `target` in the
/// dependency graph (i.e., would adding target→start create a cycle?).
///
/// We check: does a path exist from `start` to `target`?  If yes,
/// then the new edge (target → start, which we just recorded via
/// held_class→class_idx) combined with the existing path
/// (start→...→target) creates a cycle.
///
/// Simple BFS with bounded depth to avoid stack overflow.
fn has_cycle(start: u16, target: u16) -> bool {
    // BFS queue (bounded).
    let mut queue = [0u16; 32];
    let mut head = 0usize;
    let mut tail = 0usize;
    let mut visited = [false; MAX_CLASSES];

    queue[tail] = start;
    tail += 1;
    visited[start as usize] = true;

    let edge_count = EDGE_COUNT.load(Ordering::Relaxed) as usize;

    while head < tail && head < 32 {
        let current = queue[head];
        head += 1;

        // Find all edges FROM current.
        for i in 0..edge_count.min(MAX_EDGES) {
            let e = unsafe { EDGES[i] };
            if e.from == current {
                if e.to == target {
                    return true; // Cycle found!
                }
                let to_idx = e.to as usize;
                if to_idx < MAX_CLASSES && !visited[to_idx] && tail < 32 {
                    visited[to_idx] = true;
                    queue[tail] = e.to;
                    tail += 1;
                }
            }
        }
    }
    false
}

/// Report a lock ordering violation.
fn report_violation(held_class: u16, acquired_class: u16, cpu: usize) {
    VIOLATIONS.fetch_add(1, Ordering::Relaxed);

    let held_name = class_name(held_class);
    let acq_name = class_name(acquired_class);

    serial_println!(
        "[lockdep] WARNING: potential deadlock detected on CPU {}!",
        cpu
    );
    serial_println!(
        "[lockdep]   Holding lock {:?} (class {}), acquiring lock {:?} (class {})",
        held_name, held_class, acq_name, acquired_class
    );
    serial_println!(
        "[lockdep]   But the reverse order was observed previously."
    );
    serial_println!(
        "[lockdep]   This means a deadlock is possible under different scheduling."
    );
}

/// Get the name of a lock class for diagnostic output.
fn class_name(idx: u16) -> &'static str {
    let idx = idx as usize;
    if idx >= MAX_CLASSES {
        return "?";
    }
    // SAFETY: Reading from the class array within bounds.
    let class = unsafe { &CLASSES[idx] };
    let len = class.name_len as usize;
    if len == 0 {
        return "?";
    }
    // SAFETY: name bytes were copied from a valid &[u8] in register.
    core::str::from_utf8(&class.name[..len]).unwrap_or("?")
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test of the lock order validator.
///
/// Tests:
/// 1. Class registration works.
/// 2. Edge recording works.
/// 3. Cycle detection catches AB/BA inversions.
/// 4. Non-cyclic orderings are allowed.
/// 5. Release removes from held stack.
pub fn self_test() {
    serial_println!("[lockdep] Running self-test...");

    // Save and reset state for testing.
    let prev_enabled = ENABLED.load(Ordering::Relaxed);
    ENABLED.store(true, Ordering::Relaxed);
    let prev_violations = VIOLATIONS.load(Ordering::Relaxed);

    // Use fake lock addresses for testing.
    let lock_a: usize = 0xDEAD_0001;
    let lock_b: usize = 0xDEAD_0002;
    let lock_c: usize = 0xDEAD_0003;

    // Test 1: Acquire A then B (establishes A→B ordering).
    lock_acquire(lock_a, b"test-A");
    lock_acquire(lock_b, b"test-B");
    lock_release(lock_b);
    lock_release(lock_a);

    let v1 = VIOLATIONS.load(Ordering::Relaxed);
    assert_eq!(v1, prev_violations, "no violation for consistent A→B order");
    serial_println!("[lockdep]   Consistent order (A→B): OK");

    // Test 2: Acquire B then A (should detect AB/BA inversion).
    lock_acquire(lock_b, b"test-B");
    lock_acquire(lock_a, b"test-A");
    lock_release(lock_a);
    lock_release(lock_b);

    let v2 = VIOLATIONS.load(Ordering::Relaxed);
    assert_eq!(
        v2,
        prev_violations + 1,
        "should detect one violation for B→A after A→B"
    );
    serial_println!("[lockdep]   AB/BA inversion detected: OK");

    // Test 3: Non-cyclic chain (A→B→C is fine, no cycle).
    lock_acquire(lock_a, b"test-A");
    lock_acquire(lock_b, b"test-B");
    lock_acquire(lock_c, b"test-C");
    lock_release(lock_c);
    lock_release(lock_b);
    lock_release(lock_a);

    let v3 = VIOLATIONS.load(Ordering::Relaxed);
    // A→B already exists, B→C is new (no cycle: A→B→C).
    // A→C is new (no cycle: A→C direct).
    assert_eq!(v3, v2, "no new violation for non-cyclic A→B→C");
    serial_println!("[lockdep]   Non-cyclic chain (A→B→C): OK");

    // Test 4: Transitive cycle (C→A when A→B→C exists).
    lock_acquire(lock_c, b"test-C");
    lock_acquire(lock_a, b"test-A");
    lock_release(lock_a);
    lock_release(lock_c);

    let v4 = VIOLATIONS.load(Ordering::Relaxed);
    assert_eq!(
        v4,
        v3 + 1,
        "should detect violation for C→A (cycle: A→B→C→A)"
    );
    serial_println!("[lockdep]   Transitive cycle (A→B→C→A): OK");

    // Test 5: Release removes from held stack (verify no leak).
    let cpu = smp::current_cpu_index();
    let depth = unsafe { HELD[cpu].depth };
    assert_eq!(depth, 0, "held stack should be empty after all releases");
    serial_println!("[lockdep]   Release cleanup: OK");

    // Restore state.
    ENABLED.store(prev_enabled, Ordering::Relaxed);

    serial_println!(
        "[lockdep]   Stats: {} classes, {} edges, {} violations",
        CLASS_COUNT.load(Ordering::Relaxed),
        EDGE_COUNT.load(Ordering::Relaxed),
        VIOLATIONS.load(Ordering::Relaxed)
    );
    serial_println!("[lockdep] Self-test PASSED");
}

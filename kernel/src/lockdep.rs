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
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};

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
// Class index — O(1) address → class lookup
// ---------------------------------------------------------------------------
//
// This lookup runs on **every** `lock_acquire` and **every** `lock_release`,
// i.e. twice per lock operation everywhere in the kernel, so its cost is added
// to every critical section in the system.  It used to be a linear scan of the
// whole class table.
//
// Measured: with lockdep on, an uncontended `sync::Mutex` acquire+release cost
// 632 ns; with lockdep off, 232 ns.  The 400 ns difference is ~66% of the
// entire tracked-mutex overhead, and it is spent almost entirely here — two
// scans of up to `MAX_CLASSES` = 128 entries.  The cost is also *positional*:
// a lock registered early is found in a few iterations, one registered late
// pays the full scan, so the same lockdep call is cheap or expensive depending
// on boot order.  That is precisely the "linear scan on a hot path" that
// CLAUDE.md forbids, hiding inside the debugging infrastructure rather than the
// code being debugged.
//
// Replaced with an open-addressed hash index, as Linux does
// (`classhash_table` in `kernel/locking/lockdep.c`).  Entries are append-only
// and never removed, so a probe run is contiguous and terminating on the first
// empty bucket is correct.
//
// This is the fix that made the "keep lockdep in release builds or not?"
// question go away: the validator was not inherently expensive, its index was.

/// log2 of the number of hash buckets.
const CLASS_HASH_SHIFT: u32 = 9;

/// Bucket count. Power of two so the probe wrap is a mask, and 4x
/// `MAX_CLASSES` so the table never exceeds a 25% load factor — at which
/// linear probing averages well under two probes.
const CLASS_HASH_BUCKETS: usize = 1 << CLASS_HASH_SHIFT;

const _: () = assert!(
    CLASS_HASH_BUCKETS >= MAX_CLASSES * 2,
    "class hash must stay under 50% load or probe runs grow without bound"
);

/// Open-addressed index from lock address to class slot.
///
/// Holds `class_index + 1`; `0` means "empty". The bias is what lets a single
/// atomic word encode "absent" without a second array.
static CLASS_HASH: [AtomicU16; CLASS_HASH_BUCKETS] =
    [const { AtomicU16::new(0) }; CLASS_HASH_BUCKETS];

/// Read one class's identifying address.
///
/// Deliberately returns the `usize` by value rather than a reference to the
/// entry: Rust 2024 rejects `&CLASSES[i]` outright (a shared reference to a
/// mutable static is UB the moment another CPU appends), while reading a `Copy`
/// field through the index expression forms no reference at all.
#[inline]
fn class_id(idx: usize) -> Option<usize> {
    if idx >= MAX_CLASSES {
        return None;
    }
    // SAFETY: `idx` is bounds-checked above, and the read is a place expression
    // on a `Copy` field, so no reference to the static is created. Entries are
    // append-only: a slot's `id` is written once, before the slot is published.
    #[allow(clippy::indexing_slicing)]
    Some(unsafe { CLASSES[idx].id })
}

/// Bucket for a lock address.
///
/// Fibonacci hashing (Knuth 6.4): multiply by 2^64/φ and take the *high* bits.
/// Taking low bits directly would be much worse than the linear scan it
/// replaces — lock addresses are statics, so they are at least 8-byte aligned
/// and often spaced by a fixed stride, which puts every one of them in the same
/// handful of buckets. The multiply spreads that structure across the whole
/// word before the shift selects from the top.
const fn class_bucket(addr: usize) -> usize {
    let mixed = (addr as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (mixed >> (64 - CLASS_HASH_SHIFT)) as usize
}

/// Look up a lock address in the hash index. `None` if not registered.
#[inline]
fn hash_lookup(addr: usize) -> Option<u16> {
    let mut bucket = class_bucket(addr);
    // Bounded by the table size: a full table would otherwise spin forever.
    for _ in 0..CLASS_HASH_BUCKETS {
        let slot = CLASS_HASH.get(bucket)?.load(Ordering::Acquire);
        if slot == 0 {
            // Empty bucket ends the probe run. Correct only because classes are
            // never removed; a future removal would have to leave a tombstone.
            return None;
        }
        let idx = slot - 1;
        // `slot` was published only after `CLASSES[idx]` was fully written
        // (see `find_or_register_class`), and the Acquire load above pairs with
        // that Release store, so the entry is initialised.
        if class_id(idx as usize) == Some(addr) {
            return Some(idx);
        }
        bucket = (bucket + 1) & (CLASS_HASH_BUCKETS - 1);
    }
    None
}

/// Publish a freshly registered class into the hash index.
///
/// Returns the index callers should use: normally `idx`, but if another CPU
/// registered the *same* address concurrently, the winner's index instead.
/// Returning the winner matters — two class indices for one lock would split
/// its dependency edges across two nodes and quietly weaken the cycle check.
/// (The loser's `CLASSES` slot is then wasted; leaking one slot out of 128 in a
/// race that needs two CPUs to first-touch the same lock in the same instant is
/// the cheaper of the two failures.)
fn hash_insert(addr: usize, idx: u16) -> u16 {
    let mut bucket = class_bucket(addr);
    for _ in 0..CLASS_HASH_BUCKETS {
        let Some(cell) = CLASS_HASH.get(bucket) else {
            return idx;
        };
        // Release: everything written to CLASSES[idx] must be visible to any
        // CPU that Acquire-loads this slot in `hash_lookup`.
        match cell.compare_exchange(0, idx + 1, Ordering::Release, Ordering::Acquire) {
            Ok(_) => return idx,
            Err(occupied) => {
                let other = occupied - 1;
                // Same as `hash_lookup`: a published slot names an initialised
                // entry.
                if class_id(other as usize) == Some(addr) {
                    return other;
                }
            }
        }
        bucket = (bucket + 1) & (CLASS_HASH_BUCKETS - 1);
    }
    idx
}

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

/// Per-CPU re-entrancy guard.
///
/// Prevents infinite recursion when lockdep's violation reporting
/// acquires the serial lock (which would re-enter lockdep).
static mut IN_LOCKDEP: [bool; MAX_CPUS] = [false; MAX_CPUS];

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether lockdep checking is enabled.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Total violations detected.
static VIOLATIONS: AtomicU32 = AtomicU32::new(0);

/// Cap on recursive-self-acquire reports so a genuine bug can't flood serial.
const MAX_RECURSIVE_REPORTS: u32 = 8;

/// Count of recursive same-class acquire reports emitted (rate limit).
static RECURSIVE_REPORTS: AtomicU32 = AtomicU32::new(0);

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
    set_enabled(false);
}

/// Turn lock order checking on or off without the banner `init()` prints.
///
/// Used by the lock microbenchmark to measure lockdep's share of
/// `sync::Mutex`'s per-acquire cost by differencing.
///
/// **Only toggle while the calling CPU holds no tracked lock.** The per-CPU
/// held stack is maintained by `lock_acquire`/`lock_release`, and both are
/// no-ops while disabled: a lock acquired enabled and released disabled leaves
/// a stale class on the stack forever, and the reverse underflows the depth.
/// Either way every later ordering report becomes fiction — and a validator
/// that reports fiction is worse than one that is off, because it is believed.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Release);
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

    // SAFETY: Only this CPU accesses its own IN_LOCKDEP flag.
    // Prevents re-entrancy when violation reporting acquires locks
    // (e.g., serial_println! → serial lock → lock_acquire → infinite).
    let in_lockdep = unsafe { &mut IN_LOCKDEP[cpu] };
    if *in_lockdep {
        return;
    }
    *in_lockdep = true;

    // Find or register the lock class.
    let class_idx = find_or_register_class(lock_addr, name);
    let Some(class_idx) = class_idx else {
        // SAFETY: Restoring re-entrancy guard for this CPU.
        unsafe { IN_LOCKDEP[cpu] = false; }
        return; // Table full — silently skip.
    };

    // SAFETY: Only this CPU accesses its held stack (called with lock
    // not yet acquired, so no preemption concern for the stack itself).
    let held = unsafe { &mut HELD[cpu] };

    // Check all currently-held locks for ordering violations.
    for i in 0..held.depth as usize {
        let held_class = held.stack[i];
        if held_class == class_idx {
            // Re-entrant acquisition of the SAME lock instance on the same
            // CPU.  Because a lock class is keyed by the lock's address,
            // held_class == class_idx means this exact non-reentrant
            // `crate::sync::Mutex` is already held here — the about-to-run
            // real acquire will spin forever (self-deadlock).  Report it
            // immediately: this is a precise, located diagnostic that fires
            // *before* the 30 s spinlock stall detector would.
            //
            // This is reliable now that tracked mutexes disable preemption
            // while held (sched::PREEMPT_DISABLE_COUNT): a lock can no longer
            // be held across a context switch or CPU migration, so the
            // per-CPU held stack can't carry a stale entry from another task
            // that would make a legitimate acquire look recursive.
            report_recursive(class_idx, cpu);
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

    // SAFETY: Restoring re-entrancy guard for this CPU.
    unsafe { IN_LOCKDEP[cpu] = false; }
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

    // SAFETY: Re-entrancy guard — same reasoning as lock_acquire.
    let in_lockdep = unsafe { &mut IN_LOCKDEP[cpu] };
    if *in_lockdep {
        return;
    }
    *in_lockdep = true;

    let class_idx = find_class(lock_addr);
    let Some(class_idx) = class_idx else {
        // SAFETY: Only this CPU accesses its IN_LOCKDEP slot (interrupts disabled).
        unsafe { IN_LOCKDEP[cpu] = false; }
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
            // SAFETY: Restoring re-entrancy guard for this CPU.
            unsafe { IN_LOCKDEP[cpu] = false; }
            return;
        }
    }
    // Lock not found in held stack — benign (might have been acquired
    // before lockdep was enabled, or table was full at acquire time).
    // SAFETY: Restoring re-entrancy guard for this CPU.
    unsafe { IN_LOCKDEP[cpu] = false; }
}

/// Return the number of violations detected so far.
#[allow(dead_code)]
pub fn violation_count() -> u32 {
    VIOLATIONS.load(Ordering::Relaxed)
}

/// Whether lockdep is currently enabled.
#[allow(dead_code)]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Number of registered lock classes.
#[allow(dead_code)]
pub fn class_count() -> u32 {
    CLASS_COUNT.load(Ordering::Relaxed)
}

/// Number of recorded dependency edges.
#[allow(dead_code)]
pub fn edge_count() -> u32 {
    EDGE_COUNT.load(Ordering::Relaxed)
}

/// Information about a single lock class (for diagnostics).
#[derive(Debug, Clone, Copy)]
pub struct LockClassInfo {
    /// Slot index in the class table.
    pub index: u16,
    /// Address used to identify this lock class.
    pub id: usize,
    /// Human-readable name.
    pub name: [u8; 16],
    /// Length of the name.
    pub name_len: u8,
}

/// Information about a dependency edge (for diagnostics).
#[derive(Debug, Clone, Copy)]
pub struct DepEdgeInfo {
    /// Class index of the lock that was held.
    pub from: u16,
    /// Class index of the lock that was acquired.
    pub to: u16,
}

/// Snapshot of the lockdep dependency graph.
///
/// Contains all registered lock classes and edges at the time of the call.
/// Used by diagnostic tools (kshell `lockdep` command) to display the
/// lock ordering graph and detect potential issues.
#[derive(Debug)]
pub struct LockdepSnapshot {
    /// Registered lock classes.
    pub classes: alloc::vec::Vec<LockClassInfo>,
    /// Dependency edges (from → to).
    pub edges: alloc::vec::Vec<DepEdgeInfo>,
    /// Total violations detected.
    pub violations: u32,
    /// Whether lockdep is enabled.
    pub enabled: bool,
}

/// Take a snapshot of the current lockdep state for diagnostics.
///
/// Reads the class table and edge table (append-only, so no lock needed)
/// and returns a heap-allocated snapshot.  The snapshot is consistent
/// up to races with concurrent lock_acquire calls (new entries may
/// appear between reading classes and edges).
#[allow(dead_code)]
pub fn snapshot() -> LockdepSnapshot {
    let num_classes = CLASS_COUNT.load(Ordering::Relaxed) as usize;
    let num_edges = EDGE_COUNT.load(Ordering::Relaxed) as usize;

    let mut classes = alloc::vec::Vec::with_capacity(num_classes.min(MAX_CLASSES));
    for i in 0..num_classes.min(MAX_CLASSES) {
        // SAFETY: Reading from append-only array within bounds.
        let c = unsafe { &CLASSES[i] };
        classes.push(LockClassInfo {
            index: i as u16,
            id: c.id,
            name: c.name,
            name_len: c.name_len,
        });
    }

    let mut edges = alloc::vec::Vec::with_capacity(num_edges.min(MAX_EDGES));
    for i in 0..num_edges.min(MAX_EDGES) {
        // SAFETY: Reading from append-only array within bounds.
        let e = unsafe { EDGES[i] };
        edges.push(DepEdgeInfo {
            from: e.from,
            to: e.to,
        });
    }

    LockdepSnapshot {
        classes,
        edges,
        violations: VIOLATIONS.load(Ordering::Relaxed),
        enabled: ENABLED.load(Ordering::Relaxed),
    }
}

/// Get the current nesting depth for a given CPU.
///
/// Returns the number of locks currently held on that CPU according
/// to lockdep's tracking.  Useful for diagnosing potential issues
/// where a code path holds too many locks simultaneously.
#[allow(dead_code)]
pub fn held_depth(cpu: usize) -> u8 {
    if cpu >= MAX_CPUS {
        return 0;
    }
    // SAFETY: Reading a plain u8 from the per-CPU held stack.
    // Races are benign (we might see a slightly stale value if
    // another CPU is modifying its own stack — but we only read
    // the depth for a given CPU from diagnostic contexts).
    unsafe { HELD[cpu].depth }
}

/// Print the names of all locks currently held by `cpu`, in acquisition
/// order (bottom → top of the held stack).
///
/// Used by the spinlock stall detector ([`crate::sync::Mutex`]) to report
/// the lock-holding context of a CPU that appears to be wedged spinning on
/// a lock.  This is the single most useful piece of deadlock diagnostics:
/// it reveals whether the spinning CPU already holds a lock that the
/// holder of the wanted lock is itself blocked on (an AB-BA / convoy).
///
/// Best-effort and lock-free: it reads the per-CPU held stack without any
/// synchronization.  The caller is (by construction) a CPU that is stuck
/// spinning and therefore not mutating its own held stack, so the read is
/// stable; a benign race against another CPU could at worst misprint a
/// name.  Safe to call from interrupts-disabled context (it only touches
/// static arrays and the serial port).
#[allow(dead_code)]
pub fn dump_held_locks(cpu: usize) {
    if cpu >= MAX_CPUS {
        return;
    }
    // SAFETY: Reading the per-CPU held stack + depth for diagnostics only.
    // See the doc comment: the target CPU is spinning and not mutating its
    // own stack, so this snapshot is stable. Copying by value avoids
    // holding any reference into the static across the serial prints.
    let (depth, stack) = unsafe { (HELD[cpu].depth as usize, HELD[cpu].stack) };
    let count = (CLASS_COUNT.load(Ordering::Relaxed) as usize).min(MAX_CLASSES);
    serial_println!("[lockdep]   cpu {} holds {} lock(s):", cpu, depth);
    for i in 0..depth.min(MAX_DEPTH) {
        let class_idx = stack[i] as usize;
        if class_idx >= count {
            continue;
        }
        // SAFETY: class_idx < count ≤ number of registered classes; the
        // CLASSES array is append-only so this slot is fully initialized.
        let (name, name_len) = unsafe {
            (CLASSES[class_idx].name, CLASSES[class_idx].name_len as usize)
        };
        let n = name.get(..name_len.min(16)).unwrap_or(b"");
        serial_println!(
            "[lockdep]     [{}] {}",
            i,
            core::str::from_utf8(n).unwrap_or("<non-utf8>")
        );
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Find an existing class by lock address, or register a new one.
fn find_or_register_class(lock_addr: usize, name: &[u8]) -> Option<u16> {
    // O(1) index lookup. This is the hot path: on a warm kernel every acquire
    // finds an existing class here and returns.
    if let Some(idx) = hash_lookup(lock_addr) {
        return Some(idx);
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
    // Publish only after the entry is fully written — `hash_insert`'s Release
    // store is what makes it safe for another CPU to follow the index here.
    #[allow(clippy::cast_possible_truncation)] // idx < MAX_CLASSES = 128
    Some(hash_insert(lock_addr, idx as u16))
}

/// Find an existing class by lock address.
fn find_class(lock_addr: usize) -> Option<u16> {
    hash_lookup(lock_addr)
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
            // SAFETY: i < edge_count ≤ MAX_EDGES, so EDGES[i] is within bounds.
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

/// Report a recursive acquisition of the same lock instance on one CPU.
///
/// This is an unconditional self-deadlock for a non-reentrant spinlock — the
/// real acquire that follows will spin forever.  Emitting here gives an
/// instant, precisely-located diagnostic instead of waiting ~30 s for the
/// spinlock stall detector.  Rate-limited so a genuine bug can't flood serial.
///
/// Called with the per-CPU `IN_LOCKDEP` re-entrancy guard already set, so the
/// `serial_println!` here cannot recurse back into lockdep.
#[cold]
#[inline(never)]
fn report_recursive(class_idx: u16, cpu: usize) {
    VIOLATIONS.fetch_add(1, Ordering::Relaxed);
    let n = RECURSIVE_REPORTS.fetch_add(1, Ordering::Relaxed);
    if n >= MAX_RECURSIVE_REPORTS {
        return;
    }
    let name = class_name(class_idx);
    serial_println!(
        "[lockdep] *** SELF-DEADLOCK *** CPU {} is re-acquiring lock {:?} (class {}) \
         it already holds. This non-reentrant spinlock will now spin forever — the \
         acquire is a recursive self-deadlock (fix the call path).",
        cpu, name, class_idx
    );
    dump_held_locks(cpu);
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
    // SAFETY: cpu is the current CPU index (< MAX_CPUS); only this CPU reads its slot.
    let depth = unsafe { HELD[cpu].depth };
    assert_eq!(depth, 0, "held stack should be empty after all releases");
    serial_println!("[lockdep]   Release cleanup: OK");

    // Test 6: dump_held_locks reports the held stack for the current CPU.
    // Acquire two locks, dump, then release — verifies the new diagnostic
    // helper (used by the spinlock stall detector) walks the held stack and
    // resolves class names without panicking. We only assert the depth is
    // observed correctly; the printed names are for human inspection.
    lock_acquire(lock_a, b"stall-A");
    lock_acquire(lock_b, b"stall-B");
    // SAFETY: cpu is the current CPU index (< MAX_CPUS).
    let held = unsafe { HELD[cpu].depth };
    assert_eq!(held, 2, "two locks held before dump");
    serial_println!("[lockdep]   dump_held_locks (2 held) — expected output follows:");
    dump_held_locks(cpu);
    lock_release(lock_b);
    lock_release(lock_a);
    // SAFETY: same CPU, after releases.
    let held_after = unsafe { HELD[cpu].depth };
    assert_eq!(held_after, 0, "held stack empty after releases");
    serial_println!("[lockdep]   dump_held_locks: OK");

    // Test 7: Recursive same-class acquire is reported as a self-deadlock.
    // Uses fake addresses (no real spinlock), so acquiring lock_a twice while
    // held only exercises the detector — it does not actually deadlock. The
    // 'SELF-DEADLOCK' line printed below is INTENTIONAL, not a real event.
    let v6 = VIOLATIONS.load(Ordering::Relaxed);
    serial_println!(
        "[lockdep]   (self-test) intentionally re-acquiring a held lock; the \
         'SELF-DEADLOCK' line below is expected and not a real event:"
    );
    lock_acquire(lock_a, b"test-A");
    lock_acquire(lock_a, b"test-A"); // recursive → should report + count.
    let v7 = VIOLATIONS.load(Ordering::Relaxed);
    assert_eq!(v7, v6 + 1, "recursive same-class acquire should count one violation");
    // Held stack now has [A, A]; two releases clear it.
    lock_release(lock_a);
    lock_release(lock_a);
    // SAFETY: same CPU, after releases.
    let held_rec = unsafe { HELD[cpu].depth };
    assert_eq!(held_rec, 0, "held stack empty after recursive-test releases");
    serial_println!("[lockdep]   Recursive self-deadlock detection: OK");

    // Test 8: the class hash index agrees with an exhaustive scan. Run a second
    // time late in boot from `main`, when the table is actually populated —
    // see the function's own doc comment for why once is not enough.
    verify_class_index("early");

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

/// Verify the O(1) class index against the O(n) scan it replaced.
///
/// This is the test that keeps the optimisation honest. If `hash_lookup` ever
/// misses a class that is in fact registered, `find_or_register_class` silently
/// registers a *second* class for the same lock — and then the two halves of
/// that lock's dependency edges live on different graph nodes, no cycle is ever
/// found through it, and lockdep goes quiet while looking exactly as healthy as
/// before. Nothing else in the system would notice: a validator that stops
/// finding violations is indistinguishable from a kernel that stopped having
/// them.
///
/// So the linear scan is not deleted, it is demoted to the oracle: every
/// registered class must be found by the hash at the same index the scan finds
/// it at, and the two must agree on absence too.
/// Called **twice per boot**, and both calls matter for different reasons.
/// From `self_test()` the table holds ~3 classes, so the scan-vs-hash sweep is
/// nearly vacuous and only the synthetic collision case does real work; late in
/// boot it holds ~43, which is the occupancy at which a probe-sequence bug
/// would actually manifest. A single early call would be a check sized for a
/// table 7% as full as the one it defends.
///
/// `when` labels the call site, because the two runs are not interchangeable
/// and a log line that doesn't say which one it is invites reading the vacuous
/// pass as the meaningful one.
pub fn verify_class_index(when: &str) {
    let count = (CLASS_COUNT.load(Ordering::Relaxed) as usize).min(MAX_CLASSES);

    // Every registered class is found, at the index the scan would report.
    let mut checked = 0usize;
    for i in 0..count {
        let Some(id) = class_id(i) else {
            continue;
        };
        // Scan oracle: the first slot carrying this id.
        let mut scan = None;
        for j in 0..count {
            if class_id(j) == Some(id) {
                scan = Some(j as u16);
                break;
            }
        }
        assert_eq!(
            hash_lookup(id),
            scan,
            "class hash disagrees with the linear scan for a registered lock"
        );
        checked += 1;
    }

    // Absence agrees too. This address is never used as a lock: it is
    // non-canonical, so no real lock can live there, and the fake addresses the
    // tests above use are all 0xDEAD_000x.
    const UNREGISTERED: usize = 0xBADD_C0DE_BADD_C0DE;
    assert!(
        hash_lookup(UNREGISTERED).is_none(),
        "class hash reports an address that was never registered"
    );

    // Idempotence: registering the same address twice must yield one class.
    // This is the property the whole index exists to provide, and the one whose
    // failure is silent, so it is asserted directly rather than inferred.
    const FRESH: usize = 0xBADD_C0DE_0000_0001;
    let first = find_or_register_class(FRESH, b"hash-test");
    let second = find_or_register_class(FRESH, b"hash-test");
    assert!(first.is_some(), "registration failed (class table full?)");
    assert_eq!(first, second, "same address registered as two classes");

    // Collision handling: an address that hashes to the *same bucket* as
    // `FRESH` must still be found, and must be a distinct class. Without a
    // correct probe sequence this is the case that breaks, and it cannot occur
    // by luck in a test that only uses a handful of well-spread addresses — so
    // the colliding address is searched for rather than hoped for.
    let target_bucket = class_bucket(FRESH);
    let mut collider = None;
    for candidate in 1u64..100_000 {
        let addr = 0xBADD_C0DE_0000_0000usize.wrapping_add(candidate as usize);
        if addr != FRESH && class_bucket(addr) == target_bucket {
            collider = Some(addr);
            break;
        }
    }
    if let Some(addr) = collider {
        let c1 = find_or_register_class(addr, b"hash-coll");
        assert!(c1.is_some(), "colliding registration failed");
        assert_ne!(c1, first, "colliding addresses collapsed into one class");
        assert_eq!(hash_lookup(addr), c1, "colliding class not found by probe");
        assert_eq!(hash_lookup(FRESH), first, "probe run broke the earlier entry");
        serial_println!(
            "[lockdep]   class hash ({}): OK ({} classes verified vs scan, bucket collision handled)",
            when, checked
        );
    } else {
        // Not a silent pass: if no collider was found the collision path went
        // untested and the log must say so.
        serial_println!(
            "[lockdep]   class hash ({}): OK ({} classes verified vs scan) — WARNING no \
             colliding address found in 100k candidates, probe path UNTESTED",
            when, checked
        );
    }
}

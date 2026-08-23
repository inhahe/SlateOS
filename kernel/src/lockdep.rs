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
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of distinct lock classes tracked.
///
/// Raised from 128 to 256 because 128 was **not** a comfortable ceiling: once
/// `lockdep::init()` moved ahead of the ring-3 self-test battery, the bench
/// harness reported `128 lockdep classes registered` — exactly the cap, where
/// every previous run had reported 43-44. A table at exactly its limit is a
/// table that has been silently discarding classes, and a discarded class is a
/// lock the validator no longer checks at all. See
/// `report_class_table_full` for the warning that now fires instead of
/// dropping them quietly, and `known-issues.md`
/// A-LOCKDEP-RECORD-EDGE-WAS-A-LINEAR-SCAN-ON-EVERY-NESTED-ACQUIRE.
///
/// 256 is a guess at headroom, not a measured requirement: the true number of
/// distinct classes was masked by the old cap. The warning is what will tell us
/// if 256 is also too low.
const MAX_CLASSES: usize = 256;

/// Maximum nesting depth per CPU (locks held simultaneously).
const MAX_DEPTH: usize = 16;

/// Number of dependency edges the graph can hold.
///
/// Not a configurable budget: the graph is an adjacency bitmap over
/// [`MAX_CLASSES`] (see [`ADJ`]), so every representable edge fits and this is
/// simply how many that is. It exists to be reported in the startup banner.
/// There is deliberately no "edge table full" path any more — the previous
/// 512-edge list silently stopped detecting new cycles once exhausted.
const MAX_EDGES: usize = MAX_CLASSES * MAX_CLASSES;

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

/// Number of class slots *reserved*. Not the number that are readable.
///
/// A registering CPU claims a slot by bumping this counter and only then fills
/// the slot in, so a reader that bounds a loop by this value alone can reach a
/// slot that is reserved but still blank. Bound loops by it, but gate each slot
/// on [`class_is_ready`] before reading it.
static CLASS_COUNT: AtomicU32 = AtomicU32::new(0);

/// Publication flag per class slot: `true` once `CLASSES[i]` is fully written.
///
/// [`find_or_register_class`] reaches a slot two different ways, and only one of
/// them was ordered. A reader arriving through [`hash_lookup`] cannot observe the
/// slot index until `hash_insert`'s `Release` store, by which point the slot is
/// complete — that path was always safe. But three readers ([`snapshot`],
/// [`dump_held_locks`], [`verify_class_index`]) find slots by *counting* instead,
/// bounded by `CLASS_COUNT`, which is incremented *before* the slot is written.
/// Those readers could see a blank or half-written slot and print a lock name and
/// address of zero — in the held-lock dump, the very output someone is reading to
/// identify a deadlock.
///
/// A separate array rather than a field on `LockClass`, deliberately: `LockClass`
/// is `Copy` so a slot can be snapshotted in one indexing operation, and an
/// `AtomicBool` field would take that away. Keeping the flag alongside costs one
/// byte per class and leaves the hot read shape untouched.
static CLASS_READY: [AtomicBool; MAX_CLASSES] = [const { AtomicBool::new(false) }; MAX_CLASSES];

/// Instruction pointer of the most recent acquisition of each class, or `0`.
///
/// A lock class is keyed by the lock's *address*, which `ksyms` can turn back
/// into a name only when the lock lives in a `static`. A heap-allocated lock —
/// a per-mount filesystem lock, a per-connection lock — resolves to nothing, so
/// a violation involving one names one participant and leaves the other a bare
/// hex address. That is exactly the shape of the inversion this array was added
/// for: `fs::handle::OPEN_FILES` (a static, resolvable) against a heap lock at
/// `0xffff80007d6e8f20` (not).
///
/// Recording *where the lock was taken* fixes that, because the acquiring code
/// is always in `.text` and therefore always resolvable. It is the more useful
/// half of the answer in any case: knowing a lock sits at some heap address
/// does not tell you what to change, and knowing which function took it does.
///
/// Most-recent rather than first: for the lock currently *held* at the moment
/// of a violation, the most recent acquisition is precisely the one still in
/// force, so the report describes the live situation rather than a historical
/// one.
///
/// A separate array rather than a field on [`LockClass`], for the same reason
/// as [`CLASS_READY`]: `LockClass` is `Copy` so a slot can be snapshotted in a
/// single indexing operation, and an atomic field would take that away.
/// `Relaxed` throughout — this is a diagnostic hint, not a synchronisation
/// point, and no other state is published through it.
static CLASS_SITE: [AtomicUsize; MAX_CLASSES] = [const { AtomicUsize::new(0) }; MAX_CLASSES];

/// The return address of the caller of [`lock_acquire`].
///
/// Walks exactly two frames of the RBP chain: this helper's own frame, then
/// `lock_acquire`'s. Both are guaranteed to exist — we are executing inside
/// them — and the kernel is built with `-C force-frame-pointers=yes`
/// (`.cargo/config.toml`), so neither can have been omitted. That is why this
/// does no range validation the way [`crate::backtrace::walk_from`] must: that
/// walker follows a chain of *arbitrary* length into frames that may be
/// corrupt, whereas these two are ours.
///
/// `#[inline(never)]` is load-bearing: an inlined body would have no frame of
/// its own and the walk would start one level too high.
///
/// The address returned is inside `Mutex::<T>::lock` when that call was not
/// inlined — which still identifies the guarded type through the monomorphised
/// symbol name — and inside the true calling function when it was.
#[inline(never)]
fn caller_ip() -> usize {
    let rbp: u64;
    // SAFETY: Reading RBP is always safe in ring 0.
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack));
    }
    // Defensive despite the argument above: a zero or misaligned RBP would mean
    // the frame-pointer guarantee has been broken, and reading through it would
    // fault inside a diagnostic path. Returning 0 degrades the report to what
    // it printed before this array existed, which is the right failure.
    if rbp == 0 || !rbp.is_multiple_of(8) {
        return 0;
    }
    // SAFETY: `rbp` is this function's own frame pointer, established by its
    // prologue, so `[rbp]` — the saved caller RBP — is in-bounds and
    // initialised.
    let parent_rbp: u64 = unsafe { core::ptr::read_volatile(rbp as *const u64) };
    if parent_rbp == 0 || !parent_rbp.is_multiple_of(8) {
        return 0;
    }
    // SAFETY: `parent_rbp` is `lock_acquire`'s frame pointer, likewise
    // established by its prologue, so `[parent_rbp + 8]` is its return-address
    // slot — in-bounds and initialised by the `call` that reached it.
    #[allow(clippy::arithmetic_side_effects)]
    let ret: u64 = unsafe { core::ptr::read_volatile((parent_rbp + 8) as *const u64) };
    #[allow(clippy::cast_possible_truncation)]
    {
        ret as usize
    }
}

/// The recorded acquisition site for class `idx`, if one has been captured.
fn class_site(idx: u16) -> Option<usize> {
    match CLASS_SITE.get(idx as usize)?.load(Ordering::Relaxed) {
        0 => None,
        ip => Some(ip),
    }
}

/// Whether class slot `idx` is fully written and safe to read.
///
/// The `Acquire` pairs with the `Release` store in [`find_or_register_class`]: if
/// this returns `true`, every write to `CLASSES[idx]` made before publication is
/// visible to this CPU.
#[inline]
fn class_is_ready(idx: usize) -> bool {
    CLASS_READY
        .get(idx)
        .is_some_and(|f| f.load(Ordering::Acquire))
}

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
// scans of up to `MAX_CLASSES` entries (128 when this was measured).  The cost
// is also *positional*:
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

/// log2 of the number of hash buckets. Tracks [`MAX_CLASSES`]: raising the
/// class capacity without raising this would push the load factor up and
/// lengthen every probe run, which is the cost this index exists to remove.
const CLASS_HASH_SHIFT: u32 = 10;

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

/// `u64` words per adjacency row, one bit per possible destination class.
const ADJ_WORDS: usize = MAX_CLASSES.div_ceil(64);

/// Dependency graph as an adjacency bitmap: row `from`, bit `to`.
///
/// # Why a bitmap and not the edge list it replaces
///
/// This is on the hot path of *every* nested lock acquire. `lock_acquire` calls
/// [`record_edge`] once per already-held lock (up to [`MAX_DEPTH`] = 16), and
/// the old implementation answered "is this edge already known?" with a linear
/// scan over every edge recorded so far. That made the per-acquire cost
/// `O(held_depth x edges)` — and `edges` grows monotonically for the whole
/// uptime of the machine, so the cost of taking a lock grew with how long the
/// kernel had been running. Measured consequence: when `lockdep::init()` moved
/// ahead of the ring-3 self-test battery, the class table went from 43-44
/// entries at benchmark time to a *full* 128, and `page_fault_anonymous` more
/// than doubled (~2080ns -> 4978ns). See `known-issues.md`
/// A-LOCKDEP-RECORD-EDGE-WAS-A-LINEAR-SCAN-ON-EVERY-NESTED-ACQUIRE.
///
/// A bitmap makes the same question one shifted load and one `fetch_or`:
/// `O(1)`, independent of how many edges exist. `has_cycle`'s neighbour
/// enumeration likewise drops from a full-table scan per BFS node to
/// [`ADJ_WORDS`] (= 4) word loads.
///
/// It is also cheap for what it buys — 256 x 256 bits = 8 KiB, against a
/// 512-entry edge list (2 KiB) that could represent only 512 of the 65536
/// possible edges — and so it removes the "edge table full" case
/// entirely: every representable edge now fits, meaning cycle detection no
/// longer goes silently blind once a long-running kernel exhausts `MAX_EDGES`.
static ADJ: [[AtomicU64; ADJ_WORDS]; MAX_CLASSES] =
    [const { [const { AtomicU64::new(0) }; ADJ_WORDS] }; MAX_CLASSES];

/// Number of distinct edges recorded. Pure statistic: it no longer bounds any
/// loop, because [`ADJ`] can hold every representable edge.
static EDGE_COUNT: AtomicU32 = AtomicU32::new(0);

/// Set once the class table has overflowed and the warning has been printed.
static CLASS_TABLE_FULL_REPORTED: AtomicBool = AtomicBool::new(false);

/// Split a class index into its `(word, bit-mask)` position in an [`ADJ`] row.
#[inline]
fn adj_pos(class: u16) -> Option<(usize, u64)> {
    let c = class as usize;
    if c >= MAX_CLASSES {
        return None;
    }
    Some((c / 64, 1u64 << (c % 64)))
}

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
    serial_println!(
        "[lockdep] Lock order validator enabled (max {} classes, {} edges)",
        MAX_CLASSES,
        MAX_EDGES
    );
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
        // Table full. Say so exactly once: from here on this lock is invisible
        // to the validator, so a real deadlock involving it will go unreported
        // and the absence of a warning stops meaning "no violation". Silently
        // skipping made lockdep degrade into a no-op that still looked healthy.
        report_class_table_full();
        // SAFETY: Restoring re-entrancy guard for this CPU.
        unsafe {
            IN_LOCKDEP[cpu] = false;
        }
        return;
    };

    // Record where this acquisition came from, before any report can be
    // emitted below — a violation involving this class should describe the
    // acquisition that provoked it, not the one before.
    if let Some(slot) = CLASS_SITE.get(class_idx as usize) {
        slot.store(caller_ip(), Ordering::Relaxed);
    }

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
    unsafe {
        IN_LOCKDEP[cpu] = false;
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

    // SAFETY: Re-entrancy guard — same reasoning as lock_acquire.
    let in_lockdep = unsafe { &mut IN_LOCKDEP[cpu] };
    if *in_lockdep {
        return;
    }
    *in_lockdep = true;

    let class_idx = find_class(lock_addr);
    let Some(class_idx) = class_idx else {
        // SAFETY: Only this CPU accesses its IN_LOCKDEP slot (interrupts disabled).
        unsafe {
            IN_LOCKDEP[cpu] = false;
        }
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
            unsafe {
                IN_LOCKDEP[cpu] = false;
            }
            return;
        }
    }
    // Lock not found in held stack — benign (might have been acquired
    // before lockdep was enabled, or table was full at acquire time).
    // SAFETY: Restoring re-entrancy guard for this CPU.
    unsafe {
        IN_LOCKDEP[cpu] = false;
    }
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
        // A reserved-but-unfilled slot is not yet a class; omitting it is right
        // for a snapshot, whose consumers treat every element as a real entry.
        if !class_is_ready(i) {
            continue;
        }
        // SAFETY: `i < MAX_CLASSES` and `class_is_ready(i)` established that the
        // slot is fully written and never mutated again, so this `Copy` read of a
        // published slot is sound. Copied by value rather than borrowed: a shared
        // reference to a mutable static is UB the moment another CPU appends (see
        // `class_id`), which is what `&CLASSES[i]` used to form here.
        let c = unsafe { CLASSES[i] };
        classes.push(LockClassInfo {
            index: i as u16,
            id: c.id,
            name: c.name,
            name_len: c.name_len,
        });
    }

    // Materialise the edge list from the adjacency bitmap. `num_edges` is only
    // a capacity hint here: it is read before the walk, so a concurrent
    // `record_edge` can make the walk yield more (or, after a wider change,
    // fewer) than it predicted. The Vec grows if so — the count is not used to
    // bound the loop, which is the bug this whole module just got fixed for.
    let mut edges = alloc::vec::Vec::with_capacity(num_edges);
    for (from, row) in ADJ.iter().enumerate() {
        for (w, cell) in row.iter().enumerate() {
            let mut bits = cell.load(Ordering::Relaxed);
            while bits != 0 {
                let b = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                #[allow(clippy::cast_possible_truncation)] // both < MAX_CLASSES, which fits u16
                edges.push(DepEdgeInfo {
                    from: from as u16,
                    to: (w * 64 + b) as u16,
                });
            }
        }
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
    // Say so when disabled, instead of reporting an empty stack as a fact.
    // `lock_acquire`/`lock_release` are no-ops while disabled, so the held stack
    // is not merely stale but meaningless -- and the difference matters: printed
    // during a spinlock-stall dump, "cpu 0 holds 0 lock(s)" reads as evidence
    // that the CPU does *not* hold the wanted lock, which directly contradicts
    // the "RECURSIVE self-deadlock" line printed immediately above it. A reader
    // then has to pick which line to believe, and the wrong choice rules out the
    // correct explanation. This happened; see `known-issues.md`
    // BUG-BOOT-SPINLOCK-STALL-UNNAMED.
    if !ENABLED.load(Ordering::Relaxed) {
        serial_println!(
            "[lockdep]   cpu {cpu}: validator DISABLED — held-lock stack is not maintained, \
             so nothing can be concluded from it"
        );
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
        // Skip a slot that is reserved but not yet filled. Printing it would
        // render as `?` @ 0x0 -- indistinguishable from a real lock named by the
        // default name, in the one report where a wrong identity is worst.
        if !class_is_ready(class_idx) {
            serial_println!("[lockdep]     [{i}] <class {class_idx} still being registered>");
            continue;
        }
        // Copy the whole slot in one indexing operation rather than one per
        // field. `LockClass` is `Copy`, so this is no more work, and it keeps the
        // number of `indexing_slicing` sites -- each a separate panic path to
        // justify -- at one, instead of growing with every field the dump learns
        // to print.
        // SAFETY: class_idx < count ≤ number of registered classes; the CLASSES
        // array is append-only, so this slot is fully initialized and is never
        // mutated after publication.
        let class = unsafe { CLASSES[class_idx] };
        let (name, name_len, id) = (class.name, class.name_len as usize, class.id);
        let n = name.get(..name_len.min(16)).unwrap_or(b"");
        // Print the class address, not just the name. Most locks in the tree take
        // `Mutex::new`'s default name of "?", so a held stack rendered by name
        // alone reads "[0] ?" / "[1] ?" -- entries that cannot be told apart from
        // each other, let alone matched against the lock named in the stall
        // report printed immediately above. The address is the same value
        // `sync::report_spin_stall` prints, so "am I already holding the lock I am
        // stalled on?" -- the entire question a recursive self-deadlock turns
        // on -- becomes a comparison instead of a guess.
        // The address goes through `AddrDesc` so a lock living in a `static`
        // resolves to that static's name, and the recorded acquisition site is
        // appended so a heap lock — which resolves to nothing — is still
        // identified, by the code that took it.
        match class_site(stack[i]) {
            Some(ip) => serial_println!(
                "[lockdep]     [{}] {} @ {}, taken at {}",
                i,
                core::str::from_utf8(n).unwrap_or("<non-utf8>"),
                AddrDesc(id),
                AddrDesc(ip)
            ),
            None => serial_println!(
                "[lockdep]     [{}] {} @ {}",
                i,
                core::str::from_utf8(n).unwrap_or("<non-utf8>"),
                AddrDesc(id)
            ),
        }
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

    // Reserve a slot by CAS rather than an unconditional `fetch_add`.
    //
    // The previous form incremented first and "undid" the increment with a
    // `fetch_sub` when the table turned out to be full, which is not sound under
    // contention: two CPUs overflowing concurrently, interleaved with a third
    // succeeding, can leave the counter naming a slot nobody owns or *below* the
    // number of live classes — which would make the counting readers skip real
    // entries. Refusing to increment past the bound in the first place needs no
    // undo, so that whole class of interleaving disappears.
    let idx = loop {
        let cur = CLASS_COUNT.load(Ordering::Relaxed);
        if cur as usize >= MAX_CLASSES {
            // Table full. The counter is left alone, so it never exceeds
            // MAX_CLASSES and readers need no clamp to stay in bounds.
            return None;
        }
        if CLASS_COUNT
            .compare_exchange_weak(cur, cur + 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            break cur as usize;
        }
    };

    // SAFETY: We "own" slot `idx` because the CAS above gave us a unique index,
    // and no slot is ever reused or mutated after publication. No other CPU will
    // write to this slot.
    unsafe {
        CLASSES[idx].id = lock_addr;
        let copy_len = name.len().min(16);
        CLASSES[idx].name[..copy_len].copy_from_slice(&name[..copy_len]);
        CLASSES[idx].name_len = copy_len as u8;
    }
    // Publish the slot to the *counting* readers. This must come after the writes
    // above and before `hash_insert`, and it is the store `class_is_ready`'s
    // `Acquire` pairs with. `hash_insert`'s own `Release` covers only readers that
    // arrive via `hash_lookup`; it says nothing to a reader walking 0..CLASS_COUNT,
    // which is why this second publication point exists at all.
    #[allow(clippy::indexing_slicing)] // idx < MAX_CLASSES, enforced by the CAS above
    CLASS_READY[idx].store(true, Ordering::Release);
    #[allow(clippy::cast_possible_truncation)] // idx < MAX_CLASSES, which fits u16
    Some(hash_insert(lock_addr, idx as u16))
}

/// Find an existing class by lock address.
fn find_class(lock_addr: usize) -> Option<u16> {
    hash_lookup(lock_addr)
}

/// Record a dependency edge (from → to).  Returns true if this is a NEW edge.
fn record_edge(from: u16, to: u16) -> bool {
    let Some((word, mask)) = adj_pos(to) else {
        return false;
    };
    let Some(row) = ADJ.get(from as usize) else {
        return false;
    };
    let Some(cell) = row.get(word) else {
        return false;
    };

    // One atomic op replaces the whole scan-then-append sequence. `fetch_or`
    // also makes the "is it new?" answer race-free, which the old code was not:
    // two CPUs could both complete the scan before either appended, and both
    // would append the same edge and both report it as new. Here exactly one
    // CPU observes the bit as previously clear.
    let prev = cell.fetch_or(mask, Ordering::Relaxed);
    if prev & mask != 0 {
        return false; // Already recorded.
    }
    EDGE_COUNT.fetch_add(1, Ordering::Relaxed);
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
/// BFS over the whole reachable set. Complete: every class is enqueued at most
/// once, so the queue cannot need more than [`MAX_CLASSES`] slots and the walk
/// never truncates.
///
/// It used to stop after 32 nodes, which silently made cycle detection
/// incomplete -- a deadlock whose cycle ran through a 33rd class was simply not
/// reported, and a validator that misses violations without saying so is worse
/// than one that is switched off. Raising `MAX_CLASSES` to 256 would have made
/// that hole wider still. The cost of closing it is bounded and small: a
/// `[u16; 256]` queue and a 4-word visited bitmap is ~544 bytes of stack, on a
/// 16 KiB interrupt stack, in a function that only runs when an edge is
/// genuinely new.
fn has_cycle(start: u16, target: u16) -> bool {
    let mut queue = [0u16; MAX_CLASSES];
    let mut head = 0usize;
    let mut tail = 0usize;
    // Bitmap rather than `[bool; MAX_CLASSES]`: 4 words instead of 256 bytes,
    // which is most of what the larger queue costs back.
    let mut visited = [0u64; ADJ_WORDS];

    let Some((sw, smask)) = adj_pos(start) else {
        return false;
    };
    #[allow(clippy::indexing_slicing)] // adj_pos bounds `sw` to ADJ_WORDS
    {
        visited[sw] |= smask;
        queue[tail] = start;
    }
    tail += 1;

    while head < tail {
        #[allow(clippy::indexing_slicing)] // head < tail <= MAX_CLASSES = queue.len()
        let current = queue[head];
        head += 1;

        // Enumerate the successors of `current` straight out of its adjacency
        // row: ADJ_WORDS (= 4) loads, iterating only the bits that are set,
        // instead of scanning the entire edge table once per dequeued node.
        let Some(row) = ADJ.get(current as usize) else {
            continue;
        };
        for (w, cell) in row.iter().enumerate() {
            let mut bits = cell.load(Ordering::Relaxed);
            while bits != 0 {
                let b = bits.trailing_zeros() as usize;
                bits &= bits - 1; // clear the lowest set bit
                let to_idx = w * 64 + b;
                if to_idx == target as usize {
                    return true; // Cycle found!
                }
                // `to_idx < MAX_CLASSES` holds because it came out of an
                // ADJ row, whose width is exactly MAX_CLASSES bits.
                let vmask = 1u64 << b;
                #[allow(clippy::indexing_slicing)] // w indexes the row we are iterating
                if visited[w] & vmask == 0 && tail < MAX_CLASSES {
                    #[allow(clippy::indexing_slicing)] // tail < MAX_CLASSES = queue.len()
                    {
                        visited[w] |= vmask;
                        #[allow(clippy::cast_possible_truncation)] // < MAX_CLASSES, fits u16
                        {
                            queue[tail] = to_idx as u16;
                        }
                    }
                    tail += 1;
                }
            }
        }
    }
    false
}

/// Warn, once per boot, that the class table is full.
///
/// Called with the per-CPU `IN_LOCKDEP` re-entrancy guard already set, so the
/// `serial_println!` here cannot recurse back into lockdep.
///
/// Deliberately unconditional rather than rate-limited-to-N: this is a
/// capacity fact about the build, not an event stream, so one line is both
/// necessary and sufficient. The `swap` makes it exactly one even if several
/// CPUs overflow at once.
#[cold]
#[inline(never)]
fn report_class_table_full() {
    if CLASS_TABLE_FULL_REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    serial_println!(
        "[lockdep] WARNING: class table full at {} entries — further lock classes \
         are NOT being tracked, so absence of a violation warning no longer means \
         there is none. Raise MAX_CLASSES in kernel/src/lockdep.rs.",
        MAX_CLASSES
    );
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
    // Print the lock addresses, not just the names. Most locks in the tree take
    // `Mutex::new`'s default name of `"?"`, so a violation rendered by name
    // alone reads `Holding lock "?" … acquiring lock "?"` — a report that
    // cannot be acted on, because it does not say which two locks inverted.
    // `dump_held_locks` reached the same conclusion for the same reason; this
    // report was simply left behind.
    //
    // The address is also run through `ksyms`, which since it began indexing
    // data symbols resolves a lock living in a `static` to that static's name
    // — turning `"?" @ 0xffffffff828d9c48` into the one thing the reader
    // actually needs. It does not resolve heap-allocated locks (a per-mount
    // filesystem lock, say), which is why the raw address is still printed
    // beside it: that value is what `sync::report_spin_stall` and
    // `dump_held_locks` print too, so the three reports can be cross-read,
    // and it remains resolvable offline against the kernel ELF.
    serial_println!(
        "[lockdep]   Holding lock {:?} @ {} (class {}), acquiring lock {:?} @ {} (class {})",
        held_name,
        AddrDesc(class_addr(held_class)),
        held_class,
        acq_name,
        AddrDesc(class_addr(acquired_class)),
        acquired_class
    );
    // Where each lock was taken. For a heap-allocated lock this is the only
    // identifying information there is — its address resolves to no symbol —
    // and it is what a reader needs even when the address does resolve, since
    // the fix is always a change to one of these two call paths.
    match (class_site(held_class), class_site(acquired_class)) {
        (Some(h), Some(a)) => {
            serial_println!("[lockdep]     held taken at:  {}", AddrDesc(h));
            serial_println!("[lockdep]     acquiring at:   {}", AddrDesc(a));
        }
        (Some(h), None) => serial_println!("[lockdep]     held taken at:  {}", AddrDesc(h)),
        (None, Some(a)) => serial_println!("[lockdep]     acquiring at:   {}", AddrDesc(a)),
        (None, None) => {}
    }
    serial_println!("[lockdep]   But the reverse order was observed previously.");
    serial_println!("[lockdep]   This means a deadlock is possible under different scheduling.");
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
        "[lockdep] *** SELF-DEADLOCK *** CPU {} is re-acquiring lock {:?} @ {:#x} (class {}) \
         it already holds. This non-reentrant spinlock will now spin forever — the \
         acquire is a recursive self-deadlock (fix the call path).",
        cpu,
        name,
        class_addr(class_idx),
        class_idx
    );
    dump_held_locks(cpu);
}

/// Get the name of a lock class for diagnostic output.
///
/// Returns `"?"` for an out-of-range index and for a slot that is reserved but
/// not yet published, rather than reading a half-written name: the callers are
/// violation and self-deadlock reports, where a plausible-looking wrong name is
/// worse than an admitted unknown.
fn class_name(idx: u16) -> &'static str {
    let idx = idx as usize;
    if idx >= MAX_CLASSES || !class_is_ready(idx) {
        return "?";
    }
    // SAFETY: `idx` is in bounds and the read is a place expression on a `Copy`
    // field, so no reference to the mutable static is formed.
    #[allow(clippy::indexing_slicing)]
    let len = (unsafe { CLASSES[idx].name_len } as usize).min(16);
    if len == 0 {
        return "?";
    }
    // SAFETY: `class_is_ready` established the slot is fully written, and a
    // published slot is never mutated again, so these bytes are immutable for the
    // remaining life of the kernel -- which is what makes the `'static` sound.
    // `&raw const` reaches the field without forming `&CLASSES[idx]`, a shared
    // reference to a mutable static that is UB the moment another CPU appends
    // (the bug this replaced; see `class_id`).
    #[allow(clippy::indexing_slicing)]
    let bytes =
        unsafe { core::slice::from_raw_parts((&raw const CLASSES[idx].name).cast::<u8>(), len) };
    core::str::from_utf8(bytes).unwrap_or("?")
}

/// Get the address of the lock instance a class was registered from, or 0.
///
/// The companion to [`class_name`], and the reason it exists is that the name
/// is very often not an identifier: `sync::Mutex::new` gives every lock the
/// default name `"?"`, so a report that prints names alone can produce
/// `Holding lock "?" … acquiring lock "?"` — two locks that cannot be told
/// apart from each other, from a third, or from the lock in any other report.
/// The address is unique by construction, is the same value
/// `sync::report_spin_stall` and [`dump_held_locks`] print, and resolves to the
/// owning static offline against the kernel ELF's symbol table.
///
/// Returns 0 for an out-of-range index and for a slot that is reserved but not
/// yet published, matching `class_name`'s "admit the unknown" policy. That
/// readiness gate is the only thing this adds over [`class_id`], whose own
/// callers are the address→class lookup paths and deliberately *do* want to see
/// a reserved slot. The read itself is delegated rather than repeated, so the
/// `unsafe` touching `CLASSES` stays confined to one function.
fn class_addr(idx: u16) -> usize {
    let idx = idx as usize;
    if !class_is_ready(idx) {
        return 0;
    }
    class_id(idx).unwrap_or(0)
}

/// Renders a lock instance address as `0xADDR (symbol+0xNN)` when it can be
/// named, and as a bare `0xADDR` when it cannot.
///
/// A lock that lives in a `static` resolves, because [`crate::ksyms`] indexes
/// data symbols; a heap-allocated one (a per-mount filesystem lock, say) does
/// not, and there is nothing to add for it. The address is always printed
/// either way — it is the identity that ties this report to
/// [`dump_held_locks`] and to `sync::report_spin_stall`, and it is what a
/// reader resolves offline if symbols were unavailable at boot (which happens
/// if the image was stripped of `.symtab`; see [`crate::ksyms`]).
///
/// This is a `Display` adapter rather than a function returning `String`
/// because it is used from a lock-violation report, which must neither
/// allocate nor take a lock: it uses
/// [`crate::ksyms::resolve_static`], which does neither, and it formats
/// straight into the caller's `Formatter`. The rest of this module is
/// entirely lock-free and allocation-free for the same reason, and this
/// keeps it that way.
struct AddrDesc(usize);

impl core::fmt::Display for AddrDesc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:#x}", self.0)?;
        match crate::ksyms::resolve_static(self.0 as u64) {
            Some((name, 0)) => write!(f, " ({name})"),
            Some((name, offset)) => write!(f, " ({name}+{offset:#x})"),
            None => Ok(()),
        }
    }
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
    assert_eq!(
        v7,
        v6 + 1,
        "recursive same-class acquire should count one violation"
    );
    // Held stack now has [A, A]; two releases clear it.
    lock_release(lock_a);
    lock_release(lock_a);
    // SAFETY: same CPU, after releases.
    let held_rec = unsafe { HELD[cpu].depth };
    assert_eq!(
        held_rec, 0,
        "held stack empty after recursive-test releases"
    );
    serial_println!("[lockdep]   Recursive self-deadlock detection: OK");

    // Test 8: the class hash index agrees with an exhaustive scan. Run a second
    // time late in boot from `main`, when the table is actually populated —
    // see the function's own doc comment for why once is not enough.
    verify_class_index("early");

    // Test 9: slot publication. Every slot the counter claims exists must also be
    // marked ready, and slots past the counter must not be -- the invariant the
    // three counting readers (`snapshot`, `dump_held_locks`, `verify_class_index`)
    // now rely on instead of assuming a reserved slot is a filled one.
    //
    // This cannot reproduce the race it guards against, which needs a reader to
    // land inside another CPU's registration window. What it does catch is the
    // likely regression: a future edit that reserves a slot, or reorders the
    // publishing store after `hash_insert`, and leaves `CLASS_READY` unset -- at
    // which point every counting reader silently skips a real class and the dump
    // goes quiet rather than wrong. A quiet diagnostic is the failure mode worth a
    // boot-time assert.
    let published = CLASS_COUNT.load(Ordering::Relaxed) as usize;
    for i in 0..published {
        assert!(
            class_is_ready(i),
            "class slot below CLASS_COUNT is not published"
        );
    }
    assert!(
        !class_is_ready(published),
        "class slot at CLASS_COUNT is published before it was reserved"
    );
    assert!(
        !class_is_ready(MAX_CLASSES),
        "class_is_ready must refuse an out-of-range index"
    );
    // An unpublished slot must name itself unknown rather than render its blank
    // bytes as a real lock name.
    #[allow(clippy::cast_possible_truncation)] // published <= MAX_CLASSES, which fits u16
    let unpublished_name = class_name(published as u16);
    assert!(
        unpublished_name == "?",
        "unpublished class slot reported a name"
    );
    serial_println!("[lockdep]   Slot publication (ready flags vs count): OK");

    // Test 9b: acquisition-site capture. A heap-allocated lock's address
    // resolves to no symbol, so for those the recorded site is the *only*
    // identifying information a violation report can offer. Verify it is
    // actually captured, and that it points into kernel text rather than at
    // whatever an off-by-one frame walk would produce.
    {
        const SITE_PROBE: usize = 0xdead_5117;
        lock_acquire(SITE_PROBE, b"site-probe");
        let idx = find_or_register_class(SITE_PROBE, b"site-probe")
            .expect("site probe class must be registered");
        let site = class_site(idx).expect("acquiring a lock must record where from");
        assert!(
            site >= 0xFFFF_FFFF_8000_0000,
            "recorded site {site:#x} is not a kernel text address — the frame walk in \
             caller_ip() is off by a frame, or a frame pointer was omitted"
        );
        // The site must resolve to *this* function: `caller_ip` walks to
        // `lock_acquire`'s caller, and that caller is the self-test. A name
        // from anywhere else means the walk depth is wrong. `resolve_static`
        // yields nothing before ksyms has loaded, which is not a failure —
        // the address check above still holds.
        if let Some((name, _)) = crate::ksyms::resolve_static(site as u64) {
            assert!(
                name.contains("lockdep"),
                "recorded site resolved to {name}, which is not the calling self-test"
            );
        }
        lock_release(SITE_PROBE);
        serial_println!(
            "[lockdep]   Acquisition-site capture: OK ({})",
            AddrDesc(site)
        );
    }

    // Test 10: the edge bitmap. `record_edge` answers "is this edge new?" on
    // every nested acquire, so the property that matters is that it says "new"
    // exactly once per distinct edge — if it ever says "new" for an edge it
    // already holds, `has_cycle` runs on the hot path instead of only on
    // genuinely new dependencies, and the O(1) win is given straight back.
    //
    // Uses two class indices near the top of the table, which the boot has not
    // reached, so this neither perturbs real dependency data nor depends on it.
    let e_from = (MAX_CLASSES - 1) as u16;
    let e_to = (MAX_CLASSES - 2) as u16;
    let edges_before = EDGE_COUNT.load(Ordering::Relaxed);
    assert!(
        record_edge(e_from, e_to),
        "first record_edge for a fresh pair must report the edge as new"
    );
    assert!(
        !record_edge(e_from, e_to),
        "record_edge must report a duplicate edge as already known"
    );
    assert_eq!(
        EDGE_COUNT.load(Ordering::Relaxed),
        edges_before + 1,
        "a duplicate edge must not increment the edge count"
    );
    // Direction matters: an edge is not symmetric, and a bitmap indexed by the
    // wrong operand would make it look symmetric.
    assert!(
        record_edge(e_to, e_from),
        "the reverse edge is a distinct edge and must register"
    );
    // Out-of-range indices must be refused rather than aliasing onto a real
    // row, which is what an unchecked shift by `to % 64` would silently do.
    assert!(
        !record_edge(MAX_CLASSES as u16, 0),
        "record_edge must refuse an out-of-range source"
    );
    assert!(
        !record_edge(0, MAX_CLASSES as u16),
        "record_edge must refuse an out-of-range destination"
    );
    // Having just made e_from -> e_to and e_to -> e_from, a cycle between them
    // must be visible to the BFS that now walks the bitmap.
    assert!(
        has_cycle(e_to, e_from),
        "has_cycle must find the 2-cycle just recorded"
    );
    // Undo, so the self-test leaves no synthetic dependencies behind for the
    // real kernel's graph. This is the only place the graph is ever cleared.
    for (idx, class) in [(e_from, e_to), (e_to, e_from)] {
        if let (Some(row), Some((w, mask))) = (ADJ.get(idx as usize), adj_pos(class)) {
            if let Some(cell) = row.get(w) {
                cell.fetch_and(!mask, Ordering::Relaxed);
            }
        }
    }
    EDGE_COUNT.store(edges_before, Ordering::Relaxed);
    serial_println!("[lockdep]   Edge bitmap (dedup, direction, bounds): OK");

    // Test 11: BFS completeness. Build a chain longer than the 32-node bound
    // `has_cycle` used to stop at, and require the far end to be reachable.
    // Under the old bound this returned `false` — a real deadlock whose cycle
    // ran through a 33rd lock class was never reported, and the silence was
    // indistinguishable from "no deadlock". Uses the top of the class table,
    // which the boot has not reached this early.
    const CHAIN: usize = 40;
    // The chain occupies `base ..= base + CHAIN`, stopping one slot short of the
    // top of the table so that the very top class is left unconnected and can
    // serve as the negative control below. Sizing it to run all the way up
    // instead would make the "unconnected" class the chain's own tail, and the
    // negative assertion would contradict the positive one.
    let base = MAX_CLASSES - CHAIN - 2;
    let edges_before_chain = EDGE_COUNT.load(Ordering::Relaxed);
    for i in 0..CHAIN {
        #[allow(clippy::cast_possible_truncation)] // base + CHAIN < MAX_CLASSES
        let (a, b) = ((base + i) as u16, (base + i + 1) as u16);
        assert!(record_edge(a, b), "chain edge {i} should be new");
    }
    #[allow(clippy::cast_possible_truncation)] // both < MAX_CLASSES
    let (head_cls, tail_cls) = (base as u16, (base + CHAIN) as u16);
    assert!(
        has_cycle(head_cls, tail_cls),
        "has_cycle must follow a chain longer than the old 32-node BFS bound"
    );
    // The class just past the chain's end must NOT be reachable, so the test
    // cannot pass by simply returning true for everything. `base` is chosen
    // above so this slot is genuinely off the chain.
    #[allow(clippy::cast_possible_truncation)] // < MAX_CLASSES
    let off_chain = (MAX_CLASSES - 1) as u16;
    assert!(
        off_chain != tail_cls,
        "negative control must not be the chain's own tail"
    );
    assert!(
        !has_cycle(head_cls, off_chain),
        "has_cycle must not report a path to an unconnected class"
    );
    for i in 0..CHAIN {
        #[allow(clippy::cast_possible_truncation)] // base + CHAIN < MAX_CLASSES
        let (a, b) = ((base + i) as u16, (base + i + 1) as u16);
        if let (Some(row), Some((w, mask))) = (ADJ.get(a as usize), adj_pos(b)) {
            if let Some(cell) = row.get(w) {
                cell.fetch_and(!mask, Ordering::Relaxed);
            }
        }
    }
    EDGE_COUNT.store(edges_before_chain, Ordering::Relaxed);
    serial_println!("[lockdep]   BFS completeness (chain of {CHAIN} > old bound 32): OK");

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
        // A reserved-but-unfilled slot has id 0 and is not in the hash yet, so
        // checking it would report a spurious "registered class the hash cannot
        // find" -- a self-test failing on a race in itself, not a real defect.
        if !class_is_ready(i) {
            continue;
        }
        let Some(id) = class_id(i) else {
            continue;
        };
        // Scan oracle: the first slot carrying this id. Unready slots are skipped
        // here too, so the oracle and the hash are compared over the same set.
        let mut scan = None;
        for j in 0..count {
            if class_is_ready(j) && class_id(j) == Some(id) {
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
        assert_eq!(
            hash_lookup(FRESH),
            first,
            "probe run broke the earlier entry"
        );
        serial_println!(
            "[lockdep]   class hash ({}): OK ({} classes verified vs scan, bucket collision handled)",
            when,
            checked
        );
    } else {
        // Not a silent pass: if no collider was found the collision path went
        // untested and the log must say so.
        serial_println!(
            "[lockdep]   class hash ({}): OK ({} classes verified vs scan) — WARNING no \
             colliding address found in 100k candidates, probe path UNTESTED",
            when,
            checked
        );
    }
}

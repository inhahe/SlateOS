//! High-resolution kernel timers.
//!
//! Provides nanosecond-precision timer scheduling backed by the HPET
//! monotonic counter.  Timer callbacks fire from interrupt context
//! (the APIC timer ISR) with minimal latency.
//!
//! ## Design
//!
//! Each CPU maintains a **binary min-heap** of pending timers keyed on
//! `(absolute expiry, arming id)`, over a slab of slots that handles index
//! directly — so arming and expiring are O(log n) and cancelling is O(1)
//! lookup plus an O(log n) fixup.  (It was a sorted `Vec` until 2026-08-21;
//! see [`CpuTimerState`] for what that cost and why it mattered here in
//! particular.)  The APIC timer ISR checks for expired timers on every tick.  When timers are pending with deadlines
//! between regular ticks, the APIC is reprogrammed in one-shot mode
//! to fire at the next deadline — giving sub-10ms resolution.
//!
//! ## Resolution
//!
//! - **With HPET**: timestamps at ~10-25 MHz (40-100 ns resolution)
//! - **Timer dispatch**: on each APIC tick or one-shot fire (~10 ns overhead)
//! - **Worst-case latency**: 10 ms (if scheduled just after a tick with
//!   one-shot programming unavailable).  Average: < 1 ms with one-shot.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::hrtimer;
//!
//! // Fire after 1 ms
//! let handle = hrtimer::schedule_ns(1_000_000, my_callback, 42);
//!
//! // Cancel if no longer needed
//! hrtimer::cancel(handle);
//!
//! // Query system monotonic time
//! let now = hrtimer::now_ns();
//! ```
//!
//! ## References
//!
//! - Linux: kernel/time/hrtimer.c
//! - Design spec: io_uring submission target < 200 ns, IPC < 2 µs

use crate::serial_println;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Soft threshold on the per-CPU timer queue.
///
/// Not a limit: crossing it is accepted and merely reported once.  It marks
/// the depth beyond which a healthy workload has no business being — one
/// pending timer per task blocked in a timed wait — so crossing it means some
/// caller is arming timers it never cancels, and that caller is what wants
/// finding.
const MAX_TIMERS_PER_CPU: usize = 256;

/// Hard ceiling on the per-CPU timer queue.
///
/// A backstop against unbounded growth, not an operating limit.
///
/// It used to do double duty as a latency bound, because insertion was an
/// O(n) `memmove` under the lock with interrupts disabled.  That is no longer
/// true — every operation is now O(log n) or better, so at this ceiling the
/// worst case is 12 comparisons rather than a 160 KiB move.  The ceiling is
/// kept purely as the leak detector it also always was: crossing it means a
/// caller is arming timers it never cancels.
const MAX_TIMERS_HARD_CEILING: usize = 4096;

/// Maximum CPUs supported.
const MAX_CPUS: usize = 16;

// ---------------------------------------------------------------------------
// Timer entry
// ---------------------------------------------------------------------------

/// Unique handle for a scheduled timer (used for cancellation).
///
/// Carries the CPU whose list the entry was inserted into, so [`cancel()`] can
/// go straight to the one list that can possibly hold it.  A timer entry never
/// migrates: [`schedule_absolute`] inserts into `CPU_TIMERS[current_cpu_index()]`
/// and only *that* CPU's `process_expired()` ever removes it.  (The *task* that
/// armed the timer can migrate, which is a different thing — an earlier version
/// of `cancel` conflated the two and walked every CPU's list as a result.)
///
/// `cpu == usize::MAX` marks a handle for a timer that was never inserted
/// (refused at the hard ceiling); cancelling it is a no-op.
///
/// The handle also carries the *slot* the entry occupies and the slot's
/// generation at the time of arming.  That pair is what makes [`cancel()`]
/// O(1) instead of a linear search for a matching id: the slot is a direct
/// index, and the generation distinguishes "my timer is still there" from
/// "my timer went away and something else was handed the same slot".  Without
/// the generation a stale handle would cancel an innocent stranger's timer —
/// a manufactured lost wakeup, which is the failure mode
/// `BUG-HRTIMER-EVICTS-AN-ARMED-TIMER` already cost us one boot's worth of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HrTimerHandle {
    /// Globally unique timer id.  Monotonic; also the heap's tie-breaker.
    id: u64,
    /// Index of the per-CPU list holding the entry, or `usize::MAX` if none.
    cpu: usize,
    /// Index into that CPU's slot slab.  Meaningless when `cpu == usize::MAX`.
    slot: u32,
    /// The slot's generation when this handle was minted.  A mismatch means
    /// the timer has already fired or been cancelled and the slot recycled.
    generation: u32,
}

impl HrTimerHandle {
    /// The globally unique id of the timer this handle refers to.
    ///
    /// Exposed so a blocked task can record *which* timer is supposed to wake
    /// it; the id can then be matched against the pending lists and the
    /// fired/cancelled disposition rings in a hang dump.
    #[must_use]
    pub const fn id(self) -> u64 {
        self.id
    }
}

/// Sentinel for "this slot is not in the heap" / "end of the free list".
const NIL: u32 = u32::MAX;

/// The payload half of a pending timer: everything that does *not* order it.
///
/// Lives in a slab (`CpuTimerState::slots`) that is indexed directly by the
/// handle, which is what makes cancellation O(1).  Deliberately does **not**
/// hold `expiry_ns`: the expiry lives in the heap node and only there, so a
/// re-armed repeating timer cannot end up with two disagreeing deadlines.
#[derive(Clone, Copy)]
struct TimerSlot {
    /// Callback function.
    callback: fn(u64),
    /// Argument passed to callback.
    arg: u64,
    /// Whether this timer repeats (0 = one-shot, >0 = interval in ns).
    interval_ns: u64,
    /// Globally unique id, echoed into handles, ktrace and the hang dumps.
    id: u64,
    /// Bumped every time the slot is released.  A handle whose generation no
    /// longer matches is stale and must be refused.
    generation: u32,
    /// This slot's index in `heap`, or [`NIL`] when the slot is free.
    /// Maintained by every sift; it is the heap's inverse map.
    heap_pos: u32,
    /// Next free slot when this one is free, else [`NIL`].
    next_free: u32,
}

/// The ordering half: what the binary min-heap array actually holds.
///
/// Keeping the key *in* the heap array rather than behind a slot index is
/// deliberate — a sift does O(log n) comparisons, and this way they are
/// sequential reads of one array instead of that many random probes into the
/// slab.
#[derive(Clone, Copy)]
struct HeapNode {
    /// Absolute expiry time in nanoseconds (from HPET epoch).
    expiry_ns: u64,
    /// The arming id.  Breaks ties **FIFO**: ids come from one monotonic
    /// counter, so among equal deadlines the timer armed first fires first.
    /// A plain binary heap is not stable, and without this a timer could be
    /// passed over indefinitely by a stream of arrivals sharing its deadline.
    id: u64,
    /// Which slot in the slab this node orders.
    slot: u32,
}

impl HeapNode {
    /// Total order on `(expiry_ns, id)`.  `true` when `self` must fire first.
    #[inline]
    const fn precedes(&self, other: &Self) -> bool {
        if self.expiry_ns != other.expiry_ns {
            self.expiry_ns < other.expiry_ns
        } else {
            self.id < other.id
        }
    }
}

// ---------------------------------------------------------------------------
// Per-CPU timer state
// ---------------------------------------------------------------------------

/// Per-CPU pending-timer structure: a binary min-heap over a slot slab.
///
/// **This was a sorted `Vec` until 2026-08-21.**  That gave O(1) peek but made
/// every other operation a `memmove`: insert shifted the tail, `remove(0)` on
/// each expiry shifted everything, and cancel scanned for the id and then
/// shifted again.  All three ran under the per-CPU lock *with interrupts
/// disabled*, and at [`MAX_TIMERS_HARD_CEILING`] the worst case was a 160 KiB
/// move — inside the window that `process_expired()` needs in order to run at
/// all.  The cancel path is the hottest wait path in the kernel, so the old
/// shape could delay the very APIC tick that drains the queue.
///
/// Now:
///
/// | Operation | Before | Now |
/// |---|---|---|
/// | `schedule` | O(n) scan + O(n) move | O(log n) sift |
/// | `process_expired` (per timer) | O(n) move | O(log n) sift |
/// | `cancel` | O(n) scan + O(n) move | O(1) lookup + O(log n) sift |
/// | `next_expiry_ns` | O(1) | O(1) |
///
/// Both `Vec`s still grow on demand rather than being statically sized to the
/// ceiling — 16 CPUs × 4096 slots of static storage would be megabytes of
/// always-resident memory to serve a queue that is normally shorter than 64.
struct CpuTimerState {
    /// Slot slab.  Indices are stable for a slot's whole lifetime, which is
    /// what handles point at.  Never shrinks; freed slots go on `free_head`.
    slots: Vec<TimerSlot>,
    /// Head of the intrusive free list threaded through `TimerSlot::next_free`.
    free_head: u32,
    /// Binary min-heap of live entries, ordered by [`HeapNode::precedes`].
    /// `heap.len()` is the number of pending timers.
    heap: Vec<HeapNode>,
}

impl CpuTimerState {
    const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_head: NIL,
            heap: Vec::new(),
        }
    }

    /// Number of pending timers.
    fn len(&self) -> usize {
        self.heap.len()
    }

    /// Move the node at `pos` towards the root until the heap property holds.
    /// Returns the index it came to rest at.
    fn sift_up(&mut self, mut pos: usize) -> usize {
        while pos > 0 {
            // `saturating_sub` is exact here — the loop condition is `pos > 0`
            // — and satisfies `clippy::arithmetic_side_effects` without an
            // allow, which would have to be re-justified every time this loop
            // is touched.
            let parent = pos.saturating_sub(1) / 2;
            let (Some(&here), Some(&up)) = (self.heap.get(pos), self.heap.get(parent)) else {
                break;
            };
            if !here.precedes(&up) {
                break;
            }
            self.heap_swap(pos, parent);
            pos = parent;
        }
        pos
    }

    /// Move the node at `pos` towards the leaves until the heap property holds.
    fn sift_down(&mut self, mut pos: usize) {
        let n = self.heap.len();
        loop {
            // Saturating rather than plain arithmetic, and it changes nothing:
            // `pos < n <= MAX_TIMERS_HARD_CEILING`, so these cannot overflow —
            // and if they somehow did, `usize::MAX >= n` sends both through the
            // bounds checks below and out of the loop, which is the same answer
            // a leaf would give.
            let left = pos.saturating_mul(2).saturating_add(1);
            if left >= n {
                break;
            }
            let right = left.saturating_add(1);
            let mut best = left;
            if right < n {
                let (Some(l), Some(r)) = (self.heap.get(left), self.heap.get(right)) else {
                    break;
                };
                if r.precedes(l) {
                    best = right;
                }
            }
            let (Some(&here), Some(&child)) = (self.heap.get(pos), self.heap.get(best)) else {
                break;
            };
            if !child.precedes(&here) {
                break;
            }
            self.heap_swap(pos, best);
            pos = best;
        }
    }

    /// Swap two heap positions, keeping each slot's `heap_pos` in step.
    ///
    /// Every movement of a node goes through here.  That is the invariant the
    /// whole design rests on — if a single sift forgot to update `heap_pos`,
    /// `cancel` would silently remove the wrong timer, and the symptom would
    /// be a lost wakeup somewhere else entirely.
    fn heap_swap(&mut self, a: usize, b: usize) {
        if a == b {
            return;
        }
        let (Some(&na), Some(&nb)) = (self.heap.get(a), self.heap.get(b)) else {
            return;
        };
        if let Some(dst) = self.heap.get_mut(a) {
            *dst = nb;
        }
        if let Some(dst) = self.heap.get_mut(b) {
            *dst = na;
        }
        if let Some(s) = self.slots.get_mut(nb.slot as usize) {
            s.heap_pos = a as u32;
        }
        if let Some(s) = self.slots.get_mut(na.slot as usize) {
            s.heap_pos = b as u32;
        }
    }

    /// Push a node onto the heap and record where it landed.
    fn heap_push(&mut self, node: HeapNode) {
        let slot = node.slot as usize;
        self.heap.push(node);
        let last = self.heap.len().saturating_sub(1);
        if let Some(s) = self.slots.get_mut(slot) {
            s.heap_pos = last as u32;
        }
        self.sift_up(last);
    }

    /// Remove the node at `pos`, restoring the heap property.
    ///
    /// Returns the removed node.  The last element is moved into the hole and
    /// then sifted **both ways**: it came from the deepest level, so it is
    /// usually too large for the hole (sift down) — but when the hole is on a
    /// different branch it can also be too small (sift up).  Only doing one of
    /// the two is the classic way to corrupt a heap-with-arbitrary-removal,
    /// and it survives casual testing because the common case is sift-down.
    fn heap_remove_at(&mut self, pos: usize) -> Option<HeapNode> {
        let n = self.heap.len();
        if pos >= n {
            return None;
        }
        let removed = *self.heap.get(pos)?;
        if let Some(s) = self.slots.get_mut(removed.slot as usize) {
            s.heap_pos = NIL;
        }
        // Exact: the `pos >= n` guard above means `n >= 1`.
        let last = n.saturating_sub(1);
        if pos == last {
            self.heap.pop();
            return Some(removed);
        }
        // Move the tail node into the hole, then re-place it.
        let tail = *self.heap.get(last)?;
        if let Some(dst) = self.heap.get_mut(pos) {
            *dst = tail;
        }
        self.heap.pop();
        if let Some(s) = self.slots.get_mut(tail.slot as usize) {
            s.heap_pos = pos as u32;
        }
        let rested = self.sift_up(pos);
        if rested == pos {
            self.sift_down(pos);
        }
        Some(removed)
    }

    /// Take a slot from the free list, or append a new one.
    fn alloc_slot(&mut self, slot: TimerSlot) -> Option<u32> {
        if self.free_head != NIL {
            let idx = self.free_head;
            let cell = self.slots.get_mut(idx as usize)?;
            self.free_head = cell.next_free;
            // Preserve the generation across reuse — it is the only thing
            // stopping a stale handle from matching a recycled slot.
            let generation = cell.generation;
            *cell = TimerSlot {
                generation,
                next_free: NIL,
                heap_pos: NIL,
                ..slot
            };
            return Some(idx);
        }
        let idx = u32::try_from(self.slots.len()).ok()?;
        if idx == NIL {
            return None;
        }
        self.slots.push(TimerSlot {
            generation: 0,
            next_free: NIL,
            heap_pos: NIL,
            ..slot
        });
        Some(idx)
    }

    /// Return a slot to the free list, invalidating every handle to it.
    fn free_slot(&mut self, idx: u32) {
        let head = self.free_head;
        if let Some(cell) = self.slots.get_mut(idx as usize) {
            cell.generation = cell.generation.wrapping_add(1);
            cell.heap_pos = NIL;
            cell.next_free = head;
            self.free_head = idx;
        }
    }
}

/// Global array of per-CPU timer states.
static CPU_TIMERS: [Mutex<CpuTimerState>; MAX_CPUS] = {
    // const initialization of an array of Mutexes.
    const INIT: Mutex<CpuTimerState> = Mutex::new(CpuTimerState::new());
    [INIT; MAX_CPUS]
};

/// Next timer ID (globally unique, monotonically increasing).
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Whether the hrtimer subsystem is initialized.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Total timers fired since boot (all CPUs).
static TOTAL_FIRED: AtomicU64 = AtomicU64::new(0);

/// Total timers scheduled since boot.
static TOTAL_SCHEDULED: AtomicU64 = AtomicU64::new(0);

/// Total timers cancelled since boot.
static TOTAL_CANCELLED: AtomicU64 = AtomicU64::new(0);

/// Total timer requests refused at the hard ceiling since boot.
///
/// Should be 0 on every healthy boot; a non-zero value means some caller is
/// blocking on a timeout that will never arrive.
static TOTAL_REFUSED: AtomicU64 = AtomicU64::new(0);

/// Depth of the fired/cancelled disposition rings.
///
/// A timer that is gone from the pending lists went one of exactly two ways,
/// and from the blocked task's side the two are indistinguishable — both leave
/// it parked with no wakeup source.  These rings record the last
/// `DISPOSITION_RING` ids to take each exit, so a hang dump can look up the id
/// the task recorded and say which happened, instead of inferring it.
const DISPOSITION_RING: usize = 32;

/// Ids of the most recently fired timers (a wrapping ring).
static LAST_FIRED_IDS: [AtomicU64; DISPOSITION_RING] = {
    const Z: AtomicU64 = AtomicU64::new(0);
    [Z; DISPOSITION_RING]
};

/// Write cursor for [`LAST_FIRED_IDS`]; wraps via `% DISPOSITION_RING`.
static LAST_FIRED_POS: AtomicUsize = AtomicUsize::new(0);

/// Ids of the most recently cancelled timers (a wrapping ring).
static LAST_CANCELLED_IDS: [AtomicU64; DISPOSITION_RING] = {
    const Z: AtomicU64 = AtomicU64::new(0);
    [Z; DISPOSITION_RING]
};

/// Write cursor for [`LAST_CANCELLED_IDS`]; wraps via `% DISPOSITION_RING`.
static LAST_CANCELLED_POS: AtomicUsize = AtomicUsize::new(0);

/// Append `id` to a disposition ring.
///
/// Lock-free and racy by design: two CPUs can pick the same slot and one id is
/// lost.  That is acceptable for a diagnostic whose only job is to answer
/// "which exit did this particular id take", and it keeps the cost of the
/// instrumentation to one `fetch_add` plus one store on the timer fire path.
fn ring_push(ring: &[AtomicU64; DISPOSITION_RING], pos: &AtomicUsize, id: u64) {
    let idx = pos.fetch_add(1, Ordering::Relaxed) % DISPOSITION_RING;
    if let Some(slot) = ring.get(idx) {
        slot.store(id, Ordering::Relaxed);
    }
}

/// Print a disposition ring, oldest entry first.
fn dump_ring(label: &str, ring: &[AtomicU64; DISPOSITION_RING], pos: &AtomicUsize) {
    let end = pos.load(Ordering::Relaxed);
    let start = end.saturating_sub(DISPOSITION_RING);
    let mut line: [u64; DISPOSITION_RING] = [0; DISPOSITION_RING];
    let mut n = 0usize;
    for i in start..end {
        if let (Some(src), Some(dst)) = (ring.get(i % DISPOSITION_RING), line.get_mut(n)) {
            *dst = src.load(Ordering::Relaxed);
            n = n.saturating_add(1);
        }
    }
    if let Some(slice) = line.get(..n) {
        serial_println!(
            "[hrtimer]   last {} ids (oldest first): {:?}",
            label,
            slice
        );
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the hrtimer subsystem.
///
/// Called during boot after HPET initialization.  No-op if HPET is
/// not available (timers will use TSC-based fallback timing).
pub fn init() {
    INITIALIZED.store(true, Ordering::Release);
    serial_println!("[hrtimer] High-resolution timer subsystem initialized");
    if crate::hpet::is_available() {
        serial_println!(
            "[hrtimer]   Clock source: HPET ({} MHz)",
            crate::hpet::frequency_hz() / 1_000_000
        );
    } else {
        serial_println!("[hrtimer]   Clock source: TSC (fallback)");
    }
}

/// Get current monotonic time in nanoseconds.
///
/// Uses HPET when available, falls back to TSC-based approximation.
#[inline]
pub fn now_ns() -> u64 {
    if crate::hpet::is_available() {
        crate::hpet::elapsed_ns()
    } else {
        // Fallback: use TSC with calibrated frequency.
        // bench::calibrate_tsc() sets up ns_per_tsc_tick during boot.
        tsc_ns_fallback()
    }
}

/// Schedule a one-shot timer.
///
/// The callback fires after `delay_ns` nanoseconds on the current CPU's
/// timer ISR context.  Returns a handle for cancellation.
///
/// # Arguments
///
/// - `delay_ns` — delay in nanoseconds from now (minimum ~100 ns)
/// - `callback` — function to call when the timer fires
/// - `arg` — argument passed to the callback
///
/// # Returns
///
/// A handle that can be passed to [`cancel()`] to prevent firing.
pub fn schedule_ns(delay_ns: u64, callback: fn(u64), arg: u64) -> HrTimerHandle {
    let expiry = now_ns().saturating_add(delay_ns);
    schedule_absolute(expiry, 0, callback, arg)
}

/// Schedule a repeating timer.
///
/// First fires after `delay_ns`, then repeats every `interval_ns`.
/// Use [`cancel()`] to stop.
pub fn schedule_repeating(
    delay_ns: u64,
    interval_ns: u64,
    callback: fn(u64),
    arg: u64,
) -> HrTimerHandle {
    let expiry = now_ns().saturating_add(delay_ns);
    schedule_absolute(expiry, interval_ns, callback, arg)
}

/// Cancel a pending timer.
///
/// Returns `true` if the timer was found and removed, `false` if it
/// already fired or was not found (invalid handle).
///
/// Disables interrupts while holding the per-CPU timer lock to prevent
/// deadlock with the APIC timer ISR.
///
/// Exactly one lock is taken, on the CPU recorded in the handle.  This matters
/// far more than it looks: a *miss* is the common case (`cancel` runs on the
/// success path of every wait-with-timeout, where the timer has usually already
/// fired), and the previous implementation answered a miss by locking and
/// scanning **every** live CPU's list with interrupts disabled.  Since
/// `process_expired()` only runs from the APIC timer ISR, long and frequent
/// IRQ-off windows on the hottest wait path in the kernel coalesce timer ticks
/// — i.e. the cancel path could stop the very timers it was cancelling.
pub fn cancel(handle: HrTimerHandle) -> bool {
    let Some(list) = CPU_TIMERS.get(handle.cpu) else {
        // Refused at the hard ceiling: never inserted, nothing to remove.
        return false;
    };

    let found = crate::cpu::without_interrupts(|| {
        let mut state = list.lock();
        // O(1): the handle names the slot outright.  The generation check is
        // what makes that safe — a handle for a timer that already fired
        // points at a slot which may since have been handed to someone else,
        // and cancelling *their* timer would be a lost wakeup we manufactured.
        let Some(slot) = state.slots.get(handle.slot as usize) else {
            return false;
        };
        if slot.generation != handle.generation || slot.heap_pos == NIL {
            return false;
        }
        // Defence in depth: the id is redundant with (slot, generation), so a
        // disagreement means the slab and the handle have diverged in a way
        // this module's invariants say cannot happen.  Refuse rather than
        // remove something we cannot identify.
        if slot.id != handle.id {
            return false;
        }
        let pos = slot.heap_pos as usize;
        state.heap_remove_at(pos);
        state.free_slot(handle.slot);
        TOTAL_CANCELLED.fetch_add(1, Ordering::Relaxed);
        ring_push(&LAST_CANCELLED_IDS, &LAST_CANCELLED_POS, handle.id);
        true
    });

    if found {
        crate::ktrace::record(
            crate::ktrace::Category::Timer,
            crate::ktrace::event::TIMER_CANCEL,
            handle.id,
            0,
        );
    }
    found
}

/// Query the number of pending timers on the current CPU.
pub fn pending_count() -> usize {
    crate::cpu::without_interrupts(|| {
        let cpu = crate::smp::current_cpu_index();
        CPU_TIMERS[cpu].lock().len()
    })
}

/// Query total timers fired since boot.
pub fn fired_count() -> u64 {
    TOTAL_FIRED.load(Ordering::Relaxed)
}

/// Query total timers scheduled since boot.
pub fn scheduled_count() -> u64 {
    TOTAL_SCHEDULED.load(Ordering::Relaxed)
}

/// Query how many timer requests were refused at the hard ceiling.
///
/// Non-zero means a caller is blocked on a timeout that will never fire.
pub fn refused_count() -> u64 {
    TOTAL_REFUSED.load(Ordering::Relaxed)
}

/// Query the next timer expiry time on the current CPU (or None).
pub fn next_expiry_ns() -> Option<u64> {
    crate::cpu::without_interrupts(|| {
        let cpu = crate::smp::current_cpu_index();
        let state = CPU_TIMERS[cpu].lock();
        // The heap root is the minimum by construction — still O(1).
        state.heap.first().map(|n| n.expiry_ns)
    })
}

/// Dump every pending timer on every live CPU to the serial port.
///
/// Diagnostic for the hang paths only.  A task blocked in a wait-with-timeout
/// that never returns has exactly two possible explanations, and this tells
/// them apart: its timer is **still queued** (so the ISR scan has stopped
/// draining — a firing bug) or it is **gone** (so the arm was lost or the
/// timer was cancelled out from under it — a lifetime bug). Without this the
/// two are indistinguishable from a serial log, which is what made the last
/// hang take a full boot cycle per hypothesis.
///
/// `arg` is printed because every in-tree timer callback takes a task id as
/// its argument, so it identifies the waiter.
pub fn dump_pending() {
    let live_cpus = crate::smp::cpu_count().min(MAX_CPUS);
    serial_println!(
        "[hrtimer]   totals: scheduled={} fired={} cancelled={} refused={}",
        TOTAL_SCHEDULED.load(Ordering::Relaxed),
        TOTAL_FIRED.load(Ordering::Relaxed),
        TOTAL_CANCELLED.load(Ordering::Relaxed),
        TOTAL_REFUSED.load(Ordering::Relaxed),
    );
    // Match a blocked task's recorded `sleep_timer_id` against these to see
    // which exit its timer took.  Present in neither ring, and not pending
    // below, means it was never armed at all.
    dump_ring("fired", &LAST_FIRED_IDS, &LAST_FIRED_POS);
    dump_ring("cancelled", &LAST_CANCELLED_IDS, &LAST_CANCELLED_POS);
    let now = now_ns();
    for i in 0..live_cpus {
        crate::cpu::without_interrupts(|| {
            let state = CPU_TIMERS[i].lock();
            serial_println!(
                "[hrtimer]   cpu{}: {} pending (now_ns={}); listed in HEAP order, not \
                 deadline order — only the first line is guaranteed to be the soonest",
                i,
                state.len(),
                now
            );
            // Heap order, deliberately: sorting a copy here would allocate on
            // a path that exists to diagnose hangs, and the two questions this
            // dump answers ("is my id still queued?" and "is anything
            // overdue?") are both order-independent.  Element 0 is still the
            // minimum, which is the one line that wants to be first.
            for node in state.heap.iter().take(16) {
                let (arg, interval_ns) = state
                    .slots
                    .get(node.slot as usize)
                    .map_or((0, 0), |s| (s.arg, s.interval_ns));
                serial_println!(
                    "[hrtimer]     id={} expiry_ns={} arg={} interval_ns={} ({})",
                    node.id,
                    node.expiry_ns,
                    arg,
                    interval_ns,
                    if now >= node.expiry_ns {
                        "OVERDUE - the ISR scan is not draining"
                    } else {
                        "pending"
                    },
                );
            }
        });
    }
}

// ---------------------------------------------------------------------------
// ISR integration — called from the APIC timer interrupt handler
// ---------------------------------------------------------------------------

/// Process expired timers on the current CPU.
///
/// Called from the APIC timer ISR (vector 32) on every tick, and also
/// from the hrtimer self-test during boot.  Fires callbacks for all
/// timers whose expiry time has passed.
///
/// Disables interrupts to prevent re-entrant deadlock when called from
/// non-ISR context (safe no-op when already in ISR context).
///
/// Returns the number of timers fired this tick.
pub fn process_expired() -> u32 {
    /// An expired timer captured under the lock to fire afterward:
    /// (callback, argument, interval in ns, id).  The id is carried so the
    /// fire path can record the timer's disposition for hang dumps.
    type ExpiredTimer = (fn(u64), u64, u64, u64);

    if !INITIALIZED.load(Ordering::Relaxed) {
        return 0;
    }

    let cpu = crate::smp::current_cpu_index();
    let now = now_ns();
    let mut fired = 0u32;
    // Re-arms of repeating timers.  Counted into `TOTAL_SCHEDULED` below —
    // see the comment at the re-arm site for why the tripwire depends on it.
    let mut rearmed = 0u32;

    // Collect expired timers while holding the lock, then fire them
    // after releasing it (callbacks might schedule new timers).
    let mut to_fire: [Option<ExpiredTimer>; 16] = [None; 16];
    let mut fire_count = 0usize;

    // Disable interrupts while holding the per-CPU timer lock.
    // When called from ISR context, interrupts are already disabled
    // (without_interrupts is a no-op).  When called from the self-test,
    // this prevents the APIC timer ISR from re-entering and deadlocking.
    crate::cpu::without_interrupts(|| {
        let mut state = CPU_TIMERS[cpu].lock();

        // The heap root is the earliest deadline, so one peek decides whether
        // there is any work at all — and popping it is O(log n) rather than
        // the O(n) `remove(0)` this loop used to do on *every* expiry.
        while fire_count < 16 {
            let Some(&root) = state.heap.first() else {
                break;
            };
            if root.expiry_ns > now {
                break; // Everything else is later still.
            }
            let Some(slot) = state.slots.get(root.slot as usize).copied() else {
                // Unreachable given the invariants; drop the node rather than
                // spin on a root we cannot resolve.
                state.heap_remove_at(0);
                break;
            };
            state.heap_remove_at(0);

            if let Some(cell) = to_fire.get_mut(fire_count) {
                *cell = Some((slot.callback, slot.arg, slot.interval_ns, slot.id));
            }

            if slot.interval_ns > 0 {
                // Repeating: re-arm in place.  The slot is kept — and with it
                // the id and every handle that names it — so a caller holding
                // a handle to a periodic timer can still cancel it after it
                // has fired, which is the whole point of a periodic handle.
                let next_expiry = now.saturating_add(slot.interval_ns);
                state.heap_push(HeapNode {
                    expiry_ns: next_expiry,
                    id: slot.id,
                    slot: root.slot,
                });
                // A re-arm *is* an arming, and must be counted as one, or the
                // `scheduled - fired - cancelled - pending` tripwire in the
                // self-test is structurally dead: a repeating timer bumps
                // `fired` on every tick while bumping `scheduled` only once, so
                // after a few seconds `fired` exceeds `scheduled` permanently
                // and the `saturating_sub` chain floors the difference at 0 no
                // matter how many wakeups are being destroyed.  (Boot logs
                // before this fix read `scheduled=388, fired=75833` — a 195x
                // gap, and a tripwire that could never trip.)  With re-arms
                // counted, each firing of a repeating timer is preceded by
                // exactly one arming, so the two are commensurable again.
                rearmed = rearmed.saturating_add(1);
            } else {
                // One-shot: the slot dies here.  Freeing it bumps the
                // generation, which is what makes a later `cancel()` on this
                // handle answer "already fired" instead of reaching into
                // whatever timer inherits the slot next.
                state.free_slot(root.slot);
            }

            fire_count = fire_count.saturating_add(1);
        }
    });

    // Fire callbacks outside the lock (and outside the IRQ-disabled region).
    // Callbacks might schedule new timers (which take the lock with CLI).
    for slot in to_fire.iter().take(fire_count) {
        if let Some((cb, arg, _interval, id)) = *slot {
            // Defense-in-depth: validate the stored callback points into
            // kernel `.text` before `call`-ing it.  This dispatch runs from
            // the APIC timer ISR, so a corrupted/zeroed `callback` field would
            // send the CPU straight to a wild address (or `RIP=0`) in kernel
            // context with no recovery — precisely the B-KNULLJUMP-SIGNAL
            // failure signature.  A `fn(u64)` value is non-null by type, so a
            // rejected pointer here means the timer entry was corrupted (heap
            // overrun / use-after-free of the per-CPU timer state); log it,
            // skip the call, and let the machine keep running so the event is
            // diagnosable instead of a triple-fault storm.
            let cb_addr = cb as *const () as u64;
            if !crate::idt::is_kernel_text(cb_addr) {
                serial_println!(
                    "[hrtimer] CRITICAL: refusing to dispatch corrupt timer callback \
                     addr={:#x} arg={:#x} — entry corruption; skipping (see B-KNULLJUMP-SIGNAL)",
                    cb_addr,
                    arg
                );
                continue;
            }
            // Record the disposition *before* the call: the callback can
            // re-enter the scheduler and never return here on this path.
            ring_push(&LAST_FIRED_IDS, &LAST_FIRED_POS, id);
            cb(arg);
            fired = fired.saturating_add(1);
        }
    }

    if rearmed > 0 {
        TOTAL_SCHEDULED.fetch_add(u64::from(rearmed), Ordering::Relaxed);
    }

    if fired > 0 {
        TOTAL_FIRED.fetch_add(u64::from(fired), Ordering::Relaxed);

        // Trace: timers fired (arg1 = count, arg2 = now_ns timestamp).
        crate::ktrace::record(
            crate::ktrace::Category::Timer,
            crate::ktrace::event::TIMER_FIRE,
            u64::from(fired),
            now,
        );
    }

    fired
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Schedule a timer with an absolute expiry time.
///
/// Disables interrupts while holding the per-CPU timer lock to prevent
/// deadlock with `process_expired()` which runs from the APIC timer ISR.
fn schedule_absolute(
    expiry_ns: u64,
    interval_ns: u64,
    callback: fn(u64),
    arg: u64,
) -> HrTimerHandle {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut over_soft_limit = false;
    let mut refused = false;
    // Which list the entry actually landed on.  Captured out of the closure so
    // the handle can name it; `cancel` then needs exactly one lock.  Stays
    // `usize::MAX` (= "nowhere") if the request is refused below.
    let mut sched_cpu = usize::MAX;
    // Where the entry landed in the slab, captured for the handle.
    let mut sched_slot = NIL;
    let mut sched_generation = 0u32;

    // SAFETY: Must disable interrupts before taking the per-CPU timer lock.
    // The APIC timer ISR calls process_expired() which also takes this lock.
    // Without CLI, if the ISR fires while we hold the lock on the same CPU,
    // the spin::Mutex deadlocks (non-reentrant).
    crate::cpu::without_interrupts(|| {
        let cpu = crate::smp::current_cpu_index();

        let mut state = CPU_TIMERS[cpu].lock();

        // Soft threshold: past this the queue is deeper than any healthy
        // workload needs, which means something is arming timers it never
        // cancels.  Say so, but keep accepting — refusing here would break the
        // caller, and the caller is the victim, not the culprit.
        if state.len() == MAX_TIMERS_PER_CPU {
            over_soft_limit = true;
        }

        // Hard ceiling.  Refuse rather than evict.
        //
        // This used to `pop()` the furthest-out timer to make room.  That is
        // never acceptable: the evicted timer is *armed*, someone is blocked
        // waiting for it, and its owner is given no way to find out.  It is a
        // silent lost wakeup manufactured on demand — 1541 of them in a single
        // boot here, all belonging to subsystems that had done nothing wrong.
        // Refusing the newest request instead concentrates the harm on the
        // caller that is actually asking, and the caller can at least be
        // diagnosed from the message below.  See `known-issues.md` →
        // `BUG-HRTIMER-EVICTS-AN-ARMED-TIMER`.
        if state.len() >= MAX_TIMERS_HARD_CEILING {
            refused = true;
            return;
        }

        let Some(slot_idx) = state.alloc_slot(TimerSlot {
            callback,
            arg,
            interval_ns,
            id,
            // Overwritten by `alloc_slot`, which owns these three fields.
            generation: 0,
            heap_pos: NIL,
            next_free: NIL,
        }) else {
            // The slab could not grow (allocation failure, or 2^32 slots).
            // Treat it exactly as the hard ceiling: refuse loudly rather than
            // hand back a handle for a timer that will never fire.
            refused = true;
            return;
        };
        sched_generation = state
            .slots
            .get(slot_idx as usize)
            .map_or(0, |s| s.generation);
        state.heap_push(HeapNode {
            expiry_ns,
            id,
            slot: slot_idx,
        });
        sched_cpu = cpu;
        sched_slot = slot_idx;
        TOTAL_SCHEDULED.fetch_add(1, Ordering::Relaxed);
    });

    // Diagnostics *outside* `without_interrupts` and outside the lock.  The
    // old code wrote to the serial port with interrupts disabled and the
    // per-CPU timer lock held, once per overflowing schedule — which delayed
    // the very APIC tick that drains the queue, so the flood made the
    // condition it was reporting worse.
    if over_soft_limit {
        static SOFT_WARNED: AtomicBool = AtomicBool::new(false);
        if !SOFT_WARNED.swap(true, Ordering::Relaxed) {
            serial_println!(
                "[hrtimer] WARNING: per-CPU timer queue passed {} entries — some caller is \
                 arming timers it never cancels. (one-shot warning)",
                MAX_TIMERS_PER_CPU
            );
        }
    }
    if refused {
        static REFUSED_WARNED: AtomicBool = AtomicBool::new(false);
        if !REFUSED_WARNED.swap(true, Ordering::Relaxed) {
            serial_println!(
                "[hrtimer] *** BUG: per-CPU timer queue hit the hard ceiling of {} — \
                 refusing new timers. A caller that blocks on this handle will not be woken \
                 by a timeout. (one-shot warning)",
                MAX_TIMERS_HARD_CEILING
            );
        }
        TOTAL_REFUSED.fetch_add(1, Ordering::Relaxed);
        // `cpu: usize::MAX` — the entry is on no list, so cancelling is a no-op
        // rather than a scan that can never find anything.
        return HrTimerHandle {
            id,
            cpu: usize::MAX,
            slot: NIL,
            generation: 0,
        };
    }

    // Trace outside the critical section (ktrace might allocate).
    crate::ktrace::record(
        crate::ktrace::Category::Timer,
        crate::ktrace::event::TIMER_SCHEDULE,
        id,
        expiry_ns,
    );

    HrTimerHandle {
        id,
        cpu: sched_cpu,
        slot: sched_slot,
        generation: sched_generation,
    }
}

/// TSC-based nanosecond fallback when HPET is unavailable.
fn tsc_ns_fallback() -> u64 {
    let tsc: u64;
    // SAFETY: rdtsc is always available on x86_64 and has no side effects.
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") _,
            out("edx") _,
            options(nomem, nostack, preserves_flags),
        );
        // Read full 64-bit TSC.
        let lo: u32;
        let hi: u32;
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags),
        );
        tsc = ((hi as u64) << 32) | (lo as u64);
    }

    // Convert using calibrated frequency (~3.68 GHz on QEMU).
    // bench::tsc_freq() provides the calibrated value.
    let freq = crate::bench::tsc_freq();
    if freq > 0 {
        // ns = tsc * 1_000_000_000 / freq
        // To avoid overflow: ns = tsc / (freq / 1_000_000_000)
        // But freq might be < 1 GHz. Use: (tsc * 1000) / (freq / 1_000_000)
        let mhz = freq / 1_000_000;
        if mhz > 0 {
            tsc.saturating_mul(1000) / mhz
        } else {
            0
        }
    } else {
        0 // No calibration available.
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test for high-resolution timers.
pub fn self_test() {
    use core::sync::atomic::AtomicU64;

    serial_println!("[hrtimer] Running self-test...");

    // Test 1: now_ns() returns non-zero and is monotonic.
    let t1 = now_ns();
    // Spin briefly to let time advance.
    for _ in 0..1000 {
        core::hint::spin_loop();
    }
    let t2 = now_ns();
    assert!(t2 >= t1, "now_ns() is not monotonic: {} < {}", t2, t1);
    serial_println!(
        "[hrtimer]   now_ns() monotonic: OK (delta={}ns)",
        t2.saturating_sub(t1)
    );

    // Test 2: Schedule a timer and verify it fires.
    static TEST_FIRED: AtomicU64 = AtomicU64::new(0);
    fn test_cb(arg: u64) {
        TEST_FIRED.store(arg, Ordering::Release);
    }

    TEST_FIRED.store(0, Ordering::Release);
    let before_scheduled = scheduled_count();
    let fired_before = fired_count();

    // Schedule the 0-delay timer and drain it with a single manual
    // process_expired() call, both under without_interrupts().
    //
    // This is a test-only correctness fix for an intermittent boot
    // panic: the self-test runs with interrupts ENABLED, and the
    // periodic APIC timer ISR also calls process_expired().  If an APIC
    // tick landed in the window between schedule_ns() and the manual
    // process_expired() below, the ISR would fire our 0-delay timer
    // first, so the manual call returned 0 and the `n >= 1` assertion
    // panicked ("Timer with 0 delay didn't fire on process_expired()").
    // The production code is correct — this only made the *test* racy.
    // Closing the interrupt window makes the manual drain deterministic.
    // (schedule_ns/process_expired disable interrupts internally too;
    // nesting without_interrupts is a safe no-op for the inner calls.)
    let n = crate::cpu::without_interrupts(|| {
        let _handle = schedule_ns(0, test_cb, 0xDEAD);
        // The timer has a 0 ns delay, so it expires immediately and
        // fires on this process_expired() call.
        process_expired()
    });
    assert!(
        n >= 1,
        "Timer with 0 delay didn't fire on process_expired()"
    );
    assert_eq!(
        TEST_FIRED.load(Ordering::Acquire),
        0xDEAD,
        "Timer callback didn't execute with correct arg"
    );
    assert!(fired_count() > fired_before, "fired_count didn't increment");
    assert!(
        scheduled_count() > before_scheduled,
        "scheduled_count didn't increment"
    );
    serial_println!("[hrtimer]   Immediate timer: OK (fired with arg=0xDEAD)");

    // Test 3: Cancel a pending timer.
    static CANCEL_FIRED: AtomicU64 = AtomicU64::new(0);
    fn cancel_cb(arg: u64) {
        CANCEL_FIRED.store(arg, Ordering::Release);
    }

    CANCEL_FIRED.store(0, Ordering::Release);
    // The pending list is NOT globally empty at this point in boot: a
    // persistent userspace daemon (e.g. the userspace netstack daemon
    // blocked in a timed accept-wait) keeps one or more kernel hrtimers
    // pending. So verify our own timer is added/removed *relative* to the
    // ambient baseline rather than asserting an absolute count of 1/0.
    // without_interrupts closes the window in which the periodic APIC-timer
    // ISR could reap an ambient timer between capturing `base` and the
    // asserts and skew the baseline (same race class as Test 2's fix).
    let cancelled = crate::cpu::without_interrupts(|| {
        let base = pending_count();
        let h = schedule_ns(999_999_999_999, cancel_cb, 0xBAD); // Far future.
        assert_eq!(pending_count(), base + 1, "Timer not added to pending list");
        let cancelled = cancel(h);
        assert_eq!(pending_count(), base, "Timer not removed after cancel");
        cancelled
    });
    assert!(cancelled, "cancel() returned false for valid handle");
    // Verify it doesn't fire.
    process_expired();
    assert_eq!(
        CANCEL_FIRED.load(Ordering::Acquire),
        0,
        "Cancelled timer still fired"
    );
    serial_println!("[hrtimer]   Cancel: OK");

    // Test 4: Multiple timers fire in order.
    static ORDER_LOG: AtomicU64 = AtomicU64::new(0);
    fn order_cb(arg: u64) {
        // Pack firing order into the atomic (shift left by 4 bits each time).
        ORDER_LOG.fetch_add(arg, Ordering::Relaxed);
    }

    ORDER_LOG.store(0, Ordering::Relaxed);
    // Schedule in reverse order (should still fire in deadline order).
    let _h3 = schedule_ns(0, order_cb, 300);
    let _h2 = schedule_ns(0, order_cb, 20);
    let _h1 = schedule_ns(0, order_cb, 1);

    // They all have expiry=now, but insertion order for equal times is
    // append-to-end-of-equals, so they fire in schedule order.
    process_expired();
    let result = ORDER_LOG.load(Ordering::Relaxed);
    assert_eq!(result, 321, "Timers didn't fire (got sum {})", result);
    serial_println!("[hrtimer]   Multiple timers: OK (sum=321)");

    // Test 5: Repeating timer fires and re-schedules.
    static REPEAT_COUNT: AtomicU64 = AtomicU64::new(0);
    fn repeat_cb(_arg: u64) {
        REPEAT_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    REPEAT_COUNT.store(0, Ordering::Relaxed);
    // Same ambient-baseline reasoning as Test 3. Drain any expired ambient
    // timers first so `base` is stable, then check the repeating timer's
    // re-schedule/cancel relative to it — all with interrupts off so the
    // ISR can't reap an ambient timer mid-check.
    crate::cpu::without_interrupts(|| {
        process_expired(); // Stabilise the baseline (reap ambient expiries).
        let base = pending_count();
        let rh = schedule_repeating(0, 1_000_000, repeat_cb, 0); // 1ms interval, fire immediately
        process_expired(); // First fire (re-schedules our repeating timer).
        assert_eq!(
            REPEAT_COUNT.load(Ordering::Relaxed),
            1,
            "Repeating timer didn't fire"
        );
        assert_eq!(
            pending_count(),
            base + 1,
            "Repeating timer not re-scheduled"
        );
        cancel(rh);
        assert_eq!(pending_count(), base, "Repeating timer not cancelled");
    });
    serial_println!("[hrtimer]   Repeating timer: OK (fired once, re-scheduled, cancelled)");

    // -----------------------------------------------------------------
    // Tests 6-9: the heap itself.
    //
    // Tests 1-5 above all passed against the sorted `Vec` this replaced, and
    // would pass against almost any container — none of them can observe a
    // *mis-ordering*, because the only multi-timer case (Test 4) sums its
    // arguments, and a sum is order-independent.  A binary heap fails
    // differently from a sorted list: it fails by dispatching in the wrong
    // order, and by corrupting itself when an element is removed from the
    // middle (which `cancel` does).  These four tests exist for exactly those
    // two failure modes.
    //
    // All of them run under `without_interrupts` for the same reason Tests 2-5
    // do: the APIC ISR also drains this queue, and a tick landing mid-check
    // would make the drain non-deterministic.  Ambient timers armed by
    // userspace daemons are still present in the heap and may be interleaved
    // with ours; that is harmless, because `seq_cb` only ever records its own
    // arguments and the assertions are about the relative order of ours.
    // -----------------------------------------------------------------
    static ORDER: [AtomicU64; 32] = [const { AtomicU64::new(0) }; 32];
    static ORDER_N: AtomicUsize = AtomicUsize::new(0);
    fn seq_cb(arg: u64) {
        let i = ORDER_N.fetch_add(1, Ordering::Relaxed);
        if let Some(cell) = ORDER.get(i) {
            cell.store(arg, Ordering::Relaxed);
        }
    }
    /// Drain until `want` of our timers have fired, or we run out of patience.
    /// `process_expired` fires at most 16 per call and ambient timers share
    /// that budget, so a single call is not enough to be sure.
    fn drain_for(want: usize) -> usize {
        for _ in 0..16 {
            if ORDER_N.load(Ordering::Relaxed) >= want {
                break;
            }
            process_expired();
        }
        ORDER_N.load(Ordering::Relaxed)
    }

    // Test 6: dispatch order is deadline order, whatever the arrival order.
    //
    // Twelve distinct deadlines, all already in the past so one drain takes
    // them all, armed in a deliberately adversarial permutation: it starts
    // with the largest (so the very first insert must later sift all the way
    // down), ends with the smallest, and reverses two runs in the middle.
    // Arming in ascending order would exercise no sift at all, which is how a
    // broken `sift_up` survives a naive test.
    const PERM: [u64; 12] = [11, 3, 7, 0, 9, 1, 10, 4, 8, 2, 6, 5];
    crate::cpu::without_interrupts(|| {
        process_expired(); // Clear ambient expiries so they do not eat the budget.
        ORDER_N.store(0, Ordering::Relaxed);
        let base = now_ns().saturating_sub(1_000_000);
        for &k in &PERM {
            // arg == k == rank, so the fire log should read 0,1,2,...,11.
            let _h = schedule_absolute(base.saturating_add(k), 0, seq_cb, k);
        }
        let n = drain_for(PERM.len());
        assert_eq!(
            n,
            PERM.len(),
            "heap drained {} of {} timers — some never fired",
            n,
            PERM.len()
        );
        for (i, cell) in ORDER.iter().take(PERM.len()).enumerate() {
            let got = cell.load(Ordering::Relaxed);
            assert_eq!(
                got, i as u64,
                "timers fired out of deadline order: position {} holds {}, expected {}",
                i, got, i
            );
        }
    });
    serial_println!("[hrtimer]   Heap order: OK (12 timers armed shuffled, fired sorted)");

    // Test 7: equal deadlines fire in arming order (FIFO).
    //
    // A binary heap is not a stable structure, so this is a property the
    // `(expiry, id)` key buys deliberately rather than one that comes for
    // free.  It matters: without it, a timer sharing its deadline with a
    // steady stream of new arrivals can be passed over indefinitely.
    crate::cpu::without_interrupts(|| {
        process_expired();
        ORDER_N.store(0, Ordering::Relaxed);
        let same = now_ns().saturating_sub(1_000_000);
        for k in 0..6u64 {
            let _h = schedule_absolute(same, 0, seq_cb, k);
        }
        let n = drain_for(6);
        assert_eq!(n, 6, "tie-break test drained {} of 6", n);
        for (i, cell) in ORDER.iter().take(6).enumerate() {
            let got = cell.load(Ordering::Relaxed);
            assert_eq!(
                got, i as u64,
                "equal deadlines did not fire FIFO: position {} holds {}, expected {}",
                i, got, i
            );
        }
    });
    serial_println!("[hrtimer]   Equal-deadline FIFO: OK (6 timers, one deadline)");

    // Test 8: cancelling from the middle leaves the heap intact.
    //
    // This is the test the whole rewrite hangs on.  `heap_remove_at` moves the
    // tail node into the hole and must then sift it **both** ways; doing only
    // the usual sift-down leaves a heap that still looks plausible and still
    // pops *a* minimum, just not always the right one.  Cancelling a scattered
    // subset and then demanding that the survivors come out sorted is what
    // makes that visible.  Nine armed, four cancelled from positions that are
    // neither the root nor the tail.
    crate::cpu::without_interrupts(|| {
        process_expired();
        ORDER_N.store(0, Ordering::Relaxed);
        let base = now_ns().saturating_sub(1_000_000);
        // Arm 0..9 with shuffled deadlines, keeping every handle.
        const SHUF: [u64; 9] = [4, 0, 8, 2, 6, 1, 7, 3, 5];
        let mut handles = [None; 9];
        for (slot, &k) in handles.iter_mut().zip(SHUF.iter()) {
            *slot = Some(schedule_absolute(base.saturating_add(k), 0, seq_cb, k));
        }
        // Cancel ranks 1, 3, 6 and 7 — interior deadlines, and by construction
        // they sit at assorted depths rather than all at the fringe.
        let mut cancelled = 0usize;
        for (slot, &k) in handles.iter().zip(SHUF.iter()) {
            if matches!(k, 1 | 3 | 6 | 7) {
                if let Some(h) = *slot {
                    assert!(cancel(h), "cancel() refused a live handle for rank {}", k);
                    cancelled = cancelled.saturating_add(1);
                }
            }
        }
        assert_eq!(cancelled, 4, "expected to cancel 4, cancelled {}", cancelled);
        let n = drain_for(5);
        assert_eq!(
            n, 5,
            "after cancelling 4 of 9, {} fired — expected exactly 5",
            n
        );
        // The survivors, in deadline order.
        const SURVIVORS: [u64; 5] = [0, 2, 4, 5, 8];
        for (i, cell) in ORDER.iter().take(5).enumerate() {
            let got = cell.load(Ordering::Relaxed);
            let want = SURVIVORS.get(i).copied().unwrap_or(u64::MAX);
            assert_eq!(
                got, want,
                "heap corrupted by interior cancel: position {} holds {}, expected {}",
                i, got, want
            );
        }
    });
    serial_println!("[hrtimer]   Interior cancel: OK (9 armed, 4 cancelled, 5 fired sorted)");

    // Test 9: a handle to a fired timer cannot cancel its successor.
    //
    // Slots are recycled, so the handle of a one-shot that has already fired
    // names a slot that may now belong to somebody else.  The generation
    // counter is the only thing standing between that and a silently stolen
    // wakeup — the exact shape of `BUG-HRTIMER-EVICTS-AN-ARMED-TIMER`, which
    // is why this is asserted rather than assumed.
    crate::cpu::without_interrupts(|| {
        process_expired();
        ORDER_N.store(0, Ordering::Relaxed);
        // Arm and fire a one-shot, freeing its slot.
        let stale = schedule_absolute(now_ns().saturating_sub(1000), 0, seq_cb, 77);
        let n = drain_for(1);
        assert_eq!(n, 1, "stale-handle setup: victim timer did not fire");
        // Whatever we arm next is very likely handed the slot just freed.
        let victim = schedule_absolute(now_ns().saturating_add(60_000_000_000), 0, seq_cb, 88);
        let base_pending = pending_count();
        assert!(
            !cancel(stale),
            "a handle for an already-fired timer reported a successful cancel"
        );
        assert_eq!(
            pending_count(),
            base_pending,
            "the stale handle removed something — the generation check did not hold"
        );
        assert!(cancel(victim), "the live handle could no longer cancel");
    });
    serial_println!("[hrtimer]   Stale-handle rejection: OK (recycled slot not stolen)");

    // Test 10: a repeating timer's re-arms are counted as armings.
    //
    // This is the accounting property that the tripwire in Test 11 rests on,
    // and until 2026-08-21 it did not hold: `process_expired` re-armed a
    // repeating timer without incrementing `TOTAL_SCHEDULED`, so `fired`
    // outran `scheduled` without bound (boot logs read `scheduled=388,
    // fired=75833`) and the tripwire's `saturating_sub` chain floored at 0
    // forever.  A guard that reads 0 on a healthy boot *and* on a broken one
    // is not a guard — so pin the invariant that keeps it alive, rather than
    // trusting the next reader of `process_expired` to notice.
    static ACCT_COUNT: AtomicU64 = AtomicU64::new(0);
    fn acct_cb(_arg: u64) {
        ACCT_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    const ACCT_FIRES: u64 = 5;
    ACCT_COUNT.store(0, Ordering::Relaxed);
    crate::cpu::without_interrupts(|| {
        let s0 = scheduled_count();
        // 1us interval, so five firings cost ~5us of spinning with interrupts
        // off — not the ~5ms that Test 5's 1ms interval would cost.
        let h = schedule_repeating(0, 1_000, acct_cb, 0);
        // Bounded so a broken re-arm path cannot hang the boot: the loop exits
        // as soon as the count is reached, and 100k drains is orders of
        // magnitude more headroom than ~5us of deadlines needs.
        for _ in 0..100_000u32 {
            if ACCT_COUNT.load(Ordering::Relaxed) >= ACCT_FIRES {
                break;
            }
            process_expired();
        }
        let fires = ACCT_COUNT.load(Ordering::Relaxed);
        cancel(h);
        assert!(
            fires >= ACCT_FIRES,
            "repeating timer stopped re-arming after {fires} firing(s)"
        );
        // One arming for the initial `schedule_repeating`, plus one per re-arm.
        // Ambient timers can only push this delta *up*, never down, so `>=` is
        // exact in the direction that matters: with the re-arm counter missing
        // the delta is 1, and this fails.
        let delta = scheduled_count().saturating_sub(s0);
        assert!(
            delta >= fires.saturating_add(1),
            "`scheduled` grew by {delta} across {fires} firings of a repeating timer — \
             re-arms are not being counted as armings, which silently disables the \
             accounting tripwire below"
        );
    });
    serial_println!(
        "[hrtimer]   Re-arm accounting: OK ({}+ re-arms counted as armings)",
        ACCT_FIRES
    );

    // Test 11: Statistics.
    let sched = scheduled_count();
    let cancelled_n = TOTAL_CANCELLED.load(Ordering::Relaxed);
    let fired_n = fired_count();
    let refused_n = refused_count();
    let pending_n = pending_count();
    // `scheduled - fired - cancelled - pending` is the count of timers that
    // were armed and then neither fired, were cancelled, nor are still
    // waiting.  Under the old eviction policy that number was the tally of
    // silently destroyed wakeups; it must now be 0.
    //
    // This only means anything because `scheduled` counts a repeating timer's
    // re-arms as well as its first arming (see `process_expired`).  It did not
    // until 2026-08-21, and until then the subtraction was dead: `fired` grew
    // once per tick per repeating timer while `scheduled` stood still, so the
    // `saturating_sub` floored the result at 0 on every boot regardless of how
    // many wakeups were being lost.  If you ever "simplify" the re-arm counter
    // away, delete this check too rather than leave a tripwire that cannot fire.
    let unaccounted = sched
        .saturating_sub(fired_n)
        .saturating_sub(cancelled_n)
        .saturating_sub(pending_n as u64);
    serial_println!(
        "[hrtimer]   Stats: scheduled={}, fired={}, cancelled={}, pending={}, refused={}",
        sched,
        fired_n,
        cancelled_n,
        pending_n,
        refused_n
    );
    if refused_n > 0 {
        serial_println!(
            "[hrtimer]   *** {} timer request(s) refused at the hard ceiling — a caller \
             is waiting on a timeout that will never arrive",
            refused_n
        );
    }
    // Not a hard failure: `pending_count()` only sees the current CPU, so on a
    // multi-CPU boot the arithmetic legitimately under-counts.  It is still
    // the cheapest tripwire for a return of the eviction bug.
    if unaccounted > (crate::smp::cpu_count() as u64).saturating_mul(64) {
        serial_println!(
            "[hrtimer]   *** {} timer(s) unaccounted for (scheduled but never fired, \
             cancelled, or pending) — see BUG-HRTIMER-EVICTS-AN-ARMED-TIMER",
            unaccounted
        );
    }

    serial_println!("[hrtimer] Self-test PASSED");
}

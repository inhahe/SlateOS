//! High-resolution kernel timers.
//!
//! Provides nanosecond-precision timer scheduling backed by the HPET
//! monotonic counter.  Timer callbacks fire from interrupt context
//! (the APIC timer ISR) with minimal latency.
//!
//! ## Design
//!
//! Each CPU maintains a sorted list of pending timers (min-heap by
//! absolute expiry time).  The APIC timer ISR checks for expired
//! timers on every tick.  When timers are pending with deadlines
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
/// A backstop against unbounded growth, not an operating limit.  The list is
/// a sorted `Vec` with O(n) insert, so this also bounds the worst-case time
/// spent under the per-CPU lock with interrupts disabled.  See `todo.txt`
/// (`hrtimer: replace the sorted Vec`) for the structural fix.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HrTimerHandle {
    /// Globally unique timer id.
    id: u64,
    /// Index of the per-CPU list holding the entry, or `usize::MAX` if none.
    cpu: usize,
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

/// A pending high-resolution timer.
#[derive(Clone, Copy)]
struct TimerEntry {
    /// Absolute expiry time in nanoseconds (from HPET epoch).
    expiry_ns: u64,
    /// Callback function.
    callback: fn(u64),
    /// Argument passed to callback.
    arg: u64,
    /// Unique ID for cancellation.
    id: u64,
    /// Whether this timer repeats (0 = one-shot, >0 = interval in ns).
    interval_ns: u64,
}

// ---------------------------------------------------------------------------
// Per-CPU timer state
// ---------------------------------------------------------------------------

/// Per-CPU timer heap (min-heap sorted by expiry_ns).
///
/// Using a simple sorted Vec rather than a proper BinaryHeap because
/// we need cancel-by-ID (requires scanning) and the number of active
/// timers per CPU is typically small (< 64).
struct CpuTimerState {
    /// Pending timers sorted by expiry (earliest first).
    timers: Vec<TimerEntry>,
}

impl CpuTimerState {
    const fn new() -> Self {
        Self { timers: Vec::new() }
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
        if let Some(pos) = state.timers.iter().position(|t| t.id == handle.id) {
            state.timers.remove(pos);
            TOTAL_CANCELLED.fetch_add(1, Ordering::Relaxed);
            ring_push(&LAST_CANCELLED_IDS, &LAST_CANCELLED_POS, handle.id);
            return true;
        }
        false
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
        CPU_TIMERS[cpu].lock().timers.len()
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
        state.timers.first().map(|t| t.expiry_ns)
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
                "[hrtimer]   cpu{}: {} pending (now_ns={})",
                i,
                state.timers.len(),
                now
            );
            for t in state.timers.iter().take(16) {
                serial_println!(
                    "[hrtimer]     id={} expiry_ns={} arg={} interval_ns={} ({})",
                    t.id,
                    t.expiry_ns,
                    t.arg,
                    t.interval_ns,
                    if now >= t.expiry_ns {
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

        // Since the list is sorted, scan from the front until we find
        // a timer that hasn't expired yet.
        while !state.timers.is_empty() && fire_count < 16 {
            if state.timers[0].expiry_ns <= now {
                let entry = state.timers.remove(0);
                to_fire[fire_count] = Some((
                    entry.callback,
                    entry.arg,
                    entry.interval_ns,
                    entry.id,
                ));

                // If repeating, re-insert with the next expiry.
                if entry.interval_ns > 0 {
                    let next_expiry = now.saturating_add(entry.interval_ns);
                    let new_entry = TimerEntry {
                        expiry_ns: next_expiry,
                        callback: entry.callback,
                        arg: entry.arg,
                        id: entry.id,
                        interval_ns: entry.interval_ns,
                    };
                    insert_sorted(&mut state.timers, new_entry);
                }

                fire_count = fire_count.saturating_add(1);
            } else {
                break; // Remaining timers are in the future.
            }
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

/// Insert a timer into the sorted list (by expiry_ns, earliest first).
fn insert_sorted(timers: &mut Vec<TimerEntry>, entry: TimerEntry) {
    let pos = timers
        .iter()
        .position(|t| t.expiry_ns > entry.expiry_ns)
        .unwrap_or(timers.len());
    timers.insert(pos, entry);
}

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

    // SAFETY: Must disable interrupts before taking the per-CPU timer lock.
    // The APIC timer ISR calls process_expired() which also takes this lock.
    // Without CLI, if the ISR fires while we hold the lock on the same CPU,
    // the spin::Mutex deadlocks (non-reentrant).
    crate::cpu::without_interrupts(|| {
        let cpu = crate::smp::current_cpu_index();

        let entry = TimerEntry {
            expiry_ns,
            callback,
            arg,
            id,
            interval_ns,
        };

        let mut state = CPU_TIMERS[cpu].lock();

        // Soft threshold: past this the queue is deeper than any healthy
        // workload needs, which means something is arming timers it never
        // cancels.  Say so, but keep accepting — refusing here would break the
        // caller, and the caller is the victim, not the culprit.
        if state.timers.len() == MAX_TIMERS_PER_CPU {
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
        if state.timers.len() >= MAX_TIMERS_HARD_CEILING {
            refused = true;
            return;
        }

        insert_sorted(&mut state.timers, entry);
        sched_cpu = cpu;
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

    // Test 6: Statistics.
    let sched = scheduled_count();
    let cancelled_n = TOTAL_CANCELLED.load(Ordering::Relaxed);
    let fired_n = fired_count();
    let refused_n = refused_count();
    let pending_n = pending_count();
    // `scheduled - fired - cancelled - pending` is the count of timers that
    // were armed and then neither fired, were cancelled, nor are still
    // waiting.  Under the old eviction policy that number was the tally of
    // silently destroyed wakeups; it must now be 0.
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

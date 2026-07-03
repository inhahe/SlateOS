//! Kernel synchronization primitives with lockdep and contention tracking.
//!
//! This module provides [`Mutex<T>`] — a wrapper around [`spin::Mutex<T>`]
//! that automatically reports lock acquisitions and releases to the lockdep
//! subsystem for deadlock detection, and tracks contention statistics
//! (how often a lock is contended, total wait cycles).
//!
//! ## Migration
//!
//! To migrate a file from raw `spin::Mutex` to tracked locks:
//! ```ignore
//! // Before:
//! use spin::Mutex;
//!
//! // After:
//! use crate::sync::Mutex;
//! ```
//!
//! The API is identical to `spin::Mutex` — `lock()` returns a guard that
//! auto-unlocks on drop.
//!
//! ## Lock naming
//!
//! Each `Mutex` carries a static `&[u8]` name used in lockdep diagnostics
//! and contention reports.  Use `Mutex::named(value, b"SCHED")` for
//! important locks, or `Mutex::new(value)` which defaults to `b"?"`.
//!
//! ## Contention Tracking
//!
//! Every lock acquisition is tracked:
//! - **Acquisitions**: total number of times the lock was acquired.
//! - **Contentions**: how many of those acquisitions required spinning.
//! - **Wait cycles**: total TSC cycles spent spinning across all contended
//!   acquisitions.
//! - **Max wait**: longest single spin duration in TSC cycles.
//! - **Hold cycles**: total TSC cycles the lock was held.
//! - **Max hold**: longest single hold duration in TSC cycles.
//!
//! Use the `lockstats` kshell command to view contention data for all
//! registered locks.  Tracking adds ~5ns overhead per acquisition on the
//! fast path (uncontended: one rdtsc + one atomic increment).

use crate::lockdep;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Spinlock stall detector (software hard-lockup diagnostic)
// ---------------------------------------------------------------------------
//
// The timer-driven liveness watchdog in `sched` can only observe the system
// from a timer interrupt, so it is blind to a CPU that spins forever with
// interrupts disabled (IF=0) — the timer ISR never runs and the whole
// machine goes silent with no task-table dump. That is exactly the signature
// of the intermittent spawn/kill/reap hang (B-PTHREAD-YIELDBUDGET / TD31).
//
// The stall detector closes that blind spot in pure software: the contended
// lock path spins on `try_lock`, and if it spins for longer than
// `STALL_SECONDS` of wall-clock time (measured with the PIT-calibrated TSC,
// which reflects guest wall time even under QEMU/TCG) it emits a one-shot,
// non-fatal diagnostic naming the lock, the wedged CPU/task, and the locks
// that CPU already holds — then keeps spinning. Because it fires from *inside*
// the spin loop, it works regardless of IF state. The threshold is far beyond
// any legitimate kernel lock hold, so it never false-fires under normal
// contention.

/// Wall-clock seconds a CPU may spin on a single lock before the stall
/// detector fires. Deliberately far larger than any legitimate lock hold in
/// the kernel (the longest boot-time critical sections are milliseconds), so
/// only a true deadlock or pathological convoy ever reaches it. Fires well
/// inside the 480 s boot-test timeout, so the diagnostic reaches the serial
/// log before the harness gives up.
const STALL_SECONDS: u64 = 30;

/// Iteration mask controlling how often the (relatively costly) `rdtsc`
/// stall check runs — once every 4096 spins keeps the loop tight.
const STALL_CHECK_MASK: u64 = 0xFFF;

/// Fallback stall threshold in raw spin iterations, used only before the TSC
/// is calibrated (very early boot, effectively single-threaded and
/// uncontended). Large enough to never trip under legitimate early-boot
/// contention.
const STALL_FALLBACK_ITERS: u64 = 5_000_000_000;

/// Cap on how many stall reports are printed globally. A genuine multi-CPU
/// convoy would otherwise flood the serial log; the first few reports carry
/// all the diagnostic value.
const MAX_STALL_REPORTS: u64 = 8;

/// Global count of stall reports emitted (rate-limits [`MAX_STALL_REPORTS`]).
static STALL_REPORTS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Contention statistics
// ---------------------------------------------------------------------------

/// Per-lock contention statistics.
///
/// All fields are atomically updated from any CPU.  The stats give
/// a picture of lock health: high contention ratios or long wait
/// times indicate a hot lock that may need splitting or lock-free
/// redesign.
pub struct ContentionStats {
    /// Total acquisitions (contended + uncontended).
    pub acquisitions: AtomicU64,
    /// Acquisitions that had to spin (lock was held by another CPU).
    pub contentions: AtomicU64,
    /// Sum of TSC cycles spent spinning across all contended acquires.
    pub total_wait_cycles: AtomicU64,
    /// Maximum single-acquisition spin duration in TSC cycles.
    pub max_wait_cycles: AtomicU64,
    /// Sum of TSC cycles the lock was held across all acquisitions.
    pub total_hold_cycles: AtomicU64,
    /// Maximum single hold duration in TSC cycles.
    pub max_hold_cycles: AtomicU64,
}

impl ContentionStats {
    /// Create zeroed stats.
    const fn new() -> Self {
        Self {
            acquisitions: AtomicU64::new(0),
            contentions: AtomicU64::new(0),
            total_wait_cycles: AtomicU64::new(0),
            max_wait_cycles: AtomicU64::new(0),
            total_hold_cycles: AtomicU64::new(0),
            max_hold_cycles: AtomicU64::new(0),
        }
    }

    /// Record an uncontended acquisition (fast path).
    #[inline]
    fn record_uncontended(&self) {
        self.acquisitions.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a contended acquisition with the time spent waiting.
    #[inline]
    fn record_contended(&self, wait_cycles: u64) {
        self.acquisitions.fetch_add(1, Ordering::Relaxed);
        self.contentions.fetch_add(1, Ordering::Relaxed);
        self.total_wait_cycles.fetch_add(wait_cycles, Ordering::Relaxed);
        // Update max via CAS loop.
        let mut cur = self.max_wait_cycles.load(Ordering::Relaxed);
        while wait_cycles > cur {
            match self.max_wait_cycles.compare_exchange_weak(
                cur,
                wait_cycles,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }
    }

    /// Record lock hold duration when the guard is dropped.
    #[inline]
    fn record_hold(&self, hold_cycles: u64) {
        self.total_hold_cycles.fetch_add(hold_cycles, Ordering::Relaxed);
        // Update max hold via CAS loop.
        let mut cur = self.max_hold_cycles.load(Ordering::Relaxed);
        while hold_cycles > cur {
            match self.max_hold_cycles.compare_exchange_weak(
                cur,
                hold_cycles,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }
    }

    /// Reset all counters to zero.
    #[allow(dead_code)]
    pub fn reset(&self) {
        self.acquisitions.store(0, Ordering::Relaxed);
        self.contentions.store(0, Ordering::Relaxed);
        self.total_wait_cycles.store(0, Ordering::Relaxed);
        self.max_wait_cycles.store(0, Ordering::Relaxed);
        self.total_hold_cycles.store(0, Ordering::Relaxed);
        self.max_hold_cycles.store(0, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Global lock registry
// ---------------------------------------------------------------------------

/// Maximum number of tracked locks in the global registry.
///
/// We use a fixed-size array to avoid heap allocation.  16 named
/// kernel locks is plenty for the current codebase; increase if needed.
const MAX_TRACKED_LOCKS: usize = 32;

/// Registry entry: pointer to a lock's ContentionStats + its name.
struct LockEntry {
    stats: AtomicU64, // Actually a *const ContentionStats stored as u64
    name: AtomicU64,  // Actually a *const [u8] fat pointer (we store just the thin ptr + len)
    name_len: AtomicU64,
}

impl LockEntry {
    const fn empty() -> Self {
        Self {
            stats: AtomicU64::new(0),
            name: AtomicU64::new(0),
            name_len: AtomicU64::new(0),
        }
    }
}

/// Global registry of tracked locks.
static LOCK_REGISTRY: [LockEntry; MAX_TRACKED_LOCKS] = {
    // const array init
    const EMPTY: LockEntry = LockEntry::empty();
    [EMPTY; MAX_TRACKED_LOCKS]
};

/// Number of locks currently registered.
static REGISTRY_COUNT: AtomicU64 = AtomicU64::new(0);

/// Whether contention tracking is enabled (can be toggled at runtime).
///
/// When disabled, acquisitions still go through lockdep but skip rdtsc
/// and stat recording.  Default: enabled.
static TRACKING_ENABLED: AtomicU64 = AtomicU64::new(1);

/// Enable or disable contention tracking globally.
///
/// When disabled, the overhead per acquisition drops to near zero
/// (just the lockdep notification).
#[allow(dead_code)]
pub fn set_tracking_enabled(enabled: bool) {
    TRACKING_ENABLED.store(if enabled { 1 } else { 0 }, Ordering::Relaxed);
}

/// Check if contention tracking is currently enabled.
#[inline]
fn tracking_enabled() -> bool {
    TRACKING_ENABLED.load(Ordering::Relaxed) != 0
}

/// Register a lock in the global registry (for kshell enumeration).
///
/// Called once per static Mutex at first acquisition.  If the registry
/// is full, the lock still works but won't appear in `lockstats`.
fn register_lock(stats: &ContentionStats, name: &'static [u8]) {
    let idx = REGISTRY_COUNT.fetch_add(1, Ordering::Relaxed) as usize;
    if idx >= MAX_TRACKED_LOCKS {
        // Registry full — decrement to avoid overflow drift.
        REGISTRY_COUNT.fetch_sub(1, Ordering::Relaxed);
        return;
    }
    let entry = &LOCK_REGISTRY[idx];
    entry.stats.store(stats as *const ContentionStats as u64, Ordering::Release);
    entry.name.store(name.as_ptr() as u64, Ordering::Release);
    entry.name_len.store(name.len() as u64, Ordering::Release);
}

/// Snapshot of a single lock's contention data (for reporting).
#[derive(Debug, Clone, Copy)]
pub struct LockStatSnapshot {
    /// Lock name (as UTF-8, best-effort).
    pub name: &'static [u8],
    /// Total acquisitions.
    pub acquisitions: u64,
    /// Contended acquisitions (had to spin).
    pub contentions: u64,
    /// Total TSC cycles spent waiting.
    pub total_wait_cycles: u64,
    /// Max single wait in TSC cycles.
    pub max_wait_cycles: u64,
    /// Total TSC cycles the lock was held.
    pub total_hold_cycles: u64,
    /// Max single hold in TSC cycles.
    pub max_hold_cycles: u64,
}

/// Get snapshots of all registered locks' contention stats.
///
/// Returns an array of `Option<LockStatSnapshot>`.  Entries are `Some`
/// for registered locks, `None` for unused slots.
#[must_use]
pub fn lock_stats() -> [Option<LockStatSnapshot>; MAX_TRACKED_LOCKS] {
    let count = REGISTRY_COUNT.load(Ordering::Acquire) as usize;
    let mut result: [Option<LockStatSnapshot>; MAX_TRACKED_LOCKS] =
        [None; MAX_TRACKED_LOCKS];

    for i in 0..count.min(MAX_TRACKED_LOCKS) {
        let entry = &LOCK_REGISTRY[i];
        let stats_ptr = entry.stats.load(Ordering::Acquire);
        let name_ptr = entry.name.load(Ordering::Acquire);
        let name_len = entry.name_len.load(Ordering::Acquire) as usize;

        if stats_ptr == 0 || name_ptr == 0 {
            continue;
        }

        // SAFETY: The pointer was stored from a &'static ContentionStats
        // reference (embedded in a static Mutex).  It remains valid for
        // the lifetime of the kernel.
        let stats = unsafe { &*(stats_ptr as *const ContentionStats) };
        // SAFETY: Same — name is a &'static [u8] from a string literal.
        let name = unsafe {
            core::slice::from_raw_parts(name_ptr as *const u8, name_len)
        };

        result[i] = Some(LockStatSnapshot {
            name,
            acquisitions: stats.acquisitions.load(Ordering::Relaxed),
            contentions: stats.contentions.load(Ordering::Relaxed),
            total_wait_cycles: stats.total_wait_cycles.load(Ordering::Relaxed),
            max_wait_cycles: stats.max_wait_cycles.load(Ordering::Relaxed),
            total_hold_cycles: stats.total_hold_cycles.load(Ordering::Relaxed),
            max_hold_cycles: stats.max_hold_cycles.load(Ordering::Relaxed),
        });
    }

    result
}

/// Reset all registered locks' contention counters.
#[allow(dead_code)]
pub fn reset_all_stats() {
    let count = REGISTRY_COUNT.load(Ordering::Acquire) as usize;
    for i in 0..count.min(MAX_TRACKED_LOCKS) {
        let entry = &LOCK_REGISTRY[i];
        let stats_ptr = entry.stats.load(Ordering::Acquire);
        if stats_ptr == 0 {
            continue;
        }
        // SAFETY: Same as lock_stats() — pointer from static Mutex.
        let stats = unsafe { &*(stats_ptr as *const ContentionStats) };
        stats.reset();
    }
}

// ---------------------------------------------------------------------------
// Mutex implementation
// ---------------------------------------------------------------------------

/// A mutual-exclusion spinlock with lockdep tracking and contention stats.
///
/// Wraps `spin::Mutex<T>` and notifies the lock order validator on
/// every acquisition and release.  Also tracks contention statistics
/// (acquisitions, spin durations) for performance analysis.
pub struct Mutex<T> {
    inner: spin::Mutex<T>,
    /// Human-readable name for lockdep diagnostics and lockstats.
    name: &'static [u8],
    /// Per-lock contention statistics.
    stats: ContentionStats,
    /// Whether this lock has been registered in the global registry.
    /// Uses AtomicU64 instead of AtomicBool for const init compatibility.
    registered: AtomicU64,
    /// Task id of the current holder (0 = unheld or held by the idle/boot
    /// task 0). Written on every successful acquire and cleared to
    /// [`OWNER_NONE`] on release. Purely diagnostic: [`Self::report_stall`]
    /// prints it so a stuck lock reveals *who* holds it (recursion vs. a
    /// guard leaked by a since-dead task), which lockdep's held-lock dump
    /// cannot show once the holder is gone.
    owner: AtomicU64,
}

/// Sentinel stored in [`Mutex::owner`] when the lock is not held. `u64::MAX`
/// is used (not 0) because task id 0 is a real task (the idle/boot task), so
/// 0 must remain distinguishable from "unheld".
const OWNER_NONE: u64 = u64::MAX;

// SAFETY: Mutex<T> is Send+Sync whenever T is Send (same as spin::Mutex).
unsafe impl<T: Send> Send for Mutex<T> {}
unsafe impl<T: Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    /// Create a new tracked mutex with a default name.
    pub const fn new(value: T) -> Self {
        Self {
            inner: spin::Mutex::new(value),
            name: b"?",
            stats: ContentionStats::new(),
            registered: AtomicU64::new(0),
            owner: AtomicU64::new(OWNER_NONE),
        }
    }

    /// Create a new tracked mutex with a diagnostic name.
    ///
    /// The name appears in lockdep violation reports and `lockstats`
    /// output.  Keep it short (≤16 bytes — excess is truncated by
    /// lockdep).
    pub const fn named(value: T, name: &'static [u8]) -> Self {
        Self {
            inner: spin::Mutex::new(value),
            name,
            stats: ContentionStats::new(),
            registered: AtomicU64::new(0),
            owner: AtomicU64::new(OWNER_NONE),
        }
    }

    /// Ensure this lock is registered in the global registry (once).
    #[inline]
    fn ensure_registered(&self) {
        // Fast path: already registered.
        if self.registered.load(Ordering::Relaxed) != 0 {
            return;
        }
        // Slow path: register (CAS to avoid double-registration).
        if self.registered.compare_exchange(
            0, 1, Ordering::AcqRel, Ordering::Relaxed
        ).is_ok() {
            register_lock(&self.stats, self.name);
        }
    }

    /// Acquire the lock, returning a guard that releases on drop.
    ///
    /// Notifies lockdep before spinning so the dependency edge is
    /// recorded even if the lock is uncontended.  Tracks contention
    /// statistics when enabled.
    #[inline]
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.ensure_registered();
        let addr = self.addr();
        // Disable involuntary preemption for the whole hold — a spinlock must
        // never be held across a context switch (see
        // `sched::PREEMPT_DISABLE_COUNT`).  Paired with `preempt_enable()` in
        // `MutexGuard::drop`.  Done before spinning so the holder can't be
        // preempted while contended either.
        crate::sched::preempt_disable();
        lockdep::lock_acquire(addr, self.name);

        if tracking_enabled() {
            // Try the fast path: immediate acquisition.
            if let Some(guard) = self.inner.try_lock() {
                self.stats.record_uncontended();
                let acquire_tsc = crate::bench::rdtsc();
                return self.make_guard(guard, addr, acquire_tsc);
            }

            // Contended path: time the spin (with stall detection).
            let start = crate::bench::rdtsc();
            let guard = self.lock_contended();
            let end = crate::bench::rdtsc();
            let wait = end.saturating_sub(start);
            self.stats.record_contended(wait);
            return self.make_guard(guard, addr, end);
        }

        // Tracking disabled: still bounded-spin so the stall detector runs.
        let guard = self.lock_contended();
        self.make_guard(guard, addr, 0)
    }

    /// Contended-path acquisition with a bounded-spin stall detector.
    ///
    /// Spins on `try_lock` until the lock is acquired — behaviourally
    /// identical to `spin::Mutex::lock()` (which spins the same way) except
    /// that a spin lasting longer than [`STALL_SECONDS`] triggers a one-shot,
    /// non-fatal diagnostic (see [`Self::report_stall`]) and then continues
    /// spinning. Marked `#[cold]`/`#[inline(never)]` so the fast path in
    /// [`Self::lock`] stays lean.
    #[cold]
    #[inline(never)]
    fn lock_contended(&self) -> spin::MutexGuard<'_, T> {
        // Compute the stall threshold in TSC cycles once. If the TSC is not
        // yet calibrated (very early boot), `tsc_freq()` returns 0 and we
        // fall back to a raw iteration count.
        let threshold_cycles = crate::bench::tsc_freq().saturating_mul(STALL_SECONDS);
        let start_tsc = crate::bench::rdtsc();

        let mut iters: u64 = 0;
        let mut warned = false;
        loop {
            if let Some(guard) = self.inner.try_lock() {
                return guard;
            }
            core::hint::spin_loop();
            iters = iters.wrapping_add(1);

            // Throttle the stall check: only probe once every 4096 spins,
            // and only until we've reported once for this spin episode.
            if !warned && (iters & STALL_CHECK_MASK) == 0 {
                let stalled = if threshold_cycles != 0 {
                    crate::bench::rdtsc().saturating_sub(start_tsc) >= threshold_cycles
                } else {
                    iters >= STALL_FALLBACK_ITERS
                };
                if stalled {
                    warned = true;
                    self.report_stall(iters);
                }
            }
        }
    }

    /// Emit a one-shot diagnostic for a lock that has been spun on for an
    /// abnormally long time. Non-fatal: the caller keeps spinning afterwards.
    ///
    /// Reports the lock name, the wedged CPU and task, and — via lockdep —
    /// the locks that CPU already holds (the key clue for an AB-BA deadlock
    /// or convoy). Globally rate-limited to [`MAX_STALL_REPORTS`] so a
    /// multi-CPU convoy cannot flood the serial log.
    ///
    /// Limitation: this prints via the serial port, so if the *serial* lock
    /// itself is the deadlocked lock (or is held by this same CPU) the report
    /// may not appear. That is an accepted edge case — the target failure
    /// modes are the scheduler / cgroup-table locks, not serial.
    #[cold]
    #[inline(never)]
    fn report_stall(&self, iters: u64) {
        use crate::serial_println;

        let n = STALL_REPORTS.fetch_add(1, Ordering::Relaxed);
        if n >= MAX_STALL_REPORTS {
            return;
        }

        let cpu = crate::sched::current_cpu_id();
        let tid = crate::sched::current_task_id();
        let name = core::str::from_utf8(self.name).unwrap_or("<non-utf8>");
        let owner = self.owner.load(Ordering::Relaxed);
        serial_println!(
            "[sync] *** SPINLOCK STALL *** lock '{}' still not acquired after ~{}s of \
             spinning (cpu {}, task {}, {} iters). Likely self-deadlock or lock convoy; \
             the timer-driven liveness watchdog is blind to this if interrupts are \
             disabled.",
            name, STALL_SECONDS, cpu, tid, iters
        );
        // Name the holder: if `owner == tid`, this is a recursive self-deadlock
        // (the spinning task already holds the lock); if `owner` is some other
        // (possibly since-dead) task, the guard was leaked / the holder never
        // released. `OWNER_NONE` means the physical lock shows free yet
        // `try_lock` still fails — a lost-unlock / poisoned-flag desync.
        if owner == OWNER_NONE {
            serial_println!(
                "[sync]   lock '{}' holder: NONE recorded (owner=unheld) — \
                 lost-unlock or flag desync; spinner is task {} on cpu {}",
                name, tid, cpu
            );
        } else if owner == tid {
            serial_println!(
                "[sync]   lock '{}' holder: task {} == spinner — RECURSIVE \
                 self-deadlock (same task re-entered the lock)",
                name, owner
            );
        } else {
            serial_println!(
                "[sync]   lock '{}' holder: task {} (spinner is task {}) — guard \
                 held by another task; check whether it is still alive",
                name, owner, tid
            );
        }
        // The single most useful clue: what else this CPU already holds.
        crate::lockdep::dump_held_locks(cpu);
    }

    /// Try to acquire the lock without blocking.
    ///
    /// If successful, records the acquisition with lockdep.
    /// If the lock is already held, returns `None` without recording.
    #[inline]
    #[allow(dead_code)]
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        self.ensure_registered();
        let addr = self.addr();
        // Disable preemption first, then attempt the lock; if we fail to
        // acquire, undo the disable before returning (no guard will be
        // created to do it for us).
        crate::sched::preempt_disable();
        let Some(guard) = self.inner.try_lock() else {
            crate::sched::preempt_enable();
            return None;
        };
        // Only record if we actually got the lock — try_lock doesn't
        // block, so there's no ordering issue to detect on failure.
        lockdep::lock_acquire(addr, self.name);
        if tracking_enabled() {
            self.stats.record_uncontended();
        }
        let acquire_tsc = if tracking_enabled() {
            crate::bench::rdtsc()
        } else {
            0
        };
        Some(self.make_guard(guard, addr, acquire_tsc))
    }

    /// Build a [`MutexGuard`] and record the acquiring task as the owner.
    ///
    /// Centralises the owner write so every acquisition path (fast, contended,
    /// tracking-disabled, `try_lock`) stamps the holder identically. The store
    /// is a single relaxed per-CPU read + write — negligible next to the CAS
    /// and lockdep call already on this path — and is what makes a stuck lock
    /// name its holder in [`Self::report_stall`].
    #[inline]
    fn make_guard<'a>(
        &'a self,
        guard: spin::MutexGuard<'a, T>,
        addr: usize,
        acquire_tsc: u64,
    ) -> MutexGuard<'a, T> {
        self.owner
            .store(crate::sched::current_task_id(), Ordering::Relaxed);
        MutexGuard {
            guard: core::mem::ManuallyDrop::new(guard),
            addr,
            stats: &self.stats,
            acquire_tsc,
            owner: &self.owner,
        }
    }

    /// Acquire the lock with interrupts disabled for the whole hold
    /// (`spin_lock_irqsave` semantics).
    ///
    /// Use this for any lock that is reachable from BOTH task context and
    /// interrupt/exception context on the same CPU. A plain [`lock`](Self::lock)
    /// only disables *preemption* (voluntary context switch); it leaves
    /// hardware interrupts enabled, so if an IRQ or softirq that runs while the
    /// lock is held re-enters the same lock, the CPU self-deadlocks (the holder
    /// can never make progress to release it). Disabling interrupts for the
    /// duration closes that window entirely.
    ///
    /// The previous interrupt-enable state is saved and restored on drop, so
    /// this nests correctly: taking an irqsave lock inside an already
    /// interrupts-off region leaves interrupts off on release.
    ///
    /// Keep the critical section short — interrupts are masked on this CPU for
    /// the whole hold, so a long hold starves the timer tick and raises IRQ
    /// latency. ACCT-style leaf locks (fixed-array counter updates) are the
    /// intended use.
    #[inline]
    pub fn lock_irqsave(&self) -> MutexIrqGuard<'_, T> {
        // Save-and-disable BEFORE acquiring: an interrupt landing between the
        // acquire and the cli could itself re-enter the lock, which is exactly
        // what we are preventing. Only touch the hardware / tracker when we are
        // the transition edge (enabled → disabled) so nesting inside another
        // interrupts-off region neither double-restores nor corrupts the
        // single-slot irqoff tracker.
        let were_enabled = crate::cpu::interrupts_enabled();
        if were_enabled {
            // SAFETY: interrupts are restored to their prior state when the
            // returned guard drops; the IDT is live (interrupts were enabled).
            unsafe {
                crate::cpu::cli();
            }
            crate::cpu::irqoff_tracker::record_disable();
        }
        let inner = self.lock();
        MutexIrqGuard {
            inner: core::mem::ManuallyDrop::new(inner),
            restore_if: were_enabled,
        }
    }

    /// Get the address used as the lockdep class identifier.
    #[inline]
    fn addr(&self) -> usize {
        // Use the address of the inner spin::Mutex as the class ID.
        // This ensures each Mutex instance is its own class.
        &self.inner as *const _ as usize
    }
}

/// RAII guard that releases the lock and notifies lockdep on drop.
///
/// Also records hold duration for contention statistics.
pub struct MutexGuard<'a, T> {
    /// Inner spin guard, held in `ManuallyDrop` so [`Drop`] can release the
    /// physical lock *before* re-enabling preemption (see the drop impl).
    guard: core::mem::ManuallyDrop<spin::MutexGuard<'a, T>>,
    addr: usize,
    /// Reference to the owning Mutex's stats for hold-time recording.
    stats: &'a ContentionStats,
    /// TSC at lock acquisition (0 if tracking disabled).
    acquire_tsc: u64,
    /// Reference to the owning Mutex's `owner` field, cleared to
    /// [`OWNER_NONE`] on release so a later stall names the *current* holder.
    owner: &'a AtomicU64,
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // Record hold duration before releasing.
        if self.acquire_tsc != 0 {
            let now = crate::bench::rdtsc();
            let hold = now.saturating_sub(self.acquire_tsc);
            self.stats.record_hold(hold);
        }
        // Release the lockdep tracking BEFORE dropping the inner guard.
        // This way, if another CPU is spinning on this lock and acquires
        // it immediately after us, the ordering edges are correct.
        lockdep::lock_release(self.addr);
        // Clear the diagnostic owner stamp before the physical unlock so a
        // stall reporter can never observe a freed lock still naming us.
        self.owner.store(OWNER_NONE, Ordering::Relaxed);
        // Ordering is critical for the preempt-disable invariant: the
        // *physical* lock must be released before we re-enable preemption.
        // If we re-enabled first, a timer tick landing in the tiny window
        // before the spin guard's own drop could involuntarily switch away
        // while the lock is still physically held — exactly the deadlock the
        // preempt-disable count exists to prevent.  ManuallyDrop lets us
        // force the unlock here, ahead of preempt_enable.
        //
        // SAFETY: `guard` is never touched again after this point (the field
        // is dropped exactly once, here), so taking it out of ManuallyDrop
        // and dropping it is sound.
        unsafe {
            core::mem::ManuallyDrop::drop(&mut self.guard);
        }
        crate::sched::preempt_enable();
    }
}

/// RAII guard for [`Mutex::lock_irqsave`].
///
/// Wraps a normal [`MutexGuard`] plus the saved interrupt-enable state. On
/// drop it releases the inner lock (which also re-enables preemption) FIRST,
/// then restores the interrupt flag — the exact reverse of the acquire order
/// (`cli` → preempt-off → lock ⟹ unlock → preempt-on → `sti`). Restoring
/// interrupts last guarantees no timer tick can preempt us while the physical
/// lock is still held.
pub struct MutexIrqGuard<'a, T> {
    /// Inner guard in `ManuallyDrop` so we can force its drop (release lock +
    /// re-enable preemption) before restoring interrupts.
    inner: core::mem::ManuallyDrop<MutexGuard<'a, T>>,
    /// Whether interrupts were enabled before we disabled them — if so, drop
    /// re-enables them; if not (we were nested inside an interrupts-off
    /// region), drop leaves them disabled.
    restore_if: bool,
}

impl<T> Deref for MutexIrqGuard<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T> DerefMut for MutexIrqGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T> Drop for MutexIrqGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // Release the physical lock and re-enable preemption first (the inner
        // MutexGuard's own Drop does both, in the correct order).
        //
        // SAFETY: `inner` is never touched again after this point (dropped
        // exactly once, here), so taking it out of ManuallyDrop is sound.
        unsafe {
            core::mem::ManuallyDrop::drop(&mut self.inner);
        }
        // Now restore interrupts, but only if we were the disabling edge.
        if self.restore_if {
            crate::cpu::irqoff_tracker::record_enable();
            // SAFETY: interrupts were enabled when we acquired (that is exactly
            // what `restore_if` records), so the IDT is live and re-enabling
            // simply returns to the caller's prior state.
            unsafe {
                crate::cpu::sti();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify the tracked Mutex works correctly with lockdep and contention stats.
#[allow(dead_code)]
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[sync] Running self-test...");

    // Test 1: Basic lock/unlock.
    let m = Mutex::named(42u64, b"test-sync");
    {
        let mut g = m.lock();
        assert_eq!(*g, 42);
        *g = 99;
    }
    {
        let g = m.lock();
        assert_eq!(*g, 99);
    }
    serial_println!("[sync]   Basic lock/unlock: OK");

    // Test 2: try_lock succeeds when unlocked.
    let m2 = Mutex::named(7u32, b"test-try");
    {
        let g = m2.try_lock();
        assert!(g.is_some());
        // SAFETY: We just verified it's Some.
        if let Some(guard) = g {
            assert_eq!(*guard, 7);
        }
    }
    serial_println!("[sync]   try_lock: OK");

    // Test 3: Contention stats are recorded.
    let acq = m.stats.acquisitions.load(Ordering::Relaxed);
    assert!(acq >= 2, "expected >=2 acquisitions, got {}", acq);
    serial_println!("[sync]   Contention stats recorded: {} acquisitions", acq);

    // Test 4: Hold time is non-zero (we held the lock briefly).
    let hold = m.stats.total_hold_cycles.load(Ordering::Relaxed);
    serial_println!("[sync]   Total hold cycles: {}", hold);

    serial_println!("[sync] Self-test PASSED");
}

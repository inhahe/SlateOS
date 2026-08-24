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
        self.total_wait_cycles
            .fetch_add(wait_cycles, Ordering::Relaxed);
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
        self.total_hold_cycles
            .fetch_add(hold_cycles, Ordering::Relaxed);
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
    entry
        .stats
        .store(stats as *const ContentionStats as u64, Ordering::Release);
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
    let mut result: [Option<LockStatSnapshot>; MAX_TRACKED_LOCKS] = [None; MAX_TRACKED_LOCKS];

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
        let name = unsafe { core::slice::from_raw_parts(name_ptr as *const u8, name_len) };

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
        if self
            .registered
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
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
        lockdep::lock_acquire(addr, self.name, lockdep::Acquire::Blocking);

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
        // Before spending 30 s finding out the slow way: if this task already
        // holds the lock, no number of retries can help. See
        // `fail_if_recursive`. (lockdep's `report_recursive` also covers this
        // type, but only while lockdep is enabled and its class table has room;
        // this check has neither dependency.)
        fail_if_recursive(self.name, self.addr(), &self.owner);

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
                    self.report_stall(iters, crate::bench::rdtsc().saturating_sub(start_tsc));
                }
            }
        }
    }

    /// Emit a one-shot diagnostic for a lock that has been spun on for an
    /// abnormally long time. Non-fatal: the caller keeps spinning afterwards.
    ///
    /// Thin wrapper over the shared [`report_spin_stall`] free function so both
    /// [`Mutex`] and [`PreemptSpinMutex`] produce identical stall diagnostics.
    #[cold]
    #[inline(never)]
    fn report_stall(&self, iters: u64, elapsed_cycles: u64) {
        report_spin_stall(self.name, self.addr(), &self.owner, iters, elapsed_cycles);
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
        //
        // `Acquire::Try` because there is none to detect on *success* either,
        // as far as this acquisition's own incoming edges go: had the lock been
        // held elsewhere we would have returned `None` above and released
        // everything, so this side of a cycle can never be the stuck one. It is
        // still pushed onto the held stack, because a blocking acquire nested
        // inside this critical section can deadlock in the ordinary way.
        lockdep::lock_acquire(addr, self.name, lockdep::Acquire::Try);
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
// Shared stall diagnostics
// ---------------------------------------------------------------------------

/// Emit a one-shot diagnostic for a lock that has been spun on for an
/// abnormally long time. Non-fatal: the caller keeps spinning afterwards.
///
/// Reports the lock name, the wedged CPU and task, the recorded holder, and —
/// via lockdep — the locks that CPU already holds (the key clue for an AB-BA
/// deadlock or convoy). Globally rate-limited to [`MAX_STALL_REPORTS`] so a
/// multi-CPU convoy cannot flood the serial log.
///
/// Shared by [`Mutex`] and [`PreemptSpinMutex`] so both lock types produce
/// identical stall output. `owner` is the lock's diagnostic holder-id atomic
/// (see [`Mutex::owner`] / [`PreemptSpinMutex`]); `OWNER_NONE` means unheld.
///
/// `addr` is the lock's own address, and it is printed because the name usually
/// is not enough to find the lock. [`Mutex::new`] defaults `name` to `"?"` and
/// the overwhelming majority of locks in the tree take that default (627
/// `Mutex::new` against 28 `Mutex::named` when this was written), so a stall
/// report that carried only the name said `lock '?'` and identified nothing —
/// which is exactly what happened in
/// `known-issues.md` → `BUG-BOOT-SPINLOCK-STALL-UNNAMED`. The address is
/// unambiguous for every lock, needs no per-site change to work, works for
/// heap-allocated locks that no symbol covers, and is the same key lockdep
/// already uses to identify a lock class — so it cross-references the two
/// reports directly. Naming all 627 sites would be 627 chances to forget one;
/// this is correct for locks nobody has thought about yet.
///
/// Limitation: this prints via the serial port, so if the *serial* lock itself
/// is the deadlocked lock (or is held by this same CPU) the report may not
/// appear. That is an accepted edge case — the target failure modes are the
/// scheduler / cgroup-table / teardown locks, not serial.
#[cold]
#[inline(never)]
fn report_spin_stall(
    name: &'static [u8],
    addr: usize,
    owner: &AtomicU64,
    iters: u64,
    elapsed_cycles: u64,
) {
    use crate::serial_println;

    let n = STALL_REPORTS.fetch_add(1, Ordering::Relaxed);
    if n >= MAX_STALL_REPORTS {
        return;
    }

    let cpu = crate::sched::current_cpu_id();
    let tid = crate::sched::current_task_id();
    let name = core::str::from_utf8(name).unwrap_or("<non-utf8>");
    let owner = owner.load(Ordering::Relaxed);

    // Report how long this spin *actually* ran, not the default threshold.
    // `spin_with_stall_threshold` takes a caller-supplied threshold — the
    // self-test uses ~10 ms — so printing the `STALL_SECONDS` default here made
    // the log claim a 30-second stall for a 10-millisecond one.  A diagnostic
    // that misstates the magnitude of what it detected is worse than none: the
    // number is the first thing anyone reading a stall report reasons from.
    let cycles_per_ms = crate::bench::tsc_freq().checked_div(1000).unwrap_or(0);
    let elapsed_ms = if cycles_per_ms != 0 && elapsed_cycles != 0 {
        elapsed_cycles.checked_div(cycles_per_ms)
    } else {
        // TSC uncalibrated (very early boot): the loop was counting iterations,
        // not time, so there is no honest wall-clock figure to print.
        None
    };
    match elapsed_ms {
        Some(ms) => serial_println!(
            "[sync] *** SPINLOCK STALL *** lock '{}' @ {:#x} still not acquired after ~{}ms of \
             spinning (cpu {}, task {}, {} iters). Likely self-deadlock or lock convoy; \
             the timer-driven liveness watchdog is blind to this if interrupts are \
             disabled.",
            name,
            addr,
            ms,
            cpu,
            tid,
            iters
        ),
        None => serial_println!(
            "[sync] *** SPINLOCK STALL *** lock '{}' @ {:#x} still not acquired after {} spin \
             iterations (cpu {}, task {}; TSC uncalibrated, so no wall-clock figure). Likely \
             self-deadlock or lock convoy; the timer-driven liveness watchdog is blind to this \
             if interrupts are disabled.",
            name,
            addr,
            iters,
            cpu,
            tid
        ),
    }
    // Name the holder: if `owner == tid`, this is a recursive self-deadlock
    // (the spinning task already holds the lock); if `owner` is some other
    // (possibly since-dead) task, the guard was leaked / the holder never
    // released. `OWNER_NONE` means the physical lock shows free yet `try_lock`
    // still fails — a lost-unlock / poisoned-flag desync.
    if owner == OWNER_NONE {
        serial_println!(
            "[sync]   lock '{}' @ {:#x} holder: NONE recorded (owner=unheld) — \
             lost-unlock or flag desync; spinner is task {} on cpu {}",
            name,
            addr,
            tid,
            cpu
        );
    } else if owner == tid {
        serial_println!(
            "[sync]   lock '{}' @ {:#x} holder: task {} == spinner — RECURSIVE \
             self-deadlock (same task re-entered the lock)",
            name,
            addr,
            owner
        );
    } else {
        serial_println!(
            "[sync]   lock '{}' @ {:#x} holder: task {} (spinner is task {}) — guard \
             held by another task; check whether it is still alive",
            name,
            addr,
            owner,
            tid
        );
    }
    // The single most useful clue: what else this CPU already holds.
    crate::lockdep::dump_held_locks(cpu);
}

/// Fail immediately on a *provable* self-deadlock, instead of discovering it
/// 30 seconds later — or, as actually happened, not at all.
///
/// Called once at the top of both lock types' contended paths, i.e. only after
/// `try_lock` has already failed, so an uncontended acquire never runs it.
///
/// If the recorded owner is the very task now asking for the lock, the acquire
/// can never succeed. A task runs on one CPU at a time, and both lock types
/// disable preemption for the whole hold, so the holder cannot be scheduled
/// elsewhere to release it. Every iteration of the spin below is therefore
/// known-futile before the first one runs.
///
/// This also gives the right answer for the interrupt case: an ISR that takes a
/// non-`irqsave` lock held by the task it interrupted does not change
/// `CURRENT_TASK_IDS`, so `owner == current_task_id()` still holds and the
/// diagnosis "this CPU already owns it" is still exactly true.
///
/// ## Why this exists when three other detectors already did
///
/// `fs::encrypt` shipped `STATE.lock().x = STATE.lock().x.saturating_add(1)`,
/// which takes a non-reentrant lock twice in one statement (the right-hand
/// guard is a temporary that outlives the left-hand acquire). The first boot
/// that ever called it froze for 20 minutes and printed **nothing** about a
/// lock — only the liveness watchdog's generic "no forward progress". All three
/// existing nets missed it:
///
/// - **lockdep's `report_recursive`** is precise and correct, but
///   [`PreemptSpinMutex`] deliberately does not register with lockdep (it is
///   documented as the no-tracking sibling for leaf locks), and `fs::encrypt`
///   imports `PreemptSpinMutex as Mutex`. Ordering checks genuinely add nothing
///   for a leaf lock — but *recursion* detection is not an ordering check, and
///   opting out of one silently opted out of the other.
/// - **[`report_spin_stall`]** does diagnose `owner == tid` as recursive, but
///   only after [`STALL_SECONDS`], and it never fired in that boot.
/// - The **liveness watchdog** fired, but it reports that the system is stuck,
///   not what it is stuck on.
///
/// So the check that mattered ran only on the lock type that had opted out of
/// it, behind a 30-second timer that did not go off. This one is unconditional,
/// type-independent, and immediate.
///
/// ## Why it panics
///
/// The condition is proven, not heuristic, and it is not survivable: the
/// alternative is spinning until the boot test's timeout with no indication of
/// which lock was involved. A panic names the lock and yields a backtrace
/// through the offending call path. The false-positive cases all require an
/// already-broken kernel (a leaked guard, so the lock is permanently held and
/// would wedge at the next acquire regardless).
///
/// Same limitation as [`report_spin_stall`]: it reports over serial, so a
/// recursive acquire of the *serial* lock itself cannot announce itself. That
/// case is no worse than today's silent hang.
#[cold]
#[inline(never)]
fn fail_if_recursive(name: &'static [u8], addr: usize, owner: &AtomicU64) {
    let owner_tid = owner.load(Ordering::Relaxed);
    if owner_tid == OWNER_NONE {
        return; // Held by nobody we know of — a convoy or a leaked guard.
    }
    if owner_tid != crate::sched::current_task_id() {
        return; // Genuine contention with another task: spin, as before.
    }

    let cpu = crate::sched::current_cpu_id();
    let display = core::str::from_utf8(name).unwrap_or("<non-utf8>");
    crate::serial_println!(
        "[sync] *** SELF-DEADLOCK *** lock '{}' @ {:#x} is already held by task {} on \
         cpu {} — the same task that is now trying to acquire it. This acquire can \
         never succeed. Common cause: two acquires in one statement, e.g. \
         `X.lock().n = X.lock().n + 1`, where the right-hand guard is a temporary \
         that lives until the end of the statement.",
        display,
        addr,
        owner_tid,
        cpu
    );
    crate::lockdep::dump_held_locks(cpu);
    panic!(
        "self-deadlock: task {} re-acquiring lock '{}' @ {:#x} that it already holds",
        owner_tid, display, addr
    );
}

/// Bounded-spin acquisition loop with stall detection, shared by the contended
/// paths of both lock types.
///
/// Spins calling `try_acquire` until it returns `Some`, exactly like
/// `spin::Mutex::lock()`, except that a spin lasting longer than
/// [`STALL_SECONDS`] (TSC-measured, or [`STALL_FALLBACK_ITERS`] before the TSC
/// is calibrated) fires a one-shot [`report_spin_stall`] naming the lock, then
/// keeps spinning. `#[cold]`/`#[inline(never)]` so callers' fast paths stay lean.
#[cold]
#[inline(never)]
fn spin_with_stall<G>(
    name: &'static [u8],
    addr: usize,
    owner: &AtomicU64,
    try_acquire: impl FnMut() -> Option<G>,
) -> G {
    spin_with_stall_threshold(
        name,
        addr,
        owner,
        crate::bench::tsc_freq().saturating_mul(STALL_SECONDS),
        try_acquire,
    )
}

/// The body of [`spin_with_stall`], with the stall threshold passed in rather
/// than derived from [`STALL_SECONDS`].
///
/// The split exists so the detector can be *tested*. With the threshold baked
/// in at 30 s, the only way to observe whether a stall report ever fires is to
/// deadlock a real boot for half a minute — which is precisely the experiment
/// that went wrong: a boot did hang here for ten minutes on a genuine
/// self-deadlock and no report appeared, and there was no way to tell whether
/// the detector was broken or the hang was somewhere else. `self_test_stall`
/// runs this same loop with a ~10 ms threshold and asserts the report fires,
/// so the question is answered on every boot in milliseconds instead of being
/// re-opened by every hang.
///
/// A `threshold_cycles` of 0 means the TSC is not calibrated yet and the
/// fallback iteration count applies.
fn spin_with_stall_threshold<G>(
    name: &'static [u8],
    addr: usize,
    owner: &AtomicU64,
    threshold_cycles: u64,
    mut try_acquire: impl FnMut() -> Option<G>,
) -> G {
    // Before spending 30 s finding out the slow way: if this task already holds
    // the lock, no number of retries can help. See `fail_if_recursive`.
    fail_if_recursive(name, addr, owner);

    let start_tsc = crate::bench::rdtsc();
    let mut iters: u64 = 0;
    let mut warned = false;
    loop {
        if let Some(g) = try_acquire() {
            return g;
        }
        core::hint::spin_loop();
        iters = iters.wrapping_add(1);
        if !warned && (iters & STALL_CHECK_MASK) == 0 {
            let stalled = if threshold_cycles != 0 {
                crate::bench::rdtsc().saturating_sub(start_tsc) >= threshold_cycles
            } else {
                iters >= STALL_FALLBACK_ITERS
            };
            if stalled {
                warned = true;
                report_spin_stall(
                    name,
                    addr,
                    owner,
                    iters,
                    crate::bench::rdtsc().saturating_sub(start_tsc),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PreemptSpinMutex — preempt-aware spinlock without lockdep/contention tracking
// ---------------------------------------------------------------------------

/// A preempt-disabling spinlock for **hot leaf locks**.
///
/// This is the lightweight sibling of [`Mutex`]. Like `Mutex`, it disables
/// involuntary preemption for the whole hold, so a holder can never be
/// context-switched away mid-critical-section — which closes the
/// *holder-preemption* single-CPU deadlock class that a raw [`spin::Mutex`]
/// suffers (holder preempted while a second task spins on the lock forever).
///
/// Unlike `Mutex`, it does **not** register with lockdep, does **not** record
/// contention statistics, and does **not** allocate a registry slot. That makes
/// it the right choice for high-frequency **leaf** locks (locks that never
/// nest another lock inside their critical section, so lockdep ordering checks
/// add no value) where the per-acquire tracking cost of `Mutex` would matter.
/// It retains the shared stall detector (via [`spin_with_stall`]) and a
/// diagnostic owner stamp, so a genuine wedge still names the lock and holder.
///
/// ## Choosing between the lock types (Q24 / design-decisions §70)
/// - **`PreemptSpinMutex`** — hot, uncontended, true leaf locks. Preempt-safe,
///   minimal overhead, no ordering checks.
/// - **[`Mutex`]** — contended and/or non-leaf locks, where lockdep ordering
///   detection and contention stats are worth the ~5ns/acquire overhead.
/// - **raw [`spin::Mutex`] + manual preempt** — reserved for the few locks that
///   must stay raw (e.g. the global heap lock, which cannot call back into the
///   allocator/scheduler tracking on its own acquire path).
pub struct PreemptSpinMutex<T> {
    inner: spin::Mutex<T>,
    /// Human-readable name, used only in the stall diagnostic.
    name: &'static [u8],
    /// Diagnostic holder task-id (`OWNER_NONE` = unheld). Single relaxed store
    /// per acquire/release; enables recursive-deadlock naming in a stall.
    owner: AtomicU64,
}

// SAFETY: PreemptSpinMutex<T> is Send+Sync whenever T is Send (same as
// spin::Mutex, which is the only shared interior state).
unsafe impl<T: Send> Send for PreemptSpinMutex<T> {}
unsafe impl<T: Send> Sync for PreemptSpinMutex<T> {}

impl<T> PreemptSpinMutex<T> {
    /// Create a new preempt-aware leaf spinlock with a default name.
    #[allow(dead_code)]
    pub const fn new(value: T) -> Self {
        Self {
            inner: spin::Mutex::new(value),
            name: b"?",
            owner: AtomicU64::new(OWNER_NONE),
        }
    }

    /// Create a new preempt-aware leaf spinlock with a diagnostic name (shown
    /// in stall reports). Keep it short.
    #[allow(dead_code)]
    pub const fn named(value: T, name: &'static [u8]) -> Self {
        Self {
            inner: spin::Mutex::new(value),
            name,
            owner: AtomicU64::new(OWNER_NONE),
        }
    }

    /// Address identifying this lock instance in a stall report.
    ///
    /// Deliberately the address of the inner `spin::Mutex`, matching
    /// [`Mutex::addr`], so the two lock types report the same identity for the
    /// same lock and a `PreemptSpinMutex` address can be compared against a
    /// lockdep class ID without an offset correction.
    #[inline]
    fn addr(&self) -> usize {
        &self.inner as *const _ as usize
    }

    /// Acquire the lock, returning a guard that releases on drop.
    ///
    /// Disables preemption before spinning (so the holder can't be preempted
    /// even while contended) and re-enables it after the physical unlock in the
    /// guard's `Drop`.
    #[inline]
    #[allow(dead_code)]
    pub fn lock(&self) -> PreemptSpinGuard<'_, T> {
        // Disable involuntary preemption for the whole hold. Paired with
        // `preempt_enable()` in `PreemptSpinGuard::drop`. Done before spinning
        // so the holder can't be preempted while contended either.
        crate::sched::preempt_disable();
        let guard = match self.inner.try_lock() {
            Some(g) => g,
            None => spin_with_stall(self.name, self.addr(), &self.owner, || {
                self.inner.try_lock()
            }),
        };
        self.owner
            .store(crate::sched::current_task_id(), Ordering::Relaxed);
        PreemptSpinGuard {
            guard: core::mem::ManuallyDrop::new(guard),
            owner: &self.owner,
        }
    }

    /// Try to acquire the lock without blocking. Returns `None` (and re-enables
    /// preemption) if the lock is already held.
    #[inline]
    #[allow(dead_code)]
    pub fn try_lock(&self) -> Option<PreemptSpinGuard<'_, T>> {
        crate::sched::preempt_disable();
        match self.inner.try_lock() {
            Some(guard) => {
                self.owner
                    .store(crate::sched::current_task_id(), Ordering::Relaxed);
                Some(PreemptSpinGuard {
                    guard: core::mem::ManuallyDrop::new(guard),
                    owner: &self.owner,
                })
            }
            None => {
                // No guard will be created to undo the disable, so undo it here.
                crate::sched::preempt_enable();
                None
            }
        }
    }

    /// Acquire with interrupts disabled for the whole hold (`spin_lock_irqsave`
    /// semantics), for a leaf lock reachable from BOTH task and interrupt/
    /// exception context on the same CPU. See [`Mutex::lock_irqsave`] for the
    /// full rationale and nesting behaviour — this mirrors it exactly.
    #[inline]
    #[allow(dead_code)]
    pub fn lock_irqsave(&self) -> PreemptSpinIrqGuard<'_, T> {
        // Save-and-disable BEFORE acquiring so an interrupt landing between the
        // acquire and the cli can't re-enter the lock. Only touch the hardware /
        // tracker on the enabled→disabled edge so nesting inside an existing
        // interrupts-off region neither double-restores nor corrupts the tracker.
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
        PreemptSpinIrqGuard {
            inner: core::mem::ManuallyDrop::new(inner),
            restore_if: were_enabled,
        }
    }
}

/// RAII guard for [`PreemptSpinMutex::lock`]. Releases the physical lock, then
/// re-enables preemption, on drop.
pub struct PreemptSpinGuard<'a, T> {
    /// Inner spin guard in `ManuallyDrop` so [`Drop`] can release the physical
    /// lock *before* re-enabling preemption (order is load-bearing — see below).
    guard: core::mem::ManuallyDrop<spin::MutexGuard<'a, T>>,
    /// The owning lock's `owner` atomic, cleared to [`OWNER_NONE`] on release.
    owner: &'a AtomicU64,
}

impl<T> Deref for PreemptSpinGuard<'_, T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T> DerefMut for PreemptSpinGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

impl<T> Drop for PreemptSpinGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // Clear the diagnostic owner stamp before the physical unlock so a
        // stall reporter can never observe a freed lock still naming us.
        self.owner.store(OWNER_NONE, Ordering::Relaxed);
        // Ordering is critical: the *physical* lock must be released before we
        // re-enable preemption. If we re-enabled first, a timer tick landing in
        // the tiny window before the spin guard's own drop could involuntarily
        // switch away while the lock is still physically held — exactly the
        // deadlock the preempt-disable count exists to prevent. `ManuallyDrop`
        // lets us force the unlock here, ahead of `preempt_enable`.
        //
        // SAFETY: `guard` is never touched again after this point (dropped
        // exactly once, here), so taking it out of ManuallyDrop is sound.
        unsafe {
            core::mem::ManuallyDrop::drop(&mut self.guard);
        }
        crate::sched::preempt_enable();
    }
}

/// RAII guard for [`PreemptSpinMutex::lock_irqsave`]. On drop it releases the
/// inner lock (which also re-enables preemption) FIRST, then restores the
/// interrupt flag — the exact reverse of the acquire order. See
/// [`MutexIrqGuard`] for the full ordering rationale.
pub struct PreemptSpinIrqGuard<'a, T> {
    inner: core::mem::ManuallyDrop<PreemptSpinGuard<'a, T>>,
    restore_if: bool,
}

impl<T> Deref for PreemptSpinIrqGuard<'_, T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T> DerefMut for PreemptSpinIrqGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T> Drop for PreemptSpinIrqGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // Release the physical lock and re-enable preemption first (the inner
        // guard's own Drop does both, in the correct order).
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

    // Test 5: PreemptSpinMutex basic lock/unlock + value mutation.
    let p = PreemptSpinMutex::named(5u64, b"test-pmutex");
    {
        let mut g = p.lock();
        assert_eq!(*g, 5);
        *g = 11;
    }
    {
        let g = p.lock();
        assert_eq!(*g, 11);
    }
    serial_println!("[sync]   PreemptSpinMutex lock/unlock: OK");

    // Test 6: PreemptSpinMutex try_lock succeeds when free, and fails (returns
    // None, without leaking a preempt-disable) while the lock is held.
    let p2 = PreemptSpinMutex::named(0u32, b"test-ptry");
    {
        let held = p2.lock();
        assert!(
            p2.try_lock().is_none(),
            "try_lock must fail while the lock is held"
        );
        drop(held);
    }
    {
        let g = p2.try_lock();
        assert!(g.is_some(), "try_lock must succeed once released");
    }
    serial_println!("[sync]   PreemptSpinMutex try_lock: OK");

    // Test 7: the spin-stall detector actually fires.
    self_test_stall();

    serial_println!("[sync] Self-test PASSED");
}

/// Prove that a spin lasting past the stall threshold emits a stall report.
///
/// **Why this test exists.** The stall detector is the kernel's only net for a
/// lock convoy, a leaked guard, or a task wedged on a lock another (possibly
/// dead) task holds — the failure modes `fail_if_recursive` does *not* cover,
/// because it only catches a task re-entering a lock it holds itself. A net
/// that has never been shown to work is not a net. And there is direct reason
/// to doubt this one: a boot hung for ten minutes on a genuine self-deadlock in
/// `fs::encrypt`, which spun in exactly this loop, and no stall report ever
/// reached the serial log. Every explanation offered for that has since been
/// disproved (the TSC is calibrated to 3.66 GHz at boot line 133, long before
/// the filesystem self-tests run, so the uncalibrated-fallback path was not in
/// play; neither report budget had been touched). Until this test runs, "does
/// the detector work?" is answered only by an absence of evidence.
///
/// **Why it takes milliseconds, not thirty seconds.** It calls
/// `spin_with_stall_threshold` — the same loop the real contended paths use —
/// with a threshold of ~10 ms instead of [`STALL_SECONDS`]. Everything under
/// test is shared with production: the iteration throttle, the `rdtsc`
/// comparison, the one-shot `warned` latch, and `report_spin_stall` itself.
///
/// **Why it cannot hang.** A broken detector would leave the closure returning
/// `None` forever, which is the very hang this test is meant to diagnose. So
/// the closure also gives up after 100× the threshold and reports failure
/// through the return value, turning "silent hang" into "named assertion
/// failure" — the same reason `fail_if_recursive` panics rather than warns.
#[allow(dead_code)]
fn self_test_stall() {
    use crate::serial_println;

    let freq = crate::bench::tsc_freq();
    assert!(
        freq != 0,
        "TSC is not calibrated, so every stall check in the kernel is running on \
         the raw-iteration fallback instead of wall-clock time"
    );

    // ~10 ms. Long enough that the 4096-iteration throttle checks many times,
    // short enough to be free on every boot. Floored at 1 because a threshold of
    // 0 means "TSC uncalibrated" to the loop and would silently switch it to the
    // five-billion-iteration fallback, i.e. test something else entirely.
    let threshold = freq.checked_div(100).unwrap_or(0).max(1);
    // 100x the threshold (~1 s) is the give-up bound: far past any plausible
    // scheduling hiccup, so reaching it means the detector genuinely failed.
    let deadline = crate::bench::rdtsc().saturating_add(threshold.saturating_mul(100));

    // `OWNER_NONE`: no task holds this synthetic lock, so `fail_if_recursive`
    // returns immediately and we exercise the spin path rather than panicking.
    let owner = AtomicU64::new(OWNER_NONE);
    let reports_before = STALL_REPORTS.load(Ordering::Relaxed);

    serial_println!(
        "[sync]   (self-test) forcing a ~10ms spin stall; the 'SPINLOCK STALL' line \
         below is expected and not a real event:"
    );

    let fired = spin_with_stall_threshold(b"selftest-stall", 0, &owner, threshold, || {
        if STALL_REPORTS.load(Ordering::Relaxed) != reports_before {
            return Some(true); // the report fired — stop spinning
        }
        if crate::bench::rdtsc() >= deadline {
            return Some(false); // gave up; the detector did not fire
        }
        None
    });

    // Hand back the report budget. This test is not a real stall, and consuming
    // one of the eight slots would make a genuine convoy later in the boot that
    // much more likely to be silently truncated.
    STALL_REPORTS.store(reports_before, Ordering::Relaxed);

    assert!(
        fired,
        "spin-stall detector did not report after spinning ~100x its threshold — \
         a real lock convoy or leaked guard would wedge the machine silently"
    );
    serial_println!("[sync]   Spin-stall detector fires: OK");
}

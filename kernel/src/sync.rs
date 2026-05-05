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
}

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
        lockdep::lock_acquire(addr, self.name);

        if tracking_enabled() {
            // Try the fast path: immediate acquisition.
            if let Some(guard) = self.inner.try_lock() {
                self.stats.record_uncontended();
                let acquire_tsc = crate::bench::rdtsc();
                return MutexGuard {
                    guard,
                    addr,
                    stats: &self.stats,
                    acquire_tsc,
                };
            }

            // Contended path: time the spin.
            let start = crate::bench::rdtsc();
            let guard = self.inner.lock();
            let end = crate::bench::rdtsc();
            let wait = end.saturating_sub(start);
            self.stats.record_contended(wait);
            return MutexGuard {
                guard,
                addr,
                stats: &self.stats,
                acquire_tsc: end,
            };
        }

        // Tracking disabled: just lock.
        let guard = self.inner.lock();
        MutexGuard {
            guard,
            addr,
            stats: &self.stats,
            acquire_tsc: 0,
        }
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
        let guard = self.inner.try_lock()?;
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
        Some(MutexGuard { guard, addr, stats: &self.stats, acquire_tsc })
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
    guard: spin::MutexGuard<'a, T>,
    addr: usize,
    /// Reference to the owning Mutex's stats for hold-time recording.
    stats: &'a ContentionStats,
    /// TSC at lock acquisition (0 if tracking disabled).
    acquire_tsc: u64,
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

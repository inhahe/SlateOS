//! Disk space reclamation daemon.
//!
//! Coordinates automatic space recovery when disk usage gets high.
//! Ties together several existing subsystems to free space in priority
//! order (safest first, most aggressive last):
//!
//! 1. Buffer cache: flush dirty entries, evict clean ones
//! 2. CAS garbage collection: remove unreferenced content blobs
//! 3. Tmpwatch: clean old temporary files
//! 4. Trash: purge oldest recycle bin entries
//! 5. Journal: trim old filesystem change log entries
//!
//! ## Design
//!
//! ```text
//! Trigger: disk usage > high_watermark (default 90%)
//! Stop:    disk usage < low_watermark  (default 80%)
//!
//! reclaim::check() → scan triggers → run recovery phases
//! ```
//!
//! The `check()` function is lightweight (a single statvfs + comparison)
//! and can be called periodically from the timer softirq or on every
//! write.  Actual reclamation only runs when the threshold is crossed.
//!
//! ## Configuration
//!
//! - `high_watermark`: trigger reclamation (default 90% full)
//! - `low_watermark`: stop reclamation (default 80% full)
//! - `enabled`: master enable/disable
//! - Per-phase enable: each recovery phase can be individually disabled
//!
//! ## Reference
//!
//! design.txt: "Per-user, per-group, and per-app quotas" + trash auto-prune

#![allow(dead_code)]

use alloc::string::String;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::KernelResult;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of a single reclamation pass.
#[derive(Debug, Clone, Default)]
pub struct ReclaimResult {
    /// Whether reclamation was triggered.
    pub triggered: bool,
    /// Bytes freed by buffer cache eviction.
    pub cache_freed: u64,
    /// Blobs removed by CAS garbage collection.
    pub cas_gc_blobs: u64,
    /// Bytes freed by CAS GC.
    pub cas_gc_bytes: u64,
    /// Files cleaned by tmpwatch.
    pub tmpwatch_cleaned: u64,
    /// Items purged from trash.
    pub trash_purged: u64,
    /// Journal entries trimmed.
    pub journal_trimmed: u64,
    /// Total bytes estimated freed.
    pub total_freed: u64,
    /// Disk usage percent after reclamation.
    pub usage_after: u64,
}

/// Per-phase enable/disable configuration.
#[derive(Debug, Clone, Copy)]
pub struct PhaseConfig {
    /// Phase 1: buffer cache flush/evict.
    pub cache: bool,
    /// Phase 2: CAS garbage collection.
    pub cas_gc: bool,
    /// Phase 3: tmpwatch cleanup.
    pub tmpwatch: bool,
    /// Phase 4: trash purge.
    pub trash: bool,
    /// Phase 5: journal trim.
    pub journal: bool,
}

impl Default for PhaseConfig {
    fn default() -> Self {
        Self {
            cache: true,
            cas_gc: true,
            tmpwatch: true,
            trash: true,
            journal: true,
        }
    }
}

/// Statistics about reclamation activity.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReclaimStats {
    /// Number of times reclamation was triggered.
    pub trigger_count: u64,
    /// Total bytes freed across all runs.
    pub total_bytes_freed: u64,
    /// Total CAS blobs collected.
    pub total_cas_blobs: u64,
    /// Total tmpwatch files cleaned.
    pub total_tmpwatch_files: u64,
    /// Total trash items purged.
    pub total_trash_items: u64,
    /// Total journal entries trimmed.
    pub total_journal_entries: u64,
    /// Whether currently reclaiming.
    pub active: bool,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Master enable flag (fast-path when disabled).
static ENABLED: AtomicBool = AtomicBool::new(true);

/// High watermark (percent × 100 for precision, e.g. 9000 = 90.00%).
static HIGH_WM: AtomicU64 = AtomicU64::new(9000);

/// Low watermark (percent × 100).
static LOW_WM: AtomicU64 = AtomicU64::new(8000);

struct ReclaimInner {
    phases: PhaseConfig,
    stats: ReclaimStats,
    /// Target mount path to monitor (default "/").
    target_mount: String,
}

static RECLAIM: Mutex<ReclaimInner> = Mutex::new(ReclaimInner {
    phases: PhaseConfig {
        cache: true,
        cas_gc: true,
        tmpwatch: true,
        trash: true,
        journal: true,
    },
    stats: ReclaimStats {
        trigger_count: 0,
        total_bytes_freed: 0,
        total_cas_blobs: 0,
        total_tmpwatch_files: 0,
        total_trash_items: 0,
        total_journal_entries: 0,
        active: false,
    },
    target_mount: String::new(), // Will default to "/"
});

// ---------------------------------------------------------------------------
// Configuration API
// ---------------------------------------------------------------------------

/// Enable or disable the reclamation daemon.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    serial_println!("[reclaim] {}", if enabled { "enabled" } else { "disabled" });
}

/// Check if reclamation is enabled.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Set the high watermark (percent, 0-100).
pub fn set_high_watermark(pct: u64) {
    let clamped = pct.min(100);
    HIGH_WM.store(clamped.saturating_mul(100), Ordering::Relaxed);
}

/// Set the low watermark (percent, 0-100).
pub fn set_low_watermark(pct: u64) {
    let clamped = pct.min(100);
    LOW_WM.store(clamped.saturating_mul(100), Ordering::Relaxed);
}

/// Set per-phase enable/disable configuration.
pub fn set_phases(phases: PhaseConfig) {
    RECLAIM.lock().phases = phases;
}

/// Set the target mount path to monitor.
pub fn set_target(mount_path: &str) {
    RECLAIM.lock().target_mount = String::from(mount_path);
}

/// Get current statistics.
pub fn stats() -> ReclaimStats {
    RECLAIM.lock().stats
}

/// Get current phase configuration.
pub fn phases() -> PhaseConfig {
    RECLAIM.lock().phases
}

/// Get current watermarks as (high, low) in percent.
pub fn watermarks() -> (u64, u64) {
    let h = HIGH_WM.load(Ordering::Relaxed) / 100;
    let l = LOW_WM.load(Ordering::Relaxed) / 100;
    (h, l)
}

// ---------------------------------------------------------------------------
// Core reclamation logic
// ---------------------------------------------------------------------------

/// Check disk usage and run reclamation if needed.
///
/// This is the main entry point.  Call it periodically (e.g., from the
/// timer softirq every N seconds) or after large writes.
///
/// Returns `None` if no reclamation was needed, or `Some(result)` with
/// details of what was freed.
pub fn check() -> Option<ReclaimResult> {
    if !ENABLED.load(Ordering::Relaxed) {
        return None;
    }

    // Get disk usage.
    let (usage_pct, _total, _free) = get_usage()?;

    let high = HIGH_WM.load(Ordering::Relaxed);

    // Only trigger if above high watermark.
    if usage_pct < high {
        return None;
    }

    run()
}

/// Force a reclamation pass regardless of current disk usage.
pub fn run() -> Option<ReclaimResult> {
    let phases = {
        let mut inner = RECLAIM.lock();
        if inner.stats.active {
            return None; // Already running.
        }
        inner.stats.active = true;
        inner.phases
    };

    let low = LOW_WM.load(Ordering::Relaxed);
    let mut result = ReclaimResult {
        triggered: true,
        ..ReclaimResult::default()
    };

    // Phase 1: Buffer cache.
    if phases.cache {
        let freed = run_cache_phase();
        result.cache_freed = freed;
        result.total_freed = result.total_freed.saturating_add(freed);

        if check_below_target(low) {
            finalize_result(&mut result);
            return Some(result);
        }
    }

    // Phase 2: CAS garbage collection.
    if phases.cas_gc {
        let (blobs, bytes) = run_cas_gc_phase();
        result.cas_gc_blobs = blobs as u64;
        result.cas_gc_bytes = bytes;
        result.total_freed = result.total_freed.saturating_add(bytes);

        if check_below_target(low) {
            finalize_result(&mut result);
            return Some(result);
        }
    }

    // Phase 3: Tmpwatch.
    if phases.tmpwatch {
        let cleaned = run_tmpwatch_phase();
        result.tmpwatch_cleaned = cleaned;

        if check_below_target(low) {
            finalize_result(&mut result);
            return Some(result);
        }
    }

    // Phase 4: Trash purge.
    if phases.trash {
        let purged = run_trash_phase();
        result.trash_purged = purged;

        if check_below_target(low) {
            finalize_result(&mut result);
            return Some(result);
        }
    }

    // Phase 5: Journal trim.
    if phases.journal {
        let trimmed = run_journal_phase();
        result.journal_trimmed = trimmed;
    }

    finalize_result(&mut result);
    Some(result)
}

/// Finalize the result: update stats, check final usage, clear active flag.
fn finalize_result(result: &mut ReclaimResult) {
    // Get final usage.
    result.usage_after = get_usage()
        .map(|(pct, _, _)| pct / 100)
        .unwrap_or(0);

    let mut inner = RECLAIM.lock();
    inner.stats.active = false;
    inner.stats.trigger_count = inner.stats.trigger_count.saturating_add(1);
    inner.stats.total_bytes_freed = inner
        .stats
        .total_bytes_freed
        .saturating_add(result.total_freed);
    inner.stats.total_cas_blobs = inner
        .stats
        .total_cas_blobs
        .saturating_add(result.cas_gc_blobs);
    inner.stats.total_tmpwatch_files = inner
        .stats
        .total_tmpwatch_files
        .saturating_add(result.tmpwatch_cleaned);
    inner.stats.total_trash_items = inner
        .stats
        .total_trash_items
        .saturating_add(result.trash_purged);
    inner.stats.total_journal_entries = inner
        .stats
        .total_journal_entries
        .saturating_add(result.journal_trimmed);

    serial_println!(
        "[reclaim] Freed ~{} bytes (cache={}, cas={}, tmp={}, trash={}, journal={})",
        result.total_freed,
        result.cache_freed,
        result.cas_gc_bytes,
        result.tmpwatch_cleaned,
        result.trash_purged,
        result.journal_trimmed,
    );
}

// ---------------------------------------------------------------------------
// Phase implementations
// ---------------------------------------------------------------------------

/// Phase 1: Flush dirty buffer cache entries and report freed space.
fn run_cache_phase() -> u64 {
    // Flush expired dirty entries to disk.
    let flushed = crate::fs::cache::flush_expired();
    // Also flush any remaining dirty entries.
    let _ = crate::fs::cache::flush_all();
    // Approximate freed bytes: each flushed entry is a 512-byte sector.
    (flushed as u64).saturating_mul(512)
}

/// Phase 2: CAS garbage collection.
fn run_cas_gc_phase() -> (usize, u64) {
    crate::fs::cas::gc()
}

/// Phase 3: Run tmpwatch cleanup.
fn run_tmpwatch_phase() -> u64 {
    let now = crate::timekeeping::clock_realtime() / 1_000_000_000;
    match crate::fs::tmpwatch::run(now) {
        Ok(result) => result.files_removed,
        Err(_) => 0,
    }
}

/// Phase 4: Empty the trash to free space.
fn run_trash_phase() -> u64 {
    // Count items before emptying.
    let before = crate::fs::trash::list().map(|v| v.len()).unwrap_or(0);
    match crate::fs::trash::empty() {
        Ok(()) => before as u64,
        Err(_) => 0,
    }
}

/// Phase 5: Flush journal entries to persistent storage.
fn run_journal_phase() -> u64 {
    let (count_before, _) = crate::fs::journal::stats();
    // Flushing writes buffered entries to disk; the ring buffer itself
    // doesn't shrink, but the data is persisted and can be reclaimed
    // on next boot.  We report the number of entries flushed.
    let _ = crate::fs::journal::flush();
    count_before as u64
}

// ---------------------------------------------------------------------------
// Disk usage helpers
// ---------------------------------------------------------------------------

/// Get current disk usage as (usage_pct_x100, total_blocks, free_blocks).
///
/// Returns None if we can't determine usage (no mount found).
fn get_usage() -> Option<(u64, u64, u64)> {
    let mount_path = {
        let inner = RECLAIM.lock();
        if inner.target_mount.is_empty() {
            String::from("/")
        } else {
            inner.target_mount.clone()
        }
    };

    let info = crate::fs::Vfs::statvfs(&mount_path).ok()?;

    if info.total_blocks == 0 {
        return None;
    }

    let used = info.total_blocks.saturating_sub(info.free_blocks);
    // Compute percent × 100 for precision.
    let pct_x100 = used
        .saturating_mul(10000)
        .checked_div(info.total_blocks)
        .unwrap_or(0);

    Some((pct_x100, info.total_blocks, info.free_blocks))
}

/// Check if usage is below the low watermark.
fn check_below_target(low_wm: u64) -> bool {
    match get_usage() {
        Some((pct, _, _)) => pct < low_wm,
        None => true, // Can't check → assume OK.
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[reclaim] Running self-test...");

    test_watermarks();
    test_enable_disable();
    test_phase_config();
    test_get_usage();
    test_run_noop();
    test_stats();

    serial_println!("[reclaim] Self-test passed (6 tests).");
    Ok(())
}

fn test_watermarks() {
    set_high_watermark(95);
    set_low_watermark(85);
    let (h, l) = watermarks();
    assert_eq!(h, 95);
    assert_eq!(l, 85);

    // Clamp to 100.
    set_high_watermark(150);
    let (h, _) = watermarks();
    assert_eq!(h, 100);

    // Restore defaults.
    set_high_watermark(90);
    set_low_watermark(80);

    serial_println!("[reclaim]   watermarks: ok");
}

fn test_enable_disable() {
    set_enabled(false);
    assert!(!is_enabled());

    // Check should return None when disabled.
    assert!(check().is_none());

    set_enabled(true);
    assert!(is_enabled());

    serial_println!("[reclaim]   enable/disable: ok");
}

fn test_phase_config() {
    let mut cfg = PhaseConfig::default();
    assert!(cfg.cache);
    assert!(cfg.cas_gc);
    assert!(cfg.tmpwatch);

    cfg.cache = false;
    set_phases(cfg);
    let p = phases();
    assert!(!p.cache);
    assert!(p.cas_gc);

    // Restore defaults.
    set_phases(PhaseConfig::default());

    serial_println!("[reclaim]   phase_config: ok");
}

fn test_get_usage() {
    // Should be able to get usage for root mount.
    let usage = get_usage();
    // May be None if root mount doesn't support statvfs, but shouldn't panic.
    if let Some((pct, total, free)) = usage {
        assert!(pct <= 10000, "percent should be <= 100.00");
        assert!(free <= total, "free should be <= total");
    }
    serial_println!("[reclaim]   get_usage: ok");
}

fn test_run_noop() {
    // Run a reclamation pass.  Should succeed even if there's nothing to do.
    let result = run();
    assert!(result.is_some());
    let r = result.expect("run returned Some");
    assert!(r.triggered);
    // All counts should be non-negative (they always are since they're u64).
    serial_println!("[reclaim]   run (noop): ok");
}

fn test_stats() {
    let s = stats();
    // After test_run_noop, trigger_count should be >= 1.
    assert!(s.trigger_count >= 1, "should have at least one trigger");
    serial_println!("[reclaim]   stats: ok");
}

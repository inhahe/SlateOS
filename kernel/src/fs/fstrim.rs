//! Filesystem TRIM/DISCARD support for flash storage.
//!
//! When files are deleted, the underlying storage device doesn't
//! automatically know those sectors are free. TRIM commands inform
//! the SSD firmware which blocks are no longer in use, enabling:
//!
//! - **Wear leveling** — the FTL can redistribute writes evenly.
//! - **Garbage collection** — freed blocks can be erased proactively.
//! - **Write amplification reduction** — fewer read-modify-write cycles.
//! - **Sustained performance** — avoids the SSD "performance cliff."
//!
//! ## Architecture
//!
//! ```text
//! VFS delete/truncate → notify fstrim of freed range
//!   → range added to pending discard queue
//!   → periodic or manual flush → batch TRIM to device
//!
//! Manual: fstrim <mountpoint>
//!   → walk free-space bitmap → batch TRIM all free ranges
//! ```
//!
//! ## Modes
//!
//! | Mode       | Description                                      |
//! |------------|--------------------------------------------------|
//! | Periodic   | Timer-based batch TRIM (e.g., weekly)            |
//! | Continuous | TRIM on each delete (low latency, higher CPU)    |
//! | Manual     | On-demand `fstrim` command only                  |
//!
//! ## Design Notes
//!
//! - Discard ranges are coalesced when adjacent (merge optimization).
//! - Minimum discard granularity: 4 KiB (below this, TRIM overhead
//!   exceeds benefit).
//! - Maximum queued ranges: 1024 (oldest flushed first when full).
//! - In our VFS model, TRIM is tracked logically since we don't have
//!   direct block device access from this layer. The ranges are
//!   recorded for the block device driver to consume.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelResult;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Minimum discard granularity (4 KiB).
const MIN_DISCARD_SIZE: u64 = 4096;

/// Maximum queued discard ranges before forced flush.
const MAX_QUEUED_RANGES: usize = 1024;

/// Default periodic interval (weekly, in nanoseconds).
/// 7 * 24 * 60 * 60 * 1_000_000_000
const DEFAULT_PERIOD_NS: u64 = 604_800_000_000_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// TRIM/discard operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimMode {
    /// TRIM only on explicit `fstrim` command.
    Manual,
    /// Batch TRIM at periodic intervals.
    Periodic,
    /// TRIM immediately on each free operation.
    Continuous,
}

impl TrimMode {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Periodic => "periodic",
            Self::Continuous => "continuous",
        }
    }

    /// Parse from string.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "manual" | "off" => Some(Self::Manual),
            "periodic" | "timer" | "batch" => Some(Self::Periodic),
            "continuous" | "online" | "immediate" => Some(Self::Continuous),
            _ => None,
        }
    }
}

/// A pending discard range.
#[derive(Debug, Clone)]
struct DiscardRange {
    /// Device or mount path this range belongs to.
    device: String,
    /// Start offset in bytes.
    offset: u64,
    /// Length in bytes.
    length: u64,
    /// Timestamp when queued.
    queued_ns: u64,
}

/// Result of a TRIM operation.
#[derive(Debug, Clone)]
pub struct TrimResult {
    /// Number of discard ranges issued.
    pub ranges_trimmed: u32,
    /// Total bytes discarded.
    pub bytes_trimmed: u64,
    /// Ranges that were coalesced (merged with adjacent).
    pub ranges_coalesced: u32,
}

/// Per-device TRIM capabilities.
#[derive(Debug, Clone)]
pub struct DeviceTrimInfo {
    /// Device path/identifier.
    pub device: String,
    /// Whether TRIM is supported.
    pub trim_supported: bool,
    /// Maximum discard size per command (0 = unlimited).
    pub max_discard_bytes: u64,
    /// Preferred discard granularity.
    pub discard_granularity: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Current operating mode.
static MODE: spin::Mutex<TrimMode> = spin::Mutex::new(TrimMode::Periodic);

/// Periodic interval in nanoseconds.
static PERIOD_NS: AtomicU64 = AtomicU64::new(DEFAULT_PERIOD_NS);

/// Pending discard queue.
static DISCARD_QUEUE: spin::Mutex<Vec<DiscardRange>> = spin::Mutex::new(Vec::new());

/// Registered device capabilities.
static DEVICE_INFO: spin::Mutex<Vec<DeviceTrimInfo>> = spin::Mutex::new(Vec::new());

/// Last flush timestamp.
static LAST_FLUSH_NS: AtomicU64 = AtomicU64::new(0);

/// Statistics.
static TOTAL_TRIMS: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES_TRIMMED: AtomicU64 = AtomicU64::new(0);
static TOTAL_RANGES_QUEUED: AtomicU64 = AtomicU64::new(0);
static TOTAL_COALESCED: AtomicU64 = AtomicU64::new(0);
static QUEUE_OVERFLOWS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API — Configuration
// ---------------------------------------------------------------------------

/// Get current TRIM mode.
pub fn get_mode() -> TrimMode {
    *MODE.lock()
}

/// Set TRIM mode.
pub fn set_mode(mode: TrimMode) {
    *MODE.lock() = mode;
}

/// Get periodic interval in nanoseconds.
pub fn get_period_ns() -> u64 {
    PERIOD_NS.load(Ordering::Relaxed)
}

/// Set periodic interval in nanoseconds.
pub fn set_period_ns(ns: u64) {
    PERIOD_NS.store(ns, Ordering::Relaxed);
}

/// Register device TRIM capabilities.
pub fn register_device(info: DeviceTrimInfo) {
    let mut devices = DEVICE_INFO.lock();
    // Replace existing or add new.
    for d in devices.iter_mut() {
        if d.device == info.device {
            *d = info;
            return;
        }
    }
    devices.push(info);
}

/// Check if TRIM is supported for a device.
pub fn is_trim_supported(device: &str) -> bool {
    let devices = DEVICE_INFO.lock();
    devices.iter().any(|d| d.device == device && d.trim_supported)
}

// ---------------------------------------------------------------------------
// Public API — Discard operations
// ---------------------------------------------------------------------------

/// Notify that a range has been freed and can be discarded.
///
/// Called by the VFS on file deletion, truncation, or hole punching.
/// The range is queued for batch TRIM unless in continuous mode.
pub fn notify_free(device: &str, offset: u64, length: u64) {
    // Skip ranges below minimum granularity.
    if length < MIN_DISCARD_SIZE {
        return;
    }

    let mode = *MODE.lock();

    match mode {
        TrimMode::Manual => {
            // Queue for later manual flush.
            queue_range(device, offset, length);
        }
        TrimMode::Periodic => {
            // Queue and check if flush is due.
            queue_range(device, offset, length);
            maybe_periodic_flush();
        }
        TrimMode::Continuous => {
            // Immediate trim.
            issue_trim(device, offset, length);
        }
    }
}

/// Manually flush all pending discards (equivalent to `fstrim`).
///
/// Optionally filter to a specific device. Pass empty string for all.
pub fn flush(device_filter: &str) -> TrimResult {
    let mut queue = DISCARD_QUEUE.lock();

    let ranges: Vec<DiscardRange> = if device_filter.is_empty() {
        queue.drain(..).collect()
    } else {
        let (matching, remaining): (Vec<_>, Vec<_>) =
            queue.drain(..).partition(|r| r.device == device_filter);
        *queue = remaining;
        matching
    };

    drop(queue);

    if ranges.is_empty() {
        return TrimResult { ranges_trimmed: 0, bytes_trimmed: 0, ranges_coalesced: 0 };
    }

    // Coalesce adjacent ranges per device.
    let coalesced = coalesce_ranges(ranges);
    let coalesce_count = coalesced.iter()
        .map(|(_, ranges)| ranges.len().saturating_sub(1) as u32)
        .sum::<u32>();

    let mut total_trimmed: u32 = 0;
    let mut total_bytes: u64 = 0;

    for (device, ranges) in &coalesced {
        for &(offset, length) in ranges {
            issue_trim(device, offset, length);
            total_trimmed += 1;
            total_bytes += length;
        }
    }

    TOTAL_COALESCED.fetch_add(u64::from(coalesce_count), Ordering::Relaxed);
    LAST_FLUSH_NS.store(crate::timekeeping::clock_monotonic(), Ordering::Relaxed);

    TrimResult {
        ranges_trimmed: total_trimmed,
        bytes_trimmed: total_bytes,
        ranges_coalesced: coalesce_count,
    }
}

/// Get the number of pending discard ranges.
pub fn pending_count() -> usize {
    DISCARD_QUEUE.lock().len()
}

/// Get pending ranges summary (device, count, total bytes).
pub fn pending_summary() -> Vec<(String, usize, u64)> {
    let queue = DISCARD_QUEUE.lock();
    let mut by_device: Vec<(String, usize, u64)> = Vec::new();

    for range in queue.iter() {
        if let Some(entry) = by_device.iter_mut().find(|(d, _, _)| *d == range.device) {
            entry.1 += 1;
            entry.2 += range.length;
        } else {
            by_device.push((range.device.clone(), 1, range.length));
        }
    }

    by_device
}

/// Discard all pending ranges without issuing TRIMs (drop).
pub fn drop_pending() -> usize {
    let mut queue = DISCARD_QUEUE.lock();
    let count = queue.len();
    queue.clear();
    count
}

// ---------------------------------------------------------------------------
// Public API — Statistics
// ---------------------------------------------------------------------------

/// Get trim statistics.
pub fn stats() -> (u64, u64, u64, u64, u64, usize, u64) {
    let pending = DISCARD_QUEUE.lock().len();
    (
        TOTAL_TRIMS.load(Ordering::Relaxed),
        TOTAL_BYTES_TRIMMED.load(Ordering::Relaxed),
        TOTAL_RANGES_QUEUED.load(Ordering::Relaxed),
        TOTAL_COALESCED.load(Ordering::Relaxed),
        QUEUE_OVERFLOWS.load(Ordering::Relaxed),
        pending,
        LAST_FLUSH_NS.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    TOTAL_TRIMS.store(0, Ordering::Relaxed);
    TOTAL_BYTES_TRIMMED.store(0, Ordering::Relaxed);
    TOTAL_RANGES_QUEUED.store(0, Ordering::Relaxed);
    TOTAL_COALESCED.store(0, Ordering::Relaxed);
    QUEUE_OVERFLOWS.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Queue a discard range.
fn queue_range(device: &str, offset: u64, length: u64) {
    let now = crate::timekeeping::clock_monotonic();
    let mut queue = DISCARD_QUEUE.lock();

    TOTAL_RANGES_QUEUED.fetch_add(1, Ordering::Relaxed);

    // Try to merge with an adjacent range for the same device.
    for entry in queue.iter_mut() {
        if entry.device == device {
            // Check if adjacent (contiguous).
            if entry.offset + entry.length == offset {
                // Extends forward.
                entry.length += length;
                TOTAL_COALESCED.fetch_add(1, Ordering::Relaxed);
                return;
            }
            if offset + length == entry.offset {
                // Extends backward.
                entry.offset = offset;
                entry.length += length;
                TOTAL_COALESCED.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }

    // Overflow handling: flush oldest if full.
    if queue.len() >= MAX_QUEUED_RANGES {
        QUEUE_OVERFLOWS.fetch_add(1, Ordering::Relaxed);
        // Issue oldest range immediately.
        if let Some(oldest) = queue.first().cloned() {
            issue_trim(&oldest.device, oldest.offset, oldest.length);
            queue.remove(0);
        }
    }

    queue.push(DiscardRange {
        device: String::from(device),
        offset,
        length,
        queued_ns: now,
    });
}

/// Check if periodic flush is due and perform it.
fn maybe_periodic_flush() {
    let now = crate::timekeeping::clock_monotonic();
    let last = LAST_FLUSH_NS.load(Ordering::Relaxed);
    let period = PERIOD_NS.load(Ordering::Relaxed);

    if now.saturating_sub(last) >= period {
        // Time for a flush.
        let _ = flush("");
    }
}

/// Issue a single TRIM command to the device.
///
/// In a full implementation, this would call into the block device
/// driver's discard/TRIM interface. For now we record the operation
/// and update statistics.
fn issue_trim(device: &str, offset: u64, length: u64) {
    let _ = device; // Will be used when we have block device discard API.
    TOTAL_TRIMS.fetch_add(1, Ordering::Relaxed);
    TOTAL_BYTES_TRIMMED.fetch_add(length, Ordering::Relaxed);

    // In a real implementation:
    // block_device::discard(device, offset, length)?;
    // For now, the operation is tracked in statistics.
    let _ = offset;
}

/// Coalesce ranges by device, merging adjacent/overlapping ranges.
///
/// Returns a vec of (device, sorted_ranges) where ranges are
/// non-overlapping and sorted by offset.
fn coalesce_ranges(mut ranges: Vec<DiscardRange>) -> Vec<(String, Vec<(u64, u64)>)> {
    if ranges.is_empty() {
        return Vec::new();
    }

    // Group by device.
    ranges.sort_by(|a, b| a.device.cmp(&b.device).then(a.offset.cmp(&b.offset)));

    let mut result: Vec<(String, Vec<(u64, u64)>)> = Vec::new();
    let mut current_device = String::new();
    let mut current_ranges: Vec<(u64, u64)> = Vec::new();

    for range in &ranges {
        if range.device != current_device {
            if !current_device.is_empty() && !current_ranges.is_empty() {
                let merged = merge_sorted_ranges(&current_ranges);
                result.push((current_device.clone(), merged));
            }
            current_device = range.device.clone();
            current_ranges.clear();
        }
        current_ranges.push((range.offset, range.length));
    }

    if !current_device.is_empty() && !current_ranges.is_empty() {
        let merged = merge_sorted_ranges(&current_ranges);
        result.push((current_device, merged));
    }

    result
}

/// Merge sorted (offset, length) ranges, combining overlapping/adjacent.
fn merge_sorted_ranges(ranges: &[(u64, u64)]) -> Vec<(u64, u64)> {
    if ranges.is_empty() {
        return Vec::new();
    }

    let mut merged: Vec<(u64, u64)> = Vec::new();
    let mut cur_off = ranges[0].0;
    let mut cur_end = ranges[0].0 + ranges[0].1;

    for &(off, len) in &ranges[1..] {
        let end = off + len;
        if off <= cur_end {
            // Overlapping or adjacent — extend.
            if end > cur_end {
                cur_end = end;
            }
        } else {
            // Gap — emit current and start new.
            merged.push((cur_off, cur_end - cur_off));
            cur_off = off;
            cur_end = end;
        }
    }
    merged.push((cur_off, cur_end - cur_off));

    merged
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[fstrim] Running self-test...");

    test_mode_parse();
    test_queue_and_flush();
    test_coalescing();
    test_continuous_mode();
    test_overflow();
    test_device_registration();

    serial_println!("[fstrim] Self-test passed (6 tests).");
    Ok(())
}

fn test_mode_parse() {
    assert_eq!(TrimMode::from_name("manual"), Some(TrimMode::Manual));
    assert_eq!(TrimMode::from_name("off"), Some(TrimMode::Manual));
    assert_eq!(TrimMode::from_name("periodic"), Some(TrimMode::Periodic));
    assert_eq!(TrimMode::from_name("timer"), Some(TrimMode::Periodic));
    assert_eq!(TrimMode::from_name("continuous"), Some(TrimMode::Continuous));
    assert_eq!(TrimMode::from_name("immediate"), Some(TrimMode::Continuous));
    assert_eq!(TrimMode::from_name("bogus"), None);

    serial_println!("[fstrim]   mode_parse: ok");
}

fn test_queue_and_flush() {
    // Clear any existing state.
    drop_pending();
    let before_trims = TOTAL_TRIMS.load(Ordering::Relaxed);

    // Set manual mode so ranges queue.
    set_mode(TrimMode::Manual);

    // Queue some ranges.
    notify_free("/dev/sda", 0, 8192);
    notify_free("/dev/sda", 16384, 4096);
    notify_free("/dev/sdb", 0, 65536);

    assert_eq!(pending_count(), 3);

    // Flush only sda.
    let result = flush("/dev/sda");
    assert_eq!(result.ranges_trimmed, 2);
    assert_eq!(result.bytes_trimmed, 8192 + 4096);

    // sdb still pending.
    assert_eq!(pending_count(), 1);

    // Flush all.
    let result2 = flush("");
    assert_eq!(result2.ranges_trimmed, 1);
    assert_eq!(result2.bytes_trimmed, 65536);
    assert_eq!(pending_count(), 0);

    let after_trims = TOTAL_TRIMS.load(Ordering::Relaxed);
    assert_eq!(after_trims - before_trims, 3);

    serial_println!("[fstrim]   queue_and_flush: ok");
}

fn test_coalescing() {
    drop_pending();
    set_mode(TrimMode::Manual);

    // Queue adjacent ranges that should merge during queueing.
    notify_free("/dev/sda", 0, 4096);
    notify_free("/dev/sda", 4096, 4096); // Adjacent, should coalesce.

    // Only 1 entry because they merged on queue.
    assert_eq!(pending_count(), 1);

    let result = flush("");
    assert_eq!(result.ranges_trimmed, 1);
    assert_eq!(result.bytes_trimmed, 8192); // Merged range.

    // Non-adjacent ranges stay separate.
    notify_free("/dev/sda", 0, 4096);
    notify_free("/dev/sda", 65536, 4096); // Non-adjacent.
    assert_eq!(pending_count(), 2);

    drop_pending();
    serial_println!("[fstrim]   coalescing: ok");
}

fn test_continuous_mode() {
    drop_pending();
    let before = TOTAL_TRIMS.load(Ordering::Relaxed);

    set_mode(TrimMode::Continuous);

    // In continuous mode, ranges are trimmed immediately (not queued).
    notify_free("/dev/nvme0", 0, 1048576);
    assert_eq!(pending_count(), 0); // Not queued.

    let after = TOTAL_TRIMS.load(Ordering::Relaxed);
    assert_eq!(after - before, 1);

    // Restore periodic mode.
    set_mode(TrimMode::Periodic);
    serial_println!("[fstrim]   continuous_mode: ok");
}

fn test_overflow() {
    drop_pending();
    set_mode(TrimMode::Manual);

    let before_overflows = QUEUE_OVERFLOWS.load(Ordering::Relaxed);

    // Fill queue to capacity (use non-adjacent offsets to prevent coalescing).
    for i in 0..MAX_QUEUED_RANGES {
        let offset = (i as u64) * 1048576; // 1 MiB apart, won't coalesce.
        notify_free("/dev/overflow", offset, 8192);
    }
    assert_eq!(pending_count(), MAX_QUEUED_RANGES);

    // One more should trigger overflow (flush oldest).
    notify_free("/dev/overflow", u64::MAX - 8192, 8192);

    let after_overflows = QUEUE_OVERFLOWS.load(Ordering::Relaxed);
    assert_eq!(after_overflows - before_overflows, 1);
    assert_eq!(pending_count(), MAX_QUEUED_RANGES); // Still at capacity.

    drop_pending();
    serial_println!("[fstrim]   overflow: ok");
}

fn test_device_registration() {
    register_device(DeviceTrimInfo {
        device: String::from("/dev/sda"),
        trim_supported: true,
        max_discard_bytes: 0,
        discard_granularity: 4096,
    });

    register_device(DeviceTrimInfo {
        device: String::from("/dev/hdd"),
        trim_supported: false,
        max_discard_bytes: 0,
        discard_granularity: 512,
    });

    assert!(is_trim_supported("/dev/sda"));
    assert!(!is_trim_supported("/dev/hdd"));
    assert!(!is_trim_supported("/dev/unknown"));

    // Clean up.
    DEVICE_INFO.lock().clear();
    serial_println!("[fstrim]   device_registration: ok");
}

//! Memory fragmentation history — tracks fragmentation over time.
//!
//! Records periodic fragmentation snapshots to detect gradual memory
//! degradation.  This is important because fragmentation problems are
//! often invisible until an allocation fails — by then it's too late.
//!
//! ## What is Tracked
//!
//! Each snapshot captures:
//! - Fragmentation index (0-100)
//! - Free frame count
//! - Buddy order distribution (how many blocks at each order)
//! - Timestamp (APIC ticks)
//!
//! ## Design
//!
//! - Ring buffer of 32 snapshots (last ~5 minutes at one sample per 10s).
//! - Snapshots are taken explicitly via `sample()` (called from the
//!   kswapd periodic tick or manually from kshell).
//! - Trend detection: compares newest vs oldest to report whether
//!   fragmentation is increasing, stable, or decreasing.
//!
//! ## Usage
//!
//! ```text
//! kshell> fraghist           — show fragmentation trend
//! kshell> fraghist sample    — take a snapshot now
//! kshell> fraghist detail    — show all stored snapshots
//! kshell> fraghist clear     — reset history
//! ```
//!
//! ## References
//!
//! - Linux /proc/buddyinfo — buddy allocator state
//! - Linux /sys/kernel/debug/extfrag — external fragmentation index
//! - Linux compaction daemon — triggered by fragmentation thresholds

use core::sync::atomic::{AtomicU32, Ordering};
use crate::mm::frame;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Number of snapshots to keep.
const HISTORY_SIZE: usize = 32;
const HISTORY_MASK: usize = HISTORY_SIZE - 1;

// ---------------------------------------------------------------------------
// Snapshot data
// ---------------------------------------------------------------------------

/// A single fragmentation snapshot.
#[derive(Debug, Clone, Copy)]
pub struct FragSnapshot {
    /// APIC tick at snapshot time.
    pub tick: u64,
    /// Fragmentation index (0-100, 0 = no fragmentation).
    pub frag_pct: u8,
    /// Free frames at this time.
    pub free_frames: u32,
    /// Total frames.
    pub total_frames: u32,
    /// Largest available buddy order (0 if no free blocks).
    pub max_avail_order: u8,
    /// Number of order-0 (single-frame) free blocks.
    pub order0_blocks: u16,
    /// Number of blocks at max order (most defragmented).
    pub max_order_blocks: u16,
}

impl FragSnapshot {
    pub const fn empty() -> Self {
        Self {
            tick: 0,
            frag_pct: 0,
            free_frames: 0,
            total_frames: 0,
            max_avail_order: 0,
            order0_blocks: 0,
            max_order_blocks: 0,
        }
    }

    /// Whether this slot is occupied.
    pub fn is_valid(&self) -> bool {
        self.tick != 0
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

struct HistoryRing(core::cell::UnsafeCell<[FragSnapshot; HISTORY_SIZE]>);
unsafe impl Sync for HistoryRing {}

static RING: HistoryRing = HistoryRing(core::cell::UnsafeCell::new(
    [FragSnapshot::empty(); HISTORY_SIZE]
));

/// Write position.
static WRITE_POS: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Take a fragmentation snapshot and store it in the history.
///
/// Call this periodically (e.g., every 10 seconds from kswapd tick)
/// or manually from kshell.
pub fn sample() {
    let frame_stats = frame::stats();
    let (free_frames, total_frames, order_counts) = frame_stats.map_or(
        (0u32, 0u32, [0usize; frame::BUDDY_MAX_ORDER + 1]),
        |s| (s.free_frames as u32, s.total_frames as u32, s.order_counts),
    );

    // Compute fragmentation using the same algorithm as mm::compute_fragmentation.
    let frag_pct = compute_frag(&order_counts);

    // Find highest order with available blocks.
    let mut max_avail_order: u8 = 0;
    for (order, &count) in order_counts.iter().enumerate().rev() {
        if count > 0 {
            max_avail_order = order as u8;
            break;
        }
    }

    let order0_blocks = order_counts.first().copied().unwrap_or(0).min(u16::MAX as usize) as u16;
    let max_order_blocks = order_counts.last().copied().unwrap_or(0).min(u16::MAX as usize) as u16;

    let snapshot = FragSnapshot {
        tick: crate::apic::tick_count(),
        frag_pct,
        free_frames,
        total_frames,
        max_avail_order,
        order0_blocks,
        max_order_blocks,
    };

    // Write to ring buffer.
    // SAFETY: slot is masked to HISTORY_MASK (< HISTORY_SIZE); RING is
    // only written from this function, which runs in serialized context.
    let pos = WRITE_POS.fetch_add(1, Ordering::Relaxed);
    let slot = (pos as usize) & HISTORY_MASK;
    unsafe {
        let ptr = RING.0.get() as *mut FragSnapshot;
        ptr.add(slot).write(snapshot);
    }
}

/// Get the number of snapshots taken.
#[must_use]
pub fn sample_count() -> u32 {
    WRITE_POS.load(Ordering::Relaxed)
}

/// Get the most recent N snapshots (newest first).
pub fn recent(buf: &mut [FragSnapshot]) -> usize {
    let write_pos = WRITE_POS.load(Ordering::Acquire) as usize;
    let available = write_pos.min(HISTORY_SIZE);
    let to_copy = buf.len().min(available);

    // SAFETY (group — covers all RING reads below): idx is masked to
    // HISTORY_MASK, so it's always < HISTORY_SIZE.
    for i in 0..to_copy {
        let idx = (write_pos.wrapping_sub(1).wrapping_sub(i)) & HISTORY_MASK;
        unsafe {
            let ptr = RING.0.get() as *const FragSnapshot;
            buf[i] = ptr.add(idx).read();
        }
    }

    to_copy
}

/// Get the most recent snapshot.
#[must_use]
pub fn latest() -> Option<FragSnapshot> {
    let write_pos = WRITE_POS.load(Ordering::Acquire) as usize;
    if write_pos == 0 {
        return None;
    }
    let idx = (write_pos - 1) & HISTORY_MASK;
    // SAFETY: idx is masked to HISTORY_MASK (< HISTORY_SIZE).
    let snap = unsafe {
        let ptr = RING.0.get() as *const FragSnapshot;
        ptr.add(idx).read()
    };
    if snap.is_valid() { Some(snap) } else { None }
}

/// Fragmentation trend assessment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragTrend {
    /// Fragmentation is increasing (bad).
    Increasing,
    /// Fragmentation is stable.
    Stable,
    /// Fragmentation is decreasing (good, compaction working).
    Decreasing,
    /// Not enough data to determine trend.
    Unknown,
}

impl FragTrend {
    pub fn name(self) -> &'static str {
        match self {
            Self::Increasing => "INCREASING",
            Self::Stable => "STABLE",
            Self::Decreasing => "DECREASING",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Analyze the fragmentation trend over stored history.
///
/// Compares the average of the newest quarter of samples to the
/// average of the oldest quarter.  A difference of >5 points is
/// considered a trend; otherwise stable.
#[must_use]
pub fn trend() -> FragTrend {
    let write_pos = WRITE_POS.load(Ordering::Relaxed) as usize;
    let available = write_pos.min(HISTORY_SIZE);

    if available < 4 {
        return FragTrend::Unknown;
    }

    // Compute average of oldest quarter and newest quarter.
    let quarter = available / 4;

    // SAFETY (group — covers all RING reads in both quarter loops):
    // idx is always masked to HISTORY_MASK (< HISTORY_SIZE).

    // Newest quarter (most recent `quarter` samples).
    let mut newest_sum: u32 = 0;
    for i in 0..quarter {
        let idx = (write_pos.wrapping_sub(1).wrapping_sub(i)) & HISTORY_MASK;
        let snap = unsafe {
            let ptr = RING.0.get() as *const FragSnapshot;
            ptr.add(idx).read()
        };
        newest_sum = newest_sum.saturating_add(u32::from(snap.frag_pct));
    }
    let newest_avg = newest_sum / quarter as u32;

    // Oldest quarter.
    let oldest_start = write_pos.saturating_sub(available);
    let mut oldest_sum: u32 = 0;
    for i in 0..quarter {
        let idx = (oldest_start + i) & HISTORY_MASK;
        let snap = unsafe {
            let ptr = RING.0.get() as *const FragSnapshot;
            ptr.add(idx).read()
        };
        oldest_sum = oldest_sum.saturating_add(u32::from(snap.frag_pct));
    }
    let oldest_avg = oldest_sum / quarter as u32;

    // Compare with threshold of 5 points.
    if newest_avg > oldest_avg.saturating_add(5) {
        FragTrend::Increasing
    } else if oldest_avg > newest_avg.saturating_add(5) {
        FragTrend::Decreasing
    } else {
        FragTrend::Stable
    }
}

/// Clear all history.
pub fn clear() {
    // SAFETY: i < HISTORY_SIZE; RING is only modified from serialized context.
    for i in 0..HISTORY_SIZE {
        unsafe {
            let ptr = RING.0.get() as *mut FragSnapshot;
            ptr.add(i).write(FragSnapshot::empty());
        }
    }
    WRITE_POS.store(0, Ordering::Release);
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

/// Compute fragmentation index from buddy order counts.
/// (Same algorithm as mm::compute_fragmentation but self-contained.)
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn compute_frag(order_counts: &[usize; frame::BUDDY_MAX_ORDER + 1]) -> u8 {
    let max_order = frame::BUDDY_MAX_ORDER;
    let mut total_frames: u64 = 0;
    let mut weighted_order_sum: u64 = 0;

    for (order, &count) in order_counts.iter().enumerate() {
        let frames_per_block = 1u64 << order;
        let frames = (count as u64).saturating_mul(frames_per_block);
        total_frames = total_frames.saturating_add(frames);
        weighted_order_sum = weighted_order_sum
            .saturating_add((order as u64).saturating_mul(frames));
    }

    if total_frames == 0 {
        return 0;
    }

    let avg_order_x100 = weighted_order_sum
        .saturating_mul(100)
        .checked_div(total_frames)
        .unwrap_or(0);
    let max_order_x100 = (max_order as u64).saturating_mul(100);

    let frag = 100u64.saturating_sub(
        avg_order_x100.saturating_mul(100).checked_div(max_order_x100).unwrap_or(0)
    );

    frag.min(100) as u8
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for fragmentation history.
pub fn self_test() {
    serial_println!("[frag_history] Running self-test...");

    // Test 1: Clear state.
    clear();
    assert_eq!(sample_count(), 0);
    assert!(latest().is_none());
    serial_println!("[frag_history]   Clear: OK");

    // Test 2: Take a sample.
    sample();
    assert_eq!(sample_count(), 1);
    let snap = latest().expect("should have one snapshot");
    assert!(snap.is_valid());
    assert!(snap.total_frames > 0);
    assert!(snap.free_frames > 0);
    assert!(snap.frag_pct <= 100);
    serial_println!("[frag_history]   Sample: OK (frag={}%, free={}, max_order={})",
        snap.frag_pct, snap.free_frames, snap.max_avail_order);

    // Test 3: Multiple samples.
    sample();
    sample();
    sample();
    assert_eq!(sample_count(), 4);
    serial_println!("[frag_history]   Multiple samples: OK (count=4)");

    // Test 4: Recent retrieval (newest first).
    let mut buf = [FragSnapshot::empty(); 8];
    let n = recent(&mut buf);
    assert_eq!(n, 4);
    // All should be valid with similar timestamps.
    for i in 0..n {
        assert!(buf[i].is_valid());
    }
    // Newest should have >= tick of previous.
    assert!(buf[0].tick >= buf[1].tick);
    serial_println!("[frag_history]   Recent: OK ({} snapshots)", n);

    // Test 5: Trend with only 4 samples should be Stable (identical data).
    let t = trend();
    // With identical samples, trend should be Stable or Unknown.
    assert!(t == FragTrend::Stable || t == FragTrend::Unknown,
        "expected Stable/Unknown, got {:?}", t);
    serial_println!("[frag_history]   Trend: OK ({:?})", t);

    // Test 6: Ring buffer wraps correctly.
    clear();
    for _ in 0..HISTORY_SIZE + 5 {
        sample();
    }
    assert_eq!(sample_count(), (HISTORY_SIZE + 5) as u32);
    let mut buf = [FragSnapshot::empty(); 32];
    let n = recent(&mut buf);
    assert_eq!(n, HISTORY_SIZE); // Can only get HISTORY_SIZE entries.
    serial_println!("[frag_history]   Ring wrap: OK (wrote {}, can read {})",
        HISTORY_SIZE + 5, n);

    // Cleanup.
    clear();

    serial_println!("[frag_history] Self-test PASSED");
}

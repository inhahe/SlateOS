//! Memory compaction — defragmentation of physical memory.
//!
//! Over time, physical memory becomes fragmented: many small free blocks
//! scattered between allocated pages.  This prevents allocating large
//! contiguous regions (high-order buddy allocations) even when total free
//! memory is sufficient.
//!
//! ## Current Status
//!
//! This module provides the compaction framework and statistics tracking.
//! The actual page migration requires reverse-mapping infrastructure (rmap)
//! to safely find all page table entries pointing to a given frame.  Until
//! rmap is implemented, compaction reports fragmentation metrics and serves
//! as the integration point for future compaction work.
//!
//! ## Planned Algorithm
//!
//! When fully implemented, compaction will work by scanning from both ends:
//! - A **free scanner** moves upward from low addresses, finding free frames.
//! - A **migration scanner** moves downward from high addresses, finding
//!   movable pages (single-refcount user pages).
//!
//! Movable pages are copied to free frames at lower addresses, and the
//! source frame is freed — consolidating free space into larger blocks.
//!
//! ## Integration Points
//!
//! - **Frame allocator**: calls `should_compact()` on high-order alloc failure.
//! - **kswapd**: checks fragmentation periodically, triggers compaction.
//! - **kshell**: `compact` command for manual triggering and diagnostics.
//!
//! ## References
//!
//! - Linux `mm/compaction.c` — `compact_zone()`, `isolate_migratepages()`
//! - Linux `mm/migrate.c` — `migrate_pages()`, `move_to_new_folio()`
//! - Mel Gorman, "Understanding the Linux Virtual Memory Manager" §6.3

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::mm::frame::{self, FRAME_SIZE};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Fragmentation percentage above which compaction is recommended.
const FRAGMENTATION_THRESHOLD: u8 = 50;

/// Maximum order we care about freeing up via compaction.
/// Order 2 = 64 KiB contiguous, useful for DMA and large allocations.
const TARGET_ORDER: usize = 2;

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Total compaction requests.
static REQUESTS: AtomicU64 = AtomicU64::new(0);

/// Whether a compaction analysis is currently in progress.
static RUNNING: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fragmentation analysis result.
#[derive(Debug, Clone, Copy)]
pub struct FragmentationReport {
    /// Current fragmentation percentage (0-100).
    pub fragmentation_pct: u8,
    /// Total free frames.
    pub free_frames: usize,
    /// Free frames in order-0 (single frames, can't serve higher orders).
    pub order0_free: usize,
    /// Free frames in order 1+ (can serve some higher-order requests).
    pub higher_order_free: usize,
    /// Largest contiguous free block (in frames).
    pub largest_free_block: usize,
    /// Whether compaction is recommended.
    pub compaction_recommended: bool,
    /// Estimated pages that could be migrated to improve fragmentation.
    pub estimated_movable: usize,
}

/// Analyze current memory fragmentation state.
///
/// Returns a report with fragmentation metrics and whether compaction
/// would be beneficial.
#[must_use]
pub fn analyze() -> Option<FragmentationReport> {
    let stats = frame::stats()?;

    // Calculate fragmentation: ratio of free memory locked in small blocks
    // vs total free memory.  High fragmentation = lots of order-0 free frames
    // but few high-order blocks.
    let order0_free = stats.order_counts[0];
    let mut higher_order_free = 0usize;
    let mut largest_block_frames = 0usize;

    #[allow(clippy::arithmetic_side_effects)]
    for (order, &count) in stats.order_counts.iter().enumerate().skip(1) {
        let frames_per_block = 1usize << order;
        higher_order_free = higher_order_free.saturating_add(
            count.saturating_mul(frames_per_block)
        );
        if count > 0 {
            largest_block_frames = frames_per_block;
        }
    }

    // Fragmentation = fraction of free memory in order-0 blocks.
    let total_free_in_blocks = order0_free.saturating_add(higher_order_free);
    let frag_pct = if total_free_in_blocks > 0 {
        #[allow(clippy::cast_possible_truncation)]
        let pct = (order0_free.saturating_mul(100) / total_free_in_blocks) as u8;
        pct
    } else {
        0
    };

    // Estimate movable pages: user pages with refcount=1 in the upper half
    // of physical memory.  This is a rough heuristic.
    let movable_estimate = estimate_movable_pages(&stats);

    let compaction_recommended = frag_pct > FRAGMENTATION_THRESHOLD
        && movable_estimate > 0
        && stats.free_frames > stats.total_frames / 8;

    Some(FragmentationReport {
        fragmentation_pct: frag_pct,
        free_frames: stats.free_frames,
        order0_free,
        higher_order_free,
        largest_free_block: largest_block_frames,
        compaction_recommended,
        estimated_movable: movable_estimate,
    })
}

/// Request compaction of physical memory.
///
/// Currently performs analysis only (actual page migration requires rmap).
/// Returns the fragmentation report, or None if analysis couldn't proceed.
pub fn compact() -> Option<FragmentationReport> {
    if RUNNING.swap(true, Ordering::Acquire) {
        return None;
    }

    REQUESTS.fetch_add(1, Ordering::Relaxed);

    let report = analyze();

    if let Some(ref r) = report {
        if r.compaction_recommended {
            serial_println!(
                "[compact] Fragmentation: {}% (order-0: {} frames, higher: {} frames)",
                r.fragmentation_pct, r.order0_free, r.higher_order_free
            );
            serial_println!(
                "[compact] Compaction recommended but page migration not yet implemented"
            );
            serial_println!(
                "[compact] Estimated {} movable pages, largest free block: {} frames ({} KiB)",
                r.estimated_movable, r.largest_free_block,
                r.largest_free_block.saturating_mul(FRAME_SIZE) / 1024
            );
        } else {
            serial_println!(
                "[compact] Fragmentation: {}% — compaction not needed",
                r.fragmentation_pct
            );
        }
    }

    RUNNING.store(false, Ordering::Release);
    report
}

/// Check if compaction should be triggered.
///
/// Called from the frame allocator on high-order allocation failure,
/// and periodically from kswapd.
#[must_use]
pub fn should_compact() -> bool {
    if RUNNING.load(Ordering::Relaxed) {
        return false;
    }
    // Quick check using the existing fragmentation metric.
    let Some(stats) = frame::try_stats() else {
        return false;
    };
    // Only recommend compaction if we have plenty of total free memory
    // but it's fragmented.
    let free_pct = if stats.total_frames > 0 {
        stats.free_frames * 100 / stats.total_frames
    } else {
        0
    };
    // Lots of order-0 blocks relative to total free = fragmented.
    let order0_ratio = if stats.free_frames > 0 {
        stats.order_counts[0] * 100 / stats.free_frames
    } else {
        0
    };
    free_pct > 10 && order0_ratio > usize::from(FRAGMENTATION_THRESHOLD)
}

/// Get compaction statistics.
#[must_use]
pub fn stats() -> CompactStats {
    CompactStats {
        total_requests: REQUESTS.load(Ordering::Relaxed),
        is_running: RUNNING.load(Ordering::Relaxed),
    }
}

/// Compaction subsystem statistics.
#[derive(Debug, Clone, Copy)]
pub struct CompactStats {
    /// Total compaction requests since boot.
    pub total_requests: u64,
    /// Whether compaction is currently running.
    pub is_running: bool,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Estimate how many pages in the system are movable.
///
/// Uses a heuristic: user-space pages (tracked via accounting) that have
/// refcount=1 are likely movable.  Returns a conservative estimate.
fn estimate_movable_pages(stats: &frame::FrameAllocStats) -> usize {
    // Heuristic: total allocated = total - free.
    // Of those, some fraction are user pages (movable).
    // Without rmap, we estimate based on tracked address spaces.
    let allocated = stats.total_frames.saturating_sub(stats.free_frames);
    let tracked = crate::mm::accounting::tracked_count();

    // If there are user address spaces, assume ~50% of allocated memory
    // is user-movable (conservative).  If no user spaces, nothing is movable.
    if tracked > 0 {
        allocated / 4 // Very conservative: assume 25% is movable
    } else {
        0
    }
}

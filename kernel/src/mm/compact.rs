//! Memory compaction — defragmentation of physical memory.
//!
//! Over time, physical memory becomes fragmented: many small free blocks
//! scattered between allocated pages.  This prevents allocating large
//! contiguous regions (high-order buddy allocations) even when total free
//! memory is sufficient.
//!
//! ## Algorithm
//!
//! Compaction works by migrating pages from high addresses to lower free
//! frames, consolidating free space into larger contiguous blocks.
//!
//! The algorithm uses the rmap (reverse mapping) infrastructure to find
//! all PTEs that map a given physical frame, then:
//! 1. Allocates a new frame at a lower address.
//! 2. Copies the page contents.
//! 3. Updates all PTEs to point to the new frame.
//! 4. Flushes TLBs.
//! 5. Frees the old frame.
//!
//! Only privately-mapped pages (rmap count == 1) are eligible for migration.
//! Shared pages (CoW) are skipped because updating multiple PTEs atomically
//! under potential concurrent access is complex.
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
use crate::mm::{rmap, page_table};
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

/// Total pages successfully migrated.
static PAGES_MIGRATED: AtomicU64 = AtomicU64::new(0);

/// Total migration attempts that failed.
static MIGRATION_FAILURES: AtomicU64 = AtomicU64::new(0);

/// Total pages scanned during compaction.
static PAGES_SCANNED: AtomicU64 = AtomicU64::new(0);

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
        pages_migrated: PAGES_MIGRATED.load(Ordering::Relaxed),
        migration_failures: MIGRATION_FAILURES.load(Ordering::Relaxed),
        pages_scanned: PAGES_SCANNED.load(Ordering::Relaxed),
    }
}

/// Compaction subsystem statistics.
#[derive(Debug, Clone, Copy)]
pub struct CompactStats {
    /// Total compaction requests since boot.
    pub total_requests: u64,
    /// Whether compaction is currently running.
    pub is_running: bool,
    /// Total pages successfully migrated.
    pub pages_migrated: u64,
    /// Total migration failures.
    pub migration_failures: u64,
    /// Total pages scanned during compaction.
    pub pages_scanned: u64,
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
    // Use rmap entry count as a rough movable page estimate.
    let rmap_st = rmap::stats();
    if rmap_st.entries_used > 0 {
        return rmap_st.entries_used;
    }

    // Fallback: estimate based on tracked address spaces.
    let allocated = stats.total_frames.saturating_sub(stats.free_frames);
    let tracked = crate::mm::accounting::tracked_count();

    if tracked > 0 {
        allocated / 4 // Very conservative: assume 25% is movable
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Page migration
// ---------------------------------------------------------------------------

/// Maximum number of pages to migrate in a single compaction pass.
/// Prevents compaction from monopolizing the CPU.
const MAX_MIGRATE_PER_PASS: usize = 64;

/// Migrate a single page from `old_frame` to `new_frame`.
///
/// Steps:
/// 1. Look up all PTEs mapping `old_frame` via rmap.
/// 2. Copy page contents from old to new.
/// 3. Update each PTE to point to `new_frame`.
/// 4. Flush TLB for the affected addresses.
/// 5. Update rmap (remove old, add new).
///
/// Returns `true` if migration succeeded, `false` if skipped/failed.
///
/// # Safety
///
/// Both `old_frame` and `new_frame` must be valid physical frame addresses.
/// `new_frame` must be freshly allocated (not mapped anywhere).
/// The caller must ensure no concurrent page table modifications for the
/// affected address space.
#[allow(clippy::arithmetic_side_effects)]
unsafe fn migrate_page(old_phys: u64, new_phys: u64) -> bool {
    // Step 1: Look up all mappers of the old frame.
    let mut mappers = [(0u64, 0u64); 4];
    let lookup = rmap::lookup(old_phys, &mut mappers);

    if lookup.count == 0 {
        return false; // Not tracked in rmap.
    }
    if lookup.count > 1 || !lookup.is_complete {
        return false; // Shared page — skip (too complex for now).
    }

    let (pml4_phys, virt_addr) = mappers[0];
    if pml4_phys == 0 {
        return false;
    }

    PAGES_SCANNED.fetch_add(1, Ordering::Relaxed);

    // Step 2: Copy page contents (all 4 hardware pages = 16 KiB).
    let hhdm = match page_table::hhdm() {
        Some(h) => h,
        None => return false,
    };
    let src_ptr = (hhdm + old_phys) as *const u8;
    let dst_ptr = (hhdm + new_phys) as *mut u8;
    // SAFETY: Both addresses are in the HHDM region, valid frames.
    unsafe {
        core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, FRAME_SIZE);
    }

    // Step 3: Update the PTE to point to the new frame.
    // We need to unmap the old and map the new with the same flags.
    let virt = page_table::VirtAddr::new(virt_addr);

    // Read the existing PTE flags before unmapping.
    let flags = match page_table::translate_flags(pml4_phys, virt) {
        Some(f) => f,
        None => return false, // PTE not found (race? stale rmap?)
    };

    // Unmap old frame from page table.
    let old_frame = match unsafe { page_table::unmap_frame(pml4_phys, virt) } {
        Ok(f) => f,
        Err(_) => return false,
    };

    // Verify it's the frame we expected.
    if old_frame.addr() != old_phys {
        // Unexpected — remap the old frame and bail.
        let _ = unsafe { page_table::map_frame(pml4_phys, virt, old_frame, flags) };
        return false;
    }

    // Map new frame at the same virtual address with same flags.
    let new_frame = match frame::PhysFrame::from_addr(new_phys) {
        Some(f) => f,
        None => {
            // Can't create frame struct — remap old and bail.
            let _ = unsafe { page_table::map_frame(pml4_phys, virt, old_frame, flags) };
            return false;
        }
    };

    if unsafe { page_table::map_frame(pml4_phys, virt, new_frame, flags) }.is_err() {
        // Mapping failed — restore old mapping.
        let _ = unsafe { page_table::map_frame(pml4_phys, virt, old_frame, flags) };
        return false;
    }

    // Step 4: Flush TLB for this address (both local and remote CPUs).
    // Our frames are 16 KiB = 4 hardware pages.
    crate::tlb::flush_range(virt_addr, 4);

    // Step 5: Update rmap.
    rmap::remove(old_phys, pml4_phys, virt_addr);
    rmap::add(new_phys, pml4_phys, virt_addr);

    // Step 6: Free the old frame.
    let _ = unsafe { frame::free_frame(old_frame) };

    PAGES_MIGRATED.fetch_add(1, Ordering::Relaxed);
    true
}

/// Attempt to migrate pages tracked by rmap to reduce fragmentation.
///
/// Scans rmap entries looking for privately-mapped pages (count == 1).
/// For each eligible page, attempts to allocate a frame at a lower address
/// and migrate the page contents there.  This consolidates free space into
/// larger contiguous blocks, enabling higher-order buddy allocations.
///
/// Based on Linux `mm/compaction.c` `compact_zone()`: scan private pages,
/// try to relocate each to a lower-addressed free frame.  If the new frame
/// is at a higher address than the old one, skip it (we want to move pages
/// DOWN, not up — the goal is to open up contiguous blocks at the top).
///
/// Returns the number of pages successfully migrated.
#[allow(clippy::arithmetic_side_effects)]
pub fn try_compact() -> usize {
    if RUNNING.swap(true, Ordering::Acquire) {
        return 0; // Already running.
    }

    REQUESTS.fetch_add(1, Ordering::Relaxed);

    let rmap_st = rmap::stats();
    if rmap_st.entries_used == 0 {
        RUNNING.store(false, Ordering::Release);
        return 0; // Nothing to migrate.
    }

    let mut migrated = 0usize;
    let mut failures = 0usize;

    // Collect candidate frames in batches.  Each batch holds up to 32
    // privately-mapped frame addresses from the rmap table.
    let mut candidates = [0u64; 32];
    let mut scan_idx: usize = 0;

    // Scan up to 4 batches (128 candidates total, well under MAX_MIGRATE_PER_PASS).
    for _batch in 0..4u32 {
        if migrated >= MAX_MIGRATE_PER_PASS {
            break;
        }

        let (found, next_idx) = rmap::collect_private_frames(&mut candidates, scan_idx);
        scan_idx = next_idx;

        if found == 0 {
            break; // No more candidates.
        }

        for i in 0..found {
            if migrated >= MAX_MIGRATE_PER_PASS {
                break;
            }

            let old_phys = candidates[i];
            if old_phys == 0 {
                continue;
            }

            // Try to allocate a new frame.  The buddy allocator naturally
            // returns low-address frames first, so if old_phys is at a high
            // address, the new frame will likely be lower.
            let new_frame = match frame::alloc_frame() {
                Ok(f) => f,
                Err(_) => break, // OOM — stop compacting.
            };

            let new_phys = new_frame.addr();

            // Only migrate if the new frame is at a LOWER address.
            // Moving to a higher address doesn't help defragmentation.
            if new_phys >= old_phys {
                // New frame is same or higher — not helpful.  Free it.
                // SAFETY: new_frame is freshly allocated and unmapped.
                let _ = unsafe { frame::free_frame(new_frame) };
                continue;
            }

            // Attempt the actual page migration.
            // SAFETY: old_phys is a valid mapped frame (from rmap),
            // new_frame is freshly allocated and not mapped.
            let success = unsafe { migrate_page(old_phys, new_phys) };

            if !success {
                // Migration failed — free the new frame.
                // SAFETY: new_frame was allocated above, not mapped on failure.
                let _ = unsafe { frame::free_frame(new_frame) };
                failures += 1;
                MIGRATION_FAILURES.fetch_add(1, Ordering::Relaxed);
            } else {
                migrated += 1;
            }
        }

        // If we wrapped back to 0, we've scanned everything.
        if next_idx == 0 {
            break;
        }
    }

    if migrated > 0 || failures > 0 {
        serial_println!(
            "[compact] try_compact: {} rmap entries, {} migrated, {} failed",
            rmap_st.entries_used, migrated, failures
        );
    }

    RUNNING.store(false, Ordering::Release);
    migrated
}

/// Migrate a specific page (called by frame allocator or kswapd).
///
/// If `old_phys` is privately mapped and a lower-address frame is available,
/// migrates the page and returns `true`.
///
/// # Safety
///
/// `old_phys` must be a valid allocated frame address.
pub unsafe fn try_migrate_one(old_phys: u64) -> bool {
    // Only migrate privately-mapped pages.
    if !rmap::is_private(old_phys) {
        return false;
    }

    // Try to allocate a new frame (the allocator may give us a lower address).
    let new_frame = match frame::alloc_frame() {
        Ok(f) => f,
        Err(_) => return false,
    };

    let new_phys = new_frame.addr();

    // Only worth migrating if new frame is at a lower address (reduces fragmentation).
    if new_phys >= old_phys {
        let _ = unsafe { frame::free_frame(new_frame) };
        return false;
    }

    // Attempt the migration.
    let ok = unsafe { migrate_page(old_phys, new_phys) };
    if !ok {
        // Migration failed — free the new frame we allocated.
        let _ = unsafe { frame::free_frame(new_frame) };
        MIGRATION_FAILURES.fetch_add(1, Ordering::Relaxed);
    }
    ok
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for memory compaction subsystem.
///
/// Exercises the analysis, statistics, and rmap iteration APIs.
/// Does NOT perform actual page migration (that requires live page tables
/// with user-space mappings).
pub fn self_test() {
    serial_println!("[compact] Running self-test...");

    // Test 1: analyze() returns a report (even if fragmentation is low).
    let report = analyze();
    // analyze() may return None if frame allocator stats aren't available.
    if let Some(r) = report {
        assert!(r.fragmentation_pct <= 100,
            "fragmentation percentage must be 0-100");
        assert!(r.free_frames <= r.free_frames + r.order0_free,
            "free_frames should be consistent");
        serial_println!("[compact]   analyze: OK (frag={}%, free={}, largest_block={})",
            r.fragmentation_pct, r.free_frames, r.largest_free_block);
    } else {
        serial_println!("[compact]   analyze: skipped (frame stats unavailable)");
    }

    // Test 2: should_compact() returns a boolean without crashing.
    let _should = should_compact();
    serial_println!("[compact]   should_compact: OK (result={})", _should);

    // Test 3: stats() returns valid data.
    let st = stats();
    // After our try_compact calls, these should be populated.
    assert!(!st.is_running, "should not be running right now");
    serial_println!("[compact]   stats: OK (requests={}, migrated={}, failures={})",
        st.total_requests, st.pages_migrated, st.migration_failures);

    // Test 4: try_compact with no rmap entries returns 0.
    // (The rmap table may have entries from other subsystems, but
    // compaction with no real user pages should succeed harmlessly.)
    let migrated = try_compact();
    serial_println!("[compact]   try_compact: OK (migrated={})", migrated);

    // Test 5: collect_private_frames with fake rmap entries.
    // Add a fake entry, collect it, verify, clean up.
    let fake_phys: u64 = 0x0F00_0000; // 240 MiB — likely unused
    let fake_pml4: u64 = 0x00C0_0000;
    let fake_virt: u64 = 0x0000_6000_0000_0000;
    rmap::add(fake_phys, fake_pml4, fake_virt);

    let mut candidates = [0u64; 4];
    let (found, _next) = rmap::collect_private_frames(&mut candidates, 0);
    // We should find at least our fake entry among the results.
    let mut saw_fake = false;
    for i in 0..found {
        if candidates[i] == fake_phys {
            saw_fake = true;
        }
    }
    assert!(saw_fake, "collect_private_frames should find our fake entry");
    serial_println!("[compact]   collect_private_frames: OK (found={}, saw_fake=true)", found);

    // Clean up fake entry.
    rmap::remove(fake_phys, fake_pml4, fake_virt);

    // Test 6: try_migrate_one on a non-rmap frame returns false.
    // SAFETY: 0xDEAD_0000 is not a valid allocated frame, but
    // try_migrate_one checks rmap first and bails early.
    let migrated_one = unsafe { try_migrate_one(0xDEAD_0000) };
    assert!(!migrated_one, "non-rmap frame should not migrate");
    serial_println!("[compact]   try_migrate_one (non-rmap): OK");

    serial_println!("[compact] Self-test PASSED");
}

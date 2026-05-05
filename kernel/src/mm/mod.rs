//! Memory management subsystem.
//!
//! This module will contain:
//! - Physical frame allocator (buddy allocator with 16 KiB base pages)
//! - Virtual memory manager (page table operations)
//! - Kernel heap allocator (geometric size class, per-CPU caches)
//! - Demand paging and stack growth
//! - Swap support
//!
//! ## Design --- 16 KiB Pages on `x86_64`
//!
//! `x86_64` hardware only supports 4 KiB, 2 MiB, and 1 GiB page sizes.
//! Our design uses 16 KiB as the *allocator* base unit: every physical
//! frame allocation hands out 4 contiguous 4 KiB hardware pages (16 KiB
//! total).  Page table entries still point to 4 KiB pages, but they are
//! always allocated and freed in groups of 4.
//!
//! Benefits of 16 KiB base pages (matching ARM64's 16 KiB mode):
//! - Fewer TLB misses for sequential access patterns
//! - Simpler buddy allocator (fewer levels, less metadata)
//! - Better alignment for DMA and I/O buffers
//! - Reduced page table management overhead
//!
//! The downside is slightly higher memory waste for small allocations
//! (internal fragmentation within a 16 KiB page).  The slab allocator
//! for kernel heap objects mitigates this for small objects.

pub mod accounting;
pub mod compress;
pub mod cow;
pub mod dma;
pub mod fault;
pub mod frame;
pub mod heap;
pub mod kstack;
pub mod kswapd;
pub mod oom;
pub mod page_table;
pub mod pressure;
pub mod protect;
pub mod rlimits;
pub mod swap;
pub mod user;
pub mod vma;

// ---------------------------------------------------------------------------
// Unified memory information (kernel equivalent of /proc/meminfo)
// ---------------------------------------------------------------------------

/// Comprehensive snapshot of kernel memory state.
///
/// Aggregates information from the physical frame allocator, kernel
/// heap, swap subsystem, and zero-page pool into a single struct.
/// Used by the `mem` kshell command, future sysinfo syscall, and
/// process explorer.
///
/// All sizes are in bytes unless noted otherwise.
#[derive(Debug, Clone)]
pub struct MemoryInfo {
    // --- Physical memory ---
    /// Total managed physical memory (frames × 16 KiB).
    pub total_bytes: usize,
    /// Free physical memory (unallocated frames × 16 KiB).
    pub free_bytes: usize,
    /// Used physical memory (total − free).
    pub used_bytes: usize,
    /// Total managed frames.
    pub total_frames: usize,
    /// Free frames.
    pub free_frames: usize,

    // --- Buddy allocator fragmentation ---
    /// Free blocks per buddy order (order 0..=10).
    ///
    /// `order_counts[i]` = number of contiguous 2^i frame blocks on the
    /// free list.  A healthy allocator has blocks at high orders; many
    /// small blocks at order 0 with few at high orders indicates
    /// external fragmentation.
    pub order_counts: [usize; frame::BUDDY_MAX_ORDER + 1],
    /// Fragmentation index (0–100).
    ///
    /// 0 = all free memory in a single max-order block (no fragmentation).
    /// 100 = all free memory in order-0 blocks (maximum fragmentation).
    ///
    /// Calculated as: `100 − (weighted_avg_order / max_order × 100)`.
    /// where the weight of each block is its frame count.
    pub fragmentation_pct: u8,

    // --- Per-CPU frame cache ---
    /// Allocations served from per-CPU cache (no global lock).
    pub pcpu_cache_hits: u64,
    /// Allocations that missed per-CPU cache (needed global lock).
    pub pcpu_cache_misses: u64,
    /// Batch refill operations (global → per-CPU).
    pub pcpu_refill_ops: u64,
    /// Batch drain operations (per-CPU → global).
    pub pcpu_drain_ops: u64,

    // --- Zero-page pool ---
    /// Pre-zeroed frames currently in the pool.
    pub zero_pool_count: usize,
    /// Total pool hits since boot.
    pub zero_pool_hits: u64,
    /// Total pool misses since boot.
    pub zero_pool_misses: u64,

    // --- Kernel heap ---
    /// Total slab (small) allocations since boot.
    pub heap_slab_allocs: u64,
    /// Total slab (small) deallocations since boot.
    pub heap_slab_frees: u64,
    /// Total large allocations since boot.
    pub heap_large_allocs: u64,
    /// Total failed allocations since boot.
    pub heap_alloc_failures: u64,

    // --- Swap ---
    /// Total swap capacity (bytes).
    pub swap_total_bytes: usize,
    /// Used swap (bytes).
    pub swap_used_bytes: usize,
    /// Number of swap devices.
    pub swap_device_count: usize,

    // --- kswapd (background reclaimer) ---
    /// Whether the background reclaimer is running.
    pub kswapd_running: bool,
    /// Number of reclaim cycles completed since boot.
    pub kswapd_reclaim_cycles: u64,
    /// Total pages reclaimed by kswapd since boot.
    pub kswapd_total_reclaimed: u64,

    // --- OOM handler ---
    /// Number of OOM events since boot.
    pub oom_events: u64,
    /// Number of processes killed by OOM since boot.
    pub oom_kills: u64,

    // --- Per-process accounting ---
    /// Number of user-mode address spaces currently tracked.
    pub tracked_address_spaces: usize,
}

impl core::fmt::Display for MemoryInfo {
    /// Format as a multi-line summary similar to `/proc/meminfo`.
    ///
    /// Output suitable for serial console and kshell `mem` command.
    #[allow(clippy::arithmetic_side_effects)]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let total_mb = self.total_bytes / (1024 * 1024);
        let used_mb = self.used_bytes / (1024 * 1024);
        let free_mb = self.free_bytes / (1024 * 1024);
        let swap_total_mb = self.swap_total_bytes / (1024 * 1024);
        let swap_used_mb = self.swap_used_bytes / (1024 * 1024);

        writeln!(f, "Physical:  {} MiB total, {} MiB used, {} MiB free ({} frames)",
            total_mb, used_mb, free_mb, self.free_frames)?;

        // Buddy allocator fragmentation.
        write!(f, "Buddy:     frag={}%  orders=[", self.fragmentation_pct)?;
        for (i, &count) in self.order_counts.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{}", count)?;
        }
        writeln!(f, "]")?;

        // Per-CPU frame cache efficiency.
        let total_allocs = self.pcpu_cache_hits.saturating_add(self.pcpu_cache_misses);
        let hit_pct = if total_allocs > 0 {
            self.pcpu_cache_hits.saturating_mul(100) / total_allocs
        } else {
            0
        };
        writeln!(f, "PCPU:      hit={}% ({}/{})  refills={}  drains={}",
            hit_pct, self.pcpu_cache_hits, total_allocs,
            self.pcpu_refill_ops, self.pcpu_drain_ops)?;

        writeln!(f, "Zero pool: {} frames (hits: {}, misses: {})",
            self.zero_pool_count, self.zero_pool_hits, self.zero_pool_misses)?;
        writeln!(f, "Heap:      slab={}/{}  large={}  failures={}",
            self.heap_slab_allocs, self.heap_slab_frees,
            self.heap_large_allocs, self.heap_alloc_failures)?;
        writeln!(f, "Swap:      {} MiB / {} MiB ({} device{})",
            swap_used_mb, swap_total_mb,
            self.swap_device_count,
            if self.swap_device_count == 1 { "" } else { "s" })?;
        writeln!(f, "kswapd:    {} (cycles: {}, reclaimed: {} pages)",
            if self.kswapd_running { "running" } else { "stopped" },
            self.kswapd_reclaim_cycles, self.kswapd_total_reclaimed)?;
        writeln!(f, "OOM:       {} events, {} kills",
            self.oom_events, self.oom_kills)?;
        write!(f, "Tracking:  {} user address space{}",
            self.tracked_address_spaces,
            if self.tracked_address_spaces == 1 { "" } else { "s" })
    }
}

/// Collect a snapshot of the current kernel memory state.
///
/// This is a lightweight operation (no heap allocation, a few lock
/// acquisitions for counters).  Safe to call from any context that
/// can take spinlocks (not ISR context).
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
pub fn memory_info() -> MemoryInfo {
    // Physical frame allocator.
    let (total_frames, free_frames, free_bytes) =
        frame::stats().map_or((0, 0, 0), |s| {
            (s.total_frames, s.free_frames, s.free_bytes)
        });
    let total_bytes = total_frames * frame::FRAME_SIZE;
    let used_bytes = total_bytes.saturating_sub(free_bytes);

    // Zero-page pool.
    let zero_pool_count = frame::zero_pool_count();
    let (zero_pool_hits, zero_pool_misses) = frame::zero_pool_stats();

    // Kernel heap.
    let hs = heap::stats();

    // Swap.
    let (swap_total, swap_used, swap_devices) = swap::summary();

    // kswapd (background reclaimer).
    let kswapd_running = kswapd::is_running();
    let kswapd_reclaim_cycles = kswapd::reclaim_cycles();
    let kswapd_total_reclaimed = kswapd::total_reclaimed();

    // Buddy order distribution and fragmentation index.
    let order_counts = frame::stats().map_or(
        [0usize; frame::BUDDY_MAX_ORDER + 1],
        |s| s.order_counts,
    );
    let fragmentation_pct = compute_fragmentation(&order_counts);

    // Per-CPU frame cache diagnostics.
    let pcpu = frame::pcpu_cache_stats();

    MemoryInfo {
        total_bytes,
        free_bytes,
        used_bytes,
        total_frames,
        free_frames,
        order_counts,
        fragmentation_pct,
        pcpu_cache_hits: pcpu.cache_hits,
        pcpu_cache_misses: pcpu.cache_misses,
        pcpu_refill_ops: pcpu.refill_ops,
        pcpu_drain_ops: pcpu.drain_ops,
        zero_pool_count,
        zero_pool_hits,
        zero_pool_misses,
        heap_slab_allocs: hs.slab_allocs,
        heap_slab_frees: hs.slab_frees,
        heap_large_allocs: hs.large_allocs,
        heap_alloc_failures: hs.alloc_failures,
        swap_total_bytes: swap_total,
        swap_used_bytes: swap_used,
        swap_device_count: swap_devices,
        kswapd_running,
        kswapd_reclaim_cycles,
        kswapd_total_reclaimed,
        oom_events: oom::oom_event_count(),
        oom_kills: oom::oom_kill_count(),
        tracked_address_spaces: accounting::tracked_count(),
    }
}

/// Compute a fragmentation index (0–100) from buddy order counts.
///
/// The index is the complement of the weighted average order:
///   `100 − (weighted_avg_order / max_order × 100)`
///
/// Each block of order *i* contributes `2^i` frames as its weight,
/// so the average naturally reflects where the bulk of free memory
/// lives in the order spectrum.
///
/// - 0: all free memory in max-order blocks (no fragmentation).
/// - 100: all free memory in order-0 blocks (maximum fragmentation).
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn compute_fragmentation(order_counts: &[usize; frame::BUDDY_MAX_ORDER + 1]) -> u8 {
    let max_order = frame::BUDDY_MAX_ORDER;
    let mut total_frames: u64 = 0;
    let mut weighted_order_sum: u64 = 0;

    for (order, &count) in order_counts.iter().enumerate() {
        let frames_per_block = 1u64 << order;
        let frames = (count as u64).saturating_mul(frames_per_block);
        total_frames = total_frames.saturating_add(frames);
        // Weight: order × frames-in-this-order.
        weighted_order_sum = weighted_order_sum
            .saturating_add((order as u64).saturating_mul(frames));
    }

    if total_frames == 0 {
        return 0; // No free memory — nothing to fragment.
    }

    // weighted_avg_order = weighted_order_sum / total_frames  (scaled ×100).
    let avg_order_x100 = weighted_order_sum
        .saturating_mul(100)
        .checked_div(total_frames)
        .unwrap_or(0);
    let max_order_x100 = (max_order as u64).saturating_mul(100);

    // Complement: 100 when avg=0, 0 when avg=max_order.
    let frag = 100u64.saturating_sub(
        avg_order_x100.saturating_mul(100).checked_div(max_order_x100).unwrap_or(0)
    );

    // Clamp to u8 (always in 0..=100).
    frag.min(100) as u8
}

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

pub mod compress;
pub mod cow;
pub mod dma;
pub mod fault;
pub mod frame;
pub mod heap;
pub mod kswapd;
pub mod oom;
pub mod page_table;
pub mod protect;
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
        write!(f, "OOM:       {} events, {} kills",
            self.oom_events, self.oom_kills)
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

    MemoryInfo {
        total_bytes,
        free_bytes,
        used_bytes,
        total_frames,
        free_frames,
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
    }
}

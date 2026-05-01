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
pub mod page_table;
pub mod protect;
pub mod swap;
pub mod user;
pub mod vma;

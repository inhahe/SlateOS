//! Physical frame allocator.
//!
//! Allocates and frees physical memory in 16 KiB frames (4 contiguous
//! 4 KiB hardware pages).  Uses a buddy allocator for efficient
//! splitting and coalescing.
//!
//! ## Initialization
//!
//! The allocator is initialized from the Limine memory map.  Usable
//! regions are added to the free lists; everything else is reserved.
//!
//! ## Per-CPU Free Lists
//!
//! To avoid cross-CPU atomic contention on the hot path, each CPU
//! maintains a small local free list.  Allocations pull from the local
//! list first; when it's empty, a batch is refilled from the global
//! allocator.  Frees push to the local list; when it overflows, a
//! batch is returned to the global allocator.
//!
//! ## Performance Target
//!
//! Single alloc/free: < 1us (Linux buddy: 100-500ns).

/// Size of a single physical frame (our base allocation unit).
pub const FRAME_SIZE: usize = 16 * 1024; // 16 KiB

/// Number of 4 KiB hardware pages per frame.
pub const PAGES_PER_FRAME: usize = FRAME_SIZE / 4096;

/// A physical frame address (always 16 KiB aligned).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysFrame(u64);

impl PhysFrame {
    /// Create a `PhysFrame` from a raw physical address.
    ///
    /// Returns `None` if the address is not aligned to `FRAME_SIZE`.
    #[must_use]
    pub const fn from_addr(addr: u64) -> Option<Self> {
        if addr % FRAME_SIZE as u64 != 0 {
            None
        } else {
            Some(Self(addr))
        }
    }

    /// The raw physical address of this frame.
    #[must_use]
    pub const fn addr(self) -> u64 {
        self.0
    }

    /// Convert to a virtual address using the HHDM offset.
    #[must_use]
    pub const fn to_virt(self, hhdm_offset: u64) -> u64 {
        self.0 + hhdm_offset
    }
}

// TODO: Implement buddy allocator.
// - Initialize from Limine memory map (usable regions only)
// - Split and coalesce in powers of 2 (base unit = 16 KiB)
// - Per-CPU free list caches
// - Thread-safe global allocator behind a spinlock
// - Benchmark against baselines.toml targets

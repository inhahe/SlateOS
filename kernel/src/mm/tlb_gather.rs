//! TLB flush batching (mmu_gather) — defer TLB shootdown and frame free.
//!
//! When unmapping multiple pages (process exit, large munmap, CoW breakage),
//! issuing a separate TLB shootdown IPI for each page is extremely expensive.
//! This module collects unmap operations into a batch, issues a single TLB
//! shootdown for the accumulated range, and only then frees physical frames.
//!
//! ## Why Deferred Free Matters
//!
//! Without deferred free, the following race exists on SMP:
//!
//! 1. CPU A unmaps page P (PTE cleared), frees physical frame F.
//! 2. CPU B still has a stale TLB entry for P → F.
//! 3. Frame F is reallocated for something else.
//! 4. CPU B accesses P through the stale TLB entry → reads/writes the
//!    wrong data (silent corruption).
//!
//! By deferring the frame free until *after* all CPUs have invalidated
//! their TLBs, this race is eliminated.
//!
//! ## Design
//!
//! `TlbGather` is a stack-allocated structure (no heap needed) with an
//! inline buffer for up to 256 physical frames.  The caller:
//!
//! 1. Creates a `TlbGather` on the stack.
//! 2. Unmaps pages and calls `gather.add(virt_addr, phys_addr)` for each.
//! 3. Calls `gather.finish()` which:
//!    a. Issues a single TLB shootdown for the entire virtual range.
//!    b. Frees all collected physical frames.
//!
//! If the buffer fills up before `finish()` is called, the gather performs
//! a partial flush (shootdown + free of current batch), then resets for
//! more entries.  This bounds memory usage while still batching.
//!
//! ## Performance
//!
//! - Without batching: N pages × (1 IPI + spin-wait) = O(N × IPI_latency)
//! - With batching: 1 IPI + spin-wait + N × invlpg on each CPU = O(IPI_latency + N × invlpg)
//!
//! Since IPI send+wait is ~1-5 µs and invlpg is ~10-100 ns, this is a
//! massive win for N > 1.
//!
//! ## References
//!
//! - Linux `include/asm-generic/tlb.h` — struct mmu_gather
//! - Linux `mm/mmu_gather.c` — tlb_gather_mmu(), tlb_finish_mmu()
//! - Intel SDM Vol. 3A §4.10.4 — "Invalidation of TLBs"

use core::sync::atomic::{AtomicU64, Ordering};
use crate::serial_println;
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
use crate::tlb;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum frames buffered before a partial flush is forced.
///
/// 256 × 8 bytes = 2 KiB of stack space — well within kernel stack budget.
/// Chosen to amortize IPI cost while keeping stack usage bounded.
const MAX_BATCH: usize = 256;

/// Threshold (in 4 KiB pages) above which we issue a full TLB flush
/// instead of individual invlpg for each page.
///
/// On modern x86, if the flush range exceeds this many pages, a full
/// CR3 reload is cheaper than N × invlpg.  Linux uses a similar heuristic
/// (typically 33 pages, but we use a higher threshold since our base page
/// is 16 KiB = 4 hardware pages).
const FULL_FLUSH_THRESHOLD: u32 = 128;

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Total gather operations completed (each finish() call).
static GATHER_OPS: AtomicU64 = AtomicU64::new(0);

/// Total pages freed via gather (deferred free path).
static GATHER_PAGES_FREED: AtomicU64 = AtomicU64::new(0);

/// Total partial flushes (buffer full before finish).
static PARTIAL_FLUSHES: AtomicU64 = AtomicU64::new(0);

/// Total times full flush was chosen over range flush.
static FULL_FLUSH_CHOSEN: AtomicU64 = AtomicU64::new(0);

/// Total `free_frame` failures during gather finish (frame leak — indicates
/// an invariant violation upstream, but we can't panic in the shootdown path).
static FREE_FAILURES: AtomicU64 = AtomicU64::new(0);

/// Statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct GatherStats {
    /// Number of gather operations completed.
    pub gather_ops: u64,
    /// Total pages freed via deferred path.
    pub pages_freed: u64,
    /// Number of partial flushes (buffer overflow mid-gather).
    pub partial_flushes: u64,
    /// Number of times full flush was used instead of range flush.
    pub full_flush_chosen: u64,
    /// Number of `free_frame` failures (leaked frames).
    #[allow(dead_code)] // surfaced via stats() for diagnostics; not yet read from kshell
    pub free_failures: u64,
}

/// Get current gather statistics.
#[must_use]
pub fn stats() -> GatherStats {
    GatherStats {
        gather_ops: GATHER_OPS.load(Ordering::Relaxed),
        pages_freed: GATHER_PAGES_FREED.load(Ordering::Relaxed),
        partial_flushes: PARTIAL_FLUSHES.load(Ordering::Relaxed),
        full_flush_chosen: FULL_FLUSH_CHOSEN.load(Ordering::Relaxed),
        free_failures: FREE_FAILURES.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// TlbGather
// ---------------------------------------------------------------------------

/// A batch of pages to flush and free.
///
/// Stack-allocated, no heap dependency.  Create with `TlbGather::new()`,
/// add entries with `add()`, finalize with `finish()`.
///
/// If the gather is dropped without calling `finish()`, the destructor
/// will perform the flush+free to prevent resource leaks.
pub struct TlbGather {
    /// Physical addresses of frames to free after the TLB flush.
    frames: [u64; MAX_BATCH],
    /// Number of entries currently in the buffer.
    count: usize,
    /// Lowest virtual address seen (inclusive).
    vaddr_lo: u64,
    /// Highest virtual address seen (exclusive, rounded up to page).
    vaddr_hi: u64,
    /// Whether we need a full flush (range grew too large).
    need_full_flush: bool,
    /// Whether any entries have been added since creation/last partial flush.
    dirty: bool,
}

impl TlbGather {
    /// Create a new empty gather context.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            frames: [0; MAX_BATCH],
            count: 0,
            vaddr_lo: u64::MAX,
            vaddr_hi: 0,
            need_full_flush: false,
            dirty: false,
        }
    }

    /// Add a page to the gather.
    ///
    /// `virt_addr` is the virtual address that was unmapped.
    /// `phys_addr` is the physical frame to free after the TLB flush.
    ///
    /// The caller must have already cleared the PTE before calling this.
    /// The frame will NOT be freed until `finish()` is called.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn add(&mut self, virt_addr: u64, phys_addr: u64) {
        // Track the virtual address range for the flush.
        if virt_addr < self.vaddr_lo {
            self.vaddr_lo = virt_addr;
        }
        // Our frames are 16 KiB = 4 × 4 KiB hardware pages.
        let vaddr_end = virt_addr.saturating_add(FRAME_SIZE as u64);
        if vaddr_end > self.vaddr_hi {
            self.vaddr_hi = vaddr_end;
        }

        // Buffer the physical frame for deferred free.
        if self.count < MAX_BATCH {
            self.frames[self.count] = phys_addr;
            self.count += 1;
        } else {
            // Buffer full — force a partial flush, then add the new entry.
            self.flush_and_free();
            PARTIAL_FLUSHES.fetch_add(1, Ordering::Relaxed);
            self.frames[0] = phys_addr;
            self.count = 1;
            // Reset range tracking for the new batch; keep vaddr bounds
            // since we might need them for the overall flush decision.
        }

        self.dirty = true;

        // If the virtual range is getting very large, switch to full flush mode.
        let range_pages = self.vaddr_hi.saturating_sub(self.vaddr_lo) / 4096;
        if range_pages > u64::from(FULL_FLUSH_THRESHOLD) {
            self.need_full_flush = true;
        }
    }

    /// Add a page to the gather without an associated physical frame to free.
    ///
    /// Use this when the unmap doesn't require frame deallocation (e.g.,
    /// shared mappings where the frame is still referenced elsewhere, or
    /// guard pages that have no backing frame).
    pub fn add_flush_only(&mut self, virt_addr: u64) {
        if virt_addr < self.vaddr_lo {
            self.vaddr_lo = virt_addr;
        }
        let vaddr_end = virt_addr.saturating_add(FRAME_SIZE as u64);
        if vaddr_end > self.vaddr_hi {
            self.vaddr_hi = vaddr_end;
        }

        self.dirty = true;

        // Check if we should switch to full flush mode.
        let range_pages = self.vaddr_hi.saturating_sub(self.vaddr_lo) / 4096;
        if range_pages > u64::from(FULL_FLUSH_THRESHOLD) {
            self.need_full_flush = true;
        }
    }

    /// Number of frames currently buffered.
    #[must_use]
    pub fn buffered_count(&self) -> usize {
        self.count
    }

    /// Whether any entries have been added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.dirty
    }

    /// Finish the gather: flush TLBs and free all buffered frames.
    ///
    /// This is the "commit" operation.  After this call:
    /// - All CPUs have invalidated TLB entries for the affected range.
    /// - All buffered physical frames have been returned to the allocator.
    ///
    /// Returns the number of frames freed.
    pub fn finish(&mut self) -> usize {
        if !self.dirty {
            return 0;
        }

        let freed = self.flush_and_free();
        GATHER_OPS.fetch_add(1, Ordering::Relaxed);
        freed
    }

    /// Internal: perform the TLB flush and free all buffered frames.
    ///
    /// Resets internal state for potential reuse.
    #[allow(clippy::arithmetic_side_effects)]
    fn flush_and_free(&mut self) -> usize {
        if !self.dirty {
            return 0;
        }

        // Step 1: TLB flush (ensures no CPU has stale entries).
        if self.need_full_flush {
            FULL_FLUSH_CHOSEN.fetch_add(1, Ordering::Relaxed);
            tlb::flush_all();
        } else if self.vaddr_lo < self.vaddr_hi {
            // Calculate the number of 4 KiB hardware pages in the range.
            let range_bytes = self.vaddr_hi.saturating_sub(self.vaddr_lo);
            let hw_pages = (range_bytes / 4096) as u32;
            if hw_pages > 0 {
                tlb::flush_range(self.vaddr_lo, hw_pages);
            }
        }

        // Step 2: Free all buffered physical frames.
        // Now safe because all CPUs have flushed their TLBs.
        let freed = self.count;
        for i in 0..self.count {
            let phys = self.frames[i];
            if phys != 0 {
                if let Some(pf) = PhysFrame::from_addr(phys) {
                    // SAFETY: The frame was collected during an unmap operation.
                    // The TLB flush above ensures no CPU has a stale mapping.
                    // The frame is exclusively ours to free.
                    // A failure here indicates an upstream invariant violation
                    // (caller passed an invalid frame). We can't panic in the
                    // TLB shootdown path, so log and count — leaking a frame
                    // is preferable to a kernel hang.
                    if let Err(e) = unsafe { frame::free_frame(pf) } {
                        FREE_FAILURES.fetch_add(1, Ordering::Relaxed);
                        crate::serial_println!(
                            "[tlb_gather] free_frame({:#x}) failed: {:?} (leaked)",
                            phys, e
                        );
                    }
                }
            }
        }

        GATHER_PAGES_FREED.fetch_add(freed as u64, Ordering::Relaxed);

        // Reset state.
        self.count = 0;
        self.vaddr_lo = u64::MAX;
        self.vaddr_hi = 0;
        self.need_full_flush = false;
        self.dirty = false;

        freed
    }
}

impl Drop for TlbGather {
    /// Safety net: if the gather is dropped without `finish()`, perform
    /// the flush+free to prevent frame leaks.
    fn drop(&mut self) {
        if self.dirty {
            self.flush_and_free();
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the TLB gather system.
pub fn self_test() {
    serial_println!("[tlb_gather] Running self-test...");

    // Test 1: Empty gather does nothing.
    {
        let mut g = TlbGather::new();
        assert!(g.is_empty());
        let freed = g.finish();
        assert_eq!(freed, 0);
    }
    serial_println!("[tlb_gather]   Empty gather: OK");

    // Test 2: Single entry gather (allocate frame, add, finish → freed).
    {
        let frame = frame::alloc_frame().expect("alloc for gather test");
        let phys = frame.addr();
        let virt_addr: u64 = 0xFFFF_C900_0010_0000; // Test area.

        let mut g = TlbGather::new();
        g.add(virt_addr, phys);
        assert_eq!(g.buffered_count(), 1);
        assert!(!g.is_empty());

        let freed = g.finish();
        assert_eq!(freed, 1);
        assert!(g.is_empty());
    }
    serial_println!("[tlb_gather]   Single entry: OK");

    // Test 3: Multiple entries, verify range tracking.
    {
        let f1 = frame::alloc_frame().expect("alloc f1");
        let f2 = frame::alloc_frame().expect("alloc f2");
        let f3 = frame::alloc_frame().expect("alloc f3");

        let base: u64 = 0xFFFF_C900_0020_0000;

        let mut g = TlbGather::new();
        g.add(base, f1.addr());
        g.add(base + FRAME_SIZE as u64, f2.addr());
        g.add(base + 2 * FRAME_SIZE as u64, f3.addr());
        assert_eq!(g.buffered_count(), 3);

        let freed = g.finish();
        assert_eq!(freed, 3);
    }
    serial_println!("[tlb_gather]   Multiple entries: OK");

    // Test 4: Flush-only entries (no frame to free).
    {
        let mut g = TlbGather::new();
        let base: u64 = 0xFFFF_C900_0030_0000;
        g.add_flush_only(base);
        g.add_flush_only(base + FRAME_SIZE as u64);
        assert_eq!(g.buffered_count(), 0); // No frames buffered.
        assert!(!g.is_empty()); // But dirty (needs flush).

        let freed = g.finish();
        assert_eq!(freed, 0);
    }
    serial_println!("[tlb_gather]   Flush-only entries: OK");

    // Test 5: Drop without finish (safety net).
    {
        let frame = frame::alloc_frame().expect("alloc for drop test");
        let phys = frame.addr();
        let virt_addr: u64 = 0xFFFF_C900_0040_0000;

        let mut g = TlbGather::new();
        g.add(virt_addr, phys);
        // Intentionally drop without finish — Drop impl should free.
        drop(g);
    }
    serial_println!("[tlb_gather]   Drop safety net: OK");

    // Test 6: Statistics updated.
    let s = stats();
    assert!(s.gather_ops >= 3, "expected at least 3 gather ops");
    assert!(s.pages_freed >= 4, "expected at least 4 pages freed");
    serial_println!("[tlb_gather]   Stats: ops={}, freed={}, partial={}, full={}",
        s.gather_ops, s.pages_freed, s.partial_flushes, s.full_flush_chosen);

    serial_println!("[tlb_gather] Self-test PASSED");
}

//! Page aging — track hot/cold state of physical pages for reclaim.
//!
//! The x86_64 hardware sets the Accessed bit (PTE bit 5) whenever a page
//! is read or written.  The kernel must periodically **clear** this bit
//! and observe whether it gets set again to determine page activity.
//!
//! ## Algorithm (Clock / Second-Chance)
//!
//! Each page has an "age" counter (0 = hot, higher = colder):
//!
//! 1. Scanner clears the Accessed bit on all tracked pages.
//! 2. Next scan cycle: if Accessed bit is set again → page was used →
//!    reset age to 0 (hot).
//! 3. If Accessed bit is still clear → page was NOT used → increment age.
//! 4. Pages whose age exceeds `MAX_AGE` are candidates for reclaim/eviction.
//!
//! This is a simplified version of Linux's multi-generation LRU (MGLRU)
//! approach, providing the foundation for intelligent page reclaim.
//!
//! ## Dirty Tracking
//!
//! The scanner also checks the Dirty bit (PTE bit 6).  Dirty pages must
//! be written back before they can be reclaimed (swap-out or file writeback).
//! Clean pages can be reclaimed immediately (just discard).
//!
//! ## Integration
//!
//! - **kswapd**: calls `scan_cycle()` periodically to update ages.
//! - **Page reclaim**: queries `find_cold_pages()` to select eviction victims.
//! - **Working set estimation**: `working_set_pages()` returns count of
//!   recently-accessed pages.
//!
//! ## References
//!
//! - Linux `mm/vmscan.c` — page_referenced(), shrink_folio_list()
//! - Linux `mm/workingset.c` — workingset_refault()
//! - Yu Zhao, "Multi-Gen LRU" (Linux 6.1+)
//! - Intel SDM Vol. 3A §4.8 "Accessed and Dirty Flags"

use core::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use crate::serial_println;
use crate::mm::page_table::{self, PageFlags, VirtAddr};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of pages tracked for aging.
///
/// This is the capacity of the aging table.  Each entry tracks one
/// physical frame that's a candidate for reclaim.
const MAX_TRACKED: usize = 4096;

/// Maximum age before a page is considered "cold" (ready for eviction).
/// Age is incremented each scan cycle that finds the page unaccessed.
const MAX_AGE: u8 = 8;

/// Number of scan cycles between full sweeps.
/// At one scan per second, MAX_AGE=8 means pages unused for 8 seconds
/// become eviction candidates.
const _SCAN_INTERVAL_HINT: u32 = 1; // 1 second (advisory for kswapd)

// ---------------------------------------------------------------------------
// Per-page tracking entry
// ---------------------------------------------------------------------------

/// Tracking entry for a single page.
#[derive(Clone, Copy)]
struct AgeEntry {
    /// Physical address of the frame (0 = slot unused).
    phys_addr: u64,
    /// Virtual address where this frame is mapped.
    virt_addr: u64,
    /// PML4 (address space) physical address.
    pml4_phys: u64,
    /// Current age counter (0 = recently accessed, MAX_AGE = cold).
    age: u8,
    /// Whether the page was dirty on last scan.
    dirty: bool,
    /// Whether this slot is active.
    active: bool,
}

impl AgeEntry {
    const fn empty() -> Self {
        Self {
            phys_addr: 0,
            virt_addr: 0,
            pml4_phys: 0,
            age: 0,
            dirty: false,
            active: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Age tracking table.
static mut AGE_TABLE: [AgeEntry; MAX_TRACKED] = {
    const EMPTY: AgeEntry = AgeEntry::empty();
    [EMPTY; MAX_TRACKED]
};

/// Number of active entries in the age table.
static ACTIVE_COUNT: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Total scan cycles completed.
static SCAN_CYCLES: AtomicU64 = AtomicU64::new(0);

/// Total pages found accessed (hot) during scans.
static HOT_PAGES_FOUND: AtomicU64 = AtomicU64::new(0);

/// Total pages found cold (age >= MAX_AGE) during scans.
static COLD_PAGES_FOUND: AtomicU64 = AtomicU64::new(0);

/// Total pages found dirty during scans.
static DIRTY_PAGES_FOUND: AtomicU64 = AtomicU64::new(0);

/// Statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct AgeStats {
    /// Number of pages currently tracked.
    pub tracked_pages: u32,
    /// Total scan cycles since boot.
    pub scan_cycles: u64,
    /// Pages found hot (accessed recently) across all scans.
    pub hot_pages_found: u64,
    /// Pages found cold (age >= MAX_AGE) across all scans.
    pub cold_pages_found: u64,
    /// Pages found dirty across all scans.
    pub dirty_pages_found: u64,
    /// Working set estimate (pages accessed in last scan cycle).
    pub working_set: u32,
    /// Cold pages ready for eviction right now.
    pub eviction_candidates: u32,
}

/// Get current aging statistics.
#[must_use]
pub fn stats() -> AgeStats {
    let (working_set, eviction_candidates) = count_hot_cold();
    AgeStats {
        tracked_pages: ACTIVE_COUNT.load(Ordering::Relaxed),
        scan_cycles: SCAN_CYCLES.load(Ordering::Relaxed),
        hot_pages_found: HOT_PAGES_FOUND.load(Ordering::Relaxed),
        cold_pages_found: COLD_PAGES_FOUND.load(Ordering::Relaxed),
        dirty_pages_found: DIRTY_PAGES_FOUND.load(Ordering::Relaxed),
        working_set,
        eviction_candidates,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Register a page for age tracking.
///
/// Call when a new user page is mapped (page fault handler, mmap).
/// `phys_addr`: physical frame address
/// `virt_addr`: virtual address where it's mapped
/// `pml4_phys`: page table root for the owning address space
///
/// Returns `true` if successfully registered, `false` if table is full.
pub fn track(phys_addr: u64, virt_addr: u64, pml4_phys: u64) -> bool {
    let count = ACTIVE_COUNT.load(Ordering::Relaxed) as usize;
    if count >= MAX_TRACKED {
        return false;
    }

    // Find a free slot.
    for i in 0..MAX_TRACKED {
        // SAFETY: Single-threaded access assumed during page fault handling
        // (interrupts disabled or per-CPU path).
        let entry = unsafe { &mut AGE_TABLE[i] };
        if !entry.active {
            entry.phys_addr = phys_addr;
            entry.virt_addr = virt_addr;
            entry.pml4_phys = pml4_phys;
            entry.age = 0;
            entry.dirty = false;
            entry.active = true;
            ACTIVE_COUNT.fetch_add(1, Ordering::Relaxed);
            return true;
        }
    }
    false
}

/// Unregister a page from age tracking.
///
/// Call when a page is unmapped (munmap, process exit, page migration).
pub fn untrack(phys_addr: u64) {
    for i in 0..MAX_TRACKED {
        // SAFETY: Accessing static array with index bounds check.
        let entry = unsafe { &mut AGE_TABLE[i] };
        if entry.active && entry.phys_addr == phys_addr {
            entry.active = false;
            entry.phys_addr = 0;
            ACTIVE_COUNT.fetch_sub(1, Ordering::Relaxed);
            return;
        }
    }
}

/// Run one aging scan cycle.
///
/// For each tracked page:
/// 1. Read the PTE's Accessed and Dirty bits.
/// 2. If Accessed is set: clear it, reset age to 0 (hot).
/// 3. If Accessed is clear: increment age (getting colder).
/// 4. Record dirty state.
///
/// Called periodically by kswapd or a timer.
/// Returns (hot_count, cold_count) for this cycle.
#[allow(clippy::arithmetic_side_effects)]
pub fn scan_cycle() -> (u32, u32) {
    let mut hot: u32 = 0;
    let mut cold: u32 = 0;
    let mut dirty: u32 = 0;

    let hhdm = match page_table::hhdm() {
        Some(h) => h,
        None => return (0, 0),
    };

    for i in 0..MAX_TRACKED {
        // SAFETY: Bounded index access.
        let entry = unsafe { &mut AGE_TABLE[i] };
        if !entry.active {
            continue;
        }

        // Walk the page table to find the leaf PTE for this mapping.
        let pte_result = read_and_clear_accessed(
            entry.pml4_phys, entry.virt_addr, hhdm
        );

        match pte_result {
            Some((accessed, is_dirty)) => {
                if accessed {
                    // Page was used since last scan — it's hot.
                    entry.age = 0;
                    hot += 1;
                } else {
                    // Page was NOT used — age it.
                    entry.age = entry.age.saturating_add(1);
                    if entry.age >= MAX_AGE {
                        cold += 1;
                    }
                }
                if is_dirty {
                    entry.dirty = true;
                    dirty += 1;
                }
            }
            None => {
                // PTE no longer valid (unmapped?) — evict from tracking.
                entry.active = false;
                entry.phys_addr = 0;
                ACTIVE_COUNT.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    HOT_PAGES_FOUND.fetch_add(u64::from(hot), Ordering::Relaxed);
    COLD_PAGES_FOUND.fetch_add(u64::from(cold), Ordering::Relaxed);
    DIRTY_PAGES_FOUND.fetch_add(u64::from(dirty), Ordering::Relaxed);
    SCAN_CYCLES.fetch_add(1, Ordering::Relaxed);

    (hot, cold)
}

/// Get the age of a specific tracked page.
///
/// Returns `None` if the page is not tracked.
#[must_use]
pub fn get_age(phys_addr: u64) -> Option<u8> {
    for i in 0..MAX_TRACKED {
        // SAFETY: Bounded access.
        let entry = unsafe { &AGE_TABLE[i] };
        if entry.active && entry.phys_addr == phys_addr {
            return Some(entry.age);
        }
    }
    None
}

/// Find cold pages (age >= MAX_AGE) suitable for eviction.
///
/// Fills `out` with physical addresses of cold pages.
/// Returns the number of cold pages found (up to `out.len()`).
///
/// The caller should prefer clean pages (dirty=false) for immediate
/// reclaim without I/O.
pub fn find_cold_pages(out: &mut [(u64, bool)]) -> usize {
    let mut found = 0;
    for i in 0..MAX_TRACKED {
        if found >= out.len() {
            break;
        }
        // SAFETY: Bounded access.
        let entry = unsafe { &AGE_TABLE[i] };
        if entry.active && entry.age >= MAX_AGE {
            out[found] = (entry.phys_addr, entry.dirty);
            found += 1;
        }
    }
    found
}

/// Estimate the working set size (pages accessed in the last scan cycle).
///
/// Returns the number of pages with age == 0 (recently accessed).
#[must_use]
pub fn working_set_pages() -> u32 {
    let mut count: u32 = 0;
    for i in 0..MAX_TRACKED {
        // SAFETY: Bounded access.
        let entry = unsafe { &AGE_TABLE[i] };
        if entry.active && entry.age == 0 {
            count += 1;
        }
    }
    count
}

/// Get an age distribution histogram.
///
/// Returns an array where index i = count of pages with age == i.
/// Index MAX_AGE = count of pages at maximum age (cold).
#[must_use]
pub fn age_histogram() -> [u32; MAX_AGE as usize + 1] {
    let mut hist = [0u32; MAX_AGE as usize + 1];
    for i in 0..MAX_TRACKED {
        // SAFETY: Bounded access.
        let entry = unsafe { &AGE_TABLE[i] };
        if entry.active {
            let bucket = (entry.age as usize).min(MAX_AGE as usize);
            hist[bucket] = hist[bucket].saturating_add(1);
        }
    }
    hist
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Count current hot and cold pages.
fn count_hot_cold() -> (u32, u32) {
    let mut hot: u32 = 0;
    let mut cold: u32 = 0;
    for i in 0..MAX_TRACKED {
        // SAFETY: Bounded access.
        let entry = unsafe { &AGE_TABLE[i] };
        if entry.active {
            if entry.age == 0 {
                hot += 1;
            } else if entry.age >= MAX_AGE {
                cold += 1;
            }
        }
    }
    (hot, cold)
}

/// Read the Accessed and Dirty bits from a PTE, then clear Accessed.
///
/// Walks the page table hierarchy to find the leaf PTE for `virt_addr`
/// in address space `pml4_phys`.  Returns `(accessed, dirty)`, or `None`
/// if the page is not present.
///
/// After reading, the Accessed bit is cleared so the next scan can
/// detect fresh accesses.
#[allow(clippy::arithmetic_side_effects)]
fn read_and_clear_accessed(pml4_phys: u64, virt_addr: u64, hhdm: u64) -> Option<(bool, bool)> {
    // Page table index extraction (standard x86_64 4-level paging).
    let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
    let pdpt_idx = ((virt_addr >> 30) & 0x1FF) as usize;
    let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
    let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;

    // Walk PML4 → PDPT → PD → PT.
    let pml4_ptr = (hhdm + pml4_phys) as *const u64;
    // SAFETY: pml4_phys is a valid page table, hhdm maps all physical memory.
    let pml4e = unsafe { pml4_ptr.add(pml4_idx).read_volatile() };
    if pml4e & 1 == 0 {
        return None; // Not present.
    }

    let pdpt_phys = pml4e & 0x000F_FFFF_FFFF_F000;
    let pdpt_ptr = (hhdm + pdpt_phys) as *const u64;
    // SAFETY: Valid page table entry, present bit checked.
    let pdpte = unsafe { pdpt_ptr.add(pdpt_idx).read_volatile() };
    if pdpte & 1 == 0 {
        return None;
    }
    if pdpte & (1 << 7) != 0 {
        return None; // 1 GiB huge page — not tracked individually.
    }

    let pd_phys = pdpte & 0x000F_FFFF_FFFF_F000;
    let pd_ptr = (hhdm + pd_phys) as *const u64;
    // SAFETY: Valid page table entry, present bit checked.
    let pde = unsafe { pd_ptr.add(pd_idx).read_volatile() };
    if pde & 1 == 0 {
        return None;
    }
    if pde & (1 << 7) != 0 {
        return None; // 2 MiB huge page — not tracked individually.
    }

    let pt_phys = pde & 0x000F_FFFF_FFFF_F000;
    let pt_ptr = (hhdm + pt_phys) as *mut u64;

    // SAFETY: Valid page table entry, present bit checked.
    // We use read_volatile + write_volatile because the hardware may
    // concurrently set these bits (CPU page walker sets Accessed/Dirty).
    let pte = unsafe { pt_ptr.add(pt_idx).read_volatile() };
    if pte & 1 == 0 {
        return None;
    }

    let accessed = pte & (1 << 5) != 0;
    let dirty = pte & (1 << 6) != 0;

    // Clear the Accessed bit (leave Dirty intact for writeback detection).
    if accessed {
        let new_pte = pte & !(1u64 << 5);
        // SAFETY: Writing a valid PTE value (only clearing Accessed bit).
        unsafe { pt_ptr.add(pt_idx).write_volatile(new_pte); }
        // Note: TLB flush for this address is deferred to the caller.
        // The slight staleness is acceptable for aging — a few extra cycles
        // of "hot" detection is not harmful.
    }

    Some((accessed, dirty))
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the page aging system.
pub fn self_test() {
    use crate::mm::frame;

    serial_println!("[page_age] Running self-test...");

    // Allocate a real zeroed frame to use as a "fake PML4".
    // The frame is all zeros, so all PML4 entries will be "not present"
    // (bit 0 = 0), causing scan_cycle to return None gracefully.
    let pml4_frame = frame::alloc_frame_zeroed().expect("alloc frame for page_age test");
    let test_pml4: u64 = pml4_frame.addr();

    // Test 1: Track and untrack.
    let fake_phys: u64 = 0xDEAD_0000;
    let fake_virt: u64 = 0x0000_1000_0000;

    assert!(track(fake_phys, fake_virt, test_pml4));
    assert_eq!(ACTIVE_COUNT.load(Ordering::Relaxed), 1);
    serial_println!("[page_age]   Track: OK");

    // Test 2: Get age of tracked page.
    let age = get_age(fake_phys);
    assert_eq!(age, Some(0)); // Freshly tracked = age 0.
    serial_println!("[page_age]   Initial age=0: OK");

    // Test 3: Untrack.
    untrack(fake_phys);
    assert_eq!(ACTIVE_COUNT.load(Ordering::Relaxed), 0);
    let age = get_age(fake_phys);
    assert_eq!(age, None);
    serial_println!("[page_age]   Untrack: OK");

    // Test 4: Multiple pages tracked with zeroed PML4 (entries "not present").
    for i in 0..10u64 {
        assert!(track(
            0xA000_0000 + i * 0x4000,
            0x1000_0000 + i * 0x4000,
            test_pml4
        ));
    }
    assert_eq!(ACTIVE_COUNT.load(Ordering::Relaxed), 10);
    serial_println!("[page_age]   Multi-track (10 pages): OK");

    // Test 5: Scan cycle — PML4 is zeroed so all PTEs will be "not present".
    // scan_cycle should detect this and untrack all entries gracefully.
    let (hot, cold) = scan_cycle();
    // All pages should be removed (zeroed PML4 → bit 0 = 0 → None).
    assert_eq!(ACTIVE_COUNT.load(Ordering::Relaxed), 0);
    serial_println!("[page_age]   Scan with empty PTEs: OK (hot={}, cold={}, removed 10)", hot, cold);

    // Test 6: Age histogram on empty table.
    let hist = age_histogram();
    let total: u32 = hist.iter().sum();
    assert_eq!(total, 0);
    serial_println!("[page_age]   Empty histogram: OK");

    // Test 7: Working set.
    let ws = working_set_pages();
    assert_eq!(ws, 0);
    serial_println!("[page_age]   Working set (empty)=0: OK");

    // Test 8: Find cold pages on empty table.
    let mut cold_buf = [(0u64, false); 4];
    let found = find_cold_pages(&mut cold_buf);
    assert_eq!(found, 0);
    serial_println!("[page_age]   find_cold_pages (empty)=0: OK");

    // Test 9: Stats.
    let s = stats();
    assert_eq!(s.tracked_pages, 0);
    assert!(s.scan_cycles >= 1);
    serial_println!("[page_age]   Stats: cycles={}, hot={}, cold={}, dirty={}",
        s.scan_cycles, s.hot_pages_found, s.cold_pages_found, s.dirty_pages_found);

    // Cleanup: free the test PML4 frame.
    // SAFETY: We allocated this frame above, it's not mapped anywhere.
    unsafe { frame::free_frame(pml4_frame); }

    serial_println!("[page_age] Self-test PASSED");
}

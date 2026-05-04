//! Per-address-space memory accounting.
//!
//! Tracks the Resident Set Size (RSS) — the number of physical frames
//! mapped — for each user-mode address space.  This data supports:
//!
//! - **OOM killer**: Select the largest process by RSS.
//! - **Resource limits**: Enforce per-process memory caps.
//! - **Diagnostics**: `/proc/<pid>/status` VmRSS reporting.
//!
//! ## Design
//!
//! A fixed-size array of accounting entries, one per address space
//! (identified by PML4 physical address).  No heap allocation — safe
//! to call from any context including page fault handlers and early boot.
//!
//! Entries are inserted when a new PML4 is allocated ([`init_address_space`])
//! and removed when freed ([`destroy_address_space`]).  Frame counts are
//! incremented/decremented atomically by [`charge`]/[`uncharge`] on every
//! map/unmap of a user-mode page.
//!
//! ## Locking
//!
//! Uses a dedicated spinlock (`ACCOUNTING`).  Lock ordering:
//!
//! ```text
//! SCHED → frame_allocator → ACCOUNTING
//! ```
//!
//! The accounting lock is always acquired AFTER the frame allocator lock
//! (if both are needed in the same path), and is held only briefly for
//! counter updates.  No code holds ACCOUNTING while acquiring other locks.
//!
//! ## Performance
//!
//! Each `charge`/`uncharge` call acquires the spinlock and does a linear
//! scan over at most `MAX_ADDRESS_SPACES` entries (256).  On modern x86,
//! scanning 256×8-byte entries from L1 cache takes <500ns.  This is
//! acceptable on the page fault path (target <10µs total), which already
//! involves TLB flushes, frame allocation, and page table walks.
//!
//! If process counts grow beyond 256, migrate to a hash table or BTreeMap.

use crate::serial_println;
use spin::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of tracked address spaces.
///
/// 256 is far more than Phase 2 needs (typical desktop: 50-100 processes)
/// and avoids any heap allocation.  Each entry is 32 bytes → 8 KiB total.
const MAX_ADDRESS_SPACES: usize = 256;

// ---------------------------------------------------------------------------
// Entry layout
// ---------------------------------------------------------------------------

/// Accounting entry for one address space.
#[derive(Clone, Copy)]
struct AccountingEntry {
    /// PML4 physical address (identifies the address space).
    /// 0 = slot is empty.
    pml4_phys: u64,

    /// Current RSS: number of 16 KiB frames mapped in this address space.
    rss_frames: u64,

    /// Peak (high-water-mark) RSS since the address space was created.
    peak_rss_frames: u64,

    /// Virtual address space size: number of frames ever mapped (not freed).
    /// This counts the total virtual range committed, not just resident.
    /// Increments on map, does NOT decrement on unmap (tracks lifetime peak).
    total_mapped_ever: u64,
}

impl AccountingEntry {
    const EMPTY: Self = Self {
        pml4_phys: 0,
        rss_frames: 0,
        peak_rss_frames: 0,
        total_mapped_ever: 0,
    };
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

/// The accounting table.  Protected by a spinlock.
static ACCOUNTING: Mutex<[AccountingEntry; MAX_ADDRESS_SPACES]> =
    Mutex::new([AccountingEntry::EMPTY; MAX_ADDRESS_SPACES]);

/// The PML4 physical address of the kernel's own address space.
///
/// Set by [`set_kernel_pml4`] during boot.  Mappings into this address
/// space are NOT tracked (kernel memory is shared, not per-process).
static KERNEL_PML4: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Record the kernel's PML4 physical address so we can skip accounting
/// for kernel-space mappings.
///
/// Called once during boot after the kernel page tables are set up.
pub fn set_kernel_pml4(pml4_phys: u64) {
    KERNEL_PML4.store(pml4_phys, core::sync::atomic::Ordering::Release);
}

/// Register a new user-mode address space for tracking.
///
/// Call this when a per-process PML4 is allocated (e.g., during
/// `proc::create`).  Returns `true` on success, `false` if the table
/// is full.
pub fn init_address_space(pml4_phys: u64) -> bool {
    if pml4_phys == 0 {
        return false;
    }

    let mut table = ACCOUNTING.lock();

    // Check if already registered (idempotent).
    for entry in table.iter() {
        if entry.pml4_phys == pml4_phys {
            return true;
        }
    }

    // Find an empty slot.
    for entry in table.iter_mut() {
        if entry.pml4_phys == 0 {
            entry.pml4_phys = pml4_phys;
            entry.rss_frames = 0;
            entry.peak_rss_frames = 0;
            entry.total_mapped_ever = 0;
            return true;
        }
    }

    serial_println!(
        "[accounting] WARNING: table full ({} slots), cannot track PML4 {:#x}",
        MAX_ADDRESS_SPACES,
        pml4_phys,
    );
    false
}

/// Remove an address space from tracking.
///
/// Call this when a process exits and its PML4 is freed.  The slot
/// becomes available for reuse.
pub fn destroy_address_space(pml4_phys: u64) {
    if pml4_phys == 0 {
        return;
    }

    let mut table = ACCOUNTING.lock();
    for entry in table.iter_mut() {
        if entry.pml4_phys == pml4_phys {
            *entry = AccountingEntry::EMPTY;
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Charge / uncharge
// ---------------------------------------------------------------------------

/// Increment the RSS for an address space by `n` frames.
///
/// Called after successfully mapping `n` frames into the address space.
/// Skips accounting for the kernel address space (pml4 == kernel PML4).
///
/// This is the hot-path function — kept as lean as possible.
#[inline]
pub fn charge(pml4_phys: u64, n: u64) {
    // Skip kernel mappings (boot-time identity maps, HHDM, etc.).
    if pml4_phys == 0
        || pml4_phys == KERNEL_PML4.load(core::sync::atomic::Ordering::Relaxed)
    {
        return;
    }

    let mut table = ACCOUNTING.lock();
    for entry in table.iter_mut() {
        if entry.pml4_phys == pml4_phys {
            entry.rss_frames = entry.rss_frames.saturating_add(n);
            if entry.rss_frames > entry.peak_rss_frames {
                entry.peak_rss_frames = entry.rss_frames;
            }
            entry.total_mapped_ever = entry.total_mapped_ever.saturating_add(n);
            return;
        }
    }

    // Not tracked — might be early boot or a kernel-internal mapping.
    // This is not an error; we silently skip.
}

/// Decrement the RSS for an address space by `n` frames.
///
/// Called after unmapping `n` frames from the address space.
/// Skips accounting for the kernel address space.
#[inline]
pub fn uncharge(pml4_phys: u64, n: u64) {
    if pml4_phys == 0
        || pml4_phys == KERNEL_PML4.load(core::sync::atomic::Ordering::Relaxed)
    {
        return;
    }

    let mut table = ACCOUNTING.lock();
    for entry in table.iter_mut() {
        if entry.pml4_phys == pml4_phys {
            entry.rss_frames = entry.rss_frames.saturating_sub(n);
            return;
        }
    }
}

/// Reset RSS to zero for an address space.
///
/// Called by `clear_user_address_space` which bulk-frees all user pages
/// without going through `unmap_frame`.  The peak RSS is preserved (it
/// records the lifetime high-water mark across exec boundaries).
///
/// Does NOT destroy the entry — the address space is still tracked and
/// new `charge` calls will increment from zero.
pub fn reset_rss(pml4_phys: u64) {
    if pml4_phys == 0 {
        return;
    }

    let mut table = ACCOUNTING.lock();
    for entry in table.iter_mut() {
        if entry.pml4_phys == pml4_phys {
            entry.rss_frames = 0;
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Per-address-space memory snapshot.
#[derive(Clone, Copy, Debug)]
pub struct AddressSpaceStats {
    /// PML4 physical address (address space identifier).
    pub pml4_phys: u64,
    /// Current RSS in 16 KiB frames.
    pub rss_frames: u64,
    /// Peak RSS in 16 KiB frames.
    pub peak_rss_frames: u64,
    /// Total frames ever mapped (lifetime).
    pub total_mapped_ever: u64,
}

impl AddressSpaceStats {
    /// Current RSS in bytes.
    #[must_use]
    #[allow(dead_code)] // Public API for procfs/diagnostics.
    pub fn rss_bytes(&self) -> u64 {
        self.rss_frames.saturating_mul(super::frame::FRAME_SIZE as u64)
    }

    /// Peak RSS in bytes.
    #[must_use]
    #[allow(dead_code)] // Public API for procfs/diagnostics.
    pub fn peak_rss_bytes(&self) -> u64 {
        self.peak_rss_frames.saturating_mul(super::frame::FRAME_SIZE as u64)
    }
}

/// Query memory stats for a specific address space.
///
/// Returns `None` if the PML4 is not tracked.
#[must_use]
pub fn query(pml4_phys: u64) -> Option<AddressSpaceStats> {
    if pml4_phys == 0 {
        return None;
    }

    let table = ACCOUNTING.lock();
    for entry in table.iter() {
        if entry.pml4_phys == pml4_phys {
            return Some(AddressSpaceStats {
                pml4_phys: entry.pml4_phys,
                rss_frames: entry.rss_frames,
                peak_rss_frames: entry.peak_rss_frames,
                total_mapped_ever: entry.total_mapped_ever,
            });
        }
    }
    None
}

/// Get the address space with the highest current RSS.
///
/// Used by the OOM killer to select the "largest" process victim.
/// Returns `None` if no address spaces are tracked (early boot).
///
/// Excludes address spaces with RSS == 0 (empty).
#[must_use]
pub fn largest_rss() -> Option<AddressSpaceStats> {
    let table = ACCOUNTING.lock();
    let mut best: Option<&AccountingEntry> = None;

    for entry in table.iter() {
        if entry.pml4_phys != 0 && entry.rss_frames > 0 {
            match best {
                None => best = Some(entry),
                Some(b) if entry.rss_frames > b.rss_frames => best = Some(entry),
                _ => {}
            }
        }
    }

    best.map(|e| AddressSpaceStats {
        pml4_phys: e.pml4_phys,
        rss_frames: e.rss_frames,
        peak_rss_frames: e.peak_rss_frames,
        total_mapped_ever: e.total_mapped_ever,
    })
}

/// Get stats for all active address spaces (non-empty entries).
///
/// Used by diagnostics (procfs, memory dumps).  Returns a Vec since
/// the caller typically needs to iterate and format.
#[must_use]
#[allow(dead_code)] // Public API for procfs/OOM diagnostics.
pub fn all_stats() -> alloc::vec::Vec<AddressSpaceStats> {
    let table = ACCOUNTING.lock();
    table.iter()
        .filter(|e| e.pml4_phys != 0)
        .map(|e| AddressSpaceStats {
            pml4_phys: e.pml4_phys,
            rss_frames: e.rss_frames,
            peak_rss_frames: e.peak_rss_frames,
            total_mapped_ever: e.total_mapped_ever,
        })
        .collect()
}

/// Number of address spaces currently tracked.
#[must_use]
pub fn tracked_count() -> usize {
    let table = ACCOUNTING.lock();
    table.iter().filter(|e| e.pml4_phys != 0).count()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the accounting module.
///
/// Verifies:
/// 1. `init_address_space` registers a new entry.
/// 2. `charge` increments RSS and tracks peak.
/// 3. `uncharge` decrements RSS (not below zero).
/// 4. `query` returns correct stats.
/// 5. `largest_rss` selects the biggest.
/// 6. `destroy_address_space` frees the slot.
/// 7. Kernel PML4 is excluded from tracking.
pub fn self_test() {
    serial_println!("[accounting] Running self-test...");

    // Use fake PML4 addresses that won't collide with real ones.
    let pml4_a: u64 = 0xDEAD_0000;
    let pml4_b: u64 = 0xBEEF_0000;

    // -- 1. Registration --
    assert!(init_address_space(pml4_a), "init_address_space(a) failed");
    assert!(init_address_space(pml4_b), "init_address_space(b) failed");
    // Idempotent.
    assert!(init_address_space(pml4_a), "re-init should succeed");
    serial_println!("[accounting]   Registration: OK");

    // -- 2. Charge --
    charge(pml4_a, 10);
    charge(pml4_a, 5);
    let stats_a = query(pml4_a).expect("query(a) returned None");
    assert_eq!(stats_a.rss_frames, 15, "a.rss should be 15");
    assert_eq!(stats_a.peak_rss_frames, 15, "a.peak should be 15");
    assert_eq!(stats_a.total_mapped_ever, 15, "a.total should be 15");
    serial_println!("[accounting]   Charge: OK (rss=15)");

    // -- 3. Uncharge --
    uncharge(pml4_a, 7);
    let stats_a = query(pml4_a).expect("query(a) returned None");
    assert_eq!(stats_a.rss_frames, 8, "a.rss should be 8 after uncharge");
    assert_eq!(stats_a.peak_rss_frames, 15, "a.peak should still be 15");
    // Saturating uncharge below zero.
    uncharge(pml4_a, 100);
    let stats_a = query(pml4_a).expect("query(a) returned None");
    assert_eq!(stats_a.rss_frames, 0, "a.rss should be 0 after over-uncharge");
    serial_println!("[accounting]   Uncharge: OK (saturating)");

    // -- 4. Largest RSS --
    charge(pml4_a, 20);
    charge(pml4_b, 50);
    let largest = largest_rss().expect("largest_rss returned None");
    assert_eq!(largest.pml4_phys, pml4_b, "largest should be b");
    assert_eq!(largest.rss_frames, 50, "largest rss should be 50");
    serial_println!("[accounting]   Largest RSS: OK (b=50)");

    // -- 5. Kernel PML4 excluded --
    let kernel_pml4 = KERNEL_PML4.load(core::sync::atomic::Ordering::Relaxed);
    if kernel_pml4 != 0 {
        let before = query(kernel_pml4);
        charge(kernel_pml4, 1000);
        let after = query(kernel_pml4);
        // If kernel PML4 is tracked (shouldn't be), charge should be a no-op.
        assert_eq!(before, after, "kernel PML4 should not be tracked");
    }
    // Also test with pml4_phys = 0.
    charge(0, 999);
    serial_println!("[accounting]   Kernel PML4 exclusion: OK");

    // -- 6. Destroy --
    destroy_address_space(pml4_a);
    assert!(query(pml4_a).is_none(), "a should be gone after destroy");
    destroy_address_space(pml4_b);
    assert!(query(pml4_b).is_none(), "b should be gone after destroy");
    serial_println!("[accounting]   Destroy: OK");

    // -- 7. all_stats count --
    let count = tracked_count();
    // After cleanup, our test entries should be gone.  (There may be
    // real address spaces tracked by the system.)
    serial_println!("[accounting]   Tracked count: {} (after cleanup)", count);

    serial_println!("[accounting] Self-test PASSED");
}

// Implement PartialEq for query result comparison in tests.
impl PartialEq for AddressSpaceStats {
    fn eq(&self, other: &Self) -> bool {
        self.pml4_phys == other.pml4_phys
            && self.rss_frames == other.rss_frames
            && self.peak_rss_frames == other.peak_rss_frames
            && self.total_mapped_ever == other.total_mapped_ever
    }
}
impl Eq for AddressSpaceStats {}

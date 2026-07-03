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
//! **IRQ safety.** Accounting is reachable from *both* task context (syscall
//! map/unmap, CoW, process setup) *and* interrupt/softirq context — e.g. the
//! frame allocator calls `compact::try_compact()` for higher-order allocations,
//! and compaction calls [`tracked_count`]; a device IRQ or softirq that
//! allocates a multi-order buffer therefore re-enters accounting. Crucially,
//! the page-fault handler re-enables interrupts before resolving a fault (so a
//! `charge`/`uncharge` on the fault path runs with IF=1). A plain spinlock that
//! only disables preemption would let such an interrupt land while ACCOUNTING
//! is held and re-acquire it → uniprocessor self-deadlock (observed as
//! `B-ACCT-SPINLOCK-STALL`, "task N == spinner, RECURSIVE self-deadlock"). We
//! therefore acquire ACCOUNTING via `lock_irqsave()`, masking interrupts for
//! the (short, leaf-only) hold. This is the standard Linux `spin_lock_irqsave`
//! discipline for any lock shared with interrupt context.
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
use crate::sync::Mutex;

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

    /// RSS limit in frames.  0 = unlimited (default).
    ///
    /// When set, `charge()` returns `false` if the new RSS would exceed
    /// this limit.  The caller (page fault handler, mmap) should return
    /// `OutOfMemory` to the process.
    rss_limit_frames: u64,
}

impl AccountingEntry {
    const EMPTY: Self = Self {
        pml4_phys: 0,
        rss_frames: 0,
        peak_rss_frames: 0,
        total_mapped_ever: 0,
        rss_limit_frames: 0,
    };
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

/// The accounting table.  Protected by a spinlock.
static ACCOUNTING: Mutex<[AccountingEntry; MAX_ADDRESS_SPACES]> =
    Mutex::named([AccountingEntry::EMPTY; MAX_ADDRESS_SPACES], b"ACCT");

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

    let mut table = ACCOUNTING.lock_irqsave();

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

    let mut table = ACCOUNTING.lock_irqsave();
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

/// Check if charging `n` frames would exceed the RSS limit.
///
/// Returns `true` if the charge is allowed (under limit or no limit set).
/// Returns `false` if the charge would exceed the limit.
///
/// Call this BEFORE allocating/mapping frames when resource limits are
/// being enforced.  If it returns `false`, the caller should return
/// `OutOfMemory` to the process without allocating.
///
/// Does NOT modify any counters — call [`charge`] after the actual mapping.
#[inline]
#[allow(dead_code)] // Public API for page fault handler.
pub fn try_charge(pml4_phys: u64, n: u64) -> bool {
    if pml4_phys == 0
        || pml4_phys == KERNEL_PML4.load(core::sync::atomic::Ordering::Relaxed)
    {
        return true; // Kernel mappings always allowed.
    }

    let table = ACCOUNTING.lock_irqsave();
    for entry in table.iter() {
        if entry.pml4_phys == pml4_phys {
            // No limit set → always allowed.
            if entry.rss_limit_frames == 0 {
                return true;
            }
            return entry.rss_frames.saturating_add(n) <= entry.rss_limit_frames;
        }
    }

    // Not tracked → no limit → allow.
    true
}

/// Increment the RSS for an address space by `n` frames.
///
/// Called after successfully mapping `n` frames into the address space.
/// Skips accounting for the kernel address space (pml4 == kernel PML4).
///
/// This is the hot-path function — kept as lean as possible.
/// Does NOT check limits — use [`try_charge`] beforehand if limits
/// are being enforced.
#[inline]
pub fn charge(pml4_phys: u64, n: u64) {
    // Skip kernel mappings (boot-time identity maps, HHDM, etc.).
    if pml4_phys == 0
        || pml4_phys == KERNEL_PML4.load(core::sync::atomic::Ordering::Relaxed)
    {
        return;
    }

    let mut table = ACCOUNTING.lock_irqsave();
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

    let mut table = ACCOUNTING.lock_irqsave();
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

    let mut table = ACCOUNTING.lock_irqsave();
    for entry in table.iter_mut() {
        if entry.pml4_phys == pml4_phys {
            entry.rss_frames = 0;
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Resource limits
// ---------------------------------------------------------------------------

/// Set the RSS limit for an address space, in 16 KiB frames.
///
/// `limit_frames = 0` means unlimited (default).
///
/// Returns `true` if the address space was found and the limit was set.
/// Returns `false` if the PML4 is not tracked.
///
/// The limit is enforced by [`try_charge`] — call that before allocating
/// frames.  [`charge`] itself does NOT enforce limits (it's the
/// post-mapping bookkeeping path).
#[allow(dead_code)] // Public API for capability/security zone.
pub fn set_rss_limit(pml4_phys: u64, limit_frames: u64) -> bool {
    if pml4_phys == 0 {
        return false;
    }

    let mut table = ACCOUNTING.lock_irqsave();
    for entry in table.iter_mut() {
        if entry.pml4_phys == pml4_phys {
            entry.rss_limit_frames = limit_frames;
            return true;
        }
    }
    false
}

/// Get the current RSS limit for an address space.
///
/// Returns `Some(0)` if unlimited, `Some(n)` if limited, or `None`
/// if the PML4 is not tracked.
#[must_use]
#[allow(dead_code)] // Public API for capability/security zone.
pub fn get_rss_limit(pml4_phys: u64) -> Option<u64> {
    if pml4_phys == 0 {
        return None;
    }

    let table = ACCOUNTING.lock_irqsave();
    for entry in table.iter() {
        if entry.pml4_phys == pml4_phys {
            return Some(entry.rss_limit_frames);
        }
    }
    None
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
    /// RSS limit in frames (0 = unlimited).
    pub rss_limit_frames: u64,
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
    // Backs the Linux getrusage(2) ru_maxrss field (peak resident set size).
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

    let table = ACCOUNTING.lock_irqsave();
    for entry in table.iter() {
        if entry.pml4_phys == pml4_phys {
            return Some(AddressSpaceStats {
                pml4_phys: entry.pml4_phys,
                rss_frames: entry.rss_frames,
                peak_rss_frames: entry.peak_rss_frames,
                total_mapped_ever: entry.total_mapped_ever,
                rss_limit_frames: entry.rss_limit_frames,
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
    let table = ACCOUNTING.lock_irqsave();
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
        rss_limit_frames: e.rss_limit_frames,
    })
}

/// Get stats for all active address spaces (non-empty entries).
///
/// Used by diagnostics (procfs, memory dumps).  Returns a Vec since
/// the caller typically needs to iterate and format.
#[must_use]
#[allow(dead_code)] // Public API for procfs/OOM diagnostics.
pub fn all_stats() -> alloc::vec::Vec<AddressSpaceStats> {
    let table = ACCOUNTING.lock_irqsave();
    table.iter()
        .filter(|e| e.pml4_phys != 0)
        .map(|e| AddressSpaceStats {
            pml4_phys: e.pml4_phys,
            rss_frames: e.rss_frames,
            peak_rss_frames: e.peak_rss_frames,
            total_mapped_ever: e.total_mapped_ever,
            rss_limit_frames: e.rss_limit_frames,
        })
        .collect()
}

/// Number of address spaces currently tracked.
#[must_use]
pub fn tracked_count() -> usize {
    let table = ACCOUNTING.lock_irqsave();
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
    //
    // `largest_rss()` scans the GLOBAL accounting table, which during a live
    // boot also contains *real* process address spaces — some with an RSS far
    // exceeding our fake test entries.  The old assertion `largest.pml4_phys
    // == pml4_b` therefore had a false isolation assumption: it held only when
    // no concurrent real process happened to hold >50 frames at this instant,
    // and panicked (halting the whole boot) whenever one did — a load-
    // dependent flake (see known-issues).  Instead, verify the invariant that
    // is actually deterministic with real entries present:
    //   (1) among our own entries, b outranks a (the ordering this exercises);
    //   (2) `largest_rss()` never reports a maximum *below* a known live entry
    //       (b = 50) — i.e. it returns a true global upper bound.
    charge(pml4_a, 20);
    charge(pml4_b, 50);
    let qa = query(pml4_a).expect("query(a) returned None");
    let qb = query(pml4_b).expect("query(b) returned None");
    assert_eq!(qa.rss_frames, 20, "a.rss should be 20");
    assert_eq!(qb.rss_frames, 50, "b.rss should be 50");
    assert!(qb.rss_frames > qa.rss_frames, "b should outrank a");
    let largest = largest_rss().expect("largest_rss returned None");
    assert!(
        largest.rss_frames >= qb.rss_frames,
        "largest_rss must be >= any known entry (b=50), got {}",
        largest.rss_frames,
    );
    serial_println!(
        "[accounting]   Largest RSS: OK (b=50, global max={})",
        largest.rss_frames,
    );

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

    // -- 6. RSS limits --
    // Set a limit of 30 frames on address space a.
    assert!(set_rss_limit(pml4_a, 30), "set_rss_limit(a, 30) failed");
    let lim = get_rss_limit(pml4_a);
    assert_eq!(lim, Some(30), "limit should be 30");

    // a has RSS=20 (from earlier).  try_charge(a, 5) should succeed (25<=30).
    assert!(try_charge(pml4_a, 5), "try_charge(a,5) should succeed (20+5<=30)");
    // try_charge(a, 15) should fail (20+15=35>30).
    assert!(!try_charge(pml4_a, 15), "try_charge(a,15) should fail (20+15>30)");
    // Exact boundary: try_charge(a, 10) should succeed (20+10=30).
    assert!(try_charge(pml4_a, 10), "try_charge(a,10) should succeed (20+10=30)");

    // Unlimited (0) should always allow.
    assert!(set_rss_limit(pml4_a, 0), "clear limit failed");
    assert!(try_charge(pml4_a, 1000), "unlimited should allow any charge");

    // b has no limit → try_charge always succeeds.
    assert!(try_charge(pml4_b, 1000), "no limit should allow any charge");

    serial_println!("[accounting]   RSS limits: OK");

    // -- 7. Destroy --
    destroy_address_space(pml4_a);
    assert!(query(pml4_a).is_none(), "a should be gone after destroy");
    destroy_address_space(pml4_b);
    assert!(query(pml4_b).is_none(), "b should be gone after destroy");
    serial_println!("[accounting]   Destroy: OK");

    // -- 8. all_stats count --
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
            && self.rss_limit_frames == other.rss_limit_frames
    }
}
impl Eq for AddressSpaceStats {}

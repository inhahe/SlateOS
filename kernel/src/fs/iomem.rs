//! I/O Memory Statistics — MMIO region mapping and access monitoring.
//!
//! Tracks memory-mapped I/O regions, their sizes, access counts,
//! and device ownership. Essential for understanding device memory
//! layout and detecting resource conflicts.
//!
//! ## Architecture
//!
//! ```text
//! I/O memory monitoring
//!   → iomem::register(name, base, size) → register MMIO region
//!   → iomem::record_read(base) → MMIO read access
//!   → iomem::record_write(base) → MMIO write access
//!   → iomem::regions() → list all regions
//!
//! Integration:
//!   → ioport (I/O ports)
//!   → dmastat (DMA stats)
//!   → msivec (MSI vectors)
//!   → memlayout (memory layout)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// MMIO region.
#[derive(Debug, Clone)]
pub struct MmioRegion {
    pub name: String,
    pub base: u64,
    pub size: u64,
    pub reads: u64,
    pub writes: u64,
    pub cacheable: bool,
    pub prefetchable: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_REGIONS: usize = 256;

struct State {
    regions: Vec<MmioRegion>,
    total_reads: u64,
    total_writes: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise the I/O-memory (MMIO region) statistics state.
///
/// Starts with no registered regions and zero read/write totals. An MMIO
/// region is added through [`register`] when the kernel actually maps it
/// (LAPIC, IOAPIC, HPET, a device BAR, …), removed through [`unregister`], and
/// its access counters advance only through real [`record_read`] /
/// [`record_write`] calls. The `/proc/iomem` generator and the `iomem` kshell
/// command surface the region list (and [`regions`]) as if it reflects the
/// real MMIO layout and access activity, so seeding it with phantom regions
/// would be fabricated procfs data — it would claim device memory is mapped
/// and being accessed when nothing actually programmed it.
///
/// (Previously this seeded five fictional regions — LAPIC (0xFEE00000, 50M
/// reads / 10M writes), IOAPIC (0xFEC00000, 1M / 500K), HPET (0xFED00000, 5M /
/// 100K), GPU_FB (0xC0000000, 100M writes) and NVMe_BAR (0xFE800000, 20M /
/// 15M) — plus global totals of 76,000,000 reads / 125,600,000 writes.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        regions: Vec::new(),
        total_reads: 0,
        total_writes: 0,
        ops: 0,
    });
}

/// Register an MMIO region.
pub fn register(name: &str, base: u64, size: u64, cacheable: bool, prefetchable: bool) -> KernelResult<()> {
    with_state(|state| {
        if state.regions.len() >= MAX_REGIONS { return Err(KernelError::ResourceExhausted); }
        if state.regions.iter().any(|r| r.base == base) { return Err(KernelError::AlreadyExists); }
        state.regions.push(MmioRegion {
            name: String::from(name), base, size, reads: 0, writes: 0,
            cacheable, prefetchable,
        });
        Ok(())
    })
}

/// Unregister an MMIO region.
pub fn unregister(base: u64) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.regions.iter().position(|r| r.base == base)
            .ok_or(KernelError::NotFound)?;
        state.regions.remove(idx);
        Ok(())
    })
}

/// Record a read access.
pub fn record_read(base: u64) -> KernelResult<()> {
    with_state(|state| {
        let r = state.regions.iter_mut().find(|r| r.base == base)
            .ok_or(KernelError::NotFound)?;
        r.reads += 1;
        state.total_reads += 1;
        Ok(())
    })
}

/// Record a write access.
pub fn record_write(base: u64) -> KernelResult<()> {
    with_state(|state| {
        let r = state.regions.iter_mut().find(|r| r.base == base)
            .ok_or(KernelError::NotFound)?;
        r.writes += 1;
        state.total_writes += 1;
        Ok(())
    })
}

/// List all regions.
pub fn regions() -> Vec<MmioRegion> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.regions.clone())
}

/// Statistics: (region_count, total_reads, total_writes, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.regions.len(), s.total_reads, s.total_writes, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("iomem::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live region table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom regions, zero totals.
    assert_eq!(regions().len(), 0);
    let (c0, r0, w0, _) = stats();
    assert_eq!((c0, r0, w0), (0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register — region appears zeroed with its attributes; a second region
    //    at the same base is AlreadyExists.
    register("TEST", 0x1000_0000, 0x1000, false, false).expect("register");
    let r = regions().into_iter().find(|r| r.base == 0x1000_0000).expect("find");
    assert_eq!((r.size, r.reads, r.writes), (0x1000, 0, 0));
    assert!(!r.cacheable && !r.prefetchable);
    assert_eq!(regions().len(), 1);
    assert!(register("DUP", 0x1000_0000, 0x1000, false, false).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Read — per-region reads and the global total advance.
    record_read(0x1000_0000).expect("read");
    let r = regions().into_iter().find(|r| r.base == 0x1000_0000).expect("p3");
    assert_eq!(r.reads, 1);
    assert_eq!(stats().1, 1); // total_reads
    crate::serial_println!("  [3/8] read: OK");

    // 4: Write — per-region writes and the global total advance.
    record_write(0x1000_0000).expect("write");
    let r = regions().into_iter().find(|r| r.base == 0x1000_0000).expect("p4");
    assert_eq!(r.writes, 1);
    assert_eq!(stats().2, 1); // total_writes
    crate::serial_println!("  [4/8] write: OK");

    // 5: Unregister — region disappears; double unregister is NotFound.
    unregister(0x1000_0000).expect("unregister");
    assert_eq!(regions().len(), 0);
    assert!(unregister(0x1000_0000).is_err());
    crate::serial_println!("  [5/8] unregister: OK");

    // 6: Not found — read/write on an unregistered base both error.
    assert!(record_read(0xDEAD_BEEF).is_err());
    assert!(record_write(0xDEAD_BEEF).is_err());
    crate::serial_println!("  [6/8] not found: OK");

    // 7: Attributes + multiple accesses — a prefetchable region accrues exactly
    //    the reads it receives (no fabricated baseline).
    register("FB", 0x2000_0000, 0x1000_0000, false, true).expect("register_fb");
    let r = regions().into_iter().find(|r| r.base == 0x2000_0000).expect("fb");
    assert!(r.prefetchable && !r.cacheable);
    for _ in 0..100 { record_read(0x2000_0000).expect("fb_read"); }
    let r = regions().into_iter().find(|r| r.base == 0x2000_0000).expect("fb2");
    assert_eq!(r.reads, 100);
    crate::serial_println!("  [7/8] multi access: OK");

    // 8: Final stats reflect only the real activity above: 1 region (FB),
    //    101 reads (1 on TEST + 100 on FB), 1 write. Global totals are
    //    cumulative and not decremented on unregister.
    let (regs, reads, writes, ops) = stats();
    assert_eq!((regs, reads, writes), (1, 101, 1));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("iomem::self_test() — all 8 tests passed");
}

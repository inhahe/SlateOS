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

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        regions: alloc::vec![
            MmioRegion { name: String::from("LAPIC"), base: 0xFEE0_0000, size: 0x1000, reads: 50_000_000, writes: 10_000_000, cacheable: false, prefetchable: false },
            MmioRegion { name: String::from("IOAPIC"), base: 0xFEC0_0000, size: 0x1000, reads: 1_000_000, writes: 500_000, cacheable: false, prefetchable: false },
            MmioRegion { name: String::from("HPET"), base: 0xFED0_0000, size: 0x1000, reads: 5_000_000, writes: 100_000, cacheable: false, prefetchable: false },
            MmioRegion { name: String::from("GPU_FB"), base: 0xC000_0000, size: 0x1000_0000, reads: 0, writes: 100_000_000, cacheable: false, prefetchable: true },
            MmioRegion { name: String::from("NVMe_BAR"), base: 0xFE80_0000, size: 0x4000, reads: 20_000_000, writes: 15_000_000, cacheable: false, prefetchable: false },
        ],
        total_reads: 76_000_000,
        total_writes: 125_600_000,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(regions().len(), 5);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register("TEST", 0x1000_0000, 0x1000, false, false).expect("register");
    assert_eq!(regions().len(), 6);
    assert!(register("DUP", 0x1000_0000, 0x1000, false, false).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Read.
    record_read(0x1000_0000).expect("read");
    let r = regions().iter().find(|r| r.base == 0x1000_0000).cloned().unwrap();
    assert_eq!(r.reads, 1);
    crate::serial_println!("  [3/8] read: OK");

    // 4: Write.
    record_write(0x1000_0000).expect("write");
    let r = regions().iter().find(|r| r.base == 0x1000_0000).cloned().unwrap();
    assert_eq!(r.writes, 1);
    crate::serial_println!("  [4/8] write: OK");

    // 5: Unregister.
    unregister(0x1000_0000).expect("unregister");
    assert_eq!(regions().len(), 5);
    assert!(unregister(0x1000_0000).is_err());
    crate::serial_println!("  [5/8] unregister: OK");

    // 6: Not found.
    assert!(record_read(0xDEAD_BEEF).is_err());
    crate::serial_println!("  [6/8] not found: OK");

    // 7: Multiple accesses.
    for _ in 0..100 { record_read(0xFEE0_0000).expect("lapic_read"); }
    let r = regions().iter().find(|r| r.name == "LAPIC").cloned().unwrap();
    assert!(r.reads > 50_000_000);
    crate::serial_println!("  [7/8] multi access: OK");

    // 8: Stats.
    let (regs, reads, writes, ops) = stats();
    assert_eq!(regs, 5);
    assert!(reads > 76_000_000);
    assert!(writes > 125_600_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("iomem::self_test() — all 8 tests passed");
}

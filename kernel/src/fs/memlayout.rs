//! Memory Layout — physical and virtual memory region tracking.
//!
//! Provides a system-wide view of memory regions including RAM,
//! MMIO, reserved areas, and ACPI tables. Useful for system
//! information display and diagnostics.
//!
//! ## Architecture
//!
//! ```text
//! Memory layout
//!   → memlayout::add_region(start, size, type) → register region
//!   → memlayout::list() → all memory regions
//!   → memlayout::total_ram() → total usable RAM
//!
//! Integration:
//!   → sysinfo (system information)
//!   → memdiag (memory diagnostics)
//!   → sysresource (system resources)
//!   → hwmonitor (hardware monitor)
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

/// Memory region type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionType {
    Usable,
    Reserved,
    AcpiReclaimable,
    AcpiNvs,
    BadMemory,
    KernelCode,
    KernelData,
    KernelHeap,
    PageTables,
    Framebuffer,
    Mmio,
    BootloaderData,
}

impl RegionType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Usable => "Usable RAM",
            Self::Reserved => "Reserved",
            Self::AcpiReclaimable => "ACPI Reclaimable",
            Self::AcpiNvs => "ACPI NVS",
            Self::BadMemory => "Bad Memory",
            Self::KernelCode => "Kernel Code",
            Self::KernelData => "Kernel Data",
            Self::KernelHeap => "Kernel Heap",
            Self::PageTables => "Page Tables",
            Self::Framebuffer => "Framebuffer",
            Self::Mmio => "MMIO",
            Self::BootloaderData => "Bootloader",
        }
    }
}

/// A memory region.
#[derive(Debug, Clone)]
pub struct MemRegion {
    pub start: u64,
    pub size: u64,
    pub region_type: RegionType,
    pub description: String,
}

impl MemRegion {
    pub fn end(&self) -> u64 {
        self.start + self.size
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_REGIONS: usize = 256;

struct State {
    regions: Vec<MemRegion>,
    total_ram: u64,
    total_reserved: u64,
    total_kernel: u64,
    total_queries: u64,
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

fn recalculate_totals(state: &mut State) {
    state.total_ram = state.regions.iter()
        .filter(|r| r.region_type == RegionType::Usable)
        .map(|r| r.size)
        .sum();
    state.total_reserved = state.regions.iter()
        .filter(|r| matches!(r.region_type, RegionType::Reserved | RegionType::AcpiNvs | RegionType::BadMemory))
        .map(|r| r.size)
        .sum();
    state.total_kernel = state.regions.iter()
        .filter(|r| matches!(r.region_type, RegionType::KernelCode | RegionType::KernelData | RegionType::KernelHeap | RegionType::PageTables))
        .map(|r| r.size)
        .sum();
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let mut state = State {
        regions: alloc::vec![
            MemRegion { start: 0x0000_0000, size: 0x0009_FC00,
                region_type: RegionType::Usable, description: String::from("Low memory") },
            MemRegion { start: 0x0009_FC00, size: 0x0000_0400,
                region_type: RegionType::Reserved, description: String::from("Extended BIOS data") },
            MemRegion { start: 0x000A_0000, size: 0x0006_0000,
                region_type: RegionType::Reserved, description: String::from("Video memory + ROM") },
            MemRegion { start: 0x0010_0000, size: 0x0020_0000,
                region_type: RegionType::KernelCode, description: String::from("Kernel") },
            MemRegion { start: 0x0030_0000, size: 0x0010_0000,
                region_type: RegionType::KernelHeap, description: String::from("Kernel heap") },
            MemRegion { start: 0x0040_0000, size: 0x3FC0_0000,
                region_type: RegionType::Usable, description: String::from("Main memory") },
            MemRegion { start: 0xFEC0_0000, size: 0x0000_1000,
                region_type: RegionType::Mmio, description: String::from("IOAPIC") },
            MemRegion { start: 0xFEE0_0000, size: 0x0000_1000,
                region_type: RegionType::Mmio, description: String::from("Local APIC") },
        ],
        total_ram: 0,
        total_reserved: 0,
        total_kernel: 0,
        total_queries: 0,
        ops: 0,
    };
    recalculate_totals(&mut state);
    *guard = Some(state);
}

/// Add a memory region.
pub fn add_region(start: u64, size: u64, region_type: RegionType, description: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.regions.len() >= MAX_REGIONS {
            return Err(KernelError::ResourceExhausted);
        }
        state.regions.push(MemRegion {
            start, size, region_type, description: String::from(description),
        });
        recalculate_totals(state);
        Ok(())
    })
}

/// List all regions, sorted by start address.
pub fn list_regions() -> Vec<MemRegion> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut regions = s.regions.clone();
        regions.sort_by_key(|r| r.start);
        regions
    })
}

/// List regions of a specific type.
pub fn list_type(region_type: RegionType) -> Vec<MemRegion> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.regions.iter().filter(|r| r.region_type == region_type).cloned().collect()
    })
}

/// Total usable RAM.
pub fn total_ram() -> u64 {
    STATE.lock().as_ref().map_or(0, |s| s.total_ram)
}

/// Total reserved memory.
pub fn total_reserved() -> u64 {
    STATE.lock().as_ref().map_or(0, |s| s.total_reserved)
}

/// Total kernel memory.
pub fn total_kernel() -> u64 {
    STATE.lock().as_ref().map_or(0, |s| s.total_kernel)
}

/// Format bytes as human-readable.
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{}.{} GiB", bytes / 1_073_741_824, (bytes % 1_073_741_824) / 107_374_182)
    } else if bytes >= 1_048_576 {
        format!("{}.{} MiB", bytes / 1_048_576, (bytes % 1_048_576) / 104_857)
    } else if bytes >= 1024 {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{} B", bytes)
    }
}

/// Statistics: (region_count, total_ram, total_reserved, total_kernel, total_queries, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.regions.len(), s.total_ram, s.total_reserved, s.total_kernel, s.total_queries, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("memlayout::self_test() — running tests...");
    init_defaults();

    // 1: Default regions.
    let regions = list_regions();
    assert!(regions.len() >= 8);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Total RAM.
    let ram = total_ram();
    assert!(ram > 0);
    crate::serial_println!("  [2/8] total ram: OK");

    // 3: Total kernel.
    let kernel = total_kernel();
    assert!(kernel > 0);
    crate::serial_println!("  [3/8] total kernel: OK");

    // 4: Sorted by address.
    let regions = list_regions();
    for i in 1..regions.len() {
        assert!(regions[i].start >= regions[i - 1].start);
    }
    crate::serial_println!("  [4/8] sorted: OK");

    // 5: Filter by type.
    let usable = list_type(RegionType::Usable);
    assert!(!usable.is_empty());
    let mmio = list_type(RegionType::Mmio);
    assert!(!mmio.is_empty());
    crate::serial_println!("  [5/8] filter: OK");

    // 6: Add region.
    add_region(0x1_0000_0000, 0x4000_0000, RegionType::Usable, "Extra RAM").expect("add");
    let new_ram = total_ram();
    assert!(new_ram > ram);
    crate::serial_println!("  [6/8] add region: OK");

    // 7: Format size.
    assert_eq!(format_size(1_073_741_824), "1.0 GiB");
    assert_eq!(format_size(1_048_576), "1.0 MiB");
    assert_eq!(format_size(4096), "4 KiB");
    crate::serial_println!("  [7/8] format: OK");

    // 8: Stats.
    let (count, tr, tres, tk, _queries, ops) = stats();
    assert!(count >= 9);
    assert!(tr > 0);
    let _ = tres;
    assert!(tk > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("memlayout::self_test() — all 8 tests passed");
}

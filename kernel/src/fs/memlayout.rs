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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise an EMPTY, honest memory-layout table.
///
/// The REAL physical memory map is installed by [`populate_from_memmap`] during
/// boot, straight from the Limine memmap response. This function only seeds an
/// empty region table with zeroed totals so that, before the real map is
/// installed (or in any context where it never is), `/proc/memlayout`,
/// [`total_ram`] and the `memlayout` kshell command report an honest *unknown*
/// (zero) rather than a fabricated value.
///
/// (Previously this seeded a hand-invented layout — a fixed ~1 GiB "Main memory"
/// block at `0x0040_0000`, plus hardcoded low-memory / kernel / kernel-heap /
/// IOAPIC / Local-APIC ranges — so `total_ram()` always reported ~1 GiB with NO
/// relation to the machine's actual RAM, and `list_regions()` returned a region
/// table that matched no real bootloader memory map. That was fabricated procfs
/// data: the numbers looked authoritative but were never measured.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        regions: Vec::new(),
        total_ram: 0,
        total_reserved: 0,
        total_kernel: 0,
        total_queries: 0,
        ops: 0,
    });
}

/// Install a region table directly, recomputing the derived totals.
///
/// Shared by [`populate_from_memmap`] and the self-test's snapshot/restore so
/// the totals always stay consistent with `regions`.
fn install_regions(regions: Vec<MemRegion>) {
    let mut state = State {
        regions,
        total_ram: 0,
        total_reserved: 0,
        total_kernel: 0,
        total_queries: 0,
        ops: 0,
    };
    recalculate_totals(&mut state);
    *STATE.lock() = Some(state);
}

/// Install the REAL physical memory map from the Limine memmap response.
///
/// Called once during boot (after the heap is available) so that
/// `/proc/memlayout`, [`total_ram`] and the `memlayout` kshell command report
/// the machine's actual memory rather than fabricated values. Replaces any
/// existing region table. Limine memory-map types are mapped onto our finer
/// [`RegionType`] taxonomy (Limine does not distinguish kernel code/data/heap or
/// page tables, so EXECUTABLE_AND_MODULES maps to [`RegionType::KernelCode`]).
pub fn populate_from_memmap(entries: &[&crate::limine::MemmapEntry]) {
    use crate::limine::memmap_type;
    let mut regions: Vec<MemRegion> = Vec::new();
    for e in entries {
        if regions.len() >= MAX_REGIONS { break; }
        let (region_type, desc) = match e.type_ {
            memmap_type::USABLE => (RegionType::Usable, "Usable RAM"),
            memmap_type::RESERVED => (RegionType::Reserved, "Reserved"),
            memmap_type::ACPI_RECLAIMABLE => (RegionType::AcpiReclaimable, "ACPI reclaimable"),
            memmap_type::ACPI_NVS => (RegionType::AcpiNvs, "ACPI NVS"),
            memmap_type::BAD_MEMORY => (RegionType::BadMemory, "Bad memory"),
            memmap_type::BOOTLOADER_RECLAIMABLE => (RegionType::BootloaderData, "Bootloader reclaimable"),
            memmap_type::EXECUTABLE_AND_MODULES => (RegionType::KernelCode, "Kernel + modules"),
            memmap_type::FRAMEBUFFER => (RegionType::Framebuffer, "Framebuffer"),
            _ => (RegionType::Reserved, "Unknown"),
        };
        regions.push(MemRegion {
            start: e.base,
            size: e.length,
            region_type,
            description: String::from(desc),
        });
    }
    install_regions(regions);
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
    use crate::limine::{memmap_type, MemmapEntry};

    // Snapshot the LIVE region table first. This self_test is reachable from
    // the kshell `memlayout` command at any time, and the real Limine-derived
    // map is already installed by boot — the synthetic fixtures below must NOT
    // leak into /proc/memlayout or wipe the real map, so we restore the
    // snapshot at the end.
    let saved: Option<Vec<MemRegion>> =
        STATE.lock().as_ref().map(|s| s.regions.clone());

    // Install a synthetic memory map through the real populate path. Two usable
    // blocks (1 MiB + 1 GiB), one reserved page, one kernel+modules page.
    let entries_owned = [
        MemmapEntry { base: 0x0000_0000, length: 0x0010_0000, type_: memmap_type::USABLE },
        MemmapEntry { base: 0x0010_0000, length: 0x0000_1000, type_: memmap_type::RESERVED },
        MemmapEntry { base: 0x0020_0000, length: 0x4000_0000, type_: memmap_type::USABLE },
        MemmapEntry { base: 0xFEE0_0000, length: 0x0000_1000, type_: memmap_type::EXECUTABLE_AND_MODULES },
    ];
    let entries: [&MemmapEntry; 4] =
        [&entries_owned[0], &entries_owned[1], &entries_owned[2], &entries_owned[3]];
    populate_from_memmap(&entries);

    // 1: Region count matches the installed map exactly.
    let regions = list_regions();
    assert_eq!(regions.len(), 4);
    crate::serial_println!("  [1/8] populate from memmap: OK");

    // 2: Total RAM = sum of the two usable blocks (1 MiB + 1 GiB), exact.
    let ram = total_ram();
    assert_eq!(ram, 0x0010_0000 + 0x4000_0000);
    crate::serial_println!("  [2/8] total ram: OK");

    // 3: Total kernel = the single EXECUTABLE_AND_MODULES page (mapped to
    //    KernelCode); total reserved = the single reserved page. Both exact.
    assert_eq!(total_kernel(), 0x0000_1000);
    assert_eq!(total_reserved(), 0x0000_1000);
    crate::serial_println!("  [3/8] kernel/reserved totals: OK");

    // 4: list_regions() is sorted ascending by start address.
    for i in 1..regions.len() {
        assert!(regions[i].start >= regions[i - 1].start);
    }
    crate::serial_println!("  [4/8] sorted: OK");

    // 5: Filter by type — exactly two usable, one kernel-code region.
    assert_eq!(list_type(RegionType::Usable).len(), 2);
    assert_eq!(list_type(RegionType::KernelCode).len(), 1);
    crate::serial_println!("  [5/8] filter: OK");

    // 6: Add a region — RAM grows by exactly the added block.
    add_region(0x1_0000_0000, 0x4000_0000, RegionType::Usable, "Extra RAM").expect("add");
    assert_eq!(total_ram(), ram + 0x4000_0000);
    crate::serial_println!("  [6/8] add region: OK");

    // 7: Human-readable size formatting.
    assert_eq!(format_size(1_073_741_824), "1.0 GiB");
    assert_eq!(format_size(1_048_576), "1.0 MiB");
    assert_eq!(format_size(4096), "4 KiB");
    crate::serial_println!("  [7/8] format: OK");

    // 8: Stats — 5 regions now (4 installed + 1 added), exact totals.
    let (count, tr, tres, tk, _queries, ops) = stats();
    assert_eq!(count, 5);
    assert_eq!(tr, ram + 0x4000_0000);
    assert_eq!(tres, 0x0000_1000);
    assert_eq!(tk, 0x0000_1000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Restore the real, boot-installed memory map so no synthetic fixtures leak
    // into the live /proc/memlayout table.
    match saved {
        Some(regions) => install_regions(regions),
        None => { *STATE.lock() = None; }
    }
    crate::serial_println!("memlayout::self_test() — all 8 tests passed");
}

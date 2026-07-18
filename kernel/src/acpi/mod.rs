//! ACPI table parsing for hardware discovery.
//!
//! On x86_64, ACPI is the standard mechanism for discovering hardware
//! topology: how many CPUs, where the I/O APICs are, how ISA IRQs are
//! remapped, power management features, etc.
//!
//! ## Boot Flow
//!
//! 1. Limine bootloader provides the RSDP virtual address via its
//!    RSDP request/response.
//! 2. `acpi::init()` validates the RSDP, locates the RSDT or XSDT,
//!    enumerates all system description tables, and parses the MADT.
//! 3. Other subsystems (IOAPIC, APIC, future SMP) query the parsed
//!    data through the public API functions below.
//!
//! ## Currently Parsed Tables
//!
//! - **MADT** ("APIC"): I/O APIC addresses, processor Local APICs,
//!   interrupt source overrides, NMI routing.
//!
//! ## References
//!
//! - ACPI Specification 6.5
//! - Based on Linux `drivers/acpi/` and Fuchsia `zircon/kernel/platform/`
//!   table parsing patterns.

pub mod fadt;
mod madt;
mod tables;

pub use madt::{InterruptOverride, IoApicInfo, LocalApicNmi, MadtInfo, ProcessorInfo};

use alloc::vec::Vec;
use crate::sync::PreemptSpinMutex as Mutex;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Global state — parsed ACPI data
// ---------------------------------------------------------------------------

/// Parsed MADT data, available after `init()` returns.
static MADT_DATA: Mutex<Option<MadtInfo>> = Mutex::new(None);

/// Physical address of the HPET ACPI table, if found.
///
/// Stored during `init()` so that `hpet::init()` can read the table
/// without re-scanning the RSDT/XSDT.
static HPET_TABLE_PHYS: Mutex<Option<u64>> = Mutex::new(None);

/// Physical address of the FADT table, if found.
static FADT_TABLE_PHYS: Mutex<Option<u64>> = Mutex::new(None);

/// General table registry: maps 4-byte signature to physical address.
/// Allows any subsystem to find a table by signature after init.
const MAX_ACPI_TABLES: usize = 32;
static TABLE_REGISTRY: Mutex<TableRegistry> = Mutex::new(TableRegistry::new());

struct TableRegistry {
    entries: [([u8; 4], u64); MAX_ACPI_TABLES],
    count: usize,
}

impl TableRegistry {
    const fn new() -> Self {
        Self {
            entries: [([0; 4], 0); MAX_ACPI_TABLES],
            count: 0,
        }
    }

    fn insert(&mut self, sig: [u8; 4], phys: u64) {
        if self.count < MAX_ACPI_TABLES {
            self.entries[self.count] = (sig, phys);
            self.count += 1;
        }
    }

    fn find(&self, sig: &[u8; 4]) -> Option<u64> {
        for i in 0..self.count {
            if &self.entries[i].0 == sig {
                return Some(self.entries[i].1);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// RSDP discovery helpers
// ---------------------------------------------------------------------------

/// The RSDP signature: "RSD PTR " (8 bytes, note trailing space).
const RSDP_SIGNATURE: [u8; 8] = *b"RSD PTR ";

/// Try a specific address as a potential RSDP location.
///
/// Tests both the raw address (if it looks like an HHDM virtual address)
/// and the HHDM-translated address (if it looks physical).
/// Returns the valid virtual address if the RSDP signature matches.
fn try_rsdp_address(addr: u64, hhdm_offset: u64) -> Option<u64> {
    // Candidate 1: addr is already HHDM-virtual.
    if addr >= hhdm_offset {
        if check_rsdp_signature(addr) {
            return Some(addr);
        }
    }

    // Candidate 2: addr is physical, translate via HHDM.
    let virt = addr.wrapping_add(hhdm_offset);
    if check_rsdp_signature(virt) {
        return Some(virt);
    }

    serial_println!(
        "[acpi] RSDP not found at provided address {:#x} (tried virt={:#x})",
        addr, virt
    );
    None
}

/// Check if 8 bytes at the given virtual address match "RSD PTR ".
fn check_rsdp_signature(virt: u64) -> bool {
    let ptr = virt as *const [u8; 8];
    // SAFETY: caller must ensure the virtual address is mapped.
    let sig = unsafe { core::ptr::read_unaligned(ptr) };
    sig == RSDP_SIGNATURE
}

/// Ensure a physical address range is mapped in the HHDM.
///
/// Limine's HHDM may not cover all physical memory (e.g., ACPI
/// reclaimable or reserved regions may be unmapped).  This function
/// maps any unmapped 4 KiB hardware pages in the given range.
///
/// We operate at 4 KiB (hardware page) granularity rather than 16 KiB
/// (frame) granularity because the bootloader maps at 4 KiB granularity.
/// At region boundaries (e.g., reserved → ACPI reclaimable), some 4 KiB
/// pages within a 16 KiB frame may be mapped while others are not.
///
/// # Safety
///
/// `hhdm_offset` must be correct.  The physical range must be valid.
unsafe fn ensure_hhdm_mapped(phys_start: u64, phys_len: u64, hhdm_offset: u64) {
    use crate::mm::page_table::{self, PageFlags, VirtAddr};

    const HW_PAGE: u64 = 4096;
    let pml4 = page_table::cr3_to_pml4(page_table::read_cr3());
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_CACHE;

    // Align start down to 4 KiB boundary.
    let aligned_start = phys_start & !(HW_PAGE.wrapping_sub(1));
    let end = phys_start.saturating_add(phys_len);
    let mut addr = aligned_start;

    while addr < end {
        let virt = VirtAddr::new(addr.wrapping_add(hhdm_offset));
        // Map this 4 KiB page if it's not already present.
        // SAFETY: addr is within the caller-provided physical range,
        // and hhdm_offset is valid.
        match unsafe { page_table::map_4k_if_absent(pml4, virt, addr, flags) } {
            Ok(true) => {
                // Newly mapped — flush TLB for this page.
                // SAFETY: Standard TLB invalidation for a just-mapped page.
                unsafe {
                    core::arch::asm!(
                        "invlpg [{}]",
                        in(reg) virt.as_u64(),
                        options(nostack, preserves_flags),
                    );
                }
            }
            Ok(false) => {
                // Already mapped, nothing to do.
            }
            Err(_e) => {
                // Huge page at an intermediate level — the target is
                // already covered by a 2 MiB or 1 GiB mapping.  This
                // is fine; the page is accessible.
            }
        }
        addr = addr.wrapping_add(HW_PAGE);
    }
}

/// Ensure an ACPI SDT (System Description Table) at the given physical
/// address is fully mapped in the HHDM.
///
/// First maps enough to read the 36-byte SDT header, then reads the
/// table length field and maps the full extent.  Returns the HHDM
/// virtual address, or `None` if the header length looks invalid
/// (< 36 bytes).
///
/// # Safety
///
/// `phys` must be the physical start of a valid ACPI table.
/// `hhdm_offset` must be correct.
unsafe fn ensure_sdt_mapped(phys: u64, hhdm_offset: u64) -> Option<u64> {
    let header_size = tables::SdtHeader::SIZE as u64;

    // Map at least one frame so we can read the SDT header.
    // SAFETY: caller guarantees phys is a valid ACPI table address.
    unsafe { ensure_hhdm_mapped(phys, header_size, hhdm_offset) };

    let virt = phys.wrapping_add(hhdm_offset);
    let header = virt as *const tables::SdtHeader;

    // SAFETY: we just ensured the header region is mapped.
    let total_len = unsafe { (*header).length } as u64;
    if total_len < header_size {
        return None;
    }

    // Map the full table if it extends beyond the initial header region.
    if total_len > header_size {
        // SAFETY: phys..phys+total_len is within the ACPI table.
        unsafe { ensure_hhdm_mapped(phys, total_len, hhdm_offset) };
    }

    Some(virt)
}

/// Scan ACPI reclaimable memory regions for the RSDP signature.
///
/// On UEFI systems, the RSDP is placed in ACPI reclaimable memory
/// (not in the legacy BIOS area 0xE0000–0xFFFFF, which may not
/// even be mapped).  We scan all memory map regions marked as
/// ACPI reclaimable on 16-byte boundaries.
///
/// `memory_map` is the boot memory map from Limine.
fn scan_for_rsdp(
    hhdm_offset: u64,
    memory_map: &[&crate::limine::MemmapEntry],
) -> Option<u64> {
    use crate::limine::memmap_type;

    // Scan ACPI reclaimable regions first (most likely location on UEFI).
    for entry in memory_map {
        if entry.type_ != memmap_type::ACPI_RECLAIMABLE {
            continue;
        }

        serial_println!(
            "[acpi] Scanning ACPI reclaimable region {:#x}–{:#x} ({} bytes)...",
            entry.base,
            entry.base.saturating_add(entry.length),
            entry.length
        );

        // Ensure the region is mapped in HHDM before reading.
        // Limine may not map ACPI reclaimable regions.
        // SAFETY: entry.base/length are from the Limine memory map.
        unsafe { ensure_hhdm_mapped(entry.base, entry.length, hhdm_offset) };

        let mut addr = entry.base;
        let end = entry.base.saturating_add(entry.length);
        // Align to 16-byte boundary.
        addr = (addr.wrapping_add(15)) & !15;

        while addr.wrapping_add(8) <= end {
            let virt = addr.wrapping_add(hhdm_offset);
            if check_rsdp_signature(virt) {
                serial_println!("[acpi] RSDP found at phys={:#x}", addr);
                return Some(virt);
            }
            addr = addr.wrapping_add(16);
        }
    }

    // Fallback: scan usable memory regions below 1 MB (BIOS area).
    // On some legacy-boot configurations, the RSDP lives in
    // conventional memory below 1 MB.
    for entry in memory_map {
        if entry.type_ != memmap_type::USABLE {
            continue;
        }
        // Only scan below 1 MB.
        if entry.base >= 0x100000 {
            continue;
        }

        let end = entry.base.saturating_add(entry.length).min(0x100000);
        let mut addr = (entry.base.wrapping_add(15)) & !15;

        while addr.wrapping_add(8) <= end {
            let virt = addr.wrapping_add(hhdm_offset);
            if check_rsdp_signature(virt) {
                serial_println!("[acpi] RSDP found at phys={:#x} (low memory)", addr);
                return Some(virt);
            }
            addr = addr.wrapping_add(16);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Parse ACPI tables from the RSDP provided by the bootloader.
///
/// This must be called early in boot, after the heap is initialized
/// (we allocate Vecs for the parsed data) and before IOAPIC/APIC init
/// (which consumes the parsed MADT data).
///
/// `rsdp_addr`: Address of the RSDP (from Limine — may be physical or virtual).
/// `hhdm_offset`: Higher Half Direct Map offset for phys→virt translation.
/// `memory_map`: Boot memory map for RSDP fallback scanning.
///
/// # Safety
///
/// `hhdm_offset` must be valid. ACPI reclaimable memory must be mapped
/// in the HHDM.
#[allow(clippy::arithmetic_side_effects)]
pub unsafe fn init(
    rsdp_addr: u64,
    hhdm_offset: u64,
    memory_map: &[&crate::limine::MemmapEntry],
) {
    // The RSDP address from Limine may be physical, HHDM-virtual, or
    // even incorrect (observed on QEMU+edk2 where Limine returns the
    // kernel load address instead).  Try the provided address first;
    // if validation fails, fall back to scanning ACPI reclaimable
    // memory regions for the "RSD PTR " signature.
    let rsdp_virt = try_rsdp_address(rsdp_addr, hhdm_offset)
        .or_else(|| {
            serial_println!("[acpi] Limine RSDP address invalid — scanning memory...");
            scan_for_rsdp(hhdm_offset, memory_map)
        });

    let rsdp_virt = match rsdp_virt {
        Some(v) => v,
        None => {
            serial_println!("[acpi] ERROR: Could not find RSDP");
            return;
        }
    };

    serial_println!("[acpi] RSDP found at virt={:#x}", rsdp_virt);

    // Validate the RSDP.
    // SAFETY: rsdp_virt was validated by try_rsdp_address or scan_for_rsdp.
    let revision = match unsafe { tables::validate_rsdp(rsdp_virt) } {
        Some(rev) => rev,
        None => {
            serial_println!("[acpi] ERROR: RSDP validation failed");
            return;
        }
    };
    serial_println!("[acpi] RSDP revision {} (ACPI {})",
        revision,
        if revision >= 2 { "2.0+" } else { "1.0" }
    );

    // Locate the root table (prefer XSDT over RSDT).
    let mut root_table_phys: u64;
    let use_xsdt: bool;

    if revision >= 2 {
        let rsdp2 = rsdp_virt as *const tables::Rsdp2;
        // SAFETY: RSDP validation passed, revision ≥ 2 means RSDP2 is valid.
        root_table_phys = unsafe { (*rsdp2).xsdt_address };
        use_xsdt = root_table_phys != 0;

        if !use_xsdt {
            // XSDT address is zero — fall back to RSDT.
            let rsdp = rsdp_virt as *const tables::Rsdp;
            // SAFETY: RSDP validation passed; rsdt_address is always valid.
            root_table_phys = u64::from(unsafe { (*rsdp).rsdt_address });
        }
    } else {
        let rsdp = rsdp_virt as *const tables::Rsdp;
        // SAFETY: RSDP validation passed; rsdt_address is always present.
        root_table_phys = u64::from(unsafe { (*rsdp).rsdt_address });
        use_xsdt = false;
    }

    // Ensure the root table is mapped in the HHDM before reading.
    // Limine's HHDM may not cover ACPI-reserved regions.
    // SAFETY: root_table_phys is from a validated RSDP.
    let root_table_virt = match unsafe { ensure_sdt_mapped(root_table_phys, hhdm_offset) } {
        Some(virt) => virt,
        None => {
            serial_println!(
                "[acpi] ERROR: {} at phys={:#x} has invalid header",
                if use_xsdt { "XSDT" } else { "RSDT" },
                root_table_phys
            );
            return;
        }
    };
    serial_println!(
        "[acpi] {} at phys={:#x} (virt={:#x})",
        if use_xsdt { "XSDT" } else { "RSDT" },
        root_table_phys,
        root_table_virt
    );

    // Validate the root table checksum.
    // SAFETY: ensure_sdt_mapped guarantees the full table is mapped.
    if !unsafe { tables::validate_sdt(root_table_virt) } {
        serial_println!("[acpi] ERROR: {} checksum validation failed",
            if use_xsdt { "XSDT" } else { "RSDT" });
        return;
    }

    // Enumerate all SDT entries and find tables we care about.
    let mut madt_phys: Option<u64> = None;
    let mut hpet_phys: Option<u64> = None;
    let mut fadt_phys: Option<u64> = None;
    let mut table_count: usize = 0;

    let process_entry = |phys: u64| {
        // Ensure this SDT is mapped in the HHDM before reading its header.
        // SAFETY: phys is from the RSDT/XSDT entry list.
        let virt = match unsafe { ensure_sdt_mapped(phys, hhdm_offset) } {
            Some(v) => v,
            None => {
                serial_println!("[acpi]   Table at phys={:#x}: invalid header, skipping", phys);
                return;
            }
        };
        let header = virt as *const tables::SdtHeader;
        // SAFETY: ensure_sdt_mapped guarantees the header is mapped and valid.
        let sig = unsafe { (*header).signature };
        let sig_str = core::str::from_utf8(&sig).unwrap_or("????");

        serial_println!("[acpi]   Table: \"{}\" at phys={:#x}", sig_str, phys);
        table_count += 1;

        // Register in general table registry for find_table() API.
        TABLE_REGISTRY.lock().insert(sig, phys);

        if &sig == b"APIC" {
            madt_phys = Some(phys);
        } else if &sig == b"HPET" {
            hpet_phys = Some(phys);
        } else if &sig == b"FACP" {
            fadt_phys = Some(phys);
        }
    };

    if use_xsdt {
        // SAFETY: root_table_virt is validated XSDT.
        unsafe { tables::for_each_xsdt_entry(root_table_virt, process_entry) };
    } else {
        // SAFETY: root_table_virt is validated RSDT.
        unsafe { tables::for_each_rsdt_entry(root_table_virt, process_entry) };
    }

    serial_println!("[acpi] Found {} table(s)", table_count);

    // Parse the MADT if found.
    if let Some(phys) = madt_phys {
        // Ensure the MADT is fully mapped (may already be from the
        // enumeration pass, but re-ensure for defensive safety).
        // SAFETY: phys is from the RSDT/XSDT.
        let virt = match unsafe { ensure_sdt_mapped(phys, hhdm_offset) } {
            Some(v) => v,
            None => {
                serial_println!("[acpi] ERROR: MADT at phys={:#x} has invalid header", phys);
                return;
            }
        };
        // Validate MADT checksum.
        // SAFETY: ensure_sdt_mapped guarantees the full table is mapped.
        if !unsafe { tables::validate_sdt(virt) } {
            serial_println!("[acpi] ERROR: MADT checksum failed");
            return;
        }
        // SAFETY: MADT is validated and fully mapped.
        let madt_info = unsafe { madt::parse_madt(virt) };
        *MADT_DATA.lock() = Some(madt_info);
    } else {
        serial_println!("[acpi] WARNING: No MADT found — using default hardware config");
    }

    // Store HPET table address for hpet::init().
    if let Some(phys) = hpet_phys {
        *HPET_TABLE_PHYS.lock() = Some(phys);
    }

    // Parse the FADT for power management info.
    if let Some(phys) = fadt_phys {
        // SAFETY: phys is from the RSDT/XSDT.
        let virt = match unsafe { ensure_sdt_mapped(phys, hhdm_offset) } {
            Some(v) => v,
            None => {
                serial_println!("[acpi] ERROR: FADT at phys={:#x} has invalid header", phys);
                serial_println!("[acpi] ACPI table parsing complete");
                return;
            }
        };
        // Validate FADT checksum.
        // SAFETY: ensure_sdt_mapped guarantees the full table is mapped.
        if !unsafe { tables::validate_sdt(virt) } {
            serial_println!("[acpi] WARNING: FADT checksum failed, skipping power management");
        } else {
            serial_println!("[acpi] Parsing FADT for power management...");
            // SAFETY: FADT is validated and fully mapped.
            let mut power_info = unsafe { fadt::parse_fadt(virt) };

            // Try to extract S5 sleep type from DSDT.
            if power_info.dsdt_phys != 0 {
                // SAFETY: dsdt_phys from a validated FADT.
                if let Some(dsdt_virt) = unsafe { ensure_sdt_mapped(power_info.dsdt_phys, hhdm_offset) } {
                    if unsafe { tables::validate_sdt(dsdt_virt) } {
                        if let Some(slp_typ) = unsafe { fadt::scan_dsdt_for_s5(dsdt_virt) } {
                            power_info.slp_typ_s5 = slp_typ;
                        } else {
                            serial_println!("[acpi]   DSDT: \\_S5_ not found, using default SLP_TYP=5");
                        }
                    } else {
                        serial_println!("[acpi]   DSDT checksum failed, using default SLP_TYP");
                    }
                }
            }

            // Store for power module.
            crate::power::set_power_info(&power_info);
        }
        *FADT_TABLE_PHYS.lock() = Some(phys);
    } else {
        serial_println!("[acpi] WARNING: No FADT found — power management unavailable");
    }

    serial_println!("[acpi] ACPI table parsing complete");
}

// ---------------------------------------------------------------------------
// Public query API
// ---------------------------------------------------------------------------

/// Get the physical base address of the first I/O APIC from the MADT.
///
/// Returns `None` if no MADT was parsed or no I/O APIC entries exist.
/// Falls back to the standard default (`0xFEC0_0000`) at the call site.
pub fn io_apic_address() -> Option<u64> {
    MADT_DATA
        .lock()
        .as_ref()
        .and_then(|madt| madt.io_apics.first())
        .map(|ioapic| u64::from(ioapic.address))
}

/// Get all I/O APIC descriptors from the MADT.
#[allow(dead_code)] // Public API for future IOMMU and multi-IOAPIC support.
pub fn io_apics() -> Vec<IoApicInfo> {
    MADT_DATA
        .lock()
        .as_ref()
        .map(|madt| madt.io_apics.clone())
        .unwrap_or_default()
}

/// Get all interrupt source overrides from the MADT.
///
/// These describe ISA IRQ → GSI remappings.  The IOAPIC driver should
/// apply these when programming redirection table entries.
pub fn interrupt_overrides() -> Vec<InterruptOverride> {
    MADT_DATA
        .lock()
        .as_ref()
        .map(|madt| madt.interrupt_overrides.clone())
        .unwrap_or_default()
}

/// Get the list of discovered processors from the MADT.
///
/// Useful for SMP initialization — each entry has the Local APIC ID
/// and whether the processor is enabled.
pub fn processors() -> Vec<ProcessorInfo> {
    MADT_DATA
        .lock()
        .as_ref()
        .map(|madt| madt.processors.clone())
        .unwrap_or_default()
}

/// Get Local APIC NMI routing information from the MADT.
#[allow(dead_code)] // Public API for NMI routing setup on APs.
pub fn local_apic_nmis() -> Vec<LocalApicNmi> {
    MADT_DATA
        .lock()
        .as_ref()
        .map(|madt| madt.local_apic_nmis.clone())
        .unwrap_or_default()
}

/// Check if the legacy dual-8259 PIC is present (PCAT_COMPAT flag).
#[allow(dead_code)] // Used when PIC mode detection is needed.
pub fn has_legacy_pic() -> bool {
    MADT_DATA
        .lock()
        .as_ref()
        .map(|madt| madt.pcat_compat)
        .unwrap_or(true) // Assume present if no MADT (safe default).
}

/// Get the physical address of the HPET ACPI table, if present.
///
/// Used by `hpet::init()` to parse the HPET description table and
/// discover the HPET's MMIO base address.
pub fn hpet_table_phys() -> Option<u64> {
    *HPET_TABLE_PHYS.lock()
}

/// Find an ACPI table by its 4-byte signature.
///
/// Returns the physical address of the table, or `None` if not found.
/// Call this after `acpi::init()` has completed.
///
/// Common signatures: b"SRAT" (NUMA), b"SLIT" (NUMA distances),
/// b"APIC" (MADT), b"FACP" (FADT), b"HPET", b"MCFG" (PCIe).
pub fn find_table(signature: &[u8; 4]) -> Option<u64> {
    TABLE_REGISTRY.lock().find(signature)
}

/// Get the number of enabled processors discovered in the MADT.
#[allow(dead_code)] // Public API for SMP topology queries.
pub fn processor_count() -> usize {
    MADT_DATA
        .lock()
        .as_ref()
        .map(|madt| madt.processors.iter().filter(|p| p.enabled).count())
        .unwrap_or(1) // At least the BSP.
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify ACPI parsing produced sane results.
pub fn self_test() -> Result<(), &'static str> {
    serial_println!("[acpi] Running self-test...");

    let guard = MADT_DATA.lock();
    let madt = guard.as_ref().ok_or("MADT not parsed")?;

    // Must have at least one processor.
    if madt.processors.is_empty() {
        return Err("No processors found in MADT");
    }

    // Must have at least one enabled processor (the BSP).
    let enabled = madt.processors.iter().filter(|p| p.enabled).count();
    if enabled == 0 {
        return Err("No enabled processors in MADT");
    }
    serial_println!("[acpi]   Processors: {} total, {} enabled", madt.processors.len(), enabled);

    // Should have at least one I/O APIC.
    if madt.io_apics.is_empty() {
        return Err("No I/O APICs found in MADT");
    }
    serial_println!(
        "[acpi]   I/O APIC(s): {} (primary at {:#x})",
        madt.io_apics.len(),
        madt.io_apics.first().map(|i| u64::from(i.address)).unwrap_or(0)
    );

    // Log interrupt overrides.
    serial_println!("[acpi]   Interrupt overrides: {}", madt.interrupt_overrides.len());
    for ovr in &madt.interrupt_overrides {
        serial_println!(
            "[acpi]     ISA {} → GSI {} (active_low={}, level={})",
            ovr.source_irq,
            ovr.gsi,
            ovr.is_active_low(),
            ovr.is_level_triggered()
        );
    }

    serial_println!("[acpi] Self-test PASSED");
    Ok(())
}

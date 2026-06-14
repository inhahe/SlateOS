//! IOMMU DMA remapping — page table infrastructure for device isolation.
//!
//! Implements Intel VT-d second-level page tables that control which
//! physical memory regions each PCI device can access via DMA.  This is
//! the core of driver sandboxing: even if a driver (or the hardware
//! itself) attempts DMA to arbitrary addresses, the IOMMU enforces the
//! page table and generates a fault for unauthorized accesses.
//!
//! ## Architecture (Intel VT-d)
//!
//! ```text
//! BDF (Bus:Device:Function)
//!   ↓ Root Table (256 entries, one per bus)
//!     ↓ Context Table (256 entries, one per devfn)
//!       ↓ Second-Level Page Tables (4-level, same as CPU page tables)
//!         ↓ Physical Address (DMA allowed)
//! ```
//!
//! - **Root Table**: 4 KiB, indexed by PCI bus number (0–255).
//!   Each entry points to a Context Table for that bus.
//! - **Context Table**: 4 KiB, indexed by device/function (32×8 = 256).
//!   Each entry points to a domain's second-level page table.
//! - **Second-Level Page Table**: Standard 4-level (PML4→PDP→PD→PT),
//!   translates device-virtual (bus) addresses to host physical addresses.
//! - **Domain**: A group of devices sharing the same I/O page table.
//!   Each driver gets its own domain containing only the physical
//!   pages it's allowed to DMA to.
//!
//! ## Register Interface
//!
//! Key VT-d MMIO registers (per hardware unit):
//! - `GCMD/GSTS` (0x18/0x1C): Global command/status (enable translation).
//! - `RTADDR` (0x20): Root table base physical address.
//! - `CCMD` (0x28): Context command (invalidation).
//! - `FSTS/FECTL` (0x34/0x38): Fault status and control.
//! - `IQH/IQT/IQA` (0x80/0x88/0x90): Invalidation queue.
//!
//! ## Usage
//!
//! ```text
//! // During boot (after IOMMU detection):
//! iommu_remap::init();
//!
//! // When a driver requests DMA access:
//! let domain = iommu_remap::create_domain()?;
//! iommu_remap::map_dma(domain, bus_addr, phys_addr, size, perms)?;
//! iommu_remap::attach_device(domain, bus, dev, func)?;
//!
//! // When DMA access is revoked:
//! iommu_remap::unmap_dma(domain, bus_addr, size)?;
//! iommu_remap::detach_device(domain, bus, dev, func)?;
//! iommu_remap::destroy_domain(domain)?;
//! ```
//!
//! ## References
//!
//! - Intel VT-d spec §3 "DMA Remapping" — hardware page table format
//! - Intel VT-d spec §10 "Register descriptions"
//! - Linux `drivers/iommu/intel/iommu.c` — production implementation
//! - Linux `drivers/iommu/intel/pasid.c` — PASID table management
//! - Fuchsia `zircon/kernel/dev/iommu/intel/` — clean IOMMU implementation

#![allow(dead_code)] // Public API awaiting driver framework integration.

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::mm::page_table;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// VT-d register offsets (per IOMMU unit).
mod reg {
    /// Version register (32-bit, RO).
    pub const VER: u64 = 0x00;
    /// Capability register (64-bit, RO).
    pub const CAP: u64 = 0x08;
    /// Extended capability register (64-bit, RO).
    pub const ECAP: u64 = 0x10;
    /// Global command register (32-bit, RW).
    pub const GCMD: u64 = 0x18;
    /// Global status register (32-bit, RO).
    pub const GSTS: u64 = 0x1C;
    /// Root table address register (64-bit, RW).
    pub const RTADDR: u64 = 0x20;
    /// Context command register (64-bit, RW).
    pub const CCMD: u64 = 0x28;
    /// Fault status register (32-bit, RW1C).
    pub const FSTS: u64 = 0x34;
    /// Fault event control register (32-bit, RW).
    pub const FECTL: u64 = 0x38;
    /// Fault event data register (32-bit, RW).
    pub const FEDATA: u64 = 0x3C;
    /// Fault event address register (32-bit, RW).
    pub const FEADDR: u64 = 0x40;
    /// Invalidation queue head (64-bit, RO).
    pub const IQH: u64 = 0x80;
    /// Invalidation queue tail (64-bit, RW).
    pub const IQT: u64 = 0x88;
    /// Invalidation queue address (64-bit, RW).
    pub const IQA: u64 = 0x90;
}

/// GCMD (Global Command) register bits.
mod gcmd {
    /// Translation enable.
    pub const TE: u32 = 1 << 31;
    /// Set root table pointer.
    pub const SRTP: u32 = 1 << 30;
    /// Interrupt remapping enable.
    pub const IRE: u32 = 1 << 25;
    /// Queued invalidation enable.
    pub const QIE: u32 = 1 << 26;
    /// Write buffer flush.
    pub const WBF: u32 = 1 << 27;
}

/// GSTS (Global Status) register bits.
mod gsts {
    /// Translation enable status.
    pub const TES: u32 = 1 << 31;
    /// Root table pointer status.
    pub const RTPS: u32 = 1 << 30;
    /// Interrupt remapping enable status.
    pub const IRES: u32 = 1 << 25;
    /// Queued invalidation enable status.
    pub const QIES: u32 = 1 << 26;
    /// Write buffer flush status.
    pub const WBFS: u32 = 1 << 27;
}

/// CAP register bit fields.
mod cap {
    /// Number of domain IDs supported (bits 0-2).
    /// NDOMS = 2^(4+N), where N is the field value.
    pub const NDOMS_MASK: u64 = 0x07;
    /// Supported Adjusted Guest Address Widths (bits 8-12).
    pub const SAGAW_SHIFT: u32 = 8;
    pub const SAGAW_MASK: u64 = 0x1F << 8;
    /// Required Write Buffer Flushing (bit 4).
    pub const RWBF: u64 = 1 << 4;
    /// Fault Recording offset in 128-bit units (bits 24-33).
    pub const FRO_SHIFT: u32 = 24;
    pub const FRO_MASK: u64 = 0x3FF << 24;
}

/// Second-level page table entry flags (VT-d).
/// These differ slightly from CPU page table flags.
mod slpte {
    /// Read permission.
    pub const READ: u64 = 1 << 0;
    /// Write permission.
    pub const WRITE: u64 = 1 << 1;
    /// Accessed (set by hardware on DMA read/write).
    pub const ACCESSED: u64 = 1 << 8;
    /// Dirty (set by hardware on DMA write).
    pub const DIRTY: u64 = 1 << 9;
    /// Super page (2 MiB or 1 GiB mapping at PD/PDP level).
    pub const SUPER_PAGE: u64 = 1 << 7;
}

/// Physical address mask for second-level PTEs (bits 12-51).
const PHYS_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// Maximum domains we support.
const MAX_DOMAINS: usize = 256;

/// Size of a hardware page (IOMMU page tables are always 4 KiB).
const IOMMU_PAGE_SIZE: usize = 4096;

/// Entries per IOMMU page table level.
const ENTRIES_PER_TABLE: usize = 512;

// ---------------------------------------------------------------------------
// DMA permission flags (public API)
// ---------------------------------------------------------------------------

/// Permissions for a DMA mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmaPerms(u8);

impl DmaPerms {
    /// Device can read from this region.
    pub const READ: Self = Self(1);
    /// Device can write to this region.
    pub const WRITE: Self = Self(2);
    /// Device can read and write.
    pub const READ_WRITE: Self = Self(3);

    /// Convert to SLPTE flags.
    fn to_pte_flags(self) -> u64 {
        let mut flags = 0u64;
        if self.0 & 1 != 0 {
            flags |= slpte::READ;
        }
        if self.0 & 2 != 0 {
            flags |= slpte::WRITE;
        }
        flags
    }
}

// ---------------------------------------------------------------------------
// Domain management
// ---------------------------------------------------------------------------

/// IOMMU domain identifier.
pub type DomainId = u16;

/// A DMA remapping domain.
///
/// Each domain has its own second-level page table that controls which
/// physical addresses devices in this domain can access.
struct Domain {
    /// Domain ID (used in context table entries).
    id: DomainId,
    /// Physical address of the domain's PML4 (4th-level page table).
    pml4_phys: u64,
    /// Whether this domain slot is allocated.
    active: bool,
    /// Number of devices attached to this domain.
    device_count: u16,
    /// Total mapped pages (for statistics).
    mapped_pages: u64,
}

impl Domain {
    const EMPTY: Self = Self {
        id: 0,
        pml4_phys: 0,
        active: false,
        device_count: 0,
        mapped_pages: 0,
    };
}

/// Domain table.
static DOMAINS: Mutex<[Domain; MAX_DOMAINS]> = Mutex::new([Domain::EMPTY; MAX_DOMAINS]);

/// Next domain ID to allocate.
static NEXT_DOMAIN_ID: AtomicU16 = AtomicU16::new(1);

// ---------------------------------------------------------------------------
// Root and Context Tables
// ---------------------------------------------------------------------------

/// Physical address of the root table (shared by all IOMMU units).
static ROOT_TABLE_PHYS: AtomicU64 = AtomicU64::new(0);

/// Whether DMA remapping is initialized and active.
static REMAP_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Total DMA faults observed.
static TOTAL_FAULTS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Root Table Entry (128 bits)
// ---------------------------------------------------------------------------

/// Root table entry (one per PCI bus, 128 bits = 16 bytes).
///
/// Intel VT-d spec §9.1:
/// - Bits 0: Present
/// - Bits 12-63 (low qword): Context table pointer (4 KiB aligned)
/// - High qword: reserved
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct RootEntry {
    lo: u64,
    hi: u64,
}

impl RootEntry {
    const EMPTY: Self = Self { lo: 0, hi: 0 };

    /// Whether this entry is present.
    fn is_present(&self) -> bool {
        self.lo & 1 != 0
    }

    /// Get the context table physical address.
    fn context_table_phys(&self) -> u64 {
        self.lo & PHYS_ADDR_MASK
    }

    /// Create a present root entry pointing to a context table.
    fn new(context_table_phys: u64) -> Self {
        Self {
            lo: (context_table_phys & PHYS_ADDR_MASK) | 1, // Present bit
            hi: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Context Table Entry (128 bits)
// ---------------------------------------------------------------------------

/// Context table entry (one per device/function, 128 bits = 16 bytes).
///
/// Intel VT-d spec §9.3:
/// - Low qword bits 0: Present
/// - Low qword bits 1: Fault Processing Disable
/// - Low qword bits 2-3: Translation Type (00 = untranslated→translated)
/// - Low qword bits 12-63: Second-level page table pointer
/// - High qword bits 0-15: Domain ID
/// - High qword bits 16-18: Address Width (010 = 48-bit, 4-level)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct ContextEntry {
    lo: u64,
    hi: u64,
}

impl ContextEntry {
    const EMPTY: Self = Self { lo: 0, hi: 0 };

    /// Whether this entry is present.
    fn is_present(&self) -> bool {
        self.lo & 1 != 0
    }

    /// Get the second-level page table physical address.
    fn slpt_phys(&self) -> u64 {
        self.lo & PHYS_ADDR_MASK
    }

    /// Get the domain ID.
    fn domain_id(&self) -> u16 {
        (self.hi & 0xFFFF) as u16
    }

    /// Create a present context entry.
    ///
    /// - `slpt_phys`: physical address of the domain's PML4.
    /// - `domain_id`: domain ID for this device.
    /// - Translation type = 0 (untranslated requests → second-level translated).
    /// - Address width = 2 (48-bit, 4-level page table: AGAW = 48).
    fn new(slpt_phys: u64, domain_id: u16) -> Self {
        let lo = (slpt_phys & PHYS_ADDR_MASK) | 1; // Present, TT=00
        // High qword: domain ID in bits 0-15, AGAW=010 in bits 16-18
        // AGAW=010 means 4-level (48-bit) page table walk.
        let hi = (domain_id as u64) | (0b010 << 16);
        Self { lo, hi }
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize IOMMU DMA remapping.
///
/// Allocates the root table, programs each detected IOMMU unit, and
/// enables translation globally.  After this, all DMA from PCI devices
/// goes through the IOMMU page tables.
///
/// Must be called after `iommu::init()` (detection) and `mm::init()`
/// (page allocator available).
pub fn init() -> KernelResult<()> {
    if !crate::iommu::is_available() {
        serial_println!("[iommu_remap] No IOMMU hardware — skipping DMA remapping init");
        return Ok(());
    }

    if crate::iommu::vendor() != crate::iommu::IommuVendor::IntelVtd {
        serial_println!("[iommu_remap] AMD-Vi remapping not yet implemented");
        return Ok(());
    }

    serial_println!("[iommu_remap] Initializing Intel VT-d DMA remapping...");

    // Allocate and zero the root table (4 KiB = 256 × 16-byte entries).
    let root_table = page_table::alloc_pt_page()?;
    ROOT_TABLE_PHYS.store(root_table, Ordering::Release);

    serial_println!("[iommu_remap] Root table at phys {:#x}", root_table);

    // Program each IOMMU unit.
    let unit_count = crate::iommu::unit_count();
    for i in 0..unit_count {
        let unit = match crate::iommu::get_unit(i as usize) {
            Some(u) => u,
            None => continue,
        };

        if let Err(e) = init_unit(unit.register_base) {
            serial_println!(
                "[iommu_remap] WARNING: failed to init unit {} at {:#x}: {:?}",
                i, unit.register_base, e
            );
            // Continue with other units — partial protection is better
            // than none.
        }
    }

    REMAP_ACTIVE.store(true, Ordering::Release);
    serial_println!("[iommu_remap] DMA remapping active");
    Ok(())
}

/// Initialize a single IOMMU hardware unit.
///
/// Programs the root table pointer and enables translation.
fn init_unit(register_base: u64) -> KernelResult<()> {
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let mmio = register_base + hhdm;

    // Read version.
    let version = read_reg32(mmio, reg::VER);
    let major = (version >> 4) & 0xF;
    let minor = version & 0xF;
    serial_println!("[iommu_remap]   Unit at {:#x}: VT-d version {}.{}", register_base, major, minor);

    // Read capabilities.
    let cap_val = read_reg64(mmio, reg::CAP);
    let ndoms_raw = cap_val & cap::NDOMS_MASK;
    let ndoms = 1u32 << (4 + ndoms_raw as u32);
    let sagaw = (cap_val & cap::SAGAW_MASK) >> cap::SAGAW_SHIFT;
    let need_wbf = (cap_val & cap::RWBF) != 0;

    serial_println!("[iommu_remap]   CAP: ndoms={}, sagaw={:#x}, need_wbf={}", ndoms, sagaw, need_wbf);

    // Verify 4-level (48-bit) page table support (SAGAW bit 2).
    if sagaw & 0x04 == 0 {
        serial_println!("[iommu_remap]   ERROR: 48-bit (4-level) AGAW not supported");
        return Err(KernelError::NotSupported);
    }

    // Set root table pointer.
    let root_table = ROOT_TABLE_PHYS.load(Ordering::Acquire);
    write_reg64(mmio, reg::RTADDR, root_table);

    // Issue SRTP command and wait for completion.
    let gsts = read_reg32(mmio, reg::GSTS);
    write_reg32(mmio, reg::GCMD, gsts | gcmd::SRTP);
    wait_for_status(mmio, gsts::RTPS, true)?;

    serial_println!("[iommu_remap]   Root table pointer set");

    // Flush write buffer if needed.
    if need_wbf {
        write_reg32(mmio, reg::GCMD, gsts | gcmd::WBF);
        wait_for_status(mmio, gsts::WBFS, false)?; // WBF clears when done
    }

    // Enable translation.
    let gsts = read_reg32(mmio, reg::GSTS);
    write_reg32(mmio, reg::GCMD, gsts | gcmd::TE);
    wait_for_status(mmio, gsts::TES, true)?;

    serial_println!("[iommu_remap]   Translation enabled");
    Ok(())
}

// ---------------------------------------------------------------------------
// Domain API
// ---------------------------------------------------------------------------

/// Create a new DMA remapping domain.
///
/// Allocates a fresh second-level page table (initially empty — no DMA
/// permitted until pages are explicitly mapped).
pub fn create_domain() -> KernelResult<DomainId> {
    let pml4 = page_table::alloc_pt_page()?;
    let id = NEXT_DOMAIN_ID.fetch_add(1, Ordering::Relaxed);

    if id as usize >= MAX_DOMAINS {
        serial_println!("[iommu_remap] ERROR: domain ID overflow");
        return Err(KernelError::OutOfMemory);
    }

    let mut domains = DOMAINS.lock();
    let slot = id as usize;
    domains[slot] = Domain {
        id,
        pml4_phys: pml4,
        active: true,
        device_count: 0,
        mapped_pages: 0,
    };

    serial_println!("[iommu_remap] Created domain {} (PML4={:#x})", id, pml4);
    Ok(id)
}

/// Destroy a DMA remapping domain.
///
/// Fails if devices are still attached.
pub fn destroy_domain(domain_id: DomainId) -> KernelResult<()> {
    // Phase 1: validate and detach the page table from the domain table
    // under the DOMAINS lock.  Marking the domain inactive here makes every
    // subsequent map_dma / attach_device reject it, so no thread can add a
    // new mapping while we walk and free the tree in phase 2.  We capture
    // the root and release the lock *before* freeing, both to keep the
    // critical section short and to avoid holding DOMAINS across the
    // PT_PAGE_POOL lock taken by free_pt_page.
    let pml4_phys = {
        let mut domains = DOMAINS.lock();
        let slot = domain_id as usize;

        if slot >= MAX_DOMAINS || !domains[slot].active {
            return Err(KernelError::InvalidArgument);
        }
        if domains[slot].device_count > 0 {
            return Err(KernelError::DeviceBusy);
        }

        let pml4 = domains[slot].pml4_phys;
        domains[slot].active = false;
        domains[slot].mapped_pages = 0;
        domains[slot].pml4_phys = 0;
        pml4
    };

    // Phase 2: reclaim the domain's entire second-level page-table
    // structure (PML4 through PT).  The caller-owned DMA target frames
    // mapped by the PT leaves are not touched — only the structure pages
    // this domain allocated.  We can only walk the tree once the HHDM is
    // available; without it the structure pages are unreachable, so we leak
    // them rather than risk a bad access (this only happens before MM init,
    // which never destroys domains in practice).
    if pml4_phys != 0 {
        if let Some(hhdm) = page_table::hhdm() {
            // The domain is inactive with no devices attached, so no IOMMU
            // hardware walk and no concurrent map_dma can race this teardown.
            // SAFETY: pml4_phys was returned by alloc_pt_page in
            // create_domain and is now detached from the domain table.
            unsafe { free_slpt(pml4_phys, hhdm); }
        } else {
            serial_println!(
                "[iommu_remap] WARNING: HHDM unavailable; leaking SLPT pages for domain {}",
                domain_id
            );
        }
    }

    serial_println!("[iommu_remap] Destroyed domain {}", domain_id);
    Ok(())
}

// ---------------------------------------------------------------------------
// DMA mapping
// ---------------------------------------------------------------------------

/// Map a physical address range into a domain's DMA address space.
///
/// After this call, devices in this domain can DMA to `bus_addr` and
/// it will be translated to `phys_addr`.  The mapping covers `size`
/// bytes (rounded up to 4 KiB pages).
///
/// ## Arguments
///
/// - `domain_id`: target domain.
/// - `bus_addr`: device-visible (bus) address.
/// - `phys_addr`: host physical address to map to.
/// - `size`: size in bytes (rounded up to 4 KiB).
/// - `perms`: read/write permissions.
pub fn map_dma(
    domain_id: DomainId,
    bus_addr: u64,
    phys_addr: u64,
    size: usize,
    perms: DmaPerms,
) -> KernelResult<()> {
    if size == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let pml4_phys = {
        let domains = DOMAINS.lock();
        let slot = domain_id as usize;
        if slot >= MAX_DOMAINS || !domains[slot].active {
            return Err(KernelError::InvalidArgument);
        }
        domains[slot].pml4_phys
    };

    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let flags = perms.to_pte_flags();
    let pages = size.div_ceil(IOMMU_PAGE_SIZE);

    for i in 0..pages {
        let offset = (i as u64) * (IOMMU_PAGE_SIZE as u64);
        let ba = bus_addr + offset;
        let pa = phys_addr + offset;
        map_4k_slpt(pml4_phys, ba, pa, flags, hhdm)?;
    }

    // Update mapped page count.
    let mut domains = DOMAINS.lock();
    let slot = domain_id as usize;
    domains[slot].mapped_pages = domains[slot].mapped_pages.saturating_add(pages as u64);

    Ok(())
}

/// Unmap a DMA address range from a domain.
///
/// After this call, devices in this domain can no longer DMA to
/// `bus_addr..bus_addr+size`.  An IOTLB flush is issued.
pub fn unmap_dma(
    domain_id: DomainId,
    bus_addr: u64,
    size: usize,
) -> KernelResult<()> {
    if size == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let pml4_phys = {
        let domains = DOMAINS.lock();
        let slot = domain_id as usize;
        if slot >= MAX_DOMAINS || !domains[slot].active {
            return Err(KernelError::InvalidArgument);
        }
        domains[slot].pml4_phys
    };

    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let pages = size.div_ceil(IOMMU_PAGE_SIZE);

    for i in 0..pages {
        let offset = (i as u64) * (IOMMU_PAGE_SIZE as u64);
        let ba = bus_addr + offset;
        unmap_4k_slpt(pml4_phys, ba, hhdm);
    }

    // Update mapped page count.
    {
        let mut domains = DOMAINS.lock();
        let slot = domain_id as usize;
        domains[slot].mapped_pages = domains[slot].mapped_pages.saturating_sub(pages as u64);
    }

    // Flush IOTLB (invalidate cached translations).
    flush_iotlb_domain(domain_id);

    Ok(())
}

// ---------------------------------------------------------------------------
// Device attachment
// ---------------------------------------------------------------------------

/// Attach a PCI device to a domain.
///
/// Programs the IOMMU context table so that DMA from this device
/// (identified by Bus:Device:Function) goes through the domain's
/// page table.
pub fn attach_device(
    domain_id: DomainId,
    bus: u8,
    device: u8,
    function: u8,
) -> KernelResult<()> {
    if device >= 32 || function >= 8 {
        return Err(KernelError::InvalidArgument);
    }

    let pml4_phys = {
        let domains = DOMAINS.lock();
        let slot = domain_id as usize;
        if slot >= MAX_DOMAINS || !domains[slot].active {
            return Err(KernelError::InvalidArgument);
        }
        domains[slot].pml4_phys
    };

    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let root_table = ROOT_TABLE_PHYS.load(Ordering::Acquire);

    if root_table == 0 {
        return Err(KernelError::NotSupported);
    }

    // Get or allocate context table for this bus.
    let context_table = get_or_alloc_context_table(root_table, bus, hhdm)?;

    // Program the context entry for this device/function.
    let devfn = ((device as usize) << 3) | (function as usize);
    let entry = ContextEntry::new(pml4_phys, domain_id);

    // SAFETY: context_table is a valid 4 KiB page we own, devfn < 256,
    // and context entries are 16 bytes (256 entries fit in 4 KiB).
    unsafe {
        let ctx_virt = (context_table + hhdm) as *mut ContextEntry;
        let existing = core::ptr::read(ctx_virt.add(devfn));
        if existing.is_present() {
            // Device already attached — check if same domain.
            if existing.domain_id() == domain_id {
                return Ok(()); // Idempotent.
            }
            return Err(KernelError::AlreadyExists);
        }
        core::ptr::write(ctx_virt.add(devfn), entry);
    }

    // Update device count.
    {
        let mut domains = DOMAINS.lock();
        let slot = domain_id as usize;
        domains[slot].device_count = domains[slot].device_count.saturating_add(1);
    }

    // Flush context cache.
    flush_context_cache();

    serial_println!(
        "[iommu_remap] Attached {:02x}:{:02x}.{} to domain {}",
        bus, device, function, domain_id
    );
    Ok(())
}

/// Detach a PCI device from its domain.
///
/// Clears the context table entry, preventing any further DMA from
/// this device.
pub fn detach_device(
    domain_id: DomainId,
    bus: u8,
    device: u8,
    function: u8,
) -> KernelResult<()> {
    if device >= 32 || function >= 8 {
        return Err(KernelError::InvalidArgument);
    }

    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let root_table = ROOT_TABLE_PHYS.load(Ordering::Acquire);

    if root_table == 0 {
        return Err(KernelError::NotSupported);
    }

    // Read root table entry for this bus.
    // SAFETY: root_table is a valid 4 KiB page, bus < 256.
    let root_entry = unsafe {
        let root_virt = (root_table + hhdm) as *const RootEntry;
        core::ptr::read(root_virt.add(bus as usize))
    };

    if !root_entry.is_present() {
        return Err(KernelError::NotFound);
    }

    let context_table = root_entry.context_table_phys();
    let devfn = ((device as usize) << 3) | (function as usize);

    // SAFETY: context_table is a valid page, devfn < 256.
    let existing = unsafe {
        let ctx_virt = (context_table + hhdm) as *const ContextEntry;
        core::ptr::read(ctx_virt.add(devfn))
    };

    if !existing.is_present() || existing.domain_id() != domain_id {
        return Err(KernelError::NotFound);
    }

    // Clear the entry.
    // SAFETY: Same page, valid index.
    unsafe {
        let ctx_virt = (context_table + hhdm) as *mut ContextEntry;
        core::ptr::write(ctx_virt.add(devfn), ContextEntry::EMPTY);
    }

    // Update device count.
    {
        let mut domains = DOMAINS.lock();
        let slot = domain_id as usize;
        domains[slot].device_count = domains[slot].device_count.saturating_sub(1);
    }

    // Flush caches.
    flush_context_cache();
    flush_iotlb_domain(domain_id);

    serial_println!(
        "[iommu_remap] Detached {:02x}:{:02x}.{} from domain {}",
        bus, device, function, domain_id
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Second-level page table operations
// ---------------------------------------------------------------------------

/// Map a single 4 KiB page in a domain's second-level page table.
///
/// Creates intermediate levels as needed.
fn map_4k_slpt(
    pml4_phys: u64,
    bus_addr: u64,
    phys_addr: u64,
    flags: u64,
    hhdm: u64,
) -> KernelResult<()> {
    // Extract page table indices from bus_addr (same layout as x86_64 VA).
    let pml4_idx = ((bus_addr >> 39) & 0x1FF) as usize;
    let pdpt_idx = ((bus_addr >> 30) & 0x1FF) as usize;
    let pd_idx = ((bus_addr >> 21) & 0x1FF) as usize;
    let pt_idx = ((bus_addr >> 12) & 0x1FF) as usize;

    // Walk PML4 → PDPT.
    let pdpt_phys = walk_or_create(pml4_phys, pml4_idx, hhdm)?;
    // Walk PDPT → PD.
    let pd_phys = walk_or_create(pdpt_phys, pdpt_idx, hhdm)?;
    // Walk PD → PT.
    let pt_phys = walk_or_create(pd_phys, pd_idx, hhdm)?;

    // Write the leaf entry.
    let entry = (phys_addr & PHYS_ADDR_MASK) | flags;

    // SAFETY: pt_phys is a valid 4 KiB page, pt_idx < 512.
    unsafe {
        let pt_virt = (pt_phys + hhdm) as *mut u64;
        core::ptr::write(pt_virt.add(pt_idx), entry);
    }

    Ok(())
}

/// Unmap a single 4 KiB page in a domain's SLPT.
fn unmap_4k_slpt(pml4_phys: u64, bus_addr: u64, hhdm: u64) {
    let pml4_idx = ((bus_addr >> 39) & 0x1FF) as usize;
    let pdpt_idx = ((bus_addr >> 30) & 0x1FF) as usize;
    let pd_idx = ((bus_addr >> 21) & 0x1FF) as usize;
    let pt_idx = ((bus_addr >> 12) & 0x1FF) as usize;

    // Walk down without allocating — if any level is missing, the
    // page was never mapped.
    let pdpt_phys = match walk_existing(pml4_phys, pml4_idx, hhdm) {
        Some(p) => p,
        None => return,
    };
    let pd_phys = match walk_existing(pdpt_phys, pdpt_idx, hhdm) {
        Some(p) => p,
        None => return,
    };
    let pt_phys = match walk_existing(pd_phys, pd_idx, hhdm) {
        Some(p) => p,
        None => return,
    };

    // Clear the leaf entry.
    // SAFETY: pt_phys is a valid page, pt_idx < 512.
    unsafe {
        let pt_virt = (pt_phys + hhdm) as *mut u64;
        core::ptr::write(pt_virt.add(pt_idx), 0);
    }
}

/// Walk one level of the SLPT, allocating a new table if the entry
/// is not present.
fn walk_or_create(table_phys: u64, index: usize, hhdm: u64) -> KernelResult<u64> {
    // SAFETY: table_phys is a valid 4 KiB page, index < 512.
    let entry = unsafe {
        let virt = (table_phys + hhdm) as *const u64;
        core::ptr::read(virt.add(index))
    };

    // Check present (bit 0 for READ or bit 1 for WRITE — either means
    // the intermediate entry is valid for VT-d SLPTs).
    if entry & (slpte::READ | slpte::WRITE) != 0 {
        // Entry present — extract physical address.
        return Ok(entry & PHYS_ADDR_MASK);
    }

    // Allocate a new page table level.
    let new_table = page_table::alloc_pt_page()?;

    // Write the entry with R+W (intermediate entries need both for
    // the walk to succeed for any access type).
    let new_entry = (new_table & PHYS_ADDR_MASK) | slpte::READ | slpte::WRITE;

    // SAFETY: Valid page, valid index.
    unsafe {
        let virt = (table_phys + hhdm) as *mut u64;
        core::ptr::write(virt.add(index), new_entry);
    }

    Ok(new_table)
}

/// Walk one level of the SLPT without allocating.
/// Returns `None` if the entry is not present.
fn walk_existing(table_phys: u64, index: usize, hhdm: u64) -> Option<u64> {
    // SAFETY: table_phys is valid, index < 512.
    let entry = unsafe {
        let virt = (table_phys + hhdm) as *const u64;
        core::ptr::read(virt.add(index))
    };

    if entry & (slpte::READ | slpte::WRITE) != 0 {
        Some(entry & PHYS_ADDR_MASK)
    } else {
        None
    }
}

/// Read the raw SLPTE at `index` in the table at `table_phys`.
fn read_slpte(table_phys: u64, index: usize, hhdm: u64) -> u64 {
    // SAFETY: callers pass a valid 4 KiB table page and index < 512; the
    // HHDM maps the physical page.  An 8-byte read at an 8-byte-aligned
    // offset within a 4 KiB-aligned page is well-formed.
    unsafe {
        let virt = (table_phys + hhdm) as *const u64;
        core::ptr::read(virt.add(index))
    }
}

/// Decode an intermediate SLPTE into the physical address of the
/// next-level table it points to, or `None` if the entry is absent or is
/// a leaf (super-page) mapping rather than a pointer to a lower table.
///
/// A present entry with the [`slpte::SUPER_PAGE`] bit set is a large-page
/// leaf that maps caller-owned DMA memory directly — it does **not** point
/// to a table page we allocated, so it is skipped (neither recursed into
/// nor freed).  Our `map_4k_slpt` never creates super-pages, but the guard
/// keeps the teardown correct if that ever changes.
fn slpte_table_addr(entry: u64) -> Option<u64> {
    if entry & (slpte::READ | slpte::WRITE) == 0 {
        return None;
    }
    if entry & slpte::SUPER_PAGE != 0 {
        return None;
    }
    Some(entry & PHYS_ADDR_MASK)
}

/// Walk a domain's second-level page table and return every structure page
/// (PML4, PDPT, PD, PT) to the page-table page pool.
///
/// The leaf entries of the PT level point to **caller-owned** DMA target
/// frames (the physical pages a driver asked to be DMA-visible); those are
/// *not* freed here — only the four levels of SLPT structure pages the
/// domain allocated via [`walk_or_create`] are reclaimed.  This is the
/// proper fix for the prior leak where `destroy_domain` dropped the whole
/// tree on the floor.
///
/// # Safety
///
/// The domain must be inactive with no devices attached and no in-flight
/// DMA, so no IOMMU hardware page-table walk can race this teardown.  After
/// this call every structure page is back in the pool and must not be
/// touched through the freed `pml4_phys`.
unsafe fn free_slpt(pml4_phys: u64, hhdm: u64) {
    // Level 4 (PML4) → level 3 (PDPT) → level 2 (PD) → level 1 (PT).
    for i4 in 0..ENTRIES_PER_TABLE {
        let pdpt = match slpte_table_addr(read_slpte(pml4_phys, i4, hhdm)) {
            Some(p) => p,
            None => continue,
        };
        for i3 in 0..ENTRIES_PER_TABLE {
            let pd = match slpte_table_addr(read_slpte(pdpt, i3, hhdm)) {
                Some(p) => p,
                None => continue,
            };
            for i2 in 0..ENTRIES_PER_TABLE {
                let pt = match slpte_table_addr(read_slpte(pd, i2, hhdm)) {
                    Some(p) => p,
                    None => continue,
                };
                // PT-level leaf entries map caller-owned data frames, which
                // we must not free; only the PT page itself is ours.
                // SAFETY: `pt` is a structure page we allocated via
                // walk_or_create and is no longer referenced once the PD
                // entry above is dropped with the PD page below.
                unsafe { page_table::free_pt_page(pt); }
            }
            // SAFETY: `pd` is a domain-owned structure page; all its PT
            // children were just freed and nothing else references it.
            unsafe { page_table::free_pt_page(pd); }
        }
        // SAFETY: `pdpt` is a domain-owned structure page; all its PD
        // children were just freed.
        unsafe { page_table::free_pt_page(pdpt); }
    }
    // SAFETY: the PML4 root is domain-owned; all lower levels are freed.
    unsafe { page_table::free_pt_page(pml4_phys); }
}

// ---------------------------------------------------------------------------
// Context table helpers
// ---------------------------------------------------------------------------

/// Get or allocate a context table for a PCI bus.
fn get_or_alloc_context_table(root_table: u64, bus: u8, hhdm: u64) -> KernelResult<u64> {
    // Read existing root entry.
    // SAFETY: root_table is a valid 4 KiB page, bus < 256.
    // Root table has 256 entries × 16 bytes = 4096 bytes.
    let existing = unsafe {
        let root_virt = (root_table + hhdm) as *const RootEntry;
        core::ptr::read(root_virt.add(bus as usize))
    };

    if existing.is_present() {
        return Ok(existing.context_table_phys());
    }

    // Allocate a new context table (4 KiB = 256 × 16-byte entries).
    let ctx_table = page_table::alloc_pt_page()?;

    // Write root entry.
    let entry = RootEntry::new(ctx_table);
    // SAFETY: Valid page, valid index.
    unsafe {
        let root_virt = (root_table + hhdm) as *mut RootEntry;
        core::ptr::write(root_virt.add(bus as usize), entry);
    }

    Ok(ctx_table)
}

// ---------------------------------------------------------------------------
// Hardware register access
// ---------------------------------------------------------------------------

/// Read a 32-bit MMIO register.
#[inline]
fn read_reg32(mmio_base: u64, offset: u64) -> u32 {
    // SAFETY: IOMMU registers are memory-mapped at mmio_base (HHDM-adjusted).
    // The caller must have verified that mmio_base is a valid IOMMU register
    // page.  volatile ensures we actually read from hardware.
    unsafe {
        let ptr = (mmio_base + offset) as *const u32;
        core::ptr::read_volatile(ptr)
    }
}

/// Write a 32-bit MMIO register.
#[inline]
fn write_reg32(mmio_base: u64, offset: u64, value: u32) {
    // SAFETY: Same as read_reg32.
    unsafe {
        let ptr = (mmio_base + offset) as *mut u32;
        core::ptr::write_volatile(ptr, value);
    }
}

/// Read a 64-bit MMIO register.
#[inline]
fn read_reg64(mmio_base: u64, offset: u64) -> u64 {
    // SAFETY: Same as read_reg32 but for 64-bit registers.
    unsafe {
        let ptr = (mmio_base + offset) as *const u64;
        core::ptr::read_volatile(ptr)
    }
}

/// Write a 64-bit MMIO register.
#[inline]
fn write_reg64(mmio_base: u64, offset: u64, value: u64) {
    // SAFETY: Same as read_reg32 but for 64-bit registers.
    unsafe {
        let ptr = (mmio_base + offset) as *mut u64;
        core::ptr::write_volatile(ptr, value);
    }
}

/// Wait for a GSTS bit to reach the expected state.
///
/// Spins for up to ~100ms (1M iterations with memory barriers).
fn wait_for_status(mmio_base: u64, bit: u32, expected_set: bool) -> KernelResult<()> {
    for _ in 0..1_000_000 {
        let gsts = read_reg32(mmio_base, reg::GSTS);
        let is_set = (gsts & bit) != 0;
        if is_set == expected_set {
            return Ok(());
        }
        core::hint::spin_loop();
    }

    serial_println!(
        "[iommu_remap] Timeout waiting for GSTS bit {:#x} = {}",
        bit, expected_set
    );
    Err(KernelError::TimedOut)
}

// ---------------------------------------------------------------------------
// Cache invalidation
// ---------------------------------------------------------------------------

/// Flush the context cache on all IOMMU units.
///
/// Must be called after modifying context table entries.
fn flush_context_cache() {
    let unit_count = crate::iommu::unit_count();
    let hhdm = match page_table::hhdm() {
        Some(h) => h,
        None => return,
    };

    for i in 0..unit_count {
        let unit = match crate::iommu::get_unit(i as usize) {
            Some(u) => u,
            None => continue,
        };
        let mmio = unit.register_base + hhdm;

        // Issue a global context invalidation via the CCMD register.
        // Bits 63: Invalidation Command Completion (ICC) — set to trigger.
        // Bits 61-62: Invlaidation Request Granularity = 01 (global).
        let ccmd_val: u64 = (1u64 << 63) | (0b01u64 << 61);
        write_reg64(mmio, reg::CCMD, ccmd_val);

        // Wait for ICC bit to clear (hardware clears it on completion).
        for _ in 0..100_000 {
            let val = read_reg64(mmio, reg::CCMD);
            if val & (1u64 << 63) == 0 {
                break;
            }
            core::hint::spin_loop();
        }
    }
}

/// Flush IOTLB for a specific domain on all units.
///
/// Must be called after unmapping pages.
fn flush_iotlb_domain(_domain_id: DomainId) {
    // The IOTLB invalidation register is at an offset determined by
    // the ECAP register's IRO field.  For simplicity, we issue a
    // global IOTLB invalidation via the context cache (which implicitly
    // invalidates IOTLB entries for the affected contexts).
    //
    // A proper implementation would use the IOTLB registers at the
    // ECAP.IRO offset for domain-selective invalidation.
    //
    // For now, context cache flush serves as a conservative invalidation.
    flush_context_cache();
}

// ---------------------------------------------------------------------------
// Query API
// ---------------------------------------------------------------------------

/// Whether DMA remapping is active.
#[must_use]
pub fn is_active() -> bool {
    REMAP_ACTIVE.load(Ordering::Acquire)
}

/// Get the total number of DMA faults observed.
#[must_use]
pub fn fault_count() -> u64 {
    TOTAL_FAULTS.load(Ordering::Relaxed)
}

/// Statistics for the DMA remapping subsystem.
#[derive(Debug, Clone, Copy)]
pub struct RemapStats {
    /// Whether remapping is active.
    pub active: bool,
    /// Total domains allocated.
    pub total_domains: u16,
    /// Active (non-destroyed) domains.
    pub active_domains: u16,
    /// Total mapped pages across all domains.
    pub total_mapped_pages: u64,
    /// Total DMA faults.
    pub total_faults: u64,
}

/// Get DMA remapping statistics.
#[must_use]
pub fn stats() -> RemapStats {
    let domains = DOMAINS.lock();
    let active_domains = domains.iter().filter(|d| d.active).count() as u16;
    let total_mapped = domains.iter()
        .filter(|d| d.active)
        .map(|d| d.mapped_pages)
        .sum();

    RemapStats {
        active: REMAP_ACTIVE.load(Ordering::Relaxed),
        total_domains: NEXT_DOMAIN_ID.load(Ordering::Relaxed).saturating_sub(1),
        active_domains,
        total_mapped_pages: total_mapped,
        total_faults: TOTAL_FAULTS.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Fault handling
// ---------------------------------------------------------------------------

/// Handle an IOMMU DMA fault.
///
/// Called from the IOMMU fault interrupt handler (or polled from the
/// fault status register).  Records the fault and logs details.
pub fn handle_fault(unit_index: u8) {
    let unit = match crate::iommu::get_unit(unit_index as usize) {
        Some(u) => u,
        None => return,
    };

    let hhdm = match page_table::hhdm() {
        Some(h) => h,
        None => return,
    };

    let mmio = unit.register_base + hhdm;
    let fsts = read_reg32(mmio, reg::FSTS);

    if fsts & 0x01 != 0 {
        // Primary Fault — a device attempted unauthorized DMA.
        TOTAL_FAULTS.fetch_add(1, Ordering::Relaxed);

        // Read the fault recording register (at FRO offset from CAP).
        let cap_val = read_reg64(mmio, reg::CAP);
        let fro = (cap_val & cap::FRO_MASK) >> cap::FRO_SHIFT;
        let fr_base = mmio + fro * 16; // Each fault record is 128 bits.

        // Read fault info (simplified — full parsing would extract
        // source BDF, fault reason, address, etc.).
        let fr_lo = read_reg64(fr_base, 0);
        let fr_hi = read_reg64(fr_base, 8);

        let fault_addr = fr_lo & 0xFFFF_FFFF_FFFF_F000;
        let source_id = ((fr_hi >> 8) & 0xFFFF) as u16;
        let reason = (fr_hi & 0xFF) as u8;

        serial_println!(
            "[iommu_remap] DMA FAULT: addr={:#x} src={:04x} reason={}",
            fault_addr, source_id, reason
        );

        // Clear the fault by writing 1 to PFO (bit 0 of FSTS).
        write_reg32(mmio, reg::FSTS, 0x01);
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for IOMMU DMA remapping.
///
/// Tests the page table manipulation logic without requiring actual
/// IOMMU hardware (uses software-only page table operations).
pub fn self_test() -> KernelResult<()> {
    serial_println!("[iommu_remap] Running self-test...");

    // Test 1: Domain creation.
    let domain = create_domain()?;
    assert!(domain > 0, "domain ID > 0");
    serial_println!("[iommu_remap]   Domain create: OK (id={})", domain);

    // Test 2: Map a DMA page.
    let bus_addr: u64 = 0x1000; // Page-aligned bus address.
    let phys_addr: u64 = 0x200000; // Some physical address (2 MiB).
    map_dma(domain, bus_addr, phys_addr, IOMMU_PAGE_SIZE, DmaPerms::READ_WRITE)?;
    serial_println!("[iommu_remap]   Map DMA page: OK");

    // Test 3: Verify the mapping by walking the page table.
    {
        let domains = DOMAINS.lock();
        let slot = domain as usize;
        let pml4 = domains[slot].pml4_phys;
        let hhdm = page_table::hhdm().unwrap();

        // Walk PML4 → PDPT → PD → PT → leaf.
        let pml4_idx = ((bus_addr >> 39) & 0x1FF) as usize;
        let pdpt_phys = walk_existing(pml4, pml4_idx, hhdm)
            .expect("PML4 entry should exist");
        let pdpt_idx = ((bus_addr >> 30) & 0x1FF) as usize;
        let pd_phys = walk_existing(pdpt_phys, pdpt_idx, hhdm)
            .expect("PDPT entry should exist");
        let pd_idx = ((bus_addr >> 21) & 0x1FF) as usize;
        let pt_phys = walk_existing(pd_phys, pd_idx, hhdm)
            .expect("PD entry should exist");
        let pt_idx = ((bus_addr >> 12) & 0x1FF) as usize;

        // Read the leaf entry.
        // SAFETY: pt_phys + hhdm maps to the IOMMU page table; pt_idx < 512.
        let leaf = unsafe {
            let pt_virt = (pt_phys + hhdm) as *const u64;
            core::ptr::read(pt_virt.add(pt_idx))
        };

        let mapped_phys = leaf & PHYS_ADDR_MASK;
        assert_eq!(mapped_phys, phys_addr, "leaf points to correct phys");
        assert!(leaf & slpte::READ != 0, "read permission set");
        assert!(leaf & slpte::WRITE != 0, "write permission set");
        serial_println!("[iommu_remap]   Verify mapping: OK (leaf={:#x})", leaf);
    }

    // Test 4: Map multiple pages.
    map_dma(domain, 0x10000, 0x300000, IOMMU_PAGE_SIZE * 4, DmaPerms::READ)?;
    {
        let domains = DOMAINS.lock();
        assert_eq!(domains[domain as usize].mapped_pages, 5, "5 pages mapped");
    }
    serial_println!("[iommu_remap]   Multi-page map: OK (5 pages total)");

    // Test 5: Unmap.
    unmap_dma(domain, bus_addr, IOMMU_PAGE_SIZE)?;
    {
        let domains = DOMAINS.lock();
        assert_eq!(domains[domain as usize].mapped_pages, 4, "4 pages after unmap");
    }
    // Verify the leaf is now zero.
    {
        let domains = DOMAINS.lock();
        let pml4 = domains[domain as usize].pml4_phys;
        let hhdm = page_table::hhdm().unwrap();

        let pml4_idx = ((bus_addr >> 39) & 0x1FF) as usize;
        let pdpt_phys = walk_existing(pml4, pml4_idx, hhdm).unwrap();
        let pdpt_idx = ((bus_addr >> 30) & 0x1FF) as usize;
        let pd_phys = walk_existing(pdpt_phys, pdpt_idx, hhdm).unwrap();
        let pd_idx = ((bus_addr >> 21) & 0x1FF) as usize;
        let pt_phys = walk_existing(pd_phys, pd_idx, hhdm).unwrap();
        let pt_idx = ((bus_addr >> 12) & 0x1FF) as usize;
        // SAFETY: pt_phys + hhdm maps to the IOMMU page table; pt_idx < 512.
        let leaf = unsafe {
            let pt_virt = (pt_phys + hhdm) as *const u64;
            core::ptr::read(pt_virt.add(pt_idx))
        };
        assert_eq!(leaf, 0, "leaf cleared after unmap");
    }
    serial_println!("[iommu_remap]   Unmap: OK");

    // Test 6: Stats.
    let s = stats();
    assert!(s.active_domains >= 1, "at least 1 active domain");
    serial_println!("[iommu_remap]   Stats: {} active domains, {} mapped pages",
        s.active_domains, s.total_mapped_pages);

    // Test 7: Domain destruction.
    // First unmap remaining pages.
    unmap_dma(domain, 0x10000, IOMMU_PAGE_SIZE * 4)?;
    destroy_domain(domain)?;
    serial_println!("[iommu_remap]   Domain destroy: OK");

    // Test 8: DmaPerms.
    assert_eq!(DmaPerms::READ.to_pte_flags(), slpte::READ);
    assert_eq!(DmaPerms::WRITE.to_pte_flags(), slpte::WRITE);
    assert_eq!(DmaPerms::READ_WRITE.to_pte_flags(), slpte::READ | slpte::WRITE);
    serial_println!("[iommu_remap]   Permissions: OK");

    // Test 9: SLPT structure pages are reclaimed on domain destroy.
    //
    // A fresh domain owns exactly one PML4 page (allocated by
    // create_domain).  Mapping a single page at a bus address that no
    // existing entry covers allocates exactly three more structure pages
    // — PDPT, PD, PT — so destroying the domain must return PML4 + PDPT +
    // PD + PT = 4 pages to the pool.  The leaf points at a caller-owned
    // DMA frame, which must NOT be freed; verifying the delta is exactly 4
    // (not 5) proves we free the structure but leave the data frame alone.
    {
        let d = create_domain()?;
        // 0x40000000 (1 GiB) sits in its own PML4/PDPT/PD/PT chain,
        // distinct from the bus addresses used by earlier tests on other
        // (now-destroyed) domains, so this mapping creates a full fresh
        // 3-level structure under the domain's PML4.
        map_dma(d, 0x4000_0000, 0x500000, IOMMU_PAGE_SIZE, DmaPerms::READ_WRITE)?;
        let before = page_table::pt_pool_free_count();
        destroy_domain(d)?;
        let after = page_table::pt_pool_free_count();
        assert_eq!(
            after - before,
            4,
            "destroy_domain must reclaim PML4+PDPT+PD+PT (4 pages), not the data frame"
        );
        // The domain slot is now inactive and its root detached.
        {
            let domains = DOMAINS.lock();
            assert!(!domains[d as usize].active, "domain marked inactive");
            assert_eq!(domains[d as usize].pml4_phys, 0, "root detached");
        }
        serial_println!(
            "[iommu_remap]   SLPT teardown reclaim: OK (+{} pages)",
            after - before
        );
    }

    serial_println!("[iommu_remap] Self-test PASSED");
    Ok(())
}

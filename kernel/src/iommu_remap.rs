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

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
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
    /// Protected memory enable register (32-bit, RW): bit 31 turns the
    /// protected low and high memory regions on, bit 0 says they are.
    pub const PMEN: u64 = 0x64;
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
    /// Number of domain IDs supported (bits 0-2): 2^(4+2N), N the field.
    pub const NDOMS_MASK: u64 = 0x07;
    /// Supported Adjusted Guest Address Widths (bits 8-12).
    pub const SAGAW_SHIFT: u32 = 8;
    pub const SAGAW_MASK: u64 = 0x1F << 8;
    /// Required Write Buffer Flushing (bit 4).
    pub const RWBF: u64 = 1 << 4;
    /// The unit has protected low (bit 5) and high (bit 6) memory regions.
    pub const PLMR: u64 = 1 << 5;
    pub const PHMR: u64 = 1 << 6;
    /// Caching mode (bit 7): the unit may cache a translation's absence, so
    /// it must be told of new translations as well as of removed ones.
    pub const CM: u64 = 1 << 7;
    /// Fault Recording offset in 128-bit units (bits 24-33).
    pub const FRO_SHIFT: u32 = 24;
    pub const FRO_MASK: u64 = 0x3FF << 24;
    /// Number of fault-recording registers, less one (bits 40-47).
    pub const NFR_SHIFT: u32 = 40;
    pub const NFR_MASK: u64 = 0xFF << 40;
    /// An IOTLB invalidation can drain writes (bit 54) and reads (bit 55)
    /// in flight first.
    pub const DWD: u64 = 1 << 54;
    pub const DRD: u64 = 1 << 55;
}

/// ECAP register bit fields.
mod ecap {
    /// Page-walk coherency (bit 0): the unit snoops the CPU caches when it
    /// reads the tables.
    pub const C: u64 = 1 << 0;
    /// Pass-through (bit 6): a context entry may let a device's requests
    /// through untranslated.
    pub const PT: u64 = 1 << 6;
    /// The IOTLB registers' offset in 16-byte units (bits 8-17).
    pub const IRO_SHIFT: u32 = 8;
    pub const IRO_MASK: u64 = 0x3FF << 8;
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

/// GSTS bits that are states rather than one-shot commands: what a GCMD
/// write carries over to leave them as they are (VT-d §10.4.4's programming
/// note). A one-shot bit carried over would repeat its command.
const GSTS_PERSISTENT: u32 = 0x96FF_FFFF;

/// Context entry, low quadword: present; translation type (bits 3:2).
const CTX_PRESENT: u64 = 1;
const CTX_TT_TRANSLATED: u64 = 0b00 << 2;
const CTX_TT_PASS_THROUGH: u64 = 0b10 << 2;
const CTX_TT_MASK: u64 = 0b11 << 2;
/// Context entry, high bits 2:0, the address width: 2 is a 4-level, 48-bit
/// table -- what a domain's table is, and the deepest any unit this module
/// turns on supports, which a pass-through entry must name.
const CTX_AW_4LEVEL: u64 = 0b010;

/// The domain id every device shares until a driver gives it one of its own:
/// its requests pass through untranslated. Domain 0 is reserved on a unit in
/// caching mode, so this is 1, and [`create_domain`] starts after it.
const PASS_THROUGH_DID: DomainId = 1;
/// The first id [`create_domain`] gives.
const FIRST_DOMAIN_ID: DomainId = 2;

/// IOMMU units this module tracks: as many as `iommu` records.
const MAX_UNITS: usize = 8;

/// Polls of an invalidation register before giving up.
const INVALIDATION_POLLS: u32 = 1_000_000;

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
    /// Who created it, and so who may use it ([`Caller`]).
    owner: Caller,
}

impl Domain {
    const EMPTY: Self = Self {
        id: 0,
        pml4_phys: 0,
        active: false,
        device_count: 0,
        mapped_pages: 0,
        owner: Caller::Kernel,
    };

    /// Whether `caller` may use this domain: the kernel any, a process the
    /// ones it created. A process's stale id for a domain destroyed and
    /// given out again -- ids are reused -- then names nothing it may use,
    /// as before the reuse it named nothing at all.
    fn allows(&self, caller: Caller) -> bool {
        caller == Caller::Kernel || self.owner == caller
    }
}

/// Who asks for a domain operation: the kernel's own code, or a process
/// (by id) through the DMA system calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caller {
    Kernel,
    Process(u64),
}

/// Domain table.
static DOMAINS: Mutex<[Domain; MAX_DOMAINS]> = Mutex::new([Domain::EMPTY; MAX_DOMAINS]);

/// How many domains have been created, for [`stats`].
static DOMAINS_CREATED: AtomicU16 = AtomicU16::new(0);

// ---------------------------------------------------------------------------
// Root and Context Tables
// ---------------------------------------------------------------------------

/// Physical address of the root table (shared by all IOMMU units).
static ROOT_TABLE_PHYS: AtomicU64 = AtomicU64::new(0);

/// Whether DMA remapping is initialized and active.
static REMAP_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Total DMA faults observed.
static TOTAL_FAULTS: AtomicU64 = AtomicU64::new(0);

/// Held across every change to a context entry and every invalidation once
/// the units are on: a unit's invalidation registers take one request at a
/// time, and a context entry is read, checked and rewritten in steps.
/// Taken by the public entry points ([`attach_device`], [`detach_device`],
/// [`unmap_dma`]); the helpers they call assume it is held.
static REMAP_LOCK: Mutex<()> = Mutex::new(());

/// How many domain ids every unit turned on can hold (CAP.ND): a domain's
/// id is below it. Unlimited until a unit says otherwise.
static DOMAIN_IDS: AtomicU32 = AtomicU32::new(u32::MAX);

/// The units with translation on, by `iommu` unit index: the virtual base
/// of each one's registers, 0 for a unit with it off. Kept here, and not
/// looked up in `iommu`'s list of units under its lock, so that [`shutdown`]
/// can reach every unit with no lock to take.
static UNIT_REGS: [AtomicU64; MAX_UNITS] = [const { AtomicU64::new(0) }; MAX_UNITS];

/// Every unit with translation on snoops the CPU caches when it walks the
/// tables (ECAP.C). When one does not -- Intel's graphics unit often does
/// not -- every table write must be pushed out to memory ([`sync_tables`]),
/// or the unit reads what memory held before: zeros, or an old table.
static WALKS_COHERENT: AtomicBool = AtomicBool::new(true);

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
/// - Low qword bit 0: Present
/// - Low qword bit 1: Fault Processing Disable
/// - Low qword bits 2-3: Translation Type (00 translated by the domain's
///   table, 10 passed through untranslated)
/// - Low qword bits 12-63: Second-level page table pointer
/// - High qword bits 0-2: Address Width (010 = 48-bit, 4-level)
/// - High qword bits 8-23: Domain ID
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct ContextEntry {
    lo: u64,
    hi: u64,
}

impl ContextEntry {
    /// Whether this entry is present.
    fn is_present(&self) -> bool {
        self.lo & CTX_PRESENT != 0
    }

    /// Whether the device passes through untranslated: no domain of its own.
    fn is_pass_through(&self) -> bool {
        self.is_present() && self.lo & CTX_TT_MASK == CTX_TT_PASS_THROUGH
    }

    /// Get the second-level page table physical address.
    fn slpt_phys(&self) -> u64 {
        self.lo & PHYS_ADDR_MASK
    }

    /// Get the domain ID.
    fn domain_id(&self) -> DomainId {
        DomainId::try_from((self.hi >> 8) & 0xFFFF).unwrap_or(0)
    }

    /// A device in domain `domain_id`, its requests translated by the
    /// 4-level table at `slpt_phys`.
    fn translated(slpt_phys: u64, domain_id: DomainId) -> Self {
        Self {
            lo: (slpt_phys & PHYS_ADDR_MASK) | CTX_TT_TRANSLATED | CTX_PRESENT,
            hi: CTX_AW_4LEVEL | (u64::from(domain_id) << 8),
        }
    }

    /// A device with no domain of its own: its requests pass through
    /// untranslated.
    fn pass_through() -> Self {
        Self {
            lo: CTX_TT_PASS_THROUGH | CTX_PRESENT,
            hi: CTX_AW_4LEVEL | (u64::from(PASS_THROUGH_DID) << 8),
        }
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Turn DMA remapping on where it can leave every device working.
///
/// Each Intel VT-d unit that can pass a device through untranslated, and
/// whose deepest table is the 4-level one a domain uses, is pointed at one
/// shared root table in which every bus, device and function passes
/// through, and then translation is turned on: devices DMA as before, and a
/// driver that wants one sandboxed gives it a domain ([`attach_device`]). A
/// unit that cannot is left off, its devices unaffected. (Translation over
/// an empty table would block every device's DMA -- the display's
/// scan-out, USB, the disks -- the moment it was on.)
///
/// Must be called after `iommu::init()` (detection) and `mm::init()`.
///
/// # Errors
///
/// A table allocation's failure. A unit that cannot be turned on is logged
/// and left off; that is not an error.
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
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;

    let mut usable = [None; MAX_UNITS];
    let mut coherent = true;
    let units = usize::from(crate::iommu::unit_count());
    for (i, slot) in usable.iter_mut().enumerate().take(units) {
        let Some(unit) = crate::iommu::get_unit(i) else {
            continue;
        };
        match probe_unit(unit.register_base) {
            Ok((mmio, snoops)) => {
                *slot = Some(mmio);
                coherent &= snoops;
            }
            Err(e) => serial_println!(
                "[iommu_remap]   unit {} at {:#x} left off: {:?}",
                i,
                unit.register_base,
                e
            ),
        }
    }
    if usable.iter().all(Option::is_none) {
        serial_println!("[iommu_remap] No unit can pass devices through: DMA remapping left off");
        return Ok(());
    }

    WALKS_COHERENT.store(coherent, Ordering::Release);
    let root = build_pass_through_tables(hhdm)?;
    ROOT_TABLE_PHYS.store(root, Ordering::Release);
    serial_println!("[iommu_remap] Root table at phys {:#x}", root);

    let mut any = false;
    for (i, mmio) in usable.iter().enumerate() {
        let Some(mmio) = *mmio else {
            continue;
        };
        match enable_unit(mmio, root) {
            Ok(()) => {
                if let Some(regs) = UNIT_REGS.get(i) {
                    regs.store(mmio, Ordering::Release);
                }
                any = true;
            }
            Err(e) => {
                serial_println!("[iommu_remap]   unit {} not enabled: {:?}", i, e);
                // Left off, as a unit refused at probe is. A failure is said,
                // and the unit is off either way.
                let _ = disable_protected_memory(mmio);
            }
        }
    }
    if any {
        REMAP_ACTIVE.store(true, Ordering::Release);
        serial_println!(
            "[iommu_remap] DMA remapping active: every device passes through until a driver gives it a domain"
        );
    }
    Ok(())
}

/// Map a unit's registers, take back what the firmware left on, and decide
/// whether the unit can be turned on: it must pass devices through
/// (ECAP.PT), and its deepest table must be 4-level -- the depth a
/// pass-through entry names, and a domain's. Answers the registers' virtual
/// base and whether the unit's table walks snoop the CPU caches.
///
/// Every unit is quiesced first, whether or not it can be turned on:
/// translation, interrupt remapping and queued invalidation the firmware
/// left on would keep devices using the firmware's tables and queue --
/// memory the kernel now owns -- and queued invalidation forbids the
/// register-based invalidations this module uses.
fn probe_unit(register_base: u64) -> KernelResult<(u64, bool)> {
    // SAFETY: a DRHD's register base is its unit's register block: device
    // memory.
    let mmio = unsafe { page_table::map_mmio(register_base, 0x1000)? };
    let version = read_reg32(mmio, reg::VER);
    let cap_val = read_reg64(mmio, reg::CAP);
    let ecap_val = read_reg64(mmio, reg::ECAP);
    // The block reaches the IOTLB registers and the last fault record.
    let iro = (ecap_val & ecap::IRO_MASK) >> ecap::IRO_SHIFT;
    let fro = (cap_val & cap::FRO_MASK) >> cap::FRO_SHIFT;
    let nfr = (cap_val & cap::NFR_MASK) >> cap::NFR_SHIFT;
    let extent = iro
        .saturating_mul(16)
        .saturating_add(16)
        .max(fro.saturating_add(nfr).saturating_add(1).saturating_mul(16));
    if extent > 0x1000 {
        // SAFETY: as above.
        unsafe { page_table::map_mmio(register_base, extent)? };
    }
    serial_println!(
        "[iommu_remap]   Unit at {:#x}: VT-d {}.{}, CAP {:#x}, ECAP {:#x}",
        register_base,
        (version >> 4) & 0xF,
        version & 0xF,
        cap_val,
        ecap_val
    );
    quiesce_unit(mmio)?;
    if ecap_val & ecap::PT == 0 {
        serial_println!("[iommu_remap]   it cannot pass devices through (ECAP.PT clear)");
        // Left off: what it would block, nothing else will open. A failure
        // is said, and changes nothing about leaving the unit off.
        let _ = disable_protected_memory(mmio);
        return Err(KernelError::NotSupported);
    }
    // SAGAW bit 2 is a 4-level table; a higher bit, a deeper one, which a
    // pass-through entry would have to name instead.
    let sagaw = (cap_val & cap::SAGAW_MASK) >> cap::SAGAW_SHIFT;
    if sagaw & 0b100 == 0 || sagaw >> 3 != 0 {
        serial_println!(
            "[iommu_remap]   its deepest table is not 4-level (SAGAW {:#x})",
            sagaw
        );
        // As above.
        let _ = disable_protected_memory(mmio);
        return Err(KernelError::NotSupported);
    }
    // Its domain ids: no domain may have one it cannot hold.
    DOMAIN_IDS.fetch_min(domain_ids_of(cap_val), Ordering::AcqRel);
    Ok((mmio, ecap_val & ecap::C != 0))
}

/// How many domain ids a unit whose CAP register reads `cap_val` holds:
/// 2^(4+2N), N its ND field (VT-d §10.4.2).
fn domain_ids_of(cap_val: u64) -> u32 {
    let nd = u32::try_from(cap_val & cap::NDOMS_MASK).unwrap_or(0);
    1u32.checked_shl(nd.saturating_mul(2).saturating_add(4))
        .unwrap_or(u32::MAX)
}

/// Turn off what the firmware may have left on in a unit: translation,
/// interrupt remapping, and queued invalidation -- its queue drained first,
/// as the spec requires before it is turned off. Protected memory is turned
/// off later ([`disable_protected_memory`]).
fn quiesce_unit(mmio: u64) -> KernelResult<()> {
    let status = read_reg32(mmio, reg::GSTS);
    if status & gsts::TES != 0 {
        gcmd(mmio, 0, gcmd::TE);
        wait_for_status(mmio, gsts::TES, false)?;
        serial_println!("[iommu_remap]   translation the firmware left on: turned off");
    }
    if status & gsts::IRES != 0 {
        gcmd(mmio, 0, gcmd::IRE);
        wait_for_status(mmio, gsts::IRES, false)?;
        serial_println!("[iommu_remap]   interrupt remapping the firmware left on: turned off");
    }
    if status & gsts::QIES != 0 {
        let mut drained = false;
        for _ in 0..INVALIDATION_POLLS {
            if read_reg64(mmio, reg::IQH) == read_reg64(mmio, reg::IQT) {
                drained = true;
                break;
            }
            core::hint::spin_loop();
        }
        if !drained {
            serial_println!("[iommu_remap]   the firmware's invalidation queue never drained");
            return Err(KernelError::TimedOut);
        }
        gcmd(mmio, 0, gcmd::QIE);
        wait_for_status(mmio, gsts::QIES, false)?;
        serial_println!("[iommu_remap]   queued invalidation the firmware left on: turned off");
    }
    Ok(())
}

/// Turn off the protected memory regions a TXT launch (tboot) leaves on: the
/// unit blocks every DMA into them, pass-through DMA too, and every device
/// here passes through. Done once translation is on (`enable_unit`), as
/// Linux's `init_dmars` orders it, so that memory is never left open between
/// the two; and for a unit left off, whose devices could otherwise never
/// reach that memory. Linux's `iommu_disable_protect_mem_regions`.
fn disable_protected_memory(mmio: u64) -> KernelResult<()> {
    const PMEN_EPM: u32 = 1 << 31;
    const PMEN_PRS: u32 = 1;
    if read_reg64(mmio, reg::CAP) & (cap::PLMR | cap::PHMR) == 0
        || read_reg32(mmio, reg::PMEN) & (PMEN_EPM | PMEN_PRS) == 0
    {
        return Ok(());
    }
    write_reg32(mmio, reg::PMEN, read_reg32(mmio, reg::PMEN) & !PMEN_EPM);
    if !(0..INVALIDATION_POLLS).any(|_| {
        let off = read_reg32(mmio, reg::PMEN) & PMEN_PRS == 0;
        if !off {
            core::hint::spin_loop();
        }
        off
    }) {
        serial_println!("[iommu_remap]   protected memory the firmware left on would not turn off");
        return Err(KernelError::TimedOut);
    }
    serial_println!("[iommu_remap]   protected memory the firmware left on: turned off");
    Ok(())
}

/// The root table every unit shares: all 256 buses present, every device
/// and function on each passing through.
fn build_pass_through_tables(hhdm: u64) -> KernelResult<u64> {
    let root = page_table::alloc_pt_page()?;
    for bus in 0..256usize {
        let ctx = page_table::alloc_pt_page()?;
        // SAFETY: `ctx` and `root` are 4 KiB table pages of our own, 256
        // 16-byte entries each; nothing reads them until a unit is pointed
        // at `root`, after the last write is synced.
        unsafe {
            let entries = ctx.wrapping_add(hhdm) as *mut ContextEntry;
            for devfn in 0..256usize {
                core::ptr::write(entries.add(devfn), ContextEntry::pass_through());
            }
            core::ptr::write(
                (root.wrapping_add(hhdm) as *mut RootEntry).add(bus),
                RootEntry::new(ctx),
            );
        }
        sync_tables(ctx, IOMMU_PAGE_SIZE, hhdm);
    }
    sync_tables(root, IOMMU_PAGE_SIZE, hhdm);
    Ok(root)
}

/// Point a unit at the root table, invalidate everything it cached, and
/// turn translation on.
fn enable_unit(mmio: u64, root: u64) -> KernelResult<()> {
    write_reg64(mmio, reg::RTADDR, root);
    gcmd(mmio, gcmd::SRTP, 0);
    wait_for_status(mmio, gsts::RTPS, true)?;
    if read_reg64(mmio, reg::CAP) & cap::RWBF != 0 {
        gcmd(mmio, gcmd::WBF, 0);
        wait_for_status(mmio, gsts::WBFS, false)?;
    }
    // A new root table: every context and translation it cached is stale.
    invalidate_context_global(mmio)?;
    invalidate_iotlb(mmio, None, true)?;
    gcmd(mmio, gcmd::TE, 0);
    wait_for_status(mmio, gsts::TES, true)?;
    serial_println!("[iommu_remap]   Translation enabled");
    // Translation is on whatever this does, so the unit is on: a failure is
    // said, and leaves only DMA into the protected regions refused.
    let _ = disable_protected_memory(mmio);
    Ok(())
}

/// Issue global commands: set the state bits `set` and clear `clear`, the
/// other states carried over and no one-shot command repeated.
fn gcmd(mmio: u64, set: u32, clear: u32) {
    let status = read_reg32(mmio, reg::GSTS) & GSTS_PERSISTENT & !clear;
    write_reg32(mmio, reg::GCMD, status | set);
}

/// Push `len` bytes of table memory at `phys` out of the CPU caches, when a
/// unit with translation on does not snoop them ([`WALKS_COHERENT`]).
fn sync_tables(phys: u64, len: usize, hhdm: u64) {
    if WALKS_COHERENT.load(Ordering::Acquire) {
        return;
    }
    let line = u64::from(crate::cpu::cache_line_size()).max(16);
    let virt = phys.wrapping_add(hhdm);
    let end = virt.saturating_add(len as u64);
    let mut at = virt & !line.wrapping_sub(1);
    while at < end {
        // SAFETY: a line of a table page the direct map maps; clflush writes
        // it back and evicts it, changing nothing it holds.
        unsafe {
            core::arch::asm!("clflush [{}]", in(reg) at, options(nostack, preserves_flags));
        }
        at = at.saturating_add(line);
    }
    // Every line written back before a unit is told to look.
    // SAFETY: a fence.
    unsafe {
        core::arch::asm!("mfence", options(nostack, preserves_flags));
    }
}

/// Invalidate every context a unit cached (CCMD, global granularity).
fn invalidate_context_global(mmio: u64) -> KernelResult<()> {
    const ICC: u64 = 1 << 63;
    const GLOBAL: u64 = 0b01 << 61;
    write_reg64(mmio, reg::CCMD, ICC | GLOBAL);
    poll_clear(mmio, reg::CCMD, ICC)
}

/// Invalidate a unit's IOTLB: domain `did`'s translations, or every one.
/// With `drain_first`, in-flight DMA is drained first where the unit can:
/// after an unmap, no read or write may still use the old translation.
fn invalidate_iotlb(mmio: u64, did: Option<DomainId>, drain_first: bool) -> KernelResult<()> {
    const IVT: u64 = 1 << 63;
    let cap_val = read_reg64(mmio, reg::CAP);
    let iro = (read_reg64(mmio, reg::ECAP) & ecap::IRO_MASK) >> ecap::IRO_SHIFT;
    let iotlb = iro.saturating_mul(16).saturating_add(8);
    let granularity = match did {
        Some(d) => (0b10 << 60) | (u64::from(d) << 32),
        None => 0b01 << 60,
    };
    let mut drain = 0;
    if drain_first && cap_val & cap::DRD != 0 {
        drain |= 1 << 49;
    }
    if drain_first && cap_val & cap::DWD != 0 {
        drain |= 1 << 48;
    }
    write_reg64(mmio, iotlb, IVT | granularity | drain);
    poll_clear(mmio, iotlb, IVT)
}

/// Wait for a unit to clear `bit` of the 64-bit register at `offset`: an
/// invalidation's completion.
fn poll_clear(mmio: u64, offset: u64, bit: u64) -> KernelResult<()> {
    for _ in 0..INVALIDATION_POLLS {
        if read_reg64(mmio, offset) & bit == 0 {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    serial_println!(
        "[iommu_remap] Timeout waiting for register {:#x} bit {:#x} to clear",
        offset,
        bit
    );
    Err(KernelError::TimedOut)
}

// ---------------------------------------------------------------------------
// Domain API
// ---------------------------------------------------------------------------

/// Create a new DMA remapping domain.
///
/// Allocates a fresh second-level page table (initially empty — no DMA
/// permitted until pages are explicitly mapped). Its id is the lowest free
/// one, below what every unit can hold (CAP.ND): a destroyed domain's id is
/// given out again once [`destroy_domain`] has flushed it.
pub fn create_domain() -> KernelResult<DomainId> {
    create_domain_as(Caller::Kernel)
}

/// [`create_domain`] for `caller`, the only one besides the kernel that may
/// then use it ([`Caller`]).
pub fn create_domain_as(caller: Caller) -> KernelResult<DomainId> {
    // The table first, so that a refusal for want of memory takes no id.
    let pml4 = page_table::alloc_pt_page()?;
    if let Some(hhdm) = page_table::hhdm() {
        sync_tables(pml4, IOMMU_PAGE_SIZE, hhdm);
    }
    let limit = usize::try_from(DOMAIN_IDS.load(Ordering::Acquire))
        .map_or(MAX_DOMAINS, |ids| ids.min(MAX_DOMAINS));

    let mut domains = DOMAINS.lock();
    // A slot is free when it holds no table: a destroyed domain's keeps its
    // table until the units have forgotten its id.
    let free = (usize::from(FIRST_DOMAIN_ID)..limit).find(|&slot| {
        domains
            .get(slot)
            .is_some_and(|d| !d.active && d.pml4_phys == 0)
    });
    let Some((slot, id)) = free.and_then(|slot| Some((slot, DomainId::try_from(slot).ok()?)))
    else {
        drop(domains);
        // SAFETY: allocated above and never published: no entry and no walk
        // can reach it.
        unsafe { page_table::free_pt_page(pml4) };
        serial_println!("[iommu_remap] ERROR: every domain id is in use");
        return Err(KernelError::OutOfMemory);
    };
    if let Some(domain) = domains.get_mut(slot) {
        *domain = Domain {
            id,
            pml4_phys: pml4,
            active: true,
            device_count: 0,
            mapped_pages: 0,
            owner: caller,
        };
    }
    drop(domains);
    DOMAINS_CREATED.fetch_add(1, Ordering::Relaxed);

    serial_println!("[iommu_remap] Created domain {} (PML4={:#x})", id, pml4);
    Ok(id)
}

/// Destroy a DMA remapping domain.
///
/// Fails if devices are still attached.
pub fn destroy_domain(domain_id: DomainId) -> KernelResult<()> {
    destroy_domain_as(Caller::Kernel, domain_id)
}

/// [`destroy_domain`] for `caller`: `InvalidArgument` for a domain it may
/// not use ([`Caller`]).
pub fn destroy_domain_as(caller: Caller, domain_id: DomainId) -> KernelResult<()> {
    // Phase 1: validate and detach the page table from the domain table
    // under the DOMAINS lock.  Marking the domain inactive here makes every
    // subsequent map_dma / attach_device reject it, so no thread can add a
    // new mapping while we walk and free the tree in phase 2.  We capture
    // the root and release the lock *before* freeing, both to keep the
    // critical section short and to avoid holding DOMAINS across the
    // PT_PAGE_POOL lock taken by free_pt_page.
    let pml4_phys = {
        let mut domains = DOMAINS.lock();
        let Some(domain) = domains
            .get_mut(usize::from(domain_id))
            .filter(|d| d.active && d.allows(caller))
        else {
            return Err(KernelError::InvalidArgument);
        };
        if domain.device_count > 0 {
            return Err(KernelError::DeviceBusy);
        }

        // Its table stays named in the slot, which keeps the id from being
        // given out again (`create_domain`) until the units have forgotten it.
        domain.active = false;
        (domain.pml4_phys, core::mem::take(&mut domain.mapped_pages))
    };
    let (pml4_phys, mapped_pages) = pml4_phys;

    // Phase 1b: the units forget the id. A device that left the domain was
    // flushed then, but a unit may still cache the domain's page-table
    // entries -- pointers into the pages freed below -- and a new domain
    // given this id would find them.
    {
        let _invalidating = REMAP_LOCK.lock();
        if let Err(e) = flush_iotlb_domain(domain_id) {
            // Put back as it was, for the caller to try again: inactive, it
            // could never be reached -- nor its id given out -- again.
            if let Some(domain) = DOMAINS.lock().get_mut(usize::from(domain_id)) {
                domain.active = true;
                domain.mapped_pages = mapped_pages;
            }
            serial_println!(
                "[iommu_remap] WARNING: domain {} not flushed ({:?}): not destroyed",
                domain_id,
                e
            );
            return Err(e);
        }
    }

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
            unsafe {
                free_slpt(pml4_phys, hhdm);
            }
        } else {
            serial_println!(
                "[iommu_remap] WARNING: HHDM unavailable; leaking SLPT pages for domain {}",
                domain_id
            );
        }
    }

    // The id is free again.
    if let Some(domain) = DOMAINS.lock().get_mut(usize::from(domain_id)) {
        domain.pml4_phys = 0;
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
    map_dma_as(Caller::Kernel, domain_id, bus_addr, phys_addr, size, perms)
}

/// [`map_dma`] for `caller`: `InvalidArgument` for a domain it may not use
/// ([`Caller`]).
pub fn map_dma_as(
    caller: Caller,
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
        if !domains
            .get(slot)
            .is_some_and(|d| d.active && d.allows(caller))
        {
            return Err(KernelError::InvalidArgument);
        }
        domains[slot].pml4_phys
    };

    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let flags = perms.to_pte_flags();
    let pages = size.div_ceil(IOMMU_PAGE_SIZE);

    // Whether a page was mapped already, to something else or with other
    // permissions: a unit may hold its old translation, on any unit.
    let mut replaced = false;
    for i in 0..pages {
        let offset = (i as u64) * (IOMMU_PAGE_SIZE as u64);
        let ba = bus_addr + offset;
        let pa = phys_addr + offset;
        replaced |= map_4k_slpt(pml4_phys, ba, pa, flags, hhdm)?;
    }

    // Update mapped page count.
    {
        let mut domains = DOMAINS.lock();
        if let Some(domain) = domains.get_mut(domain_id as usize) {
            domain.mapped_pages = domain.mapped_pages.saturating_add(pages as u64);
        }
    }

    // A replaced translation is flushed as an unmapped one is, on every unit,
    // DMA drained first; new ones need telling only where a unit may have
    // cached their absence, or holds table writes in a buffer.
    let _invalidating = REMAP_LOCK.lock();
    if replaced {
        flush_iotlb_domain(domain_id)?;
    } else {
        flush_new_mappings(domain_id)?;
    }

    Ok(())
}

/// Unmap a DMA address range from a domain.
///
/// After this call, devices in this domain can no longer DMA to
/// `bus_addr..bus_addr+size`.  An IOTLB flush is issued.
pub fn unmap_dma(domain_id: DomainId, bus_addr: u64, size: usize) -> KernelResult<()> {
    unmap_dma_as(Caller::Kernel, domain_id, bus_addr, size)
}

/// [`unmap_dma`] for `caller`: `InvalidArgument` for a domain it may not use
/// ([`Caller`]).
pub fn unmap_dma_as(
    caller: Caller,
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
        if !domains
            .get(slot)
            .is_some_and(|d| d.active && d.allows(caller))
        {
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
    let _invalidating = REMAP_LOCK.lock();
    flush_iotlb_domain(domain_id)?;

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
pub fn attach_device(domain_id: DomainId, bus: u8, device: u8, function: u8) -> KernelResult<()> {
    attach_device_as(Caller::Kernel, domain_id, bus, device, function)
}

/// [`attach_device`] for `caller`: `InvalidArgument` for a domain it may not
/// use ([`Caller`]).
pub fn attach_device_as(
    caller: Caller,
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
        if !domains
            .get(slot)
            .is_some_and(|d| d.active && d.allows(caller))
        {
            return Err(KernelError::InvalidArgument);
        }
        domains[slot].pml4_phys
    };

    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let root_table = ROOT_TABLE_PHYS.load(Ordering::Acquire);

    if root_table == 0 {
        return Err(KernelError::NotSupported);
    }

    let _changing = REMAP_LOCK.lock();
    let context_table = context_table_of(root_table, bus, hhdm)?;
    let devfn = ((device as usize) << 3) | (function as usize);
    // SAFETY: one of the context tables init built; devfn < 256.
    let existing = unsafe {
        core::ptr::read_volatile(
            (context_table.wrapping_add(hhdm) as *const ContextEntry).add(devfn),
        )
    };
    if existing.is_present() && !existing.is_pass_through() {
        if existing.domain_id() == domain_id {
            return Ok(()); // Idempotent.
        }
        return Err(KernelError::AlreadyExists);
    }
    write_context_entry(
        context_table,
        devfn,
        ContextEntry::translated(pml4_phys, domain_id),
        hhdm,
    )?;

    // Update device count.
    {
        let mut domains = DOMAINS.lock();
        let slot = domain_id as usize;
        domains[slot].device_count = domains[slot].device_count.saturating_add(1);
    }

    // The device's old translations were the pass-through domain's.
    flush_context_cache()?;
    flush_iotlb_domain(PASS_THROUGH_DID)?;

    serial_println!(
        "[iommu_remap] Attached {:02x}:{:02x}.{} to domain {}",
        bus,
        device,
        function,
        domain_id
    );
    Ok(())
}

/// Detach a PCI device from its domain.
///
/// The device passes through untranslated again, as every device the
/// kernel drives itself does.
pub fn detach_device(domain_id: DomainId, bus: u8, device: u8, function: u8) -> KernelResult<()> {
    detach_device_as(Caller::Kernel, domain_id, bus, device, function)
}

/// [`detach_device`] for `caller`: `InvalidArgument` for a domain it may not
/// use ([`Caller`]).
pub fn detach_device_as(
    caller: Caller,
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

    let _changing = REMAP_LOCK.lock();
    // Under the lock a destroy takes too, so the domain cannot change hands
    // between this and the detach.
    if !DOMAINS
        .lock()
        .get(usize::from(domain_id))
        .is_some_and(|d| d.active && d.allows(caller))
    {
        return Err(KernelError::InvalidArgument);
    }
    let context_table = context_table_of(root_table, bus, hhdm)?;
    let devfn = ((device as usize) << 3) | (function as usize);

    // SAFETY: one of the context tables init built; devfn < 256.
    let existing = unsafe {
        core::ptr::read_volatile(
            (context_table.wrapping_add(hhdm) as *const ContextEntry).add(devfn),
        )
    };

    if !existing.is_present() || existing.is_pass_through() || existing.domain_id() != domain_id {
        return Err(KernelError::NotFound);
    }

    write_context_entry(context_table, devfn, ContextEntry::pass_through(), hhdm)?;

    // Update device count.
    {
        let mut domains = DOMAINS.lock();
        let slot = domain_id as usize;
        domains[slot].device_count = domains[slot].device_count.saturating_sub(1);
    }

    // Flush caches.
    flush_context_cache()?;
    flush_iotlb_domain(domain_id)?;

    serial_println!(
        "[iommu_remap] Detached {:02x}:{:02x}.{} from domain {}",
        bus,
        device,
        function,
        domain_id
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Second-level page table operations
// ---------------------------------------------------------------------------

/// Map a single 4 KiB page in a domain's second-level page table.
///
/// Creates intermediate levels as needed. True when it replaced a leaf that
/// was present: a unit may still hold that translation, which only a flush
/// takes away (`map_dma`).
fn map_4k_slpt(
    pml4_phys: u64,
    bus_addr: u64,
    phys_addr: u64,
    flags: u64,
    hhdm: u64,
) -> KernelResult<bool> {
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
    let old = unsafe {
        let pt_virt = (pt_phys + hhdm) as *mut u64;
        core::ptr::replace(pt_virt.add(pt_idx), entry)
    };
    sync_tables(entry_at(pt_phys, pt_idx, 8), 8, hhdm);

    Ok(old & (slpte::READ | slpte::WRITE) != 0)
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
    sync_tables(entry_at(pt_phys, pt_idx, 8), 8, hhdm);
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

    // Allocate a new page table level, its zeros where a unit reads them.
    let new_table = page_table::alloc_pt_page()?;
    sync_tables(new_table, IOMMU_PAGE_SIZE, hhdm);

    // Write the entry with R+W (intermediate entries need both for
    // the walk to succeed for any access type).
    let new_entry = (new_table & PHYS_ADDR_MASK) | slpte::READ | slpte::WRITE;

    // SAFETY: Valid page, valid index.
    unsafe {
        let virt = (table_phys + hhdm) as *mut u64;
        core::ptr::write(virt.add(index), new_entry);
    }
    sync_tables(entry_at(table_phys, index, 8), 8, hhdm);

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
                unsafe {
                    page_table::free_pt_page(pt);
                }
            }
            // SAFETY: `pd` is a domain-owned structure page; all its PT
            // children were just freed and nothing else references it.
            unsafe {
                page_table::free_pt_page(pd);
            }
        }
        // SAFETY: `pdpt` is a domain-owned structure page; all its PD
        // children were just freed.
        unsafe {
            page_table::free_pt_page(pdpt);
        }
    }
    // SAFETY: the PML4 root is domain-owned; all lower levels are freed.
    unsafe {
        page_table::free_pt_page(pml4_phys);
    }
}

// ---------------------------------------------------------------------------
// Context table helpers
// ---------------------------------------------------------------------------

/// The physical address of entry `index`, `size` bytes each, of the table at
/// `table`: an index below the table's entry count, inside its page.
fn entry_at(table: u64, index: usize, size: u64) -> u64 {
    table.wrapping_add((index as u64).wrapping_mul(size))
}

/// The context table of `bus`: init built all 256.
fn context_table_of(root_table: u64, bus: u8, hhdm: u64) -> KernelResult<u64> {
    // SAFETY: root_table is the root table init built: 256 16-byte entries.
    let entry = unsafe {
        core::ptr::read_volatile(
            (root_table.wrapping_add(hhdm) as *const RootEntry).add(bus as usize),
        )
    };
    if entry.is_present() {
        Ok(entry.context_table_phys())
    } else {
        Err(KernelError::NotFound)
    }
}

/// Replace the context entry of `devfn` in the table at `context_table`:
/// first made not present, and that invalidated, so that no unit reads half
/// of the old entry and half of the new; then the high quadword, then the
/// low one, which makes it present. The caller invalidates after.
fn write_context_entry(
    context_table: u64,
    devfn: usize,
    entry: ContextEntry,
    hhdm: u64,
) -> KernelResult<()> {
    let at = entry_at(context_table, devfn, 16);
    let lo = at.wrapping_add(hhdm) as *mut u64;
    // SAFETY: an entry of a context table init built (devfn < 256). The
    // units read it, so each store is volatile, and in this order.
    unsafe { core::ptr::write_volatile(lo, 0) };
    sync_tables(at, 16, hhdm);
    flush_context_cache()?;
    // SAFETY: as above.
    unsafe {
        core::ptr::write_volatile(lo.add(1), entry.hi);
        core::ptr::write_volatile(lo, entry.lo);
    }
    sync_tables(at, 16, hhdm);
    Ok(())
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
    if status_reached(mmio_base, bit, expected_set) {
        return Ok(());
    }
    serial_println!(
        "[iommu_remap] Timeout waiting for GSTS bit {:#x} = {}",
        bit,
        expected_set
    );
    Err(KernelError::TimedOut)
}

/// Whether GSTS `bit` comes to read `expected_set` within a million polls.
/// Prints nothing, so that a caller with the other CPUs stopped -- one of
/// them may hold the serial log's lock for good -- can use it (`shutdown`).
fn status_reached(mmio_base: u64, bit: u32, expected_set: bool) -> bool {
    (0..1_000_000).any(|_| {
        let reached = (read_reg32(mmio_base, reg::GSTS) & bit != 0) == expected_set;
        if !reached {
            core::hint::spin_loop();
        }
        reached
    })
}

// ---------------------------------------------------------------------------
// Cache invalidation
// ---------------------------------------------------------------------------

/// Invalidate every context the units with translation on cached: after a
/// context entry changed.
fn flush_context_cache() -> KernelResult<()> {
    for_each_unit_on(invalidate_context_global)
}

/// Invalidate domain `domain_id`'s translations on every unit with
/// translation on: after its table changed, or a device left it.
fn flush_iotlb_domain(domain_id: DomainId) -> KernelResult<()> {
    for_each_unit_on(|mmio| invalidate_iotlb(mmio, Some(domain_id), true))
}

/// Tell every unit of domain `domain_id`'s new translations as it needs:
/// one in caching mode may have cached their absence, and has its IOTLB
/// invalidated (VT-d §6.1); one that is not, but asks for write-buffer
/// flushing (CAP.RWBF), has its write buffer flushed, so that its walks see
/// the new entries (§6.8, Linux's `iommu_flush_write_buffer`). No DMA in
/// flight can use a translation that did not exist, so none is drained.
fn flush_new_mappings(domain_id: DomainId) -> KernelResult<()> {
    for_each_unit_on(|mmio| {
        let cap_val = read_reg64(mmio, reg::CAP);
        if cap_val & cap::CM != 0 {
            return invalidate_iotlb(mmio, Some(domain_id), false);
        }
        if cap_val & cap::RWBF != 0 {
            gcmd(mmio, gcmd::WBF, 0);
            return wait_for_status(mmio, gsts::WBFS, false);
        }
        Ok(())
    })
}

/// Run `f` on each unit with translation on, by its registers' virtual
/// base: every unit tried, and the first failure answered.
fn for_each_unit_on(f: impl Fn(u64) -> KernelResult<()>) -> KernelResult<()> {
    let mut first = Ok(());
    for (i, regs) in UNIT_REGS.iter().enumerate() {
        let mmio = regs.load(Ordering::Acquire);
        if mmio == 0 {
            continue;
        }
        if let Err(e) = f(mmio) {
            serial_println!("[iommu_remap] unit {}: invalidation failed: {:?}", i, e);
            if first.is_ok() {
                first = Err(e);
            }
        }
    }
    first
}

// ---------------------------------------------------------------------------
// Query API
// ---------------------------------------------------------------------------

/// Turn translation off on every unit that has it on, for a kexec into
/// another kernel: its tables lie in memory the next kernel will reuse. A
/// unit that does not answer is left as it is; the next kernel turns it off
/// before it replaces the tables (`quiesce_unit`).
///
/// Called with every other CPU stopped where it stood (`kexec::quiesce`),
/// so it takes no lock and prints nothing: one of them may hold
/// [`REMAP_LOCK`], or the serial log's lock, for good. What that CPU was
/// doing to a unit's tables no longer matters once translation is off.
pub fn shutdown() {
    for regs in &UNIT_REGS {
        let mmio = regs.load(Ordering::Acquire);
        if mmio == 0 {
            continue;
        }
        gcmd(mmio, 0, gcmd::TE);
        if status_reached(mmio, gsts::TES, false) {
            regs.store(0, Ordering::Release);
        }
    }
    REMAP_ACTIVE.store(
        UNIT_REGS
            .iter()
            .any(|regs| regs.load(Ordering::Acquire) != 0),
        Ordering::Release,
    );
}

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
    let total_mapped = domains
        .iter()
        .filter(|d| d.active)
        .map(|d| d.mapped_pages)
        .sum();

    RemapStats {
        active: REMAP_ACTIVE.load(Ordering::Relaxed),
        total_domains: DOMAINS_CREATED.load(Ordering::Relaxed),
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
            fault_addr,
            source_id,
            reason
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
    map_dma(
        domain,
        bus_addr,
        phys_addr,
        IOMMU_PAGE_SIZE,
        DmaPerms::READ_WRITE,
    )?;
    serial_println!("[iommu_remap]   Map DMA page: OK");

    // Test 3: Verify the mapping by walking the page table.
    {
        let domains = DOMAINS.lock();
        let slot = domain as usize;
        let pml4 = domains[slot].pml4_phys;
        let hhdm = page_table::hhdm().unwrap();

        // Walk PML4 → PDPT → PD → PT → leaf.
        let pml4_idx = ((bus_addr >> 39) & 0x1FF) as usize;
        let pdpt_phys = walk_existing(pml4, pml4_idx, hhdm).expect("PML4 entry should exist");
        let pdpt_idx = ((bus_addr >> 30) & 0x1FF) as usize;
        let pd_phys = walk_existing(pdpt_phys, pdpt_idx, hhdm).expect("PDPT entry should exist");
        let pd_idx = ((bus_addr >> 21) & 0x1FF) as usize;
        let pt_phys = walk_existing(pd_phys, pd_idx, hhdm).expect("PD entry should exist");
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
    map_dma(
        domain,
        0x10000,
        0x300000,
        IOMMU_PAGE_SIZE * 4,
        DmaPerms::READ,
    )?;
    {
        let domains = DOMAINS.lock();
        assert_eq!(domains[domain as usize].mapped_pages, 5, "5 pages mapped");
    }
    serial_println!("[iommu_remap]   Multi-page map: OK (5 pages total)");

    // Test 5: Unmap.
    unmap_dma(domain, bus_addr, IOMMU_PAGE_SIZE)?;
    {
        let domains = DOMAINS.lock();
        assert_eq!(
            domains[domain as usize].mapped_pages, 4,
            "4 pages after unmap"
        );
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
    serial_println!(
        "[iommu_remap]   Stats: {} active domains, {} mapped pages",
        s.active_domains,
        s.total_mapped_pages
    );

    // Test 7: Domain destruction.
    // First unmap remaining pages.
    unmap_dma(domain, 0x10000, IOMMU_PAGE_SIZE * 4)?;
    destroy_domain(domain)?;
    serial_println!("[iommu_remap]   Domain destroy: OK");

    // Test 8: DmaPerms.
    assert_eq!(DmaPerms::READ.to_pte_flags(), slpte::READ);
    assert_eq!(DmaPerms::WRITE.to_pte_flags(), slpte::WRITE);
    assert_eq!(
        DmaPerms::READ_WRITE.to_pte_flags(),
        slpte::READ | slpte::WRITE
    );
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
        map_dma(
            d,
            0x4000_0000,
            0x500000,
            IOMMU_PAGE_SIZE,
            DmaPerms::READ_WRITE,
        )?;
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

    // Test 9b: a destroyed domain's id is given out again -- the lowest free
    // one -- so that a driver creating and destroying domains cannot use up
    // the few ids a unit holds. Three made, the outer two destroyed: the next
    // is the lower of those two.
    {
        let a = create_domain()?;
        let b = create_domain()?;
        let c = create_domain()?;
        destroy_domain(a)?;
        destroy_domain(c)?;
        let d = create_domain()?;
        let want = a.min(c);
        destroy_domain(b)?;
        destroy_domain(d)?;
        if d != want {
            serial_println!(
                "[iommu_remap]   FAIL: domains {}, {} and {}, {} and {} destroyed, then {} given (want {})",
                a,
                b,
                c,
                a,
                c,
                d,
                want
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[iommu_remap]   domain ids given out again, the lowest first: OK");
    }

    // Test 9c: a mapping made over one that is present says so, which is
    // what has map_dma flush it on every unit: a unit not in caching mode
    // would otherwise keep the old translation.
    {
        let d = create_domain()?;
        let pml4 = DOMAINS
            .lock()
            .get(usize::from(d))
            .map_or(0, |dom| dom.pml4_phys);
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        let bus = 0x7000_0000;
        let rw = slpte::READ | slpte::WRITE;
        let fresh = map_4k_slpt(pml4, bus, 0x60_0000, rw, hhdm);
        let over = map_4k_slpt(pml4, bus, 0x61_0000, slpte::READ, hhdm);
        unmap_4k_slpt(pml4, bus, hhdm);
        let after_unmap = map_4k_slpt(pml4, bus, 0x62_0000, rw, hhdm);
        destroy_domain(d)?;
        if (fresh, over, after_unmap) != (Ok(false), Ok(true), Ok(false)) {
            serial_println!(
                "[iommu_remap]   FAIL: replaced leaf reported as {:?}, {:?}, {:?} (want false, true, false)",
                fresh,
                over,
                after_unmap
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[iommu_remap]   a mapping over a present one says so: OK");
    }

    // Test 9d: a process's domain is its own: another process's map and
    // destroy of it are refused, as of a domain that is not there; its own
    // go through.
    {
        let (mine, theirs) = (Caller::Process(0x7001), Caller::Process(0x7002));
        let d = create_domain_as(mine)?;
        let page = IOMMU_PAGE_SIZE;
        let foreign_map = map_dma_as(theirs, d, 0x7100_0000, 0x63_0000, page, DmaPerms::READ);
        let foreign_destroy = destroy_domain_as(theirs, d);
        let own_map = map_dma_as(mine, d, 0x7100_0000, 0x63_0000, page, DmaPerms::READ);
        let own_destroy = destroy_domain_as(mine, d);
        if foreign_map != Err(KernelError::InvalidArgument)
            || foreign_destroy != Err(KernelError::InvalidArgument)
            || own_map.is_err()
            || own_destroy.is_err()
        {
            serial_println!(
                "[iommu_remap]   FAIL: another process's map {:?} and destroy {:?}, the owner's {:?} and {:?}",
                foreign_map,
                foreign_destroy,
                own_map,
                own_destroy
            );
            let _ = destroy_domain(d);
            return Err(KernelError::InternalError);
        }
        serial_println!("[iommu_remap]   a process's domain refuses every other process: OK");
    }

    // Test 10: context entries as VT-d lays them out (§9.3): the address
    // width in high bits 2:0, the domain in 23:8, the translation type in
    // low bits 3:2.
    let t = ContextEntry::translated(0x1234_5000, 7);
    let p = ContextEntry::pass_through();
    if t.lo != 0x1234_5001
        || t.hi != (0b010 | (7 << 8))
        || t.domain_id() != 7
        || t.is_pass_through()
        || p.lo != 0b1001
        || p.hi != (0b010 | (u64::from(PASS_THROUGH_DID) << 8))
        || !p.is_pass_through()
    {
        serial_println!(
            "[iommu_remap]   FAIL: context entries are not laid out as VT-d reads them: {:#x?} {:#x?}",
            (t.lo, t.hi),
            (p.lo, p.hi)
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[iommu_remap]   Context entry layout: OK");

    // Test 10b: the domain ids a unit holds, from its CAP register.
    let ids = [
        domain_ids_of(0),
        domain_ids_of(2),
        domain_ids_of(6),
        domain_ids_of(0x80 | 1),
    ];
    if ids != [16, 256, 65536, 64] {
        serial_println!("[iommu_remap]   FAIL: domain ids from CAP.ND: {:?}", ids);
        return Err(KernelError::InternalError);
    }

    // Test 11: with a unit on, every device passes through, and a device
    // given a domain is translated by it until it is detached.
    if is_active() {
        remap_live_test()?;
    }

    serial_println!("[iommu_remap] Self-test PASSED");
    Ok(())
}

/// For [`self_test`], with translation on: each unit marked on says so, the
/// root table passes every device through, and a function that is not there
/// -- FF:1F.7 -- given a domain is translated by it, is refused to a second
/// domain, and passes through again once detached.
fn remap_live_test() -> KernelResult<()> {
    fn fail(what: &str) -> KernelResult<()> {
        serial_println!("[iommu_remap]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let root = ROOT_TABLE_PHYS.load(Ordering::Acquire);
    let entry_of = |bus: u8, devfn: usize| -> KernelResult<ContextEntry> {
        let ctx = context_table_of(root, bus, hhdm)?;
        // SAFETY: one of init's context tables; devfn < 256.
        Ok(unsafe {
            core::ptr::read_volatile((ctx.wrapping_add(hhdm) as *const ContextEntry).add(devfn))
        })
    };
    for regs in &UNIT_REGS {
        let mmio = regs.load(Ordering::Acquire);
        if mmio != 0 && read_reg32(mmio, reg::GSTS) & gsts::TES == 0 {
            return fail("a unit marked on does not have translation on");
        }
    }
    if !entry_of(0, 0)?.is_pass_through() || !entry_of(255, 255)?.is_pass_through() {
        return fail("a device does not pass through by default");
    }
    let (bus, dev, func) = (0xFF_u8, 0x1F_u8, 7_u8);
    // A function that is not there answers all ones.
    if crate::pci::config_read16(bus, dev, func, 0) != 0xFFFF {
        serial_println!("[iommu_remap]   FF:1F.7 is a real function here: domain test skipped");
        return Ok(());
    }
    let devfn = (usize::from(dev) << 3) | usize::from(func);
    let d = create_domain()?;
    let result = (|| -> KernelResult<()> {
        attach_device(d, bus, dev, func)?;
        let attached = entry_of(bus, devfn)?;
        let pml4 = DOMAINS
            .lock()
            .get(usize::from(d))
            .map_or(0, |x| x.pml4_phys);
        let again = attach_device(d, bus, dev, func);
        let other = create_domain()?;
        let taken = attach_device(other, bus, dev, func);
        // The second domain never held the device; destroying it cannot fail
        // on its account, and a leak here would only cost a table page.
        let _ = destroy_domain(other);
        detach_device(d, bus, dev, func)?;
        let detached = entry_of(bus, devfn)?;
        if attached.is_pass_through() || attached.domain_id() != d || attached.slpt_phys() != pml4 {
            return fail("an attached function is not translated by its domain");
        }
        if again != Ok(()) || taken != Err(KernelError::AlreadyExists) {
            return fail("attaching twice, or to a second domain, answered wrongly");
        }
        if !detached.is_pass_through() {
            return fail("a detached function does not pass through again");
        }
        Ok(())
    })();
    // The test's own domain: after a failure it may still hold the function.
    let _ = destroy_domain(d);
    result?;
    serial_println!(
        "[iommu_remap]   Live: every device passes through, a domain translates its function, detach restores: OK"
    );
    Ok(())
}

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
    /// Interrupt remapping table address (64-bit, RW).
    pub const IRTA: u64 = 0xB8;
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
    /// Set interrupt remapping table pointer.
    pub const SIRTP: u32 = 1 << 24;
    /// Compatibility format interrupts let through.
    pub const CFI: u32 = 1 << 23;
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
    /// Interrupt remapping table pointer status.
    pub const IRTPS: u32 = 1 << 24;
    /// Compatibility format interrupts let through.
    pub const CFIS: u32 = 1 << 23;
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
    /// Queued invalidation (bit 1): the unit takes invalidations as
    /// descriptors on a queue in memory ([`super::Queue`]).
    pub const QI: u64 = 1 << 1;
    /// Pass-through (bit 6): a context entry may let a device's requests
    /// through untranslated.
    pub const PT: u64 = 1 << 6;
    /// Interrupt remapping (bit 3).
    pub const IR: u64 = 1 << 3;
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

/// How many units passed [`probe_unit`] -- could be turned on -- and then
/// failed to come on: a timeout, or an invalidation the unit refused. A
/// unit refused at probe lacks something remapping needs; one counted here
/// is a fault in the unit or in this module, which [`self_test`] reports.
static UNITS_FAILED: AtomicU32 = AtomicU32::new(0);

/// Held across every change to a context entry and every invalidation once
/// the units are on: a context entry is read, checked and rewritten in
/// steps, and a unit's invalidation registers take one request at a time.
/// (Its queue has a lock of its own, [`QUEUES`], which this lock never
/// needs to be held to take.)
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

/// Each unit's invalidation queue, by `iommu` unit index: `None` for a unit
/// without one (no ECAP.QI), which is invalidated through its registers.
///
/// Once a unit's queue is on, every invalidation it gets must come through
/// the queue -- the register interface no longer answers -- and the
/// interrupt-entry cache interrupt remapping needs has no register interface
/// at all (VT-d §6.5.2). One submission at a time per unit: [`queue_run`]
/// holds the lock from the first descriptor written until the unit has done
/// them all, so the queue is empty whenever no one holds it.
static QUEUES: [Mutex<Option<Queue>>; MAX_UNITS] = [const { Mutex::new(None) }; MAX_UNITS];

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

    // Every unit's registers once it is quiesced, and those of the units
    // that can translate.
    let mut probed = [None; MAX_UNITS];
    let mut usable = [None; MAX_UNITS];
    // Of every unit probed, not only those that translate: a unit that only
    // remaps interrupts reads the interrupt remapping table, which ECAP.C
    // covers too.
    let mut coherent = true;
    let units = usize::from(crate::iommu::unit_count());
    for (i, (slot, usable)) in probed
        .iter_mut()
        .zip(usable.iter_mut())
        .enumerate()
        .take(units)
    {
        let Some(unit) = crate::iommu::get_unit(i) else {
            continue;
        };
        match probe_unit(unit.register_base) {
            Ok(unit) => {
                *slot = Some(unit.mmio);
                coherent &= unit.snoops;
                if unit.translates {
                    *usable = Some(unit.mmio);
                }
            }
            Err(e) => serial_println!(
                "[iommu_remap]   unit {} at {:#x} left off: {:?}",
                i,
                unit.register_base,
                e
            ),
        }
    }
    WALKS_COHERENT.store(coherent, Ordering::Release);
    if usable.iter().all(Option::is_none) {
        serial_println!("[iommu_remap] No unit can pass devices through: DMA remapping left off");
    } else {
        enable_translation_everywhere(&usable, hhdm)?;
    }
    report_interrupt_remapping(enable_interrupt_remapping(&probed));
    Ok(())
}

/// Turn translation on in each unit that can translate (`usable`, their
/// registers by unit index), through tables that pass every device through.
fn enable_translation_everywhere(usable: &[Option<u64>; MAX_UNITS], hhdm: u64) -> KernelResult<()> {
    let root = build_pass_through_tables(hhdm)?;
    ROOT_TABLE_PHYS.store(root, Ordering::Release);
    serial_println!("[iommu_remap] Root table at phys {:#x}", root);

    let mut any = false;
    for (i, mmio) in usable.iter().enumerate() {
        let Some(mmio) = *mmio else {
            continue;
        };
        match enable_unit(i, mmio, root) {
            Ok(()) => {
                if let Some(regs) = UNIT_REGS.get(i) {
                    regs.store(mmio, Ordering::Release);
                }
                any = true;
            }
            Err(e) => {
                serial_println!("[iommu_remap]   unit {} not enabled: {:?}", i, e);
                UNITS_FAILED.fetch_add(1, Ordering::Relaxed);
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

/// Say what came of turning interrupt remapping on, and count a failure
/// that is not the hardware's or the firmware's lacking it.
fn report_interrupt_remapping(outcome: KernelResult<()>) {
    match outcome {
        Ok(()) => serial_println!(
            "[iommu_remap] Interrupt remapping active: a device's interrupt is delivered only through an entry naming it"
        ),
        Err(KernelError::NotSupported) => {
            serial_println!("[iommu_remap] Interrupt remapping left off");
        }
        Err(e) => {
            serial_println!("[iommu_remap] Interrupt remapping failed: {:?}", e);
            REMAPPING_FAILED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// A unit [`probe_unit`] mapped and quiesced.
#[derive(Debug, Clone, Copy)]
struct Probed {
    /// Its registers' virtual base.
    mmio: u64,
    /// Whether its table walks snoop the CPU caches (ECAP.C).
    snoops: bool,
    /// Whether it can translate: it passes devices through (ECAP.PT), and
    /// its deepest table is 4-level -- the depth a pass-through entry names,
    /// and a domain's. One that cannot may still remap interrupts.
    translates: bool,
}

/// Map a unit's registers, take back what the firmware left on, and decide
/// whether the unit can translate ([`Probed`]).
///
/// Every unit is quiesced first, whether or not it can be turned on:
/// translation, interrupt remapping and queued invalidation the firmware
/// left on would keep devices using the firmware's tables and queue --
/// memory the kernel now owns -- and [`enable_unit`] builds its own queue,
/// or invalidates through registers a running queue would refuse.
fn probe_unit(register_base: u64) -> KernelResult<Probed> {
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
    let unit = Probed {
        mmio,
        snoops: ecap_val & ecap::C != 0,
        translates: false,
    };
    if ecap_val & ecap::PT == 0 {
        serial_println!(
            "[iommu_remap]   it cannot pass devices through (ECAP.PT clear): no translation"
        );
        // Left off: what it would block, nothing else will open. A failure
        // is said, and changes nothing about leaving translation off.
        let _ = disable_protected_memory(mmio);
        return Ok(unit);
    }
    // SAGAW bit 2 is a 4-level table; a higher bit, a deeper one, which a
    // pass-through entry would have to name instead.
    let sagaw = (cap_val & cap::SAGAW_MASK) >> cap::SAGAW_SHIFT;
    if sagaw & 0b100 == 0 || sagaw >> 3 != 0 {
        serial_println!(
            "[iommu_remap]   its deepest table is not 4-level (SAGAW {:#x}): no translation",
            sagaw
        );
        // As above.
        let _ = disable_protected_memory(mmio);
        return Ok(unit);
    }
    // Its domain ids: no domain may have one it cannot hold.
    DOMAIN_IDS.fetch_min(domain_ids_of(cap_val), Ordering::AcqRel);
    Ok(Probed {
        translates: true,
        ..unit
    })
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
        if !drain_and_stop_queue(mmio) {
            serial_println!(
                "[iommu_remap]   the firmware's invalidation queue would not drain and turn off"
            );
            return Err(KernelError::TimedOut);
        }
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

/// Point unit `index` at the root table -- its invalidation queue turned on
/// first, where it has one -- invalidate everything it cached, and turn
/// translation on.
fn enable_unit(index: usize, mmio: u64, root: u64) -> KernelResult<()> {
    // A unit whose queue will not come on is invalidated through its
    // registers instead: that loses nothing DMA remapping needs.
    if read_reg64(mmio, reg::ECAP) & ecap::QI != 0
        && let Err(e) = enable_queue(index, mmio)
    {
        serial_println!(
            "[iommu_remap]   queued invalidation could not be turned on ({:?}): registers instead",
            e
        );
    }
    let enabled = enable_translation(index, mmio, root);
    if enabled.is_err() {
        // Left off, its queue with it: a unit left off is quiet.
        disable_queue(index, mmio);
    }
    enabled
}

/// [`enable_unit`] after the queue: the root table, its invalidations, TE.
fn enable_translation(index: usize, mmio: u64, root: u64) -> KernelResult<()> {
    write_reg64(mmio, reg::RTADDR, root);
    gcmd(mmio, gcmd::SRTP, 0);
    wait_for_status(mmio, gsts::RTPS, true)?;
    if read_reg64(mmio, reg::CAP) & cap::RWBF != 0 {
        gcmd(mmio, gcmd::WBF, 0);
        wait_for_status(mmio, gsts::WBFS, false)?;
    }
    // A new root table: every context and translation it cached is stale.
    invalidate_context_global(index, mmio)?;
    invalidate_iotlb(index, mmio, None, true)?;
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

/// Invalidate every context a unit cached: a global context-cache
/// invalidation, on its queue where it has one, else through CCMD.
fn invalidate_context_global(index: usize, mmio: u64) -> KernelResult<()> {
    if let Some(done) = queue_run(index, mmio, &[[desc::CONTEXT | desc::CONTEXT_GLOBAL, 0]]) {
        return done;
    }
    const ICC: u64 = 1 << 63;
    const GLOBAL: u64 = 0b01 << 61;
    write_reg64(mmio, reg::CCMD, ICC | GLOBAL);
    poll_clear(mmio, reg::CCMD, ICC)
}

/// Invalidate a unit's IOTLB: domain `did`'s translations, or every one.
/// With `drain_first`, in-flight DMA is drained first where the unit can:
/// after an unmap, no read or write may still use the old translation.
fn invalidate_iotlb(
    index: usize,
    mmio: u64,
    did: Option<DomainId>,
    drain_first: bool,
) -> KernelResult<()> {
    const IVT: u64 = 1 << 63;
    let cap_val = read_reg64(mmio, reg::CAP);
    let drain_reads = drain_first && cap_val & cap::DRD != 0;
    let drain_writes = drain_first && cap_val & cap::DWD != 0;
    let queued = desc::IOTLB
        | did.map_or(desc::IOTLB_GLOBAL, |d| {
            desc::IOTLB_DOMAIN | (u64::from(d) << desc::DID_SHIFT)
        })
        | if drain_reads {
            desc::IOTLB_DRAIN_READS
        } else {
            0
        }
        | if drain_writes {
            desc::IOTLB_DRAIN_WRITES
        } else {
            0
        };
    if let Some(done) = queue_run(index, mmio, &[[queued, 0]]) {
        return done;
    }
    let iro = (read_reg64(mmio, reg::ECAP) & ecap::IRO_MASK) >> ecap::IRO_SHIFT;
    let iotlb = iro.saturating_mul(16).saturating_add(8);
    let granularity = match did {
        Some(d) => (0b10 << 60) | (u64::from(d) << 32),
        None => 0b01 << 60,
    };
    let mut drain = 0;
    if drain_reads {
        drain |= 1 << 49;
    }
    if drain_writes {
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
// Queued invalidation (VT-d §6.5.2)
// ---------------------------------------------------------------------------

/// Invalidation descriptors, 128 bits each: the low qword's fields. The high
/// qword is 0 for every kind here but the wait, where it is the address the
/// status is written to.
mod desc {
    /// Context-cache invalidation (type 1), global granularity.
    pub const CONTEXT: u64 = 0x1;
    pub const CONTEXT_GLOBAL: u64 = 0b01 << 4;
    /// IOTLB invalidation (type 2): global, or one domain's.
    pub const IOTLB: u64 = 0x2;
    pub const IOTLB_GLOBAL: u64 = 0b01 << 4;
    pub const IOTLB_DOMAIN: u64 = 0b10 << 4;
    pub const IOTLB_DRAIN_WRITES: u64 = 1 << 6;
    pub const IOTLB_DRAIN_READS: u64 = 1 << 7;
    /// Where a domain id goes in a context or IOTLB descriptor.
    pub const DID_SHIFT: u32 = 16;
    /// Invalidation wait (type 5): write the status (bits 63:32) to the
    /// address in the high qword once every descriptor before it is done
    /// (SW), and fence what follows behind it (FN).
    pub const WAIT: u64 = 0x5;
    pub const WAIT_STATUS_WRITE: u64 = 1 << 5;
    pub const WAIT_FENCE: u64 = 1 << 6;
    pub const WAIT_STATUS_SHIFT: u32 = 32;
    /// Interrupt entry cache invalidation (type 4): global, or one entry's
    /// (bit 4) with its index at bits 47:32.
    pub const IEC: u64 = 0x4;
    pub const IEC_INDEX: u64 = 1 << 4;
    pub const IEC_INDEX_SHIFT: u32 = 32;
}

/// FSTS's invalidation queue error (IQE): the unit could not take the
/// descriptor at the queue's head, and fetches nothing more until it is
/// cleared -- after which it fetches that descriptor again.
const FSTS_IQE: u32 = 1 << 4;
/// FSTS's invalidation completion error (ICE: a device answered a
/// device-TLB invalidation the unit did not expect) and time-out error (ITE:
/// a device never answered one). This module sends no device-TLB
/// invalidations, so either means something is wrong with the unit.
const FSTS_ICE_ITE: u32 = (1 << 5) | (1 << 6);

/// Descriptors on one queue: one 4 KiB page of 16-byte descriptors (IQA's
/// size field 0).
const QUEUE_ENTRIES: u32 = 256;

/// One unit's invalidation queue.
struct Queue {
    /// The page of descriptors.
    descs_phys: u64,
    /// The page whose first word the wait descriptors write their status to.
    status_phys: u64,
    /// Where the next descriptor goes.
    tail: u32,
    /// What the last wait wrote; the next writes one more.
    seq: u32,
    /// A run timed out, or the queue was found with descriptors in it: the
    /// unit may still be behind, so nothing more is put on it, and every
    /// invalidation on the unit fails rather than overwrite entries it has
    /// yet to read.
    broken: bool,
}

/// The queue's head, as an index: where the unit will fetch next (IQH bits
/// 18:4 for 128-bit descriptors).
fn queue_head(mmio: u64) -> u32 {
    // The index field is 15 bits, and the ring has 256 entries.
    #[allow(clippy::cast_possible_truncation)]
    let index = ((read_reg64(mmio, reg::IQH) >> 4) & 0x7FFF) as u32;
    index % QUEUE_ENTRIES
}

/// Give unit `index` an invalidation queue and turn it on: its sticky
/// queue errors cleared (they survive a reset, and one left set would stop
/// the first fetch), a descriptor page and a status page, IQT at 0 and IQA
/// at the page -- 128-bit descriptors, one page of them -- then QIE.
///
/// # Errors
///
/// What the page pool answers, or `TimedOut` if the queue never comes on --
/// in which case QIE is cleared again, and the pages are kept from the pool
/// unless the unit lets go of them.
fn enable_queue(index: usize, mmio: u64) -> KernelResult<()> {
    let slot = QUEUES.get(index).ok_or(KernelError::InvalidArgument)?;
    write_reg32(mmio, reg::FSTS, FSTS_IQE | FSTS_ICE_ITE);
    let descs_phys = page_table::alloc_pt_page()?;
    let status_phys = match page_table::alloc_pt_page() {
        Ok(p) => p,
        Err(e) => {
            // SAFETY: just taken from the pool; nothing refers to it yet.
            unsafe { page_table::free_pt_page(descs_phys) };
            return Err(e);
        }
    };
    write_reg64(mmio, reg::IQT, 0);
    write_reg64(mmio, reg::IQA, descs_phys);
    gcmd(mmio, gcmd::QIE, 0);
    if let Err(e) = wait_for_status(mmio, gsts::QIES, true) {
        gcmd(mmio, 0, gcmd::QIE);
        if wait_for_status(mmio, gsts::QIES, false).is_ok() {
            // SAFETY: the queue is off, so the unit holds neither page.
            unsafe {
                page_table::free_pt_page(descs_phys);
                page_table::free_pt_page(status_phys);
            }
        }
        return Err(e);
    }
    *slot.lock() = Some(Queue {
        descs_phys,
        status_phys,
        tail: 0,
        seq: 0,
        broken: false,
    });
    serial_println!("[iommu_remap]   Queued invalidation on");
    Ok(())
}

/// Turn unit `index`'s queue off, if it has one: drained (as far as the
/// unit will go), QIE cleared, and its pages given back once the unit has
/// let go of them. For a unit that is being left off after its queue came
/// on, and for [`shutdown`]'s kin.
fn disable_queue(index: usize, mmio: u64) {
    let Some(queue) = QUEUES.get(index).and_then(|q| q.lock().take()) else {
        return;
    };
    // A queue that will not turn off keeps its pages: the unit may still
    // read them.
    if drain_and_stop_queue(mmio) {
        // SAFETY: the queue is off, so the unit holds neither page.
        unsafe {
            page_table::free_pt_page(queue.descs_phys);
            page_table::free_pt_page(queue.status_phys);
        }
    }
}

/// Wait, a bounded while, for the queue of the unit at `mmio` to drain, then
/// turn it off: whether it is off (or was). A queue that never drains is
/// left on, as the spec requires of one with descriptors in it. Takes no
/// lock and prints nothing, for [`shutdown`].
fn drain_and_stop_queue(mmio: u64) -> bool {
    if read_reg32(mmio, reg::GSTS) & gsts::QIES == 0 {
        return true;
    }
    let drained = (0..INVALIDATION_POLLS).any(|_| {
        let empty = read_reg64(mmio, reg::IQH) == read_reg64(mmio, reg::IQT);
        if !empty {
            core::hint::spin_loop();
        }
        empty
    });
    if !drained {
        return false;
    }
    gcmd(mmio, 0, gcmd::QIE);
    status_reached(mmio, gsts::QIES, false)
}

/// Put `descs` on unit `index`'s invalidation queue (registers at `mmio`),
/// with a wait behind them, and return once the unit has done them all:
/// `None` when the unit has no queue, which the caller then invalidates
/// through its registers.
///
/// The queue is empty whenever no one holds its lock, and is found so on
/// entry or the run fails. A descriptor the unit refuses (IQE) is replaced
/// at the queue's head by this run's wait -- Linux's `qi_check_fault` -- so
/// the unit goes on past it, the queue drains, and the run reports the
/// failure. A run that times out leaves the queue [`Queue::broken`].
///
/// # Errors
///
/// `InvalidArgument` for more descriptors than the ring holds beside a wait;
/// `InternalError` when the unit refused a descriptor, reported an
/// invalidation error, or the queue was not empty; `TimedOut` when the wait
/// never completed, or the queue is broken.
fn queue_run(index: usize, mmio: u64, descs: &[[u64; 2]]) -> Option<KernelResult<()>> {
    // With interrupts off for the whole run: nothing that runs from an
    // interrupt or a softirq on this CPU can then meet the lock held.
    crate::cpu::without_interrupts(|| queue_run_unlocked(index, mmio, descs))
}

/// [`queue_run`], interrupts already off.
fn queue_run_unlocked(index: usize, mmio: u64, descs: &[[u64; 2]]) -> Option<KernelResult<()>> {
    let mut guard = QUEUES.get(index)?.lock();
    let q = guard.as_mut()?;
    if q.broken {
        return Some(Err(KernelError::TimedOut));
    }
    // One entry is always left empty, so a full ring is never mistaken for
    // an empty one.
    if descs.len().saturating_add(1) >= QUEUE_ENTRIES as usize {
        return Some(Err(KernelError::InvalidArgument));
    }
    if queue_head(mmio) != q.tail {
        q.broken = true;
        serial_println!("[iommu_remap] invalidation queue not empty between runs: left alone");
        return Some(Err(KernelError::InternalError));
    }
    let Some(hhdm) = page_table::hhdm() else {
        return Some(Err(KernelError::NotSupported));
    };
    // One more than the last, never 0: the status word is reset to 0 before
    // each run.
    q.seq = q.seq.wrapping_add(1).max(1);
    let status = q.status_phys.wrapping_add(hhdm) as *mut u32;
    // SAFETY: the status page is ours, and no wait on it is outstanding:
    // the queue is empty.
    unsafe { core::ptr::write_volatile(status, 0) };
    let wait = [
        desc::WAIT
            | desc::WAIT_STATUS_WRITE
            | desc::WAIT_FENCE
            | (u64::from(q.seq) << desc::WAIT_STATUS_SHIFT),
        q.status_phys,
    ];
    let ring = q.descs_phys.wrapping_add(hhdm) as *mut [u64; 2];
    for d in descs.iter().chain(core::iter::once(&wait)) {
        // SAFETY: `tail` < QUEUE_ENTRIES, inside the descriptor page; the
        // queue is empty, so the unit is reading no entry.
        unsafe { core::ptr::write_volatile(ring.add(q.tail as usize), *d) };
        q.tail = q.tail.wrapping_add(1) % QUEUE_ENTRIES;
    }
    // The unit reads the queue coherently whatever ECAP.C says (VT-d
    // §6.5.2), so nothing is flushed: the descriptors need only be in
    // memory's order before the tail moves.
    core::sync::atomic::fence(Ordering::SeqCst);
    write_reg64(mmio, reg::IQT, u64::from(q.tail) << 4);
    let mut failed = false;
    for _ in 0..INVALIDATION_POLLS {
        let errors = read_reg32(mmio, reg::FSTS) & (FSTS_IQE | FSTS_ICE_ITE);
        if errors & FSTS_IQE != 0 {
            // The refused descriptor gives way to a harmless one, and the
            // unit fetches it once the error is cleared.
            let bad = queue_head(mmio);
            // SAFETY: `bad` < QUEUE_ENTRIES; the unit has stopped fetching.
            unsafe { core::ptr::write_volatile(ring.add(bad as usize), wait) };
            core::sync::atomic::fence(Ordering::SeqCst);
            serial_println!(
                "[iommu_remap] invalidation queue refused descriptor {} ({:#x}): replaced",
                bad,
                errors
            );
            failed = true;
        }
        if errors != 0 {
            write_reg32(mmio, reg::FSTS, errors);
            failed |= errors & FSTS_ICE_ITE != 0;
        }
        if errors & FSTS_IQE != 0 {
            // The same tail again, which moves nothing: a unit that fetches
            // only when the tail is written (QEMU's) goes on from the head.
            write_reg64(mmio, reg::IQT, u64::from(q.tail) << 4);
        }
        // SAFETY: as above. A wait from an earlier run that lands late writes
        // an earlier `seq`.
        let done = unsafe { core::ptr::read_volatile(status) } == q.seq
            && (!failed || queue_head(mmio) == q.tail);
        if done {
            return Some(if failed {
                Err(KernelError::InternalError)
            } else {
                Ok(())
            });
        }
        core::hint::spin_loop();
    }
    q.broken = true;
    serial_println!("[iommu_remap] Timeout waiting for the invalidation queue: left alone");
    Some(Err(KernelError::TimedOut))
}

/// The last two descriptors put on unit `index`'s queue -- the last run's
/// final descriptor and its wait -- with the status the wait writes and
/// where: for [`queue_test`] to check their encoding.
fn queue_last_two(index: usize) -> Option<([u64; 2], [u64; 2], u32, u64)> {
    crate::cpu::without_interrupts(|| queue_last_two_unlocked(index))
}

/// [`queue_last_two`], interrupts already off.
fn queue_last_two_unlocked(index: usize) -> Option<([u64; 2], [u64; 2], u32, u64)> {
    let guard = QUEUES.get(index)?.lock();
    let q = guard.as_ref()?;
    let hhdm = page_table::hhdm()?;
    let ring = q.descs_phys.wrapping_add(hhdm) as *const [u64; 2];
    let at = |back: u32| {
        let i = q.tail.wrapping_add(QUEUE_ENTRIES).wrapping_sub(back) % QUEUE_ENTRIES;
        // SAFETY: `i` < QUEUE_ENTRIES, inside the descriptor page; the
        // queue is empty, so the unit is not writing it.
        unsafe { core::ptr::read_volatile(ring.add(i as usize)) }
    };
    Some((at(2), at(1), q.seq, q.status_phys))
}

/// Every unit on that can queue invalidations has its queue on; a wait
/// alone, and each invalidation this module makes, completes on it, put
/// there exactly as VT-d encodes it (a wrong field would complete as well,
/// and invalidate the wrong thing); and a descriptor the unit refuses fails
/// its run and leaves the queue working for the next.
fn queue_test() -> KernelResult<()> {
    fn fail(unit: usize, what: &str) -> KernelResult<()> {
        serial_println!(
            "[iommu_remap]   FAIL: unit {}'s invalidation queue: {}",
            unit,
            what
        );
        Err(KernelError::InternalError)
    }
    let mut queued = 0u32;
    for (i, regs) in UNIT_REGS.iter().enumerate() {
        let mmio = regs.load(Ordering::Acquire);
        if mmio == 0 {
            continue;
        }
        let can = read_reg64(mmio, reg::ECAP) & ecap::QI != 0;
        let has = QUEUES.get(i).is_some_and(|q| q.lock().is_some());
        if can != has {
            return fail(
                i,
                "a unit that can queue has no queue, or one that cannot has",
            );
        }
        if !has {
            continue;
        }
        let cap_val = read_reg64(mmio, reg::CAP);
        let drains = if cap_val & cap::DRD != 0 {
            desc::IOTLB_DRAIN_READS
        } else {
            0
        } | if cap_val & cap::DWD != 0 {
            desc::IOTLB_DRAIN_WRITES
        } else {
            0
        };
        type Run<'a> = (&'a str, &'a dyn Fn() -> KernelResult<()>, Option<u64>);
        let runs: [Run<'_>; 4] = [
            (
                "a wait alone",
                &|| queue_run(i, mmio, &[]).unwrap_or(Err(KernelError::NotFound)),
                None,
            ),
            (
                "a global context invalidation",
                &|| invalidate_context_global(i, mmio),
                Some(0x11),
            ),
            (
                "a global IOTLB invalidation, drained",
                &|| invalidate_iotlb(i, mmio, None, true),
                Some(0x12 | drains),
            ),
            (
                "a domain's IOTLB invalidation",
                &|| invalidate_iotlb(i, mmio, Some(PASS_THROUGH_DID), false),
                Some(0x22 | (u64::from(PASS_THROUGH_DID) << 16)),
            ),
        ];
        for (what, run, expected) in runs {
            if run().is_err() {
                return fail(i, what);
            }
            let Some((last, wait, seq, status_phys)) = queue_last_two(i) else {
                return fail(i, "its descriptors could not be read back");
            };
            if wait != [0x65 | (u64::from(seq) << 32), status_phys] {
                return fail(i, "a wait is not encoded as VT-d encodes it");
            }
            if expected.is_some_and(|low| last != [low, 0]) {
                serial_println!("[iommu_remap]   {}: {:#x?}", what, last);
                return fail(i, "an invalidation is not encoded as VT-d encodes it");
            }
        }
        // A descriptor of a type VT-d does not define: refused, and the run
        // says so; the next run works.
        if queue_run(i, mmio, &[[0xF, 0]]) != Some(Err(KernelError::InternalError)) {
            return fail(i, "a refused descriptor did not fail its run");
        }
        if queue_run(i, mmio, &[]) != Some(Ok(())) {
            return fail(i, "the queue did not work again after a refused descriptor");
        }
        queued = queued.saturating_add(1);
    }
    serial_println!(
        "[iommu_remap]   Queued invalidation: {} unit(s); a wait and each invalidation completed, encoded as VT-d encodes them; a refused descriptor failed its run and the queue went on: OK",
        queued
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Interrupt remapping (VT-d §5.1, §9.10)
// ---------------------------------------------------------------------------

/// An interrupt remapping table entry's fields (VT-d §9.10): the low qword's,
/// then the high qword's.
mod irte {
    /// Present.
    pub const PRESENT: u64 = 1 << 0;
    /// Trigger mode level (TM): an I/O APIC pin's that is level-triggered.
    /// Clear, edge -- as every MSI is.
    pub const LEVEL: u64 = 1 << 4;
    /// Where the vector goes.
    pub const VECTOR_SHIFT: u32 = 16;
    /// Where an xAPIC destination id goes: bits 15:8 of the destination
    /// field, which is bits 63:32.
    pub const DEST_SHIFT: u32 = 40;
    /// High qword: the requester id must equal the entry's source id, all
    /// 16 bits of it (SVT 01, SQ 00).
    pub const VERIFY_SID: u64 = 0b01 << 18;
    /// High qword: the requester id's bus must lie between the source id's
    /// high byte and its low byte (SVT 10b), for a function behind a
    /// PCIe-to-PCI bridge.
    pub const VERIFY_BUS: u64 = 0b10 << 18;
}

/// Entries in the interrupt remapping table: one 4 KiB page of 16-byte
/// entries, which IRTA's size field calls 2^(7+1).
const IRT_ENTRIES: u16 = 256;
const IRTA_SIZE: u64 = 7;

/// Handles below this are the I/O APIC's pins', the handle the pin's own
/// number; MSIs are given the rest.
pub const IOAPIC_PINS: u16 = 24;
const _: () = assert!(IOAPIC_PINS as usize == crate::ioapic::MAX_IRQ);

/// The interrupt remapping table every unit shares: a handle means the same
/// entry whichever unit a message passes through.
struct Irt {
    /// Its page.
    phys: u64,
    /// Which handles are given out, a bit each.
    used: [u64; IRT_ENTRIES as usize / 64],
    /// Which entries were last written without every unit's being made to
    /// forget the old one: what is in the table may not be what a unit
    /// delivers through, so the next write of the same entry is done again.
    unsettled: [u64; IRT_ENTRIES as usize / 64],
}

/// Bit `handle` of a bitmap of entries.
fn bit(map: &[u64; IRT_ENTRIES as usize / 64], handle: u16) -> bool {
    map.get(usize::from(handle / 64))
        .is_some_and(|w| w & (1 << (handle % 64)) != 0)
}

/// Set or clear bit `handle` of a bitmap of entries.
fn set_bit(map: &mut [u64; IRT_ENTRIES as usize / 64], handle: u16, on: bool) {
    if let Some(w) = map.get_mut(usize::from(handle / 64)) {
        if on {
            *w |= 1 << (handle % 64);
        } else {
            *w &= !(1 << (handle % 64));
        }
    }
}

impl Irt {
    fn is_used(&self, handle: u16) -> bool {
        bit(&self.used, handle)
    }

    fn set_used(&mut self, handle: u16, used: bool) {
        set_bit(&mut self.used, handle, used);
    }

    /// Entry `handle`'s two qwords, low first.
    fn entry(&self, handle: u16, hhdm: u64) -> Option<*mut [u64; 2]> {
        (handle < IRT_ENTRIES).then(|| {
            // Inside the table's page: `handle` < 256 entries of 16 bytes.
            (self.phys.wrapping_add(hhdm) as *mut [u64; 2]).wrapping_add(usize::from(handle))
        })
    }

    /// Entry `handle` as the units read it.
    fn read(&self, handle: u16) -> Option<[u64; 2]> {
        let at = self.entry(handle, page_table::hhdm()?)?;
        // SAFETY: an entry of the table's page, which the direct map maps.
        Some(unsafe { core::ptr::read_volatile(at) })
    }

    /// Make entry `handle` read `[low, high]`, and have every unit forget
    /// what it cached of it. An entry coming present gets its high qword
    /// first and one going away loses its low (and present bit) first, so a
    /// unit never reads one half-written: the high qword -- the source id --
    /// is never changed in a present entry. Until every unit has forgotten
    /// the old entry, the entry is unsettled.
    fn write(&mut self, handle: u16, low: u64, high: u64) -> KernelResult<()> {
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        let at = self
            .entry(handle, hhdm)
            .ok_or(KernelError::InvalidArgument)? as *mut u64;
        // SAFETY: the entry's two qwords, in the table's page; the IRT lock
        // is held, so no one else writes them.
        unsafe {
            if low & irte::PRESENT != 0 {
                core::ptr::write_volatile(at.wrapping_add(1), high);
                core::sync::atomic::fence(Ordering::SeqCst);
                core::ptr::write_volatile(at, low);
            } else {
                core::ptr::write_volatile(at, low);
                core::sync::atomic::fence(Ordering::SeqCst);
                core::ptr::write_volatile(at.wrapping_add(1), high);
            }
        }
        sync_tables(
            self.phys.wrapping_add(u64::from(handle).wrapping_mul(16)),
            16,
            hhdm,
        );
        set_bit(&mut self.unsettled, handle, true);
        invalidate_interrupt_entry(handle)?;
        set_bit(&mut self.unsettled, handle, false);
        Ok(())
    }
}

/// The table, once interrupt remapping is on.
///
/// Locks: held with interrupts off ([`with_irt`]), as each unit's queue is
/// ([`queue_run`]), since an interrupt's or a softirq's code may change an
/// entry (`ioapic::set_irq_affinity`, from the IRQ balancer) and must not
/// find either held by the code it interrupted. The order is [`REMAP_LOCK`],
/// then this, then a unit's [`QUEUES`] lock; none of them is held while the
/// I/O APIC's register window is taken.
static IRT: Mutex<Option<Irt>> = Mutex::new(None);

/// `f` on the table, under its lock with interrupts off; `NotSupported`
/// when interrupts are not remapped.
fn with_irt<R>(f: impl FnOnce(&mut Irt) -> KernelResult<R>) -> KernelResult<R> {
    crate::cpu::without_interrupts(|| f(IRT.lock().as_mut().ok_or(KernelError::NotSupported)?))
}

/// The units remapping interrupts, by unit index: each one's registers'
/// virtual base, 0 for one not. Apart from [`UNIT_REGS`]: a unit that cannot
/// translate may still remap, and must, for its devices' interrupts to be
/// checked. Lock-free, so [`shutdown`] can reach every unit.
static IR_REGS: [AtomicU64; MAX_UNITS] = [const { AtomicU64::new(0) }; MAX_UNITS];

/// Whether every unit remaps interrupts: devices' MSIs and the I/O APIC's
/// pins must then be programmed in remappable format.
static INTERRUPTS_REMAPPED: AtomicBool = AtomicBool::new(false);

/// How many times turning interrupt remapping on failed for a reason other
/// than the hardware's lacking it: a fault [`self_test`] reports.
static REMAPPING_FAILED: AtomicU32 = AtomicU32::new(0);

/// The requester id the I/O APIC's interrupts carry (from its DMAR scope),
/// which each of its pins' entries checks.
static IOAPIC_SID: AtomicU32 = AtomicU32::new(0);

/// Whether interrupts are remapped: whether [`alloc_msi_entry`] and
/// [`set_ioapic_entry`] must be used, and a device's MSI or an I/O APIC pin
/// programmed in remappable format.
#[must_use]
pub fn interrupts_remapped() -> bool {
    INTERRUPTS_REMAPPED.load(Ordering::Acquire)
}

/// Every unit on forgets entry `handle` (an interrupt-entry-cache
/// invalidation of that index, VT-d §6.5.2.7).
///
/// # Errors
///
/// The first unit's that failed, every unit tried regardless.
fn invalidate_interrupt_entry(handle: u16) -> KernelResult<()> {
    let one = desc::IEC | desc::IEC_INDEX | (u64::from(handle) << desc::IEC_INDEX_SHIFT);
    let mut first = Ok(());
    for (i, regs) in IR_REGS.iter().enumerate() {
        let mmio = regs.load(Ordering::Acquire);
        if mmio == 0 {
            continue;
        }
        if let Err(e) = queue_run(i, mmio, &[[one, 0]]).unwrap_or(Err(KernelError::NotSupported)) {
            serial_println!(
                "[iommu_remap] unit {}: interrupt entry {} not forgotten: {:?}",
                i,
                handle,
                e
            );
            if first.is_ok() {
                first = Err(e);
            }
        }
    }
    first
}

/// Turn interrupt remapping on in every unit, one table shared: compatibility
/// -format interrupts -- the only kind a device could send before, carrying
/// any vector to any CPU -- blocked (CFI clear), so that only a message
/// naming an entry is delivered, and only from the function the entry names.
///
/// All or nothing: a device behind a unit without it would have its
/// remappable-format messages read as compatibility-format ones. So the
/// DMAR must say the platform supports it (its INTR_REMAP flag, which a
/// firmware clears to refuse it, as Linux's `dmar_ir_support` reads it);
/// every unit it lists must have been probed (`probed`, their registers by
/// index), whether or not it translates, and must have ECAP.IR and a queue
/// (the entry cache has no register to invalidate it through), which a unit
/// that does not translate is given here; and the I/O APIC the MADT names
/// must have a DMAR scope, since its pins' entries check its requester id.
///
/// # Errors
///
/// `NotSupported` when the hardware or the tables lack any of that, which
/// leaves interrupts as they were; otherwise what turning it on answered,
/// with every unit's remapping turned off again.
fn enable_interrupt_remapping(probed: &[Option<u64>; MAX_UNITS]) -> KernelResult<()> {
    if !crate::iommu::interrupt_remapping_supported() {
        serial_println!(
            "[iommu_remap]   the DMAR says the platform does not support interrupt remapping"
        );
        return Err(KernelError::NotSupported);
    }
    let units = usize::from(crate::iommu::unit_count()).min(MAX_UNITS);
    let mut mmios = [0u64; MAX_UNITS];
    for (i, (slot, probe)) in mmios.iter_mut().zip(probed).enumerate().take(units) {
        let Some(mmio) = *probe else {
            // Its probe failed, and said why: a fault, not a lack.
            serial_println!("[iommu_remap]   unit {} could not be probed", i);
            return Err(KernelError::InternalError);
        };
        let ecap_val = read_reg64(mmio, reg::ECAP);
        if ecap_val & ecap::IR == 0 || ecap_val & ecap::QI == 0 {
            serial_println!(
                "[iommu_remap]   unit {} cannot remap interrupts (ECAP {:#x})",
                i,
                ecap_val
            );
            return Err(KernelError::NotSupported);
        }
        *slot = mmio;
    }
    let Some(ioapic) = crate::acpi::io_apics().first().copied() else {
        serial_println!("[iommu_remap]   the MADT names no I/O APIC");
        return Err(KernelError::NotSupported);
    };
    let Some(sid) = crate::iommu::ioapic_scope(ioapic.id).and_then(|scope| {
        scope.source_id(|bus, device, function| {
            crate::pci::config_read8(bus, device, function, 0x19)
        })
    }) else {
        serial_println!(
            "[iommu_remap]   the DMAR gives no scope for I/O APIC {}",
            ioapic.id
        );
        return Err(KernelError::NotSupported);
    };
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    // A table page comes zeroed: every entry not present.
    let phys = page_table::alloc_pt_page()?;
    sync_tables(phys, IOMMU_PAGE_SIZE, hhdm);
    IOAPIC_SID.store(u32::from(sid), Ordering::Release);
    *IRT.lock() = Some(Irt {
        phys,
        used: [0; IRT_ENTRIES as usize / 64],
        unsettled: [0; IRT_ENTRIES as usize / 64],
    });
    let units_on = mmios.get(..units).unwrap_or(&[]);
    for (i, &mmio) in units_on.iter().enumerate() {
        // A unit that does not translate has no queue yet.
        let queued = if QUEUES.get(i).is_some_and(|q| q.lock().is_none()) {
            enable_queue(i, mmio)
        } else {
            Ok(())
        };
        if let Err(e) = queued.and_then(|()| remap_unit(i, mmio, phys)) {
            serial_println!(
                "[iommu_remap]   unit {} would not remap interrupts: {:?}",
                i,
                e
            );
            // Every unit back to compatibility format, and the table kept
            // from the pool unless every unit has let go of it.
            // Every unit asked, not only up to the first that stays on.
            let mut all_off = true;
            for &m in units_on {
                all_off &= stop_remapping(m);
            }
            for regs in &IR_REGS {
                regs.store(0, Ordering::Release);
            }
            // A queue turned on here, for a unit that does not translate,
            // off again: nothing will use it, nor would kexec reach it.
            for (j, &m) in units_on.iter().enumerate() {
                if UNIT_REGS
                    .get(j)
                    .is_some_and(|r| r.load(Ordering::Acquire) == 0)
                {
                    disable_queue(j, m);
                }
            }
            if let Some(irt) = IRT.lock().take()
                && all_off
            {
                // SAFETY: no unit remaps interrupts, so none reads the page.
                unsafe { page_table::free_pt_page(irt.phys) };
            }
            return Err(e);
        }
        if let Some(regs) = IR_REGS.get(i) {
            regs.store(mmio, Ordering::Release);
        }
    }
    INTERRUPTS_REMAPPED.store(true, Ordering::Release);
    Ok(())
}

/// Point unit `index` (registers at `mmio`) at the table at `phys`, have it
/// forget every entry it cached, block compatibility-format interrupts, and
/// turn remapping on (VT-d §10.4.4's sequence, as Linux's
/// `iommu_set_irq_remapping` and `iommu_enable_irq_remapping` do it).
fn remap_unit(index: usize, mmio: u64, phys: u64) -> KernelResult<()> {
    write_reg64(mmio, reg::IRTA, phys | IRTA_SIZE);
    gcmd(mmio, gcmd::SIRTP, 0);
    wait_for_status(mmio, gsts::IRTPS, true)?;
    queue_run(index, mmio, &[[desc::IEC, 0]]).unwrap_or(Err(KernelError::NotSupported))?;
    if read_reg32(mmio, reg::GSTS) & gsts::CFIS != 0 {
        gcmd(mmio, 0, gcmd::CFI);
        wait_for_status(mmio, gsts::CFIS, false)?;
    }
    gcmd(mmio, gcmd::IRE, 0);
    wait_for_status(mmio, gsts::IRES, true)
}

/// Turn the unit at `mmio`'s interrupt remapping off: whether it is off.
/// Takes no lock and prints nothing, for [`shutdown`].
fn stop_remapping(mmio: u64) -> bool {
    if read_reg32(mmio, reg::GSTS) & gsts::IRES == 0 {
        return true;
    }
    gcmd(mmio, 0, gcmd::IRE);
    status_reached(mmio, gsts::IRES, false)
}

/// The entry's low qword for `vector` delivered to the CPU whose APIC id is
/// `dest`: present, fixed delivery, physical destination, edge or level.
fn irte_low(vector: u8, dest: u8, level: bool) -> u64 {
    irte::PRESENT
        | if level { irte::LEVEL } else { 0 }
        | (u64::from(vector) << irte::VECTOR_SHIFT)
        | (u64::from(dest) << irte::DEST_SHIFT)
}

/// Point I/O APIC pin `pin` -- whose entry's handle is `pin` -- at `vector`
/// on the CPU whose APIC id is `dest`, level- or edge-triggered as the pin
/// is. Only the I/O APIC's requester id may raise it. An entry already so,
/// and known to every unit as so, is left alone, and no unit is told.
///
/// # Errors
///
/// `NotSupported` when interrupts are not remapped; `InvalidArgument` for a
/// pin past [`IOAPIC_PINS`]; or a unit's failure to forget the old entry.
pub fn set_ioapic_entry(pin: u8, vector: u8, dest: u8, level: bool) -> KernelResult<()> {
    let handle = u16::from(pin);
    if handle >= IOAPIC_PINS {
        return Err(KernelError::InvalidArgument);
    }
    with_irt(|irt| {
        let entry = [
            irte_low(vector, dest, level),
            u64::from(IOAPIC_SID.load(Ordering::Acquire)) | irte::VERIFY_SID,
        ];
        if irt.is_used(handle) && !bit(&irt.unsettled, handle) && irt.read(handle) == Some(entry) {
            return Ok(());
        }
        irt.set_used(handle, true);
        let [low, high] = entry;
        irt.write(handle, low, high)
    })
}

/// Give a device's MSI an entry: `vector`, edge-triggered, on the CPU whose
/// APIC id is `dest`, raised only by what `source` names -- the requester
/// id the function's messages arrive with, exactly, or the buses one behind
/// a PCIe-to-PCI bridge may arrive from ([`crate::pci::msi_source`]).
/// Returns its handle, which the device's MSI address names
/// ([`msi_address`]).
///
/// # Errors
///
/// `NotSupported` when interrupts are not remapped; `OutOfMemory` when
/// every handle is given out; or a unit's failure to forget the entry, in
/// which case the handle is not given out.
pub fn alloc_msi_entry(source: crate::pci::MsiSource, vector: u8, dest: u8) -> KernelResult<u16> {
    let high = match source {
        crate::pci::MsiSource::Exact(sid) => u64::from(sid) | irte::VERIFY_SID,
        crate::pci::MsiSource::Buses { start, end } => {
            (u64::from(start) << 8) | u64::from(end) | irte::VERIFY_BUS
        }
    };
    with_irt(|irt| {
        let handle = (IOAPIC_PINS..IRT_ENTRIES)
            .find(|&h| !irt.is_used(h))
            .ok_or(KernelError::OutOfMemory)?;
        irt.set_used(handle, true);
        let written = irt.write(handle, irte_low(vector, dest, false), high);
        if written.is_err() {
            // Not present again -- whatever a unit still holds of it, the
            // handle is never given out to another device while it may.
            // That write's own failure changes nothing about the handle.
            let _ = irt.write(handle, 0, 0);
        }
        written.map(|()| handle)
    })
}

/// Take back an entry [`alloc_msi_entry`] gave, once its device's MSI is
/// off. The handle goes back to the pool only once every unit has forgotten
/// the entry; until then it stays given out, so no other device can be
/// handed what a unit may still deliver for the first.
///
/// # Errors
///
/// `InvalidArgument` for a handle that is an I/O APIC pin's or not given
/// out; `NotSupported` when interrupts are not remapped; or a unit's
/// failure to forget the entry.
pub fn free_msi_entry(handle: u16) -> KernelResult<()> {
    with_irt(|irt| {
        if handle < IOAPIC_PINS || !irt.is_used(handle) {
            return Err(KernelError::InvalidArgument);
        }
        irt.write(handle, 0, 0)?;
        irt.set_used(handle, false);
        Ok(())
    })
}

/// The address a device's MSI writes to raise entry `handle`: remappable
/// format (bit 4), the handle's bits 14:0 at 19:5 and bit 15 at 2, no
/// subhandle (VT-d §5.1.2.2). Its data is 0.
#[must_use]
pub const fn msi_address(handle: u16) -> u32 {
    0xFEE0_0000 | (((handle & 0x7FFF) as u32) << 5) | (1 << 4) | (((handle >> 15) as u32) << 2)
}

/// The handle an MSI address in remappable format names ([`msi_address`]'s
/// inverse): `None` for an address in compatibility format.
#[must_use]
pub const fn msi_handle(address: u32) -> Option<u16> {
    if address & 0xFFF0_0000 != 0xFEE0_0000 || address & (1 << 4) == 0 {
        return None;
    }
    // 15 bits and 1: a u16.
    #[allow(clippy::cast_possible_truncation)]
    Some((((address >> 5) & 0x7FFF) | (((address >> 2) & 1) << 15)) as u16)
}

/// Entry `handle` as the units read it, for the I/O APIC's self-test.
#[must_use]
pub fn interrupt_entry(handle: u16) -> Option<[u64; 2]> {
    with_irt(|irt| irt.read(handle).ok_or(KernelError::InvalidArgument)).ok()
}

/// For [`self_test`], with interrupts remapped: every unit says so, blocks
/// compatibility-format interrupts and names the table; an MSI's entry is
/// laid out as VT-d reads it, its handle reaches the address in the right
/// bits, every unit is told when it goes, and its handle comes back; the
/// handles run out, and say so, rather than overlapping the I/O APIC's.
fn interrupt_remap_test() -> KernelResult<()> {
    fn fail(what: &str) -> KernelResult<()> {
        serial_println!("[iommu_remap]   FAIL: interrupt remapping: {}", what);
        Err(KernelError::InternalError)
    }
    let Ok(phys) = with_irt(|irt| Ok(irt.phys)) else {
        return fail("on, but no table");
    };
    let units = usize::from(crate::iommu::unit_count()).min(MAX_UNITS);
    for regs in IR_REGS.iter().take(units) {
        let mmio = regs.load(Ordering::Acquire);
        if mmio == 0 {
            return fail("a unit the DMAR lists is not remapping");
        }
        let status = read_reg32(mmio, reg::GSTS);
        if status & gsts::IRES == 0 || status & gsts::CFIS != 0 {
            return fail(
                "a unit is not remapping, or lets compatibility-format interrupts through",
            );
        }
        if read_reg64(mmio, reg::IRTA) & !0x800 != phys | IRTA_SIZE {
            return fail("a unit names another table");
        }
    }
    if msi_address(0x8001) != 0xFEE0_0034
        || msi_address(0x1234) != 0xFEE2_4690
        || msi_handle(0xFEE0_0034) != Some(0x8001)
        || msi_handle(msi_address(0x1234)) != Some(0x1234)
        || msi_handle(0xFEE0_1000).is_some()
    {
        return fail("an MSI address does not carry its handle as VT-d reads it");
    }
    let exact = crate::pci::MsiSource::Exact(0x0A18);
    let handle = alloc_msi_entry(exact, 0x41, 3)?;
    let entry = interrupt_entry(handle);
    // Behind a PCIe-to-PCI bridge: the bus range, SVT 10b.
    let behind = alloc_msi_entry(crate::pci::MsiSource::Buses { start: 5, end: 6 }, 0x42, 3);
    let range = behind.ok().and_then(interrupt_entry);
    let freed_range = behind.is_ok_and(|h| free_msi_entry(h).is_ok());
    if handle < IOAPIC_PINS
        || entry != Some([0x0000_0300_0041_0001, 0x4_0A18])
        || range != Some([0x0000_0300_0042_0001, 0x8_0506])
        || !freed_range
    {
        serial_println!(
            "[iommu_remap]   handle {}: {:#x?}; behind a bridge: {:#x?}",
            handle,
            entry,
            range
        );
        return fail("an MSI's entry is not laid out as VT-d reads it");
    }
    free_msi_entry(handle)?;
    let told = IR_REGS
        .iter()
        .enumerate()
        .filter(|(_, r)| r.load(Ordering::Acquire) != 0)
        .all(|(i, _)| {
            queue_last_two(i)
                .is_some_and(|(last, ..)| last == [0x14 | (u64::from(handle) << 32), 0])
        });
    if !told || interrupt_entry(handle) != Some([0, 0]) {
        return fail("a freed entry was left present, or a unit was not told");
    }
    if free_msi_entry(handle) != Err(KernelError::InvalidArgument)
        || free_msi_entry(0) != Err(KernelError::InvalidArgument)
    {
        return fail("a free handle, or an I/O APIC pin's, was taken back");
    }
    // An I/O APIC pin: none past the last; and one set as it already is is
    // left alone -- no unit is told.
    let pin0 = interrupt_entry(0).filter(|[low, _]| low & irte::PRESENT != 0);
    let unchanged = pin0.is_some_and(|[low, _]| {
        #[allow(clippy::cast_possible_truncation)] // Fields of the qword.
        let (vector, dest) = (
            (low >> irte::VECTOR_SHIFT) as u8,
            (low >> irte::DEST_SHIFT) as u8,
        );
        let seq = |i| queue_last_two(i).map(|(.., seq, _)| seq);
        let before: alloc::vec::Vec<_> = (0..MAX_UNITS).map(seq).collect();
        let again = set_ioapic_entry(0, vector, dest, low & irte::LEVEL != 0);
        let after: alloc::vec::Vec<_> = (0..MAX_UNITS).map(seq).collect();
        again == Ok(()) && before == after && interrupt_entry(0) == pin0
    });
    #[allow(clippy::cast_possible_truncation)] // IOAPIC_PINS is 24.
    let past = set_ioapic_entry(IOAPIC_PINS as u8, 0x30, 0, false);
    if pin0.is_none() || !unchanged || past != Err(KernelError::InvalidArgument) {
        return fail(
            "an I/O APIC pin's entry set as it was told a unit, or one past the last was taken",
        );
    }
    // Every handle the MSIs may have, then one more: those already given
    // out (a driver's) counted, the pins' with them.
    let (in_use, pins_unused) = with_irt(|irt| {
        let in_use = irt
            .used
            .iter()
            .map(|w| w.count_ones() as usize)
            .sum::<usize>();
        Ok((
            in_use,
            (0..IOAPIC_PINS).filter(|&h| !irt.is_used(h)).count(),
        ))
    })?;
    let mut given = alloc::vec::Vec::new();
    let exhausted = loop {
        match alloc_msi_entry(exact, 0x41, 3) {
            Ok(h) => given.push(h),
            Err(e) => break e,
        }
    };
    // Every one given back, whichever fail: one left given out would starve
    // every driver after this test.
    let mut freed = true;
    for &h in &given {
        freed &= free_msi_entry(h).is_ok();
    }
    let first = alloc_msi_entry(exact, 0x41, 3);
    freed &= first.is_ok_and(|h| free_msi_entry(h).is_ok());
    if exhausted != KernelError::OutOfMemory
        || given.iter().any(|&h| h < IOAPIC_PINS)
        || given
            .len()
            .saturating_add(in_use)
            .saturating_add(pins_unused)
            != usize::from(IRT_ENTRIES)
        || !freed
    {
        serial_println!(
            "[iommu_remap]   {} handle(s) given before {:?}",
            given.len(),
            exhausted
        );
        return fail("the handles did not run out cleanly, or did not come back");
    }
    serial_println!(
        "[iommu_remap]   Interrupt remapping: every unit on with compatibility format blocked; an MSI's entry, its address, its freeing and {} handles: OK",
        given.len()
    );
    Ok(())
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

/// The host physical address a device in `domain_id` reaches at `bus_addr`,
/// or `None` where the domain maps nothing (or there is no such domain): the
/// domain's own tables read back, as the IOMMU walks them.
#[must_use]
pub fn translate_bus(domain_id: DomainId, bus_addr: u64) -> Option<u64> {
    let hhdm = page_table::hhdm()?;
    // Held through the walk: a domain's tables are freed under it, when the
    // domain is destroyed.
    let domains = DOMAINS.lock();
    let root = domains
        .iter()
        .find(|d| d.active && d.id == domain_id)
        .map(|d| d.pml4_phys)?;
    let index = |shift: u32| ((bus_addr >> shift) & 0x1FF) as usize;
    let pdpt = walk_existing(root, index(39), hhdm)?;
    let pd = walk_existing(pdpt, index(30), hhdm)?;
    let pt = walk_existing(pd, index(21), hhdm)?;
    let page = walk_existing(pt, index(12), hhdm)?;
    Some(page | (bus_addr & 0xFFF))
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
    for_each_unit_on(|i, mmio| invalidate_iotlb(i, mmio, Some(domain_id), true))
}

/// Tell every unit of domain `domain_id`'s new translations as it needs:
/// one in caching mode may have cached their absence, and has its IOTLB
/// invalidated (VT-d §6.1); one that is not, but asks for write-buffer
/// flushing (CAP.RWBF), has its write buffer flushed, so that its walks see
/// the new entries (§6.8, Linux's `iommu_flush_write_buffer`). No DMA in
/// flight can use a translation that did not exist, so none is drained.
fn flush_new_mappings(domain_id: DomainId) -> KernelResult<()> {
    for_each_unit_on(|i, mmio| {
        let cap_val = read_reg64(mmio, reg::CAP);
        if cap_val & cap::CM != 0 {
            return invalidate_iotlb(i, mmio, Some(domain_id), false);
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
fn for_each_unit_on(f: impl Fn(usize, u64) -> KernelResult<()>) -> KernelResult<()> {
    let mut first = Ok(());
    for (i, regs) in UNIT_REGS.iter().enumerate() {
        let mmio = regs.load(Ordering::Acquire);
        if mmio == 0 {
            continue;
        }
        if let Err(e) = f(i, mmio) {
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
    for (regs, ir) in UNIT_REGS.iter().zip(&IR_REGS) {
        let translating = regs.load(Ordering::Acquire);
        if translating != 0 {
            gcmd(translating, 0, gcmd::TE);
            if status_reached(translating, gsts::TES, false) {
                regs.store(0, Ordering::Release);
            }
        }
        let mmio = if translating != 0 {
            translating
        } else {
            ir.load(Ordering::Acquire)
        };
        if mmio == 0 {
            continue;
        }
        // Nor a table of interrupt entries, nor a queue running, in pages
        // the next kernel may reuse. Their pages stay out of the pool: this
        // kernel is going away.
        if stop_remapping(mmio) {
            ir.store(0, Ordering::Release);
        }
        drain_and_stop_queue(mmio);
    }
    INTERRUPTS_REMAPPED.store(false, Ordering::Release);
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

/// How many faults [`take_faults`] has taken.
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

/// A fault a unit recorded (VT-d §7.2.1): a DMA request it refused, or an
/// interrupt request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault {
    /// The requester id (bus << 8 | device << 3 | function) the request
    /// carried.
    pub source_id: u16,
    /// Why (VT-d Appendix A): [`FAULT_IR_SOURCE_ID`], say.
    pub reason: u8,
    /// The record's low qword: for an interrupt, the entry it named in bits
    /// 63:48; for a DMA request, the page it was for.
    pub info: u64,
}

/// An interrupt request naming an entry that does not name its function
/// (VT-d Appendix A, fault reason 26h).
pub const FAULT_IR_SOURCE_ID: u8 = 0x26;

/// A fault record's high qword: the record holds a fault (F, bit 63, which
/// writing 1 clears), its reason at bits 39:32, the requester id at 15:0.
const FRCD_FAULT: u64 = 1 << 63;
/// FSTS's primary fault overflow (PFO): a fault came with every record
/// full, and no more are recorded until it is cleared.
const FSTS_PFO: u32 = 1 << 0;

/// Serialises [`take_faults`]: a record is read and then cleared, and two
/// callers between the two would both take it.
static FAULT_RECORDS: Mutex<()> = Mutex::new(());

/// Take every fault the units on have recorded: each record read and
/// cleared, and an overflow cleared with them, so that the next fault is
/// recorded rather than dropped. Each is taken once, whoever else calls.
/// Counted in [`fault_count`].
pub fn take_faults() -> alloc::vec::Vec<Fault> {
    let _taking = FAULT_RECORDS.lock();
    let mut faults = alloc::vec::Vec::new();
    for regs in &UNIT_REGS {
        let mmio = regs.load(Ordering::Acquire);
        if mmio == 0 {
            continue;
        }
        let cap_val = read_reg64(mmio, reg::CAP);
        let fro = (cap_val & cap::FRO_MASK) >> cap::FRO_SHIFT;
        let nfr = (cap_val & cap::NFR_MASK) >> cap::NFR_SHIFT;
        for record in 0..=nfr {
            // Inside the register block `probe_unit` mapped: it reaches
            // the last record.
            let at = fro.saturating_add(record).saturating_mul(16);
            let high = read_reg64(mmio, at.saturating_add(8));
            if high & FRCD_FAULT == 0 {
                continue;
            }
            #[allow(clippy::cast_possible_truncation)] // Fields of the qword.
            faults.push(Fault {
                source_id: high as u16,
                reason: (high >> 32) as u8,
                info: read_reg64(mmio, at),
            });
            write_reg32(mmio, at.saturating_add(12), 1 << 31);
        }
        write_reg32(mmio, reg::FSTS, FSTS_PFO);
    }
    TOTAL_FAULTS.fetch_add(faults.len() as u64, Ordering::Relaxed);
    faults
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

    // Test 10c: every unit that could be turned on came on.
    let failed = UNITS_FAILED.load(Ordering::Relaxed);
    if failed != 0 {
        serial_println!(
            "[iommu_remap]   FAIL: {} unit(s) that could be turned on did not come on",
            failed
        );
        return Err(KernelError::InternalError);
    }
    if REMAPPING_FAILED.load(Ordering::Relaxed) != 0 {
        serial_println!(
            "[iommu_remap]   FAIL: interrupt remapping the hardware has could not be turned on"
        );
        return Err(KernelError::InternalError);
    }

    // Test 11: with a unit on, every device passes through, and a device
    // given a domain is translated by it until it is detached.
    if is_active() {
        remap_live_test()?;
        queue_test()?;
    }
    if interrupts_remapped() {
        interrupt_remap_test()?;
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

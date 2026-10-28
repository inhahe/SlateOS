//! IOMMU detection and status — DMA sandboxing infrastructure.
//!
//! Detects Intel VT-d and AMD-Vi IOMMU hardware via ACPI tables and
//! reports whether DMA sandboxing is available.
//!
//! ## Design (from design.txt)
//!
//! > "Sandboxed drivers using IOMMU (available on all modern systems,
//! > but sometimes disabled by default in the BIOS) are much faster
//! > than userspace drivers."
//!
//! > "We should probably ask the user to enable IOMMU in their BIOS
//! > if we detect that it's disabled."
//!
//! ## Detection Method
//!
//! - **Intel VT-d**: Look for ACPI DMAR (DMA Remapping) table.
//!   Contains DMA Remapping Hardware Unit Definitions (DRHD) that
//!   describe each IOMMU unit's register base address and device scope.
//!
//! - **AMD-Vi**: Look for ACPI IVRS (I/O Virtualization Reporting
//!   Structure) table.
//!
//! ## Current Scope
//!
//! This module handles detection only:
//! 1. Probe for DMAR/IVRS ACPI tables during boot.
//! 2. Parse hardware unit addresses and capabilities.
//! 3. Report whether IOMMU is available and how many units exist.
//! 4. Log a recommendation if IOMMU is not detected.
//!
//! Actual IOMMU page table setup and DMA remapping will be added when
//! the driver sandboxing system is implemented.
//!
//! ## References
//!
//! - Intel VT-d specification (Vt-directed-io-spec.pdf)
//! - AMD I/O Virtualization Technology (IOMMU) specification
//! - Linux `drivers/iommu/intel/dmar.c` — DMAR table parsing
//! - Linux `drivers/iommu/amd/init.c` — IVRS table parsing

use crate::acpi;
use crate::error::{KernelError, KernelResult};
use crate::mm;
use crate::serial_println;

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

// ---------------------------------------------------------------------------
// DMAR table structures (Intel VT-d)
// ---------------------------------------------------------------------------

/// DMAR table header (after standard ACPI header).
#[repr(C, packed)]
struct DmarHeader {
    /// Width of host address (physical address width for DMA).
    host_address_width: u8,
    /// Flags:
    /// - Bit 0: INTR_REMAP — interrupt remapping supported
    /// - Bit 1: X2APIC_OPT_OUT
    /// - Bit 2: DMA_CTRL_PLATFORM_OPT_IN_FLAG
    flags: u8,
    /// Reserved.
    _reserved: [u8; 10],
}

/// DMAR remapping structure type.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Documented for spec completeness; only DRHD parsed currently.
enum DmarStructType {
    /// DMA Remapping Hardware Unit Definition.
    Drhd = 0,
    /// Reserved Memory Region Reporting.
    Rmrr = 1,
    /// Root Port ATS Capability Reporting.
    Atsr = 2,
    /// Remapping Hardware Static Affinity.
    Rhsa = 3,
    /// ACPI Name-space Device Declaration.
    Andd = 4,
}

/// DRHD (DMA Remapping Hardware Unit Definition) structure.
///
/// Describes a single IOMMU hardware unit.
#[repr(C, packed)]
struct DrhdEntry {
    /// Structure type (must be 0 = DRHD).
    struct_type: u16,
    /// Total length of this structure including device scope entries.
    length: u16,
    /// Flags:
    /// - Bit 0: INCLUDE_PCI_ALL — this unit handles all PCI devices
    ///   not covered by other DRHD entries.
    flags: u8,
    /// Reserved.
    _reserved: u8,
    /// PCI segment number.
    segment: u16,
    /// Base address of IOMMU registers (4KiB page-aligned).
    register_base: u64,
}

// ---------------------------------------------------------------------------
// Detection state
// ---------------------------------------------------------------------------

/// Whether IOMMU hardware was detected.
static IOMMU_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// IOMMU vendor type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IommuVendor {
    /// No IOMMU detected.
    None = 0,
    /// Intel VT-d (DMAR table).
    IntelVtd = 1,
    /// AMD-Vi (IVRS table).
    AmdVi = 2,
}

/// Detected IOMMU vendor.
static IOMMU_VENDOR: AtomicU8 = AtomicU8::new(0);

/// Maximum number of IOMMU hardware units we track.
const MAX_IOMMU_UNITS: usize = 8;

/// Information about a single IOMMU hardware unit.
#[derive(Debug, Clone, Copy)]
pub struct IommuUnit {
    /// Register base physical address.
    pub register_base: u64,
    /// PCI segment number.
    pub segment: u16,
    /// Whether this unit covers all PCI devices (INCLUDE_PCI_ALL).
    pub include_all: bool,
    /// Whether the unit is active/populated.
    pub active: bool,
}

impl IommuUnit {
    const EMPTY: Self = Self {
        register_base: 0,
        segment: 0,
        include_all: false,
        active: false,
    };
}

/// Discovered IOMMU units.
static IOMMU_UNITS: spin::Mutex<[IommuUnit; MAX_IOMMU_UNITS]> =
    spin::Mutex::new([IommuUnit::EMPTY; MAX_IOMMU_UNITS]);

/// Number of active IOMMU units.
static IOMMU_UNIT_COUNT: AtomicU8 = AtomicU8::new(0);

/// Host address width from DMAR table.
static HOST_ADDRESS_WIDTH: AtomicU8 = AtomicU8::new(0);

/// Whether interrupt remapping is supported.
static INTERRUPT_REMAP: AtomicBool = AtomicBool::new(false);

/// DMAR device scope type of an I/O APIC (VT-d §8.3.1).
const SCOPE_IOAPIC: u8 = 3;
/// Bytes of a DRHD before its device scopes.
const DRHD_HEADER: usize = 16;
/// Bytes of a device scope before its path.
const SCOPE_HEADER: usize = 6;
/// Hops a scope's path may take; a longer one is not believed.
const MAX_HOPS: usize = 8;
/// I/O APIC scopes kept, from every DRHD.
const MAX_IOAPIC_SCOPES: usize = 8;

/// A DMAR device scope naming an I/O APIC: which one, and the PCI path to
/// the function whose requester id its interrupts carry -- what interrupt
/// remapping checks them against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoapicScope {
    /// The I/O APIC's id, as the MADT gives it (the scope's enumeration id).
    pub id: u8,
    /// The bus the path starts on.
    pub start_bus: u8,
    /// Each hop's device and function: the bridges in turn, then the I/O
    /// APIC's own function.
    path: [(u8, u8); MAX_HOPS],
    /// How many of `path` are hops: at least one.
    hops: u8,
}

impl IoapicScope {
    /// The path's hops, the I/O APIC's own function last.
    #[must_use]
    pub fn path(&self) -> &[(u8, u8)] {
        self.path.get(..usize::from(self.hops)).unwrap_or(&[])
    }

    /// The requester id (bus << 8 | device << 3 | function) the I/O APIC's
    /// interrupts carry. Each bridge on the path names the bus behind it,
    /// which `secondary(bus, device, function)` reads (config offset 0x19),
    /// as Linux's `ir_parse_one_ioapic_scope` does. `None` for an empty path.
    #[must_use]
    pub fn source_id(&self, secondary: impl Fn(u8, u8, u8) -> u8) -> Option<u16> {
        let (&(device, function), bridges) = self.path().split_last()?;
        let bus = bridges
            .iter()
            .fold(self.start_bus, |bus, &(d, f)| secondary(bus, d, f));
        Some(
            crate::pci::PciAddress {
                bus,
                device,
                function,
            }
            .requester_id(),
        )
    }
}

/// The I/O APIC scopes the DMAR's DRHDs list.
static IOAPIC_SCOPES: spin::Mutex<[Option<IoapicScope>; MAX_IOAPIC_SCOPES]> =
    spin::Mutex::new([None; MAX_IOAPIC_SCOPES]);

/// Give `found` every I/O APIC scope in `drhd`, one DRHD structure's bytes
/// (its header included). The walk ends at the first scope that is shorter
/// than its own header or runs past the structure; an I/O APIC scope whose
/// path is empty, odd-sized or longer than [`MAX_HOPS`] is passed over.
fn ioapic_scopes(drhd: &[u8], mut found: impl FnMut(IoapicScope)) {
    let mut at = DRHD_HEADER;
    while let Some(&[kind, len, _, _, id, start_bus]) =
        drhd.get(at..at.saturating_add(SCOPE_HEADER))
    {
        let len = usize::from(len);
        let Some(scope) = drhd.get(at..at.saturating_add(len)) else {
            break;
        };
        let Some(path_bytes) = scope.get(SCOPE_HEADER..) else {
            break;
        };
        let hops = path_bytes.len() / 2;
        if kind == SCOPE_IOAPIC && path_bytes.len() % 2 == 0 && (1..=MAX_HOPS).contains(&hops) {
            let mut path = [(0, 0); MAX_HOPS];
            for (slot, pair) in path.iter_mut().zip(path_bytes.chunks_exact(2)) {
                if let &[device, function] = pair {
                    *slot = (device, function);
                }
            }
            found(IoapicScope {
                id,
                start_bus,
                path,
                // At most MAX_HOPS (8).
                hops: u8::try_from(hops).unwrap_or(0),
            });
        }
        at = at.saturating_add(len);
    }
}

/// The DMAR's scope for the I/O APIC whose id is `id`, if it lists one.
#[must_use]
pub fn ioapic_scope(id: u8) -> Option<IoapicScope> {
    IOAPIC_SCOPES
        .lock()
        .iter()
        .flatten()
        .find(|s| s.id == id)
        .copied()
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Detect IOMMU hardware via ACPI tables.
///
/// Called during boot after ACPI init. Probes for Intel VT-d (DMAR table)
/// and AMD-Vi (IVRS table).
pub fn init() {
    serial_println!("[iommu] Probing for IOMMU hardware...");

    // Try Intel VT-d first (DMAR table).
    if detect_intel_vtd() {
        return;
    }

    // Try AMD-Vi (IVRS table).
    if detect_amd_vi() {
        return;
    }

    // No IOMMU found.
    serial_println!("[iommu] No IOMMU hardware detected");
    serial_println!("[iommu] NOTE: DMA sandboxing for drivers is unavailable");
    serial_println!("[iommu] TIP: Enable Intel VT-d or AMD-Vi in BIOS/UEFI settings");
}

/// Detect Intel VT-d via the DMAR ACPI table.
fn detect_intel_vtd() -> bool {
    let dmar_phys = match acpi::find_table(b"DMAR") {
        Some(addr) => addr,
        None => return false,
    };

    serial_println!("[iommu] Found DMAR table at {:#x}", dmar_phys);

    // Read the standard ACPI header to get table length.
    let hhdm = match mm::page_table::hhdm() {
        Some(h) => h,
        None => {
            serial_println!("[iommu] HHDM not initialized, cannot parse DMAR");
            return false;
        }
    };

    // SAFETY: ACPI table is in physical memory, mapped via HHDM.
    let table_len = unsafe {
        let header_ptr = (hhdm + dmar_phys) as *const u8;
        // Length field is at offset 4 (4-byte signature, then u32 length).
        let len_ptr = header_ptr.add(4) as *const u32;
        core::ptr::read_unaligned(len_ptr) as usize
    };

    if table_len < 36 + core::mem::size_of::<DmarHeader>() {
        serial_println!("[iommu] DMAR table too small ({} bytes)", table_len);
        return false;
    }

    // Parse DMAR-specific header (after 36-byte standard ACPI header).
    // SAFETY: Table mapped via HHDM, within bounds.
    let (host_width, flags) = unsafe {
        let dmar_hdr_ptr = (hhdm + dmar_phys + 36) as *const DmarHeader;
        let hdr = core::ptr::read_unaligned(dmar_hdr_ptr);
        (hdr.host_address_width, hdr.flags)
    };

    HOST_ADDRESS_WIDTH.store(host_width, Ordering::Relaxed);
    let intr_remap = (flags & 0x01) != 0;
    INTERRUPT_REMAP.store(intr_remap, Ordering::Relaxed);

    serial_println!(
        "[iommu] VT-d: host address width={}, intr_remap={}",
        host_width,
        intr_remap
    );

    // Walk remapping structures after the DMAR header.
    let struct_offset = 36 + core::mem::size_of::<DmarHeader>();
    let mut offset = struct_offset;
    let mut unit_count: u8 = 0;

    while offset + 4 <= table_len {
        // Each structure starts with (u16 type, u16 length).
        // SAFETY: Within table bounds, mapped via HHDM.
        let (struct_type, struct_len) = unsafe {
            let base = (hhdm + dmar_phys + offset as u64) as *const u8;
            let stype = core::ptr::read_unaligned(base as *const u16);
            let slen = core::ptr::read_unaligned(base.add(2) as *const u16);
            (stype, slen as usize)
        };

        if struct_len < 4 {
            break; // Malformed.
        }

        if struct_type == DmarStructType::Drhd as u16 {
            // Parse DRHD entry.
            if struct_len >= core::mem::size_of::<DrhdEntry>() && offset + struct_len <= table_len {
                // SAFETY: Within bounds, mapped via HHDM.
                // SAFETY: Within bounds, mapped via HHDM. Read individual
                // fields with read_unaligned because DrhdEntry is packed.
                let (reg_base, seg, flags_byte) = unsafe {
                    let base_ptr = (hhdm + dmar_phys + offset as u64) as *const u8;
                    let rb = core::ptr::read_unaligned(base_ptr.add(8) as *const u64);
                    let sg = core::ptr::read_unaligned(base_ptr.add(6) as *const u16);
                    let fl = core::ptr::read_unaligned(base_ptr.add(4));
                    (rb, sg, fl)
                };

                let include_all = (flags_byte & 0x01) != 0;

                serial_println!(
                    "[iommu] DRHD unit {}: base={:#x} seg={} include_all={}",
                    unit_count,
                    reg_base,
                    seg,
                    include_all
                );

                if (unit_count as usize) < MAX_IOMMU_UNITS {
                    let mut units = IOMMU_UNITS.lock();
                    units[unit_count as usize] = IommuUnit {
                        register_base: reg_base,
                        segment: seg,
                        include_all,
                        active: true,
                    };
                }
                unit_count = unit_count.saturating_add(1);

                // SAFETY: the DRHD's bytes, inside the table (checked above),
                // which the direct map maps and nothing writes.
                let drhd = unsafe {
                    core::slice::from_raw_parts(
                        hhdm.wrapping_add(dmar_phys).wrapping_add(offset as u64) as *const u8,
                        struct_len,
                    )
                };
                ioapic_scopes(drhd, |scope| {
                    let mut scopes = IOAPIC_SCOPES.lock();
                    if let Some(slot) = scopes.iter_mut().find(|s| s.is_none()) {
                        *slot = Some(scope);
                    }
                    serial_println!(
                        "[iommu]   its scope: I/O APIC {} at bus {:#x}, path {:x?}",
                        scope.id,
                        scope.start_bus,
                        scope.path()
                    );
                });
            }
        }

        offset = offset.saturating_add(struct_len);
    }

    if unit_count > 0 {
        IOMMU_AVAILABLE.store(true, Ordering::Release);
        IOMMU_VENDOR.store(IommuVendor::IntelVtd as u8, Ordering::Relaxed);
        IOMMU_UNIT_COUNT.store(unit_count, Ordering::Relaxed);
        serial_println!(
            "[iommu] Intel VT-d detected: {} hardware unit(s)",
            unit_count
        );
        true
    } else {
        serial_println!("[iommu] DMAR table found but no DRHD units");
        false
    }
}

/// Detect AMD-Vi via the IVRS ACPI table.
fn detect_amd_vi() -> bool {
    let ivrs_phys = match acpi::find_table(b"IVRS") {
        Some(addr) => addr,
        None => return false,
    };

    serial_println!("[iommu] Found IVRS table at {:#x}", ivrs_phys);

    // Basic detection: just confirm the table exists.
    // Full IVRS parsing (IVHDs, IVMDs) will be added when AMD IOMMU
    // page table support is implemented.

    IOMMU_AVAILABLE.store(true, Ordering::Release);
    IOMMU_VENDOR.store(IommuVendor::AmdVi as u8, Ordering::Relaxed);
    IOMMU_UNIT_COUNT.store(1, Ordering::Relaxed); // At least one.

    serial_println!("[iommu] AMD-Vi detected (basic, detailed parsing deferred)");
    true
}

// ---------------------------------------------------------------------------
// Public query API
// ---------------------------------------------------------------------------

/// Whether any IOMMU hardware is available.
#[must_use]
pub fn is_available() -> bool {
    IOMMU_AVAILABLE.load(Ordering::Acquire)
}

/// Get the detected IOMMU vendor.
#[must_use]
pub fn vendor() -> IommuVendor {
    match IOMMU_VENDOR.load(Ordering::Relaxed) {
        1 => IommuVendor::IntelVtd,
        2 => IommuVendor::AmdVi,
        _ => IommuVendor::None,
    }
}

/// Number of detected IOMMU hardware units.
#[must_use]
pub fn unit_count() -> u8 {
    IOMMU_UNIT_COUNT.load(Ordering::Relaxed)
}

/// Get information about a specific IOMMU unit.
#[must_use]
pub fn get_unit(index: usize) -> Option<IommuUnit> {
    if index >= MAX_IOMMU_UNITS {
        return None;
    }
    let units = IOMMU_UNITS.lock();
    if units[index].active {
        Some(units[index])
    } else {
        None
    }
}

/// Get the host address width (physical address bits for DMA).
///
/// Only meaningful for Intel VT-d. Returns 0 if not available.
#[must_use]
pub fn host_address_width() -> u8 {
    HOST_ADDRESS_WIDTH.load(Ordering::Relaxed)
}

/// Whether interrupt remapping is supported.
#[must_use]
pub fn interrupt_remapping_supported() -> bool {
    INTERRUPT_REMAP.load(Ordering::Relaxed)
}

/// Get a human-readable status summary.
pub fn status_summary() -> alloc::string::String {
    use alloc::format;

    if !is_available() {
        return alloc::string::String::from("IOMMU: not detected (DMA sandboxing unavailable)");
    }

    let vendor_str = match vendor() {
        IommuVendor::IntelVtd => "Intel VT-d",
        IommuVendor::AmdVi => "AMD-Vi",
        IommuVendor::None => "Unknown",
    };

    let units = unit_count();
    let haw = host_address_width();
    let ir = if interrupt_remapping_supported() {
        "yes"
    } else {
        "no"
    };

    format!(
        "IOMMU: {} ({} unit(s), {}bit DMA, intr_remap={})",
        vendor_str, units, haw, ir
    )
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run IOMMU detection self-test.
///
/// Doesn't assert specific hardware — just verifies the detection API
/// works correctly and the results are self-consistent.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[iommu] Running self-test...");

    // Test 1: API consistency.
    let available = is_available();
    let v = vendor();
    let units = unit_count();

    if available {
        // If available, vendor should be set and units > 0.
        if v == IommuVendor::None {
            serial_println!("[iommu]   FAIL: available but vendor=None");
            return Err(KernelError::InternalError);
        }
        if units == 0 {
            serial_println!("[iommu]   FAIL: available but units=0");
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[iommu]   Detection: {} with {} unit(s)",
            match v {
                IommuVendor::IntelVtd => "Intel VT-d",
                IommuVendor::AmdVi => "AMD-Vi",
                IommuVendor::None => "None",
            },
            units
        );
    } else {
        // If not available, vendor should be None and units = 0.
        if v != IommuVendor::None {
            serial_println!("[iommu]   FAIL: not available but vendor={:?}", v);
            return Err(KernelError::InternalError);
        }
        serial_println!("[iommu]   Detection: no IOMMU hardware");
    }
    serial_println!("[iommu]   API consistency: OK");

    // Test 2: Status summary doesn't panic.
    let summary = status_summary();
    if summary.is_empty() {
        serial_println!("[iommu]   FAIL: empty status summary");
        return Err(KernelError::InternalError);
    }
    serial_println!("[iommu]   Status: {}", summary);
    serial_println!("[iommu]   Summary: OK");

    // Test 3: Unit query (out-of-bounds returns None).
    if get_unit(MAX_IOMMU_UNITS).is_some() {
        serial_println!("[iommu]   FAIL: out-of-bounds unit returned Some");
        return Err(KernelError::InternalError);
    }
    serial_println!("[iommu]   Bounds check: OK");

    // Test 4: a DRHD's I/O APIC scopes, read from bytes laid out as VT-d
    // §8.3.1 lays them out -- an endpoint scope passed over, an I/O APIC
    // directly on its bus, one behind a bridge, one with an odd-sized path,
    // and one running past the structure, which ends the walk.
    let drhd = [0u8; DRHD_HEADER];
    let mut bytes = alloc::vec::Vec::from(drhd);
    bytes.extend_from_slice(&[1, 8, 0, 0, 0, 0, 3, 0]); // Endpoint 00:03.0.
    bytes.extend_from_slice(&[3, 8, 0, 0, 2, 0xF0, 0x1F, 0]); // I/O APIC 2 at F0:1F.0.
    bytes.extend_from_slice(&[3, 10, 0, 0, 5, 0, 1, 0, 0, 3]); // I/O APIC 5 behind 00:01.0.
    bytes.extend_from_slice(&[3, 9, 0, 0, 6, 0, 1, 0, 7]); // Odd path: passed over.
    bytes.extend_from_slice(&[3, 40, 0, 0, 7, 0, 1, 0]); // Runs past the end.
    let mut found = alloc::vec::Vec::new();
    ioapic_scopes(&bytes, |s| found.push(s));
    // A bridge's secondary bus: 9 behind 00:01.0, nothing else asked.
    let secondary = |bus, device, function| {
        if (bus, device, function) == (0, 1, 0) {
            9
        } else {
            0xEE
        }
    };
    let read: alloc::vec::Vec<_> = found
        .iter()
        .map(|s| (s.id, s.start_bus, s.path().to_vec(), s.source_id(secondary)))
        .collect();
    let expected = [
        (2, 0xF0, alloc::vec![(0x1F, 0)], Some(0xF0F8)),
        (5, 0, alloc::vec![(1, 0), (0, 3)], Some(0x0903)),
    ];
    if read != expected {
        serial_println!("[iommu]   FAIL: I/O APIC scopes read as {:x?}", read);
        return Err(KernelError::InternalError);
    }
    // A scope of length 0 cannot loop the walk.
    let mut zero = alloc::vec::Vec::from(drhd);
    zero.extend_from_slice(&[3, 0, 0, 0, 2, 0, 0x1F, 0]);
    let mut none = true;
    ioapic_scopes(&zero, |_| none = false);
    if !none {
        serial_println!("[iommu]   FAIL: a scope of length 0 was read");
        return Err(KernelError::InternalError);
    }
    // An I/O APIC no scope names is found in none: an id none of the
    // scopes read at boot carries.
    let listed: alloc::vec::Vec<u8> = IOAPIC_SCOPES
        .lock()
        .iter()
        .flatten()
        .map(|s| s.id)
        .collect();
    let absent = (0..=u8::MAX).find(|id| !listed.contains(id));
    if absent.is_none_or(|id| ioapic_scope(id).is_some())
        || listed
            .iter()
            .any(|&id| ioapic_scope(id).is_none_or(|s| s.id != id))
    {
        serial_println!(
            "[iommu]   FAIL: an I/O APIC's scope was found for the wrong id, or not found"
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[iommu]   DMAR I/O APIC scopes: OK");

    serial_println!("[iommu] Self-test PASSED");
    Ok(())
}

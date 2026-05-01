//! MADT (Multiple APIC Description Table) parsing.
//!
//! The MADT describes the interrupt controller topology: which processors
//! have Local APICs, where the I/O APICs are, and any interrupt source
//! overrides (ISA IRQ remapping required by ACPI).
//!
//! ## Entry Types We Parse
//!
//! | Type | Name                           | Use |
//! |------|--------------------------------|-----|
//! |  0   | Processor Local APIC           | CPU discovery for SMP |
//! |  1   | I/O APIC                       | IOAPIC base address(es) |
//! |  2   | Interrupt Source Override       | ISA IRQ→GSI remapping |
//! |  4   | Local APIC NMI                 | NMI routing |
//! |  5   | Local APIC Address Override     | 64-bit LAPIC base |
//!
//! ## References
//!
//! - ACPI Specification 6.5, Section 5.2.12
//! - <https://wiki.osdev.org/MADT>

use alloc::vec::Vec;
use crate::serial_println;

// ---------------------------------------------------------------------------
// MADT header (follows the standard SDT header)
// ---------------------------------------------------------------------------

/// MADT-specific header fields (after the SDT header).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtFields {
    /// Physical address of the Local APIC (default, may be overridden
    /// by a type-5 entry).
    pub local_apic_address: u32,
    /// Flags.  Bit 0: PCAT_COMPAT — if set, dual-8259 PICs are present
    /// and must be disabled before APIC use.
    pub flags: u32,
}

// ---------------------------------------------------------------------------
// MADT entry types
// ---------------------------------------------------------------------------

/// Common header for all MADT interrupt controller structure entries.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct MadtEntryHeader {
    /// Entry type (0–5 are standard, others are reserved/OEM).
    entry_type: u8,
    /// Total length of this entry including the header.
    length: u8,
}

/// MADT entry type constants.
const MADT_LOCAL_APIC: u8 = 0;
const MADT_IO_APIC: u8 = 1;
const MADT_INTERRUPT_OVERRIDE: u8 = 2;
const MADT_LOCAL_APIC_NMI: u8 = 4;
const MADT_LOCAL_APIC_ADDR_OVERRIDE: u8 = 5;

// ---------------------------------------------------------------------------
// Parsed MADT data structures (heap-allocated, stored in global state)
// ---------------------------------------------------------------------------

/// Information about a processor's Local APIC (MADT type 0).
#[derive(Debug, Clone, Copy)]
pub struct ProcessorInfo {
    /// ACPI processor UID (unique per processor).
    pub acpi_processor_id: u8,
    /// Local APIC ID for this processor.
    pub apic_id: u8,
    /// Whether this processor is enabled (can execute code).
    pub enabled: bool,
    /// Whether this processor can be brought online at runtime
    /// (ACPI 6.3+, for hot-plug CPUs).
    pub online_capable: bool,
}

/// Information about an I/O APIC (MADT type 1).
#[derive(Debug, Clone, Copy)]
pub struct IoApicInfo {
    /// I/O APIC hardware ID.
    pub id: u8,
    /// Physical base address of the I/O APIC MMIO registers.
    pub address: u32,
    /// Global System Interrupt (GSI) base for this I/O APIC.
    ///
    /// IOAPIC input pin N corresponds to GSI `gsi_base + N`.
    /// On single-IOAPIC systems this is typically 0.
    pub gsi_base: u32,
}

/// ISA interrupt source override (MADT type 2).
///
/// The ACPI standard defines that ISA IRQs 0–15 identity-map to
/// GSI 0–15 by default.  Source overrides remap specific ISA IRQs
/// to different GSI numbers and/or change their trigger/polarity.
///
/// Common example: ISA IRQ 0 (PIT timer) → GSI 2 on many systems.
#[derive(Debug, Clone, Copy)]
pub struct InterruptOverride {
    /// Bus source (always 0 = ISA in practice).
    pub bus: u8,
    /// IRQ source (the ISA IRQ number being overridden).
    pub source_irq: u8,
    /// Global System Interrupt this source maps to.
    pub gsi: u32,
    /// Flags (bits 1:0 = polarity, bits 3:2 = trigger mode).
    ///
    /// Polarity: 00 = bus default, 01 = active high, 10 = reserved,
    ///           11 = active low.
    /// Trigger:  00 = bus default, 01 = edge, 10 = reserved,
    ///           11 = level.
    pub flags: u16,
}

impl InterruptOverride {
    /// Returns true if the override specifies active-low polarity.
    pub fn is_active_low(&self) -> bool {
        (self.flags & 0x3) == 3
    }

    /// Returns true if the override specifies level-triggered mode.
    pub fn is_level_triggered(&self) -> bool {
        ((self.flags >> 2) & 0x3) == 3
    }
}

/// Local APIC NMI routing (MADT type 4).
#[derive(Debug, Clone, Copy)]
pub struct LocalApicNmi {
    /// ACPI processor ID (0xFF = all processors).
    pub acpi_processor_id: u8,
    /// Flags (same encoding as interrupt override).
    pub flags: u16,
    /// LINT pin (0 or 1) that receives the NMI.
    pub lint: u8,
}

// ---------------------------------------------------------------------------
// Parsed MADT result
// ---------------------------------------------------------------------------

/// Fully parsed MADT contents.
pub struct MadtInfo {
    /// Default Local APIC physical address (may be overridden).
    pub local_apic_address: u64,
    /// Whether the dual-8259 PIC is present (PCAT_COMPAT flag).
    pub pcat_compat: bool,
    /// Discovered processors (Local APIC entries).
    pub processors: Vec<ProcessorInfo>,
    /// I/O APICs.
    pub io_apics: Vec<IoApicInfo>,
    /// Interrupt source overrides (ISA IRQ remapping).
    pub interrupt_overrides: Vec<InterruptOverride>,
    /// Local APIC NMI routing.
    pub local_apic_nmis: Vec<LocalApicNmi>,
}

// ---------------------------------------------------------------------------
// MADT parsing
// ---------------------------------------------------------------------------

/// Parse the MADT at the given virtual address.
///
/// # Safety
///
/// `madt_virt` must point to a valid, mapped MADT (signature "APIC",
/// checksum validated).
pub unsafe fn parse_madt(madt_virt: u64) -> MadtInfo {
    use super::tables::SdtHeader;

    let header = madt_virt as *const SdtHeader;
    // SAFETY: madt_virt is valid.
    let total_len = unsafe { (*header).length } as usize;

    // MADT-specific fields start after the SDT header.
    let fields_offset = SdtHeader::SIZE;
    let fields_ptr = madt_virt.wrapping_add(fields_offset as u64) as *const MadtFields;
    // SAFETY: MADT is at least SDT header + MadtFields large.
    let fields = unsafe { core::ptr::read_unaligned(fields_ptr) };

    let mut local_apic_address = u64::from(fields.local_apic_address);
    let madt_flags = fields.flags;
    let pcat_compat = (madt_flags & 1) != 0;

    serial_println!(
        "[acpi] MADT: LAPIC addr={:#x}, flags={:#x} (PCAT_COMPAT={})",
        local_apic_address,
        madt_flags,
        pcat_compat
    );

    // Parse variable-length entries.
    let mut processors = Vec::new();
    let mut io_apics = Vec::new();
    let mut interrupt_overrides = Vec::new();
    let mut local_apic_nmis = Vec::new();

    #[allow(clippy::arithmetic_side_effects)]
    let entries_start = fields_offset + core::mem::size_of::<MadtFields>();
    let mut offset = entries_start;

    while offset + 2 <= total_len {
        let entry_ptr = madt_virt.wrapping_add(offset as u64) as *const MadtEntryHeader;
        // SAFETY: within MADT bounds.
        let entry_header = unsafe { core::ptr::read_unaligned(entry_ptr) };
        let entry_len = entry_header.length as usize;

        // Prevent infinite loop on malformed entries.
        if entry_len < 2 {
            serial_println!("[acpi] MADT: entry at offset {} has length {}, stopping", offset, entry_len);
            break;
        }

        let entry_data = madt_virt.wrapping_add(offset as u64);

        match entry_header.entry_type {
            MADT_LOCAL_APIC => {
                if entry_len >= 8 {
                    // SAFETY: entry is at least 8 bytes.
                    let data = entry_data as *const u8;
                    let acpi_processor_id = unsafe { *data.add(2) };
                    let apic_id = unsafe { *data.add(3) };
                    let flags = unsafe { core::ptr::read_unaligned(data.add(4) as *const u32) };
                    let enabled = (flags & 1) != 0;
                    let online_capable = (flags & 2) != 0;

                    processors.push(ProcessorInfo {
                        acpi_processor_id,
                        apic_id,
                        enabled,
                        online_capable,
                    });
                }
            }

            MADT_IO_APIC => {
                if entry_len >= 12 {
                    // SAFETY: entry is at least 12 bytes.
                    let data = entry_data as *const u8;
                    let id = unsafe { *data.add(2) };
                    // Byte 3 is reserved.
                    let address = unsafe {
                        core::ptr::read_unaligned(data.add(4) as *const u32)
                    };
                    let gsi_base = unsafe {
                        core::ptr::read_unaligned(data.add(8) as *const u32)
                    };

                    serial_println!(
                        "[acpi] MADT:   I/O APIC id={}, addr={:#x}, gsi_base={}",
                        id,
                        address,
                        gsi_base
                    );

                    io_apics.push(IoApicInfo {
                        id,
                        address,
                        gsi_base,
                    });
                }
            }

            MADT_INTERRUPT_OVERRIDE => {
                if entry_len >= 10 {
                    // SAFETY: entry is at least 10 bytes.
                    let data = entry_data as *const u8;
                    let bus = unsafe { *data.add(2) };
                    let source_irq = unsafe { *data.add(3) };
                    let gsi = unsafe {
                        core::ptr::read_unaligned(data.add(4) as *const u32)
                    };
                    let flags = unsafe {
                        core::ptr::read_unaligned(data.add(8) as *const u16)
                    };

                    serial_println!(
                        "[acpi] MADT:   IRQ override: ISA {} → GSI {} (flags={:#x})",
                        source_irq,
                        gsi,
                        flags
                    );

                    interrupt_overrides.push(InterruptOverride {
                        bus,
                        source_irq,
                        gsi,
                        flags,
                    });
                }
            }

            MADT_LOCAL_APIC_NMI => {
                if entry_len >= 6 {
                    // SAFETY: entry is at least 6 bytes.
                    let data = entry_data as *const u8;
                    let acpi_processor_id = unsafe { *data.add(2) };
                    let flags = unsafe {
                        core::ptr::read_unaligned(data.add(3) as *const u16)
                    };
                    let lint = unsafe { *data.add(5) };

                    local_apic_nmis.push(LocalApicNmi {
                        acpi_processor_id,
                        flags,
                        lint,
                    });
                }
            }

            MADT_LOCAL_APIC_ADDR_OVERRIDE => {
                if entry_len >= 12 {
                    // SAFETY: entry is at least 12 bytes.
                    let data = entry_data as *const u8;
                    // Bytes 2-3 are reserved.
                    let addr64 = unsafe {
                        core::ptr::read_unaligned(data.add(4) as *const u64)
                    };
                    serial_println!(
                        "[acpi] MADT:   LAPIC address override: {:#x}",
                        addr64
                    );
                    local_apic_address = addr64;
                }
            }

            other => {
                serial_println!(
                    "[acpi] MADT:   Unknown entry type {} (len={}), skipping",
                    other,
                    entry_len
                );
            }
        }

        // Advance to the next entry.
        offset = offset.wrapping_add(entry_len);
    }

    serial_println!(
        "[acpi] MADT summary: {} processor(s), {} I/O APIC(s), {} override(s), {} NMI(s)",
        processors.len(),
        io_apics.len(),
        interrupt_overrides.len(),
        local_apic_nmis.len(),
    );

    MadtInfo {
        local_apic_address,
        pcat_compat,
        processors,
        io_apics,
        interrupt_overrides,
        local_apic_nmis,
    }
}

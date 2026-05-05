//! FADT (Fixed ACPI Description Table) parsing.
//!
//! The FADT provides information about fixed hardware features including:
//! - Power management control register addresses (PM1a/PM1b)
//! - Reset mechanism (ACPI 2.0+)
//! - System Control Interrupt (SCI) IRQ number
//! - PM timer block address and width
//!
//! ## Shutdown Mechanism
//!
//! ACPI shutdown (S5 sleep state) requires writing `(SLP_TYP | SLP_EN)`
//! to the PM1a_CNT register.  The SLP_TYP value is normally found in the
//! DSDT via the \_S5 object, but parsing AML bytecode is complex.  We use
//! a simplified approach: scan the DSDT raw bytes for the \_S5_ package
//! and extract the sleep type value directly.
//!
//! ## Reboot Mechanism
//!
//! ACPI 2.0+ provides a RESET_REG in the FADT.  Fallback methods:
//! - Keyboard controller reset (pulse 0xFE to port 0x64)
//! - Triple fault (load null IDT + `int3`)
//!
//! ## References
//!
//! - ACPI Specification 6.5, Section 5.2.9 (FADT)
//! - ACPI Specification 6.5, Section 4.8.3.4 (Sleeping States)
//! - Linux `drivers/acpi/sleep.c`, `arch/x86/kernel/reboot.c`
//! - OSDev Wiki: ACPI Shutdown, ACPI Reset

#![allow(dead_code)]

use crate::serial_println;

// ---------------------------------------------------------------------------
// FADT structure (Fixed ACPI Description Table)
// ---------------------------------------------------------------------------

/// Fixed ACPI Description Table — ACPI 1.0 fields (revision 1).
///
/// Only the fields we actually need are typed; the rest are reserved
/// or unused space that we skip via the packed repr.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Fadt {
    /// Standard SDT header (signature = "FACP").
    pub header: super::tables::SdtHeader,
    /// Physical address of the FACS (Firmware ACPI Control Structure).
    pub firmware_ctrl: u32,
    /// Physical address of the DSDT.
    pub dsdt: u32,
    /// Reserved in ACPI 2.0+ (was INT_MODEL in ACPI 1.0).
    pub _reserved1: u8,
    /// Preferred Power Management Profile.
    pub preferred_pm_profile: u8,
    /// System Control Interrupt (SCI) IRQ number.
    pub sci_interrupt: u16,
    /// Port address of the SMI Command port.
    pub smi_command: u32,
    /// Value to write to SMI_CMD to enable ACPI.
    pub acpi_enable: u8,
    /// Value to write to SMI_CMD to disable ACPI.
    pub acpi_disable: u8,
    /// Value for S4BIOS state control.
    pub s4bios_req: u8,
    /// Value for processor performance state control.
    pub pstate_control: u8,
    /// Port address of PM1a Event block.
    pub pm1a_evt_blk: u32,
    /// Port address of PM1b Event block (0 if not supported).
    pub pm1b_evt_blk: u32,
    /// Port address of PM1a Control block.
    pub pm1a_cnt_blk: u32,
    /// Port address of PM1b Control block (0 if not supported).
    pub pm1b_cnt_blk: u32,
    /// Port address of PM2 Control block.
    pub pm2_cnt_blk: u32,
    /// Port address of PM Timer block.
    pub pm_tmr_blk: u32,
    /// Port address of GPE0 block.
    pub gpe0_blk: u32,
    /// Port address of GPE1 block.
    pub gpe1_blk: u32,
    /// Length of PM1 Event registers (in bytes).
    pub pm1_evt_len: u8,
    /// Length of PM1 Control registers (in bytes).
    pub pm1_cnt_len: u8,
    /// Length of PM2 Control register (in bytes).
    pub pm2_cnt_len: u8,
    /// Length of PM Timer register (in bytes).
    pub pm_tmr_len: u8,
    /// Length of GPE0 block (in bytes).
    pub gpe0_blk_len: u8,
    /// Length of GPE1 block (in bytes).
    pub gpe1_blk_len: u8,
    /// Offset within GPE1 block where GPE1 events start.
    pub gpe1_base: u8,
    /// Value for processor C-state change notification.
    pub cst_cnt: u8,
    /// Worst-case latency to enter C2 state (microseconds).
    pub p_lvl2_lat: u16,
    /// Worst-case latency to enter C3 state (microseconds).
    pub p_lvl3_lat: u16,
    /// Cache flush size (bytes) for WBINVD flush.
    pub flush_size: u16,
    /// Cache flush stride (bytes).
    pub flush_stride: u16,
    /// Processor duty cycle offset.
    pub duty_offset: u8,
    /// Processor duty cycle width.
    pub duty_width: u8,
    /// RTC day-of-month alarm.
    pub day_alarm: u8,
    /// RTC month alarm.
    pub month_alarm: u8,
    /// RTC century BCD value.
    pub century: u8,
    /// IA-PC Boot Architecture Flags (ACPI 2.0+).
    pub iapc_boot_arch: u16,
    /// Reserved.
    pub _reserved2: u8,
    /// Fixed feature flags.
    pub flags: u32,
    // ACPI 2.0+ fields follow (GAS reset register, etc.)
    // We read these separately based on the table length.
}

/// ACPI Generic Address Structure (GAS) — describes a register.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct GenericAddress {
    /// Address space (0 = system memory, 1 = system I/O, 2 = PCI config).
    pub address_space: u8,
    /// Register bit width.
    pub bit_width: u8,
    /// Register bit offset.
    pub bit_offset: u8,
    /// Access size (1=byte, 2=word, 3=dword, 4=qword).
    pub access_size: u8,
    /// Register address.
    pub address: u64,
}

/// Offset of the RESET_REG field in FADT (ACPI 2.0+).
/// After the flags field (offset 112), there's the RESET_REG GAS at offset 116.
const FADT_RESET_REG_OFFSET: usize = 116;
/// Offset of the RESET_VALUE field.
const FADT_RESET_VALUE_OFFSET: usize = 128;
/// Offset of X_DSDT (64-bit DSDT address) in FADT revision 3+.
const FADT_X_DSDT_OFFSET: usize = 140;

// ---------------------------------------------------------------------------
// Parsed power management info
// ---------------------------------------------------------------------------

/// Parsed power management information from the FADT.
#[derive(Debug, Clone, Copy)]
pub struct PowerInfo {
    /// PM1a Control Block port address.
    pub pm1a_cnt_blk: u16,
    /// PM1b Control Block port address (0 if not present).
    pub pm1b_cnt_blk: u16,
    /// SLP_TYP value for S5 (shutdown) state.
    /// Extracted from DSDT \_S5 object, or default (5) if not found.
    pub slp_typ_s5: u8,
    /// Whether we have a valid ACPI reset register.
    pub has_reset_reg: bool,
    /// Reset register address space (1 = I/O).
    pub reset_addr_space: u8,
    /// Reset register address.
    pub reset_address: u64,
    /// Value to write to reset register.
    pub reset_value: u8,
    /// Physical address of the DSDT.
    pub dsdt_phys: u64,
    /// SCI interrupt number.
    pub sci_irq: u16,
}

// ---------------------------------------------------------------------------
// FADT parsing
// ---------------------------------------------------------------------------

/// Parse the FADT at the given virtual address.
///
/// Returns the extracted power management information.
///
/// # Safety
///
/// `fadt_virt` must point to a valid, mapped FADT of at least
/// `(*header).length` bytes.
pub unsafe fn parse_fadt(fadt_virt: u64) -> PowerInfo {
    let fadt = fadt_virt as *const Fadt;

    // SAFETY: caller guarantees the FADT is valid and mapped.
    let pm1a = unsafe { (*fadt).pm1a_cnt_blk } as u16;
    let pm1b = unsafe { (*fadt).pm1b_cnt_blk } as u16;
    let sci_irq = unsafe { (*fadt).sci_interrupt };
    let dsdt_32 = unsafe { (*fadt).dsdt };
    let flags = unsafe { (*fadt).flags };
    let header = unsafe { (*fadt).header };
    let total_len = header.length as usize;

    serial_println!("[acpi]   PM1a_CNT: {:#x}, PM1b_CNT: {:#x}, SCI: IRQ {}",
        pm1a, pm1b, sci_irq);
    serial_println!("[acpi]   Flags: {:#010x} (RESET_REG_SUP={})",
        flags, (flags >> 10) & 1);

    // Get 64-bit DSDT address (ACPI 2.0+) or fall back to 32-bit.
    let dsdt_phys = if total_len > FADT_X_DSDT_OFFSET + 8 {
        let x_dsdt_ptr = (fadt_virt as usize + FADT_X_DSDT_OFFSET) as *const u64;
        // SAFETY: within bounds of the FADT.
        let x_dsdt = unsafe { core::ptr::read_unaligned(x_dsdt_ptr) };
        if x_dsdt != 0 { x_dsdt } else { u64::from(dsdt_32) }
    } else {
        u64::from(dsdt_32)
    };

    // Parse reset register (ACPI 2.0+, FADT revision ≥ 3).
    let (has_reset, reset_space, reset_addr, reset_val) =
        if total_len > FADT_RESET_VALUE_OFFSET && (flags & (1 << 10)) != 0 {
            let gas_ptr = (fadt_virt as usize + FADT_RESET_REG_OFFSET) as *const GenericAddress;
            // SAFETY: within bounds, RESET_REG_SUP flag is set.
            let gas = unsafe { core::ptr::read_unaligned(gas_ptr) };
            let val_ptr = (fadt_virt as usize + FADT_RESET_VALUE_OFFSET) as *const u8;
            // SAFETY: within bounds.
            let val = unsafe { core::ptr::read_unaligned(val_ptr) };

            // Copy fields before logging (packed struct fields can't be referenced).
            let gas_space = gas.address_space;
            let gas_addr = gas.address;
            serial_println!("[acpi]   Reset register: space={}, addr={:#x}, val={:#x}",
                gas_space, gas_addr, val);

            (true, gas_space, gas_addr, val)
        } else {
            (false, 0, 0, 0)
        };

    serial_println!("[acpi]   DSDT at phys={:#x}", dsdt_phys);

    PowerInfo {
        pm1a_cnt_blk: pm1a,
        pm1b_cnt_blk: pm1b,
        slp_typ_s5: 5, // Default; updated after DSDT scan.
        has_reset_reg: has_reset,
        reset_addr_space: reset_space,
        reset_address: reset_addr,
        reset_value: reset_val,
        dsdt_phys,
        sci_irq,
    }
}

/// Scan the DSDT for the \_S5_ sleep type value.
///
/// The \_S5_ object in AML is typically a Package containing the SLP_TYP
/// values for PM1a and PM1b.  We search for the byte pattern that encodes
/// this object and extract the first byte value.
///
/// ## AML encoding of \_S5_
///
/// ```text
/// 08 5F 53 35 5F 12 ...  (NameOp "_S5_" PackageOp ...)
/// ```
///
/// After the PackageOp (0x12) and package length, the first element
/// is typically a ByteConst (0x0A followed by the value) or a ZeroOp (0x00)
/// or a OneOp (0x01).
///
/// # Safety
///
/// `dsdt_virt` must point to a valid, mapped DSDT of at least the length
/// specified in its SDT header.
pub unsafe fn scan_dsdt_for_s5(dsdt_virt: u64) -> Option<u8> {
    let header = dsdt_virt as *const super::tables::SdtHeader;
    // SAFETY: caller guarantees DSDT is valid.
    let total_len = unsafe { (*header).length } as usize;

    if total_len < super::tables::SdtHeader::SIZE + 10 {
        return None;
    }

    // We scan for the byte sequence: 08 5F 53 35 5F (NameOp "_S5_")
    // followed by 12 (PackageOp).
    let s5_signature: [u8; 5] = [0x08, b'_', b'S', b'5', b'_'];

    let data_start = dsdt_virt as usize + super::tables::SdtHeader::SIZE;
    let data_len = total_len.saturating_sub(super::tables::SdtHeader::SIZE);

    if data_len < s5_signature.len() + 5 {
        return None;
    }

    let data = core::slice::from_raw_parts(data_start as *const u8, data_len);

    // Search for the \_S5_ name.
    for i in 0..data_len.saturating_sub(s5_signature.len() + 5) {
        if data.get(i..i + 5) == Some(&s5_signature) {
            // Found \_S5_ name.  Next byte should be PackageOp (0x12).
            let pkg_offset = i + 5;
            if data.get(pkg_offset).copied() != Some(0x12) {
                continue;
            }

            // Skip PackageOp and package length encoding.
            // Package length is variable-length (1-4 bytes).
            let len_byte = data.get(pkg_offset + 1).copied().unwrap_or(0);
            let (pkg_data_offset, _pkg_len) = decode_pkg_length(len_byte, &data[pkg_offset + 1..]);

            // After package length comes NumElements (1 byte), then the
            // first element which is the SLP_TYPa value.
            let first_elem_offset = pkg_offset + 1 + pkg_data_offset + 1;

            if let Some(&elem_byte) = data.get(first_elem_offset) {
                let slp_typ = match elem_byte {
                    0x00 => 0, // ZeroOp
                    0x01 => 1, // OneOp
                    0x0A => {
                        // ByteConst — next byte is the value.
                        data.get(first_elem_offset + 1).copied().unwrap_or(5)
                    }
                    0x0B => {
                        // WordConst — next two bytes (little-endian), take low byte.
                        data.get(first_elem_offset + 1).copied().unwrap_or(5)
                    }
                    v if v <= 0x0F => v, // Small integer constants in some AML variants.
                    _ => continue, // Not a recognized encoding; try next match.
                };

                serial_println!("[acpi]   DSDT: found \\_S5_ SLP_TYP = {}", slp_typ);
                return Some(slp_typ);
            }
        }
    }

    None
}

/// Decode AML package length encoding.
///
/// Returns (number of bytes consumed for the length encoding, decoded length).
fn decode_pkg_length(first_byte: u8, data: &[u8]) -> (usize, usize) {
    let follow_bytes = (first_byte >> 6) as usize;
    if follow_bytes == 0 {
        // Single-byte encoding: bits 0-5 are the length.
        (1, (first_byte & 0x3F) as usize)
    } else {
        // Multi-byte: bits 0-3 of first byte are low nibble.
        let mut len = (first_byte & 0x0F) as usize;
        for i in 0..follow_bytes {
            let b = data.get(1 + i).copied().unwrap_or(0) as usize;
            len |= b << (4 + i * 8);
        }
        (1 + follow_bytes, len)
    }
}

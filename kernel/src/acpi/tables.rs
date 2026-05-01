//! ACPI table structure definitions and parsing helpers.
//!
//! This module provides the low-level structures for navigating the ACPI
//! table hierarchy: RSDP → RSDT/XSDT → individual description tables.
//!
//! ## References
//!
//! - ACPI Specification 6.5, Section 5.2 (ACPI System Description Tables)
//! - <https://wiki.osdev.org/RSDP>

use crate::serial_println;

// ---------------------------------------------------------------------------
// RSDP — Root System Description Pointer
// ---------------------------------------------------------------------------

/// ACPI 1.0 RSDP (20 bytes).
///
/// Located by the bootloader (Limine provides the address directly).
/// Contains a pointer to the RSDT.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Rsdp {
    /// "RSD PTR " (8 bytes, space-padded).
    pub signature: [u8; 8],
    /// Checksum — all bytes of this struct must sum to zero.
    pub checksum: u8,
    /// OEM identification string.
    pub oem_id: [u8; 6],
    /// 0 = ACPI 1.0 (RSDT only), 2 = ACPI 2.0+ (XSDT available).
    pub revision: u8,
    /// Physical address of the RSDT (32-bit).
    pub rsdt_address: u32,
}

/// ACPI 2.0+ extended RSDP (36 bytes).
///
/// Extends the base RSDP with 64-bit XSDT address and additional
/// checksum.  XSDT should be preferred over RSDT when available.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Rsdp2 {
    /// ACPI 1.0 header (first 20 bytes).
    pub base: Rsdp,
    /// Length of the full RSDP structure (should be 36).
    pub length: u32,
    /// Physical address of the XSDT (64-bit).
    pub xsdt_address: u64,
    /// Checksum of the entire extended structure.
    pub extended_checksum: u8,
    /// Reserved, must be zero.
    pub reserved: [u8; 3],
}

// ---------------------------------------------------------------------------
// SDT Header — common header for all system description tables
// ---------------------------------------------------------------------------

/// Standard ACPI System Description Table header (36 bytes).
///
/// Every ACPI table (RSDT, XSDT, MADT, FADT, etc.) starts with this
/// header.  The `signature` field identifies the table type.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SdtHeader {
    /// 4-byte ASCII signature identifying the table.
    pub signature: [u8; 4],
    /// Total length of the table including this header.
    pub length: u32,
    /// Table revision.
    pub revision: u8,
    /// Checksum — all bytes of the table must sum to zero.
    pub checksum: u8,
    /// OEM identification.
    pub oem_id: [u8; 6],
    /// OEM-specific table identifier.
    pub oem_table_id: [u8; 8],
    /// OEM-specific revision.
    pub oem_revision: u32,
    /// ID of the utility that created the table.
    pub creator_id: u32,
    /// Revision of the creating utility.
    pub creator_revision: u32,
}

impl SdtHeader {
    /// Size of the SDT header in bytes.
    pub const SIZE: usize = core::mem::size_of::<Self>();

    /// Get the 4-byte signature as a string slice (for logging).
    #[allow(dead_code)] // Useful for diagnostic logging of unknown tables.
    pub fn signature_str(&self) -> &str {
        core::str::from_utf8(&self.signature).unwrap_or("????")
    }
}

// ---------------------------------------------------------------------------
// Checksum validation
// ---------------------------------------------------------------------------

/// Validate an ACPI table checksum.
///
/// All bytes in the structure must sum to zero (mod 256).
///
/// # Safety
///
/// `ptr` must point to at least `len` readable bytes.
pub unsafe fn validate_checksum(ptr: *const u8, len: usize) -> bool {
    let mut sum: u8 = 0;
    for i in 0..len {
        // SAFETY: caller guarantees ptr..ptr+len is readable.
        sum = sum.wrapping_add(unsafe { *ptr.add(i) });
    }
    sum == 0
}

// ---------------------------------------------------------------------------
// RSDP validation and parsing
// ---------------------------------------------------------------------------

/// Validate the RSDP structure at the given virtual address.
///
/// Returns the RSDP revision (0 or 2+) on success.
///
/// # Safety
///
/// `rsdp_virt` must point to a valid, mapped RSDP structure.
pub unsafe fn validate_rsdp(rsdp_virt: u64) -> Option<u8> {
    let rsdp = rsdp_virt as *const Rsdp;

    // Check signature.
    // SAFETY: rsdp_virt is valid and mapped (bootloader guarantee).
    let sig = unsafe { (*rsdp).signature };
    if &sig != b"RSD PTR " {
        serial_println!("[acpi] RSDP signature mismatch: {:?}", sig);
        return None;
    }

    // Validate ACPI 1.0 checksum (first 20 bytes).
    let rsdp_size_v1 = core::mem::size_of::<Rsdp>();
    // SAFETY: rsdp points to at least 20 bytes.
    if !unsafe { validate_checksum(rsdp.cast(), rsdp_size_v1) } {
        serial_println!("[acpi] RSDP v1 checksum failed");
        return None;
    }

    // SAFETY: checked signature and v1 checksum.
    let revision = unsafe { (*rsdp).revision };

    // For ACPI 2.0+, also validate the extended checksum.
    if revision >= 2 {
        let rsdp2 = rsdp_virt as *const Rsdp2;
        let rsdp2_len = unsafe { (*rsdp2).length } as usize;
        // Sanity check the length.
        if rsdp2_len < core::mem::size_of::<Rsdp2>() {
            serial_println!("[acpi] RSDP2 length too small: {}", rsdp2_len);
            return None;
        }
        // SAFETY: rsdp2 points to rsdp2_len bytes (bootloader mapped).
        if !unsafe { validate_checksum(rsdp2.cast(), rsdp2_len) } {
            serial_println!("[acpi] RSDP2 extended checksum failed");
            return None;
        }
    }

    Some(revision)
}

// ---------------------------------------------------------------------------
// RSDT / XSDT table enumeration
// ---------------------------------------------------------------------------

/// Iterate over the SDT entries in the RSDT (32-bit pointers).
///
/// Calls `f(phys_address)` for each table pointer in the RSDT.
///
/// # Safety
///
/// `rsdt_virt` must point to a valid, mapped RSDT.
pub unsafe fn for_each_rsdt_entry<F>(rsdt_virt: u64, mut f: F)
where
    F: FnMut(u64),
{
    let header = rsdt_virt as *const SdtHeader;
    // SAFETY: rsdt_virt is valid (HHDM-mapped).
    let total_len = unsafe { (*header).length } as usize;
    if total_len < SdtHeader::SIZE {
        return;
    }

    // Entries follow the header as packed 32-bit physical addresses.
    #[allow(clippy::arithmetic_side_effects)]
    let entries_len = total_len - SdtHeader::SIZE;
    let num_entries = entries_len / 4;

    let entries_base = rsdt_virt.wrapping_add(SdtHeader::SIZE as u64);
    for i in 0..num_entries {
        let entry_ptr = entries_base.wrapping_add((i * 4) as u64) as *const u32;
        // SAFETY: within the RSDT bounds.
        let phys = unsafe { core::ptr::read_unaligned(entry_ptr) };
        f(u64::from(phys));
    }
}

/// Iterate over the SDT entries in the XSDT (64-bit pointers).
///
/// Calls `f(phys_address)` for each table pointer in the XSDT.
///
/// # Safety
///
/// `xsdt_virt` must point to a valid, mapped XSDT.
pub unsafe fn for_each_xsdt_entry<F>(xsdt_virt: u64, mut f: F)
where
    F: FnMut(u64),
{
    let header = xsdt_virt as *const SdtHeader;
    // SAFETY: xsdt_virt is valid (HHDM-mapped).
    let total_len = unsafe { (*header).length } as usize;
    if total_len < SdtHeader::SIZE {
        return;
    }

    // Entries follow the header as packed 64-bit physical addresses.
    #[allow(clippy::arithmetic_side_effects)]
    let entries_len = total_len - SdtHeader::SIZE;
    let num_entries = entries_len / 8;

    let entries_base = xsdt_virt.wrapping_add(SdtHeader::SIZE as u64);
    for i in 0..num_entries {
        let entry_ptr = entries_base.wrapping_add((i * 8) as u64) as *const u64;
        // SAFETY: within the XSDT bounds.
        let phys = unsafe { core::ptr::read_unaligned(entry_ptr) };
        f(phys);
    }
}

/// Validate an SDT header checksum.
///
/// # Safety
///
/// `sdt_virt` must point to a valid, mapped SDT with at least `length`
/// readable bytes.
pub unsafe fn validate_sdt(sdt_virt: u64) -> bool {
    let header = sdt_virt as *const SdtHeader;
    // SAFETY: sdt_virt is valid.
    let len = unsafe { (*header).length } as usize;
    if len < SdtHeader::SIZE {
        return false;
    }
    // SAFETY: sdt_virt points to len bytes.
    unsafe { validate_checksum(sdt_virt as *const u8, len) }
}

//! ACPI 2.0+ extensions — Generic Address Structure, GPE blocks, FADT v3+.
//!
//! This is the second part of the ACPI userspace surface that didn't
//! fit cleanly in `linux_acpi_user_types`: the 2.0-era register-access
//! structures and the wider extended-mode fields added to the FADT and
//! MADT in versions 2 and later.

// ---------------------------------------------------------------------------
// Generic Address Structure (GAS) `address_space_id`
// ---------------------------------------------------------------------------

pub const ACPI_AS_SYSTEM_MEMORY: u8 = 0x00;
pub const ACPI_AS_SYSTEM_IO: u8 = 0x01;
pub const ACPI_AS_PCI_CONFIG: u8 = 0x02;
pub const ACPI_AS_EMBEDDED_CONTROLLER: u8 = 0x03;
pub const ACPI_AS_SMBUS: u8 = 0x04;
pub const ACPI_AS_SYSTEM_CMOS: u8 = 0x05;
pub const ACPI_AS_PCI_BAR_TARGET: u8 = 0x06;
pub const ACPI_AS_IPMI: u8 = 0x07;
pub const ACPI_AS_GPIO: u8 = 0x08;
pub const ACPI_AS_GSBUS: u8 = 0x09;
pub const ACPI_AS_PLATFORM_COMM: u8 = 0x0A;
pub const ACPI_AS_FFH: u8 = 0x7F; // Functional Fixed Hardware

// ---------------------------------------------------------------------------
// GAS `access_size` enum
// ---------------------------------------------------------------------------

pub const ACPI_ACCESS_UNDEFINED: u8 = 0;
pub const ACPI_ACCESS_BYTE: u8 = 1;
pub const ACPI_ACCESS_WORD: u8 = 2;
pub const ACPI_ACCESS_DWORD: u8 = 3;
pub const ACPI_ACCESS_QWORD: u8 = 4;

// ---------------------------------------------------------------------------
// GAS struct layout — 12 bytes total
// ---------------------------------------------------------------------------

pub const ACPI_GAS_SIZE: usize = 12;
pub const ACPI_GAS_OFFSET_ADDRESS_SPACE_ID: usize = 0;
pub const ACPI_GAS_OFFSET_BIT_WIDTH: usize = 1;
pub const ACPI_GAS_OFFSET_BIT_OFFSET: usize = 2;
pub const ACPI_GAS_OFFSET_ACCESS_SIZE: usize = 3;
pub const ACPI_GAS_OFFSET_ADDRESS: usize = 4;

// ---------------------------------------------------------------------------
// FADT v3+ fields — minimum table revisions where they appeared
// ---------------------------------------------------------------------------

pub const ACPI_FADT_REV_V1: u8 = 1;
pub const ACPI_FADT_REV_V3: u8 = 3;
pub const ACPI_FADT_REV_V4: u8 = 4;
pub const ACPI_FADT_REV_V5: u8 = 5;
pub const ACPI_FADT_REV_V6: u8 = 6;

// ---------------------------------------------------------------------------
// MADT 2.0 entry types (top of `union acpi_subtable_headers`)
// ---------------------------------------------------------------------------

pub const ACPI_MADT_LOCAL_APIC: u8 = 0;
pub const ACPI_MADT_IO_APIC: u8 = 1;
pub const ACPI_MADT_INTERRUPT_OVERRIDE: u8 = 2;
pub const ACPI_MADT_NMI_SOURCE: u8 = 3;
pub const ACPI_MADT_LOCAL_APIC_NMI: u8 = 4;
pub const ACPI_MADT_LOCAL_APIC_OVERRIDE: u8 = 5;
pub const ACPI_MADT_IO_SAPIC: u8 = 6;
pub const ACPI_MADT_LOCAL_SAPIC: u8 = 7;
pub const ACPI_MADT_PLATFORM_INTERRUPT: u8 = 8;
pub const ACPI_MADT_LOCAL_X2APIC: u8 = 9;
pub const ACPI_MADT_LOCAL_X2APIC_NMI: u8 = 10;
pub const ACPI_MADT_GICC: u8 = 11;
pub const ACPI_MADT_GICD: u8 = 12;
pub const ACPI_MADT_GIC_MSI: u8 = 13;
pub const ACPI_MADT_GICR: u8 = 14;
pub const ACPI_MADT_GIC_ITS: u8 = 15;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_space_ids_dense_0_to_a_plus_ffh() {
        let a = [
            ACPI_AS_SYSTEM_MEMORY,
            ACPI_AS_SYSTEM_IO,
            ACPI_AS_PCI_CONFIG,
            ACPI_AS_EMBEDDED_CONTROLLER,
            ACPI_AS_SMBUS,
            ACPI_AS_SYSTEM_CMOS,
            ACPI_AS_PCI_BAR_TARGET,
            ACPI_AS_IPMI,
            ACPI_AS_GPIO,
            ACPI_AS_GSBUS,
            ACPI_AS_PLATFORM_COMM,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // FFH is the special "functional fixed hardware" slot at 0x7F.
        assert_eq!(ACPI_AS_FFH, 0x7F);
        // All low IDs fit in 4 bits.
        for v in a {
            assert!(v < 0x10);
        }
    }

    #[test]
    fn test_access_size_dense_0_to_4() {
        let s = [
            ACPI_ACCESS_UNDEFINED,
            ACPI_ACCESS_BYTE,
            ACPI_ACCESS_WORD,
            ACPI_ACCESS_DWORD,
            ACPI_ACCESS_QWORD,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_gas_struct_offsets_sum_to_size() {
        // Layout: u8 + u8 + u8 + u8 + u64 = 12 bytes.
        assert_eq!(ACPI_GAS_OFFSET_ADDRESS_SPACE_ID, 0);
        assert_eq!(ACPI_GAS_OFFSET_BIT_WIDTH, 1);
        assert_eq!(ACPI_GAS_OFFSET_BIT_OFFSET, 2);
        assert_eq!(ACPI_GAS_OFFSET_ACCESS_SIZE, 3);
        assert_eq!(ACPI_GAS_OFFSET_ADDRESS, 4);
        // 4 bytes of header + 8-byte u64 address = 12.
        assert_eq!(ACPI_GAS_OFFSET_ADDRESS + 8, ACPI_GAS_SIZE);
    }

    #[test]
    fn test_fadt_revisions_increasing() {
        let r = [
            ACPI_FADT_REV_V1,
            ACPI_FADT_REV_V3,
            ACPI_FADT_REV_V4,
            ACPI_FADT_REV_V5,
            ACPI_FADT_REV_V6,
        ];
        for w in r.windows(2) {
            assert!(w[0] < w[1]);
        }
        // v2 was skipped in the wild — only v1, v3+ are shipped by real
        // firmware.
        assert_eq!(ACPI_FADT_REV_V3 - ACPI_FADT_REV_V1, 2);
    }

    #[test]
    fn test_madt_entry_types_dense_0_to_15() {
        let m = [
            ACPI_MADT_LOCAL_APIC,
            ACPI_MADT_IO_APIC,
            ACPI_MADT_INTERRUPT_OVERRIDE,
            ACPI_MADT_NMI_SOURCE,
            ACPI_MADT_LOCAL_APIC_NMI,
            ACPI_MADT_LOCAL_APIC_OVERRIDE,
            ACPI_MADT_IO_SAPIC,
            ACPI_MADT_LOCAL_SAPIC,
            ACPI_MADT_PLATFORM_INTERRUPT,
            ACPI_MADT_LOCAL_X2APIC,
            ACPI_MADT_LOCAL_X2APIC_NMI,
            ACPI_MADT_GICC,
            ACPI_MADT_GICD,
            ACPI_MADT_GIC_MSI,
            ACPI_MADT_GICR,
            ACPI_MADT_GIC_ITS,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}

//! `<linux/ccix.h>` — Cache Coherent Interconnect for Accelerators.
//!
//! CCIX adds cache-coherent peer-to-peer attach over PCIe extensions
//! for accelerators (FPGAs, GPUs, NICs). The kernel exposes CCIX
//! protocol identifiers and link-config parameters through PCIe
//! capability structures.

// ---------------------------------------------------------------------------
// PCIe extended-capability ID for CCIX
// ---------------------------------------------------------------------------

/// PCI Express extended-capability ID assigned to CCIX (vendor 0x1957).
pub const PCI_EXT_CAP_ID_CCIX: u16 = 0x002D;

/// CCIX vendor ID (PCI-SIG assignment for the CCIX Consortium).
pub const CCIX_VENDOR_ID: u16 = 0x1957;

// ---------------------------------------------------------------------------
// Link speeds (GT/s)
// ---------------------------------------------------------------------------

pub const CCIX_LINK_SPEED_2_5_GTS: u32 = 1;
pub const CCIX_LINK_SPEED_5_GTS: u32 = 2;
pub const CCIX_LINK_SPEED_8_GTS: u32 = 3;
pub const CCIX_LINK_SPEED_16_GTS: u32 = 4;
pub const CCIX_LINK_SPEED_20_GTS: u32 = 5;
pub const CCIX_LINK_SPEED_25_GTS: u32 = 6;
pub const CCIX_LINK_SPEED_32_GTS: u32 = 7;

// ---------------------------------------------------------------------------
// Link widths (in lanes — must be power of two ≤ 32)
// ---------------------------------------------------------------------------

pub const CCIX_LINK_WIDTH_X1: u32 = 1;
pub const CCIX_LINK_WIDTH_X2: u32 = 2;
pub const CCIX_LINK_WIDTH_X4: u32 = 4;
pub const CCIX_LINK_WIDTH_X8: u32 = 8;
pub const CCIX_LINK_WIDTH_X16: u32 = 16;
pub const CCIX_LINK_WIDTH_X32: u32 = 32;

// ---------------------------------------------------------------------------
// Address-translation services flag bits
// ---------------------------------------------------------------------------

/// Bit indicating CCIX ATC (Address Translation Cache) is supported.
pub const CCIX_CAP_ATC_SUPPORT: u32 = 1 << 0;

/// Bit indicating Page Request Interface is supported.
pub const CCIX_CAP_PRI_SUPPORT: u32 = 1 << 1;

/// Bit indicating PASID is supported.
pub const CCIX_CAP_PASID_SUPPORT: u32 = 1 << 2;

/// Bit indicating snooped memory regions are supported.
pub const CCIX_CAP_SNOOPED_MEM: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Maximum address-space ID (PASID width)
// ---------------------------------------------------------------------------

/// PASID is a 20-bit field; max value is (1 << 20) - 1.
pub const CCIX_PASID_MAX: u32 = (1 << 20) - 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vendor_and_cap_id() {
        assert_eq!(CCIX_VENDOR_ID, 0x1957);
        assert_eq!(PCI_EXT_CAP_ID_CCIX, 0x002D);
    }

    #[test]
    fn test_link_speeds_dense_1_to_7() {
        let s = [
            CCIX_LINK_SPEED_2_5_GTS,
            CCIX_LINK_SPEED_5_GTS,
            CCIX_LINK_SPEED_8_GTS,
            CCIX_LINK_SPEED_16_GTS,
            CCIX_LINK_SPEED_20_GTS,
            CCIX_LINK_SPEED_25_GTS,
            CCIX_LINK_SPEED_32_GTS,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_link_widths_are_powers_of_two_1_to_32() {
        let w = [
            CCIX_LINK_WIDTH_X1,
            CCIX_LINK_WIDTH_X2,
            CCIX_LINK_WIDTH_X4,
            CCIX_LINK_WIDTH_X8,
            CCIX_LINK_WIDTH_X16,
            CCIX_LINK_WIDTH_X32,
        ];
        for (i, &v) in w.iter().enumerate() {
            assert!(v.is_power_of_two());
            assert_eq!(v, 1 << i);
        }
        assert_eq!(CCIX_LINK_WIDTH_X32, 32);
    }

    #[test]
    fn test_cap_bits_distinct_single_bits() {
        let f = [
            CCIX_CAP_ATC_SUPPORT,
            CCIX_CAP_PRI_SUPPORT,
            CCIX_CAP_PASID_SUPPORT,
            CCIX_CAP_SNOOPED_MEM,
        ];
        for &v in &f {
            assert!(v.is_power_of_two());
        }
        for (i, &a) in f.iter().enumerate() {
            for &b in &f[i + 1..] {
                assert_eq!(a & b, 0);
            }
        }
    }

    #[test]
    fn test_pasid_max_is_20_bit() {
        assert_eq!(CCIX_PASID_MAX, (1u32 << 20) - 1);
        assert_eq!(CCIX_PASID_MAX, 0xFFFFF);
    }

    #[test]
    fn test_speed_grade_progression() {
        // Each speed grade strictly increases.
        let s = [
            CCIX_LINK_SPEED_2_5_GTS,
            CCIX_LINK_SPEED_5_GTS,
            CCIX_LINK_SPEED_8_GTS,
            CCIX_LINK_SPEED_16_GTS,
            CCIX_LINK_SPEED_20_GTS,
            CCIX_LINK_SPEED_25_GTS,
            CCIX_LINK_SPEED_32_GTS,
        ];
        for w in s.windows(2) {
            assert!(w[0] < w[1]);
        }
    }
}

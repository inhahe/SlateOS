//! `<linux/capability.h>` — version-3 `cap_user_*` ABI.
//!
//! Linux capabilities went through three on-the-wire formats. v3 is
//! the current ABI and uses 64 bits laid out as two 32-bit halves
//! (`__u32` per `cap_user_data` element), allowing more than 32
//! capabilities to be addressed.

// ---------------------------------------------------------------------------
// Capability header magic
// ---------------------------------------------------------------------------

/// `_LINUX_CAPABILITY_VERSION_1` — original ABI, 32 caps, single u32.
pub const LINUX_CAPABILITY_VERSION_1: u32 = 0x1998_0330;

/// `_LINUX_CAPABILITY_VERSION_2` — deprecated 64-bit transitional ABI.
pub const LINUX_CAPABILITY_VERSION_2: u32 = 0x2007_1026;

/// `_LINUX_CAPABILITY_VERSION_3` — current (since 2.6.26).
pub const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;

/// Number of `cap_user_data_t` entries in the v3 ABI.
pub const LINUX_CAPABILITY_U32S_3: u32 = 2;

// ---------------------------------------------------------------------------
// `cap_user_header_t` field offsets
// ---------------------------------------------------------------------------

pub const CAP_HEADER_OFF_VERSION: usize = 0;
pub const CAP_HEADER_OFF_PID: usize = 4;
pub const CAP_HEADER_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// `cap_user_data_t` (one element of array)
// ---------------------------------------------------------------------------

pub const CAP_DATA_OFF_EFFECTIVE: usize = 0;
pub const CAP_DATA_OFF_PERMITTED: usize = 4;
pub const CAP_DATA_OFF_INHERITABLE: usize = 8;
pub const CAP_DATA_SIZE: usize = 12;

/// Total v3 data area: 2 entries × 12 bytes.
pub const CAP_DATA_V3_TOTAL_SIZE: usize = 24;

// ---------------------------------------------------------------------------
// Last-cap query (`/proc/sys/kernel/cap_last_cap`)
// ---------------------------------------------------------------------------

/// 5.x kernels expose this many capabilities (CAP_LAST_CAP).
pub const LINUX_CAP_LAST_CAP_TYPICAL: u32 = 40;

/// CAPNG-style bound: with 64 total slots, valid IDs are 0..63.
pub const LINUX_CAP_ID_MAX: u32 = 63;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_versions_distinct() {
        let v = [
            LINUX_CAPABILITY_VERSION_1,
            LINUX_CAPABILITY_VERSION_2,
            LINUX_CAPABILITY_VERSION_3,
        ];
        for (i, &a) in v.iter().enumerate() {
            for &b in &v[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // The high half encodes a YYYY-style year.
        assert_eq!(LINUX_CAPABILITY_VERSION_1 >> 16, 0x1998);
        assert_eq!(LINUX_CAPABILITY_VERSION_2 >> 16, 0x2007);
        assert_eq!(LINUX_CAPABILITY_VERSION_3 >> 16, 0x2008);
    }

    #[test]
    fn test_v3_u32_count_is_two() {
        // 2 × u32 = 64 capability bits per mask.
        assert_eq!(LINUX_CAPABILITY_U32S_3, 2);
    }

    #[test]
    fn test_header_layout_two_u32s() {
        assert_eq!(CAP_HEADER_OFF_VERSION, 0);
        assert_eq!(CAP_HEADER_OFF_PID, 4);
        assert_eq!(CAP_HEADER_SIZE, 8);
        assert_eq!(CAP_HEADER_OFF_PID - CAP_HEADER_OFF_VERSION, 4);
    }

    #[test]
    fn test_data_layout_three_packed_u32s() {
        let o = [
            CAP_DATA_OFF_EFFECTIVE,
            CAP_DATA_OFF_PERMITTED,
            CAP_DATA_OFF_INHERITABLE,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v, i * 4);
        }
        assert_eq!(CAP_DATA_SIZE, 3 * 4);
    }

    #[test]
    fn test_v3_total_size_two_entries() {
        // The v3 ABI returns an array of two cap_user_data_t.
        assert_eq!(
            CAP_DATA_V3_TOTAL_SIZE,
            CAP_DATA_SIZE * LINUX_CAPABILITY_U32S_3 as usize
        );
    }

    #[test]
    fn test_cap_id_range_fits_in_64_bits() {
        // 64 capability slots in the v3 layout.
        assert_eq!(LINUX_CAP_ID_MAX + 1, 64);
        // Real kernels currently expose ~40, leaving 24 reserved slots.
        assert!(LINUX_CAP_LAST_CAP_TYPICAL < LINUX_CAP_ID_MAX);
    }
}

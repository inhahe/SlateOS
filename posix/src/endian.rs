//! `<endian.h>` — byte order conversion.
//!
//! Provides `htobe16`, `htobe32`, `htobe64`, `betoh16`, `betoh32`,
//! `betoh64`, `htole16`, `htole32`, `htole64`, `le16toh`, `le32toh`,
//! `le64toh`, and related constants.
//!
//! These are fully functional implementations that swap bytes on
//! little-endian targets and are identity on big-endian targets.
//! Our primary target (x86_64) is little-endian.
//!
//! ## Note
//!
//! `htons`/`htonl`/`ntohs`/`ntohl` live in the `socket` module
//! (they are traditionally in `<arpa/inet.h>`).  The functions here
//! come from the BSD/glibc `<endian.h>` extension header.

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Little-endian byte order.
pub const LITTLE_ENDIAN: i32 = 1234;
/// Big-endian byte order.
pub const BIG_ENDIAN: i32 = 4321;
/// PDP-endian byte order (historical, not supported).
pub const PDP_ENDIAN: i32 = 3412;

/// The byte order of this machine.
///
/// On x86_64, this is always `LITTLE_ENDIAN`.
#[cfg(target_endian = "little")]
pub const BYTE_ORDER: i32 = LITTLE_ENDIAN;

#[cfg(target_endian = "big")]
pub const BYTE_ORDER: i32 = BIG_ENDIAN;

// ---------------------------------------------------------------------------
// Host → big-endian (htobe*)
// ---------------------------------------------------------------------------

/// Convert a 16-bit value from host byte order to big-endian.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn htobe16(x: u16) -> u16 {
    x.to_be()
}

/// Convert a 32-bit value from host byte order to big-endian.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn htobe32(x: u32) -> u32 {
    x.to_be()
}

/// Convert a 64-bit value from host byte order to big-endian.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn htobe64(x: u64) -> u64 {
    x.to_be()
}

// ---------------------------------------------------------------------------
// Big-endian → host (be*toh)
// ---------------------------------------------------------------------------

/// Convert a 16-bit value from big-endian to host byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn be16toh(x: u16) -> u16 {
    u16::from_be(x)
}

/// Convert a 32-bit value from big-endian to host byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn be32toh(x: u32) -> u32 {
    u32::from_be(x)
}

/// Convert a 64-bit value from big-endian to host byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn be64toh(x: u64) -> u64 {
    u64::from_be(x)
}

// ---------------------------------------------------------------------------
// Host → little-endian (htole*)
// ---------------------------------------------------------------------------

/// Convert a 16-bit value from host byte order to little-endian.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn htole16(x: u16) -> u16 {
    x.to_le()
}

/// Convert a 32-bit value from host byte order to little-endian.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn htole32(x: u32) -> u32 {
    x.to_le()
}

/// Convert a 64-bit value from host byte order to little-endian.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn htole64(x: u64) -> u64 {
    x.to_le()
}

// ---------------------------------------------------------------------------
// Little-endian → host (le*toh)
// ---------------------------------------------------------------------------

/// Convert a 16-bit value from little-endian to host byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn le16toh(x: u16) -> u16 {
    u16::from_le(x)
}

/// Convert a 32-bit value from little-endian to host byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn le32toh(x: u32) -> u32 {
    u32::from_le(x)
}

/// Convert a 64-bit value from little-endian to host byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn le64toh(x: u64) -> u64 {
    u64::from_le(x)
}

// ---------------------------------------------------------------------------
// BSD aliases: betoh* → be*toh
// ---------------------------------------------------------------------------

/// BSD-style alias for `be16toh`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn betoh16(x: u16) -> u16 {
    be16toh(x)
}

/// BSD-style alias for `be32toh`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn betoh32(x: u32) -> u32 {
    be32toh(x)
}

/// BSD-style alias for `be64toh`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn betoh64(x: u64) -> u64 {
    be64toh(x)
}

/// BSD-style alias for `le16toh`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn letoh16(x: u16) -> u16 {
    le16toh(x)
}

/// BSD-style alias for `le32toh`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn letoh32(x: u32) -> u32 {
    le32toh(x)
}

/// BSD-style alias for `le64toh`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn letoh64(x: u64) -> u64 {
    le64toh(x)
}

// ---------------------------------------------------------------------------
// Byte-swap helpers
// ---------------------------------------------------------------------------

/// Swap bytes of a 16-bit value (unconditional).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn bswap_16(x: u16) -> u16 {
    x.swap_bytes()
}

/// Swap bytes of a 32-bit value (unconditional).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn bswap_32(x: u32) -> u32 {
    x.swap_bytes()
}

/// Swap bytes of a 64-bit value (unconditional).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn bswap_64(x: u64) -> u64 {
    x.swap_bytes()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_endian_constants() {
        assert_eq!(LITTLE_ENDIAN, 1234);
        assert_eq!(BIG_ENDIAN, 4321);
        assert_eq!(PDP_ENDIAN, 3412);
    }

    #[test]
    #[cfg(target_endian = "little")]
    fn test_byte_order_little_endian() {
        assert_eq!(BYTE_ORDER, LITTLE_ENDIAN);
    }

    #[test]
    #[cfg(target_endian = "big")]
    fn test_byte_order_big_endian() {
        assert_eq!(BYTE_ORDER, BIG_ENDIAN);
    }

    // -----------------------------------------------------------------------
    // htobe* / be*toh round-trips
    // -----------------------------------------------------------------------

    #[test]
    fn test_htobe16_roundtrip() {
        for &val in &[0u16, 1, 0x0102, 0x1234, 0xFFFF, 0x8000] {
            assert_eq!(be16toh(htobe16(val)), val);
        }
    }

    #[test]
    fn test_htobe32_roundtrip() {
        for &val in &[0u32, 1, 0x01020304, 0xDEADBEEF, 0xFFFFFFFF, 0x80000000] {
            assert_eq!(be32toh(htobe32(val)), val);
        }
    }

    #[test]
    fn test_htobe64_roundtrip() {
        for &val in &[0u64, 1, 0x0102030405060708, 0xDEADBEEFCAFEBABE, u64::MAX] {
            assert_eq!(be64toh(htobe64(val)), val);
        }
    }

    // -----------------------------------------------------------------------
    // htole* / le*toh round-trips
    // -----------------------------------------------------------------------

    #[test]
    fn test_htole16_roundtrip() {
        for &val in &[0u16, 1, 0x0102, 0xABCD, 0xFFFF] {
            assert_eq!(le16toh(htole16(val)), val);
        }
    }

    #[test]
    fn test_htole32_roundtrip() {
        for &val in &[0u32, 1, 0x01020304, 0xCAFEBABE, u32::MAX] {
            assert_eq!(le32toh(htole32(val)), val);
        }
    }

    #[test]
    fn test_htole64_roundtrip() {
        for &val in &[0u64, 1, 0x0102030405060708, u64::MAX, 0x8000000000000000] {
            assert_eq!(le64toh(htole64(val)), val);
        }
    }

    // -----------------------------------------------------------------------
    // Specific byte patterns on little-endian
    // -----------------------------------------------------------------------

    #[test]
    #[cfg(target_endian = "little")]
    fn test_htobe16_swaps_on_le() {
        assert_eq!(htobe16(0x0102), 0x0201);
        assert_eq!(htobe16(0xABCD), 0xCDAB);
    }

    #[test]
    #[cfg(target_endian = "little")]
    fn test_htobe32_swaps_on_le() {
        assert_eq!(htobe32(0x01020304), 0x04030201);
        assert_eq!(htobe32(0xDEADBEEF), 0xEFBEADDE);
    }

    #[test]
    #[cfg(target_endian = "little")]
    fn test_htobe64_swaps_on_le() {
        assert_eq!(htobe64(0x0102030405060708), 0x0807060504030201);
    }

    #[test]
    #[cfg(target_endian = "little")]
    fn test_htole16_identity_on_le() {
        // On LE, host→LE is identity.
        assert_eq!(htole16(0x0102), 0x0102);
        assert_eq!(htole16(0xABCD), 0xABCD);
    }

    #[test]
    #[cfg(target_endian = "little")]
    fn test_htole32_identity_on_le() {
        assert_eq!(htole32(0x01020304), 0x01020304);
    }

    #[test]
    #[cfg(target_endian = "little")]
    fn test_htole64_identity_on_le() {
        assert_eq!(htole64(0x0102030405060708), 0x0102030405060708);
    }

    // -----------------------------------------------------------------------
    // be*toh on little-endian
    // -----------------------------------------------------------------------

    #[test]
    #[cfg(target_endian = "little")]
    fn test_be16toh_swaps_on_le() {
        assert_eq!(be16toh(0x0102), 0x0201);
    }

    #[test]
    #[cfg(target_endian = "little")]
    fn test_be32toh_swaps_on_le() {
        assert_eq!(be32toh(0x01020304), 0x04030201);
    }

    #[test]
    #[cfg(target_endian = "little")]
    fn test_be64toh_swaps_on_le() {
        assert_eq!(be64toh(0x0102030405060708), 0x0807060504030201);
    }

    // -----------------------------------------------------------------------
    // le*toh on little-endian
    // -----------------------------------------------------------------------

    #[test]
    #[cfg(target_endian = "little")]
    fn test_le16toh_identity_on_le() {
        assert_eq!(le16toh(0x0102), 0x0102);
    }

    #[test]
    #[cfg(target_endian = "little")]
    fn test_le32toh_identity_on_le() {
        assert_eq!(le32toh(0x01020304), 0x01020304);
    }

    #[test]
    #[cfg(target_endian = "little")]
    fn test_le64toh_identity_on_le() {
        assert_eq!(le64toh(0x0102030405060708), 0x0102030405060708);
    }

    // -----------------------------------------------------------------------
    // BSD aliases
    // -----------------------------------------------------------------------

    #[test]
    fn test_betoh_aliases() {
        assert_eq!(betoh16(0x0102), be16toh(0x0102));
        assert_eq!(betoh32(0x01020304), be32toh(0x01020304));
        assert_eq!(betoh64(0x0102030405060708), be64toh(0x0102030405060708));
    }

    #[test]
    fn test_letoh_aliases() {
        assert_eq!(letoh16(0x0102), le16toh(0x0102));
        assert_eq!(letoh32(0x01020304), le32toh(0x01020304));
        assert_eq!(letoh64(0x0102030405060708), le64toh(0x0102030405060708));
    }

    // -----------------------------------------------------------------------
    // bswap_*
    // -----------------------------------------------------------------------

    #[test]
    fn test_bswap_16() {
        assert_eq!(bswap_16(0x0102), 0x0201);
        assert_eq!(bswap_16(0x0000), 0x0000);
        assert_eq!(bswap_16(0xFFFF), 0xFFFF);
        assert_eq!(bswap_16(0xFF00), 0x00FF);
        assert_eq!(bswap_16(0x00FF), 0xFF00);
    }

    #[test]
    fn test_bswap_32() {
        assert_eq!(bswap_32(0x01020304), 0x04030201);
        assert_eq!(bswap_32(0x00000000), 0x00000000);
        assert_eq!(bswap_32(0xFFFFFFFF), 0xFFFFFFFF);
        assert_eq!(bswap_32(0xFF000000), 0x000000FF);
        assert_eq!(bswap_32(0x000000FF), 0xFF000000);
    }

    #[test]
    fn test_bswap_64() {
        assert_eq!(bswap_64(0x0102030405060708), 0x0807060504030201);
        assert_eq!(bswap_64(0x0000000000000000), 0x0000000000000000);
        assert_eq!(bswap_64(u64::MAX), u64::MAX);
        assert_eq!(bswap_64(0xFF00000000000000), 0x00000000000000FF);
    }

    // -----------------------------------------------------------------------
    // bswap double-swap is identity
    // -----------------------------------------------------------------------

    #[test]
    fn test_bswap_16_double_identity() {
        for &v in &[0u16, 1, 0x0102, 0xABCD, 0xFFFF] {
            assert_eq!(bswap_16(bswap_16(v)), v);
        }
    }

    #[test]
    fn test_bswap_32_double_identity() {
        for &v in &[0u32, 1, 0x01020304, 0xDEADBEEF, u32::MAX] {
            assert_eq!(bswap_32(bswap_32(v)), v);
        }
    }

    #[test]
    fn test_bswap_64_double_identity() {
        for &v in &[0u64, 1, 0x0102030405060708, 0xDEADBEEFCAFEBABE, u64::MAX] {
            assert_eq!(bswap_64(bswap_64(v)), v);
        }
    }

    // -----------------------------------------------------------------------
    // Zero is always zero
    // -----------------------------------------------------------------------

    #[test]
    fn test_zero_invariants() {
        assert_eq!(htobe16(0), 0);
        assert_eq!(htobe32(0), 0);
        assert_eq!(htobe64(0), 0);
        assert_eq!(htole16(0), 0);
        assert_eq!(htole32(0), 0);
        assert_eq!(htole64(0), 0);
        assert_eq!(be16toh(0), 0);
        assert_eq!(be32toh(0), 0);
        assert_eq!(be64toh(0), 0);
        assert_eq!(le16toh(0), 0);
        assert_eq!(le32toh(0), 0);
        assert_eq!(le64toh(0), 0);
    }

    // -----------------------------------------------------------------------
    // Max values are preserved through round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn test_max_value_roundtrips() {
        assert_eq!(be16toh(htobe16(u16::MAX)), u16::MAX);
        assert_eq!(be32toh(htobe32(u32::MAX)), u32::MAX);
        assert_eq!(be64toh(htobe64(u64::MAX)), u64::MAX);
        assert_eq!(le16toh(htole16(u16::MAX)), u16::MAX);
        assert_eq!(le32toh(htole32(u32::MAX)), u32::MAX);
        assert_eq!(le64toh(htole64(u64::MAX)), u64::MAX);
    }

    // -----------------------------------------------------------------------
    // Conversion consistency with socket byte-order functions
    // -----------------------------------------------------------------------

    #[test]
    fn test_htobe16_matches_htons() {
        // htobe16 should produce the same result as htons (both are host→network).
        for &v in &[0u16, 1, 80, 443, 8080, 0xFFFF] {
            assert_eq!(htobe16(v), crate::socket::htons(v));
        }
    }

    #[test]
    fn test_htobe32_matches_htonl() {
        for &v in &[0u32, 1, 0x7F000001, 0xFFFFFFFF] {
            assert_eq!(htobe32(v), crate::socket::htonl(v));
        }
    }

    #[test]
    fn test_be16toh_matches_ntohs() {
        for &v in &[0u16, 1, 80, 443, 0xFFFF] {
            assert_eq!(be16toh(v), crate::socket::ntohs(v));
        }
    }

    #[test]
    fn test_be32toh_matches_ntohl() {
        for &v in &[0u32, 1, 0x7F000001, 0xFFFFFFFF] {
            assert_eq!(be32toh(v), crate::socket::ntohl(v));
        }
    }
}

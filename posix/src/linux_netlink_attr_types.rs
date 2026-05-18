//! `<linux/netlink.h>` — Netlink attribute type constants.
//!
//! Netlink attributes (NLA) carry typed key-value data within
//! netlink messages. Each attribute has a type (nla_type) that
//! identifies what data it contains. These are used by rtnetlink,
//! generic netlink, and other netlink families.

// ---------------------------------------------------------------------------
// Netlink attribute header flags (nla_type upper bits)
// ---------------------------------------------------------------------------

/// Attribute is nested (contains sub-attributes).
pub const NLA_F_NESTED: u16 = 1 << 15;
/// Attribute is in network byte order.
pub const NLA_F_NET_BYTEORDER: u16 = 1 << 14;
/// Mask for actual attribute type.
pub const NLA_TYPE_MASK: u16 = !(NLA_F_NESTED | NLA_F_NET_BYTEORDER);

// ---------------------------------------------------------------------------
// Common attribute data types (for validation)
// ---------------------------------------------------------------------------

/// Unspecified attribute format.
pub const NLA_UNSPEC: u16 = 0;
/// 8-bit unsigned integer.
pub const NLA_U8: u16 = 1;
/// 16-bit unsigned integer.
pub const NLA_U16: u16 = 2;
/// 32-bit unsigned integer.
pub const NLA_U32: u16 = 3;
/// 64-bit unsigned integer.
pub const NLA_U64: u16 = 4;
/// NUL-terminated string.
pub const NLA_STRING: u16 = 5;
/// Fixed-size flag (no payload).
pub const NLA_FLAG: u16 = 6;
/// 64-bit millisecond timestamp.
pub const NLA_MSECS: u16 = 7;
/// Nested attributes.
pub const NLA_NESTED: u16 = 8;
/// Nested array of attributes.
pub const NLA_NESTED_ARRAY: u16 = 9;
/// Binary blob.
pub const NLA_BINARY: u16 = 10;
/// 8-bit signed integer.
pub const NLA_S8: u16 = 11;
/// 16-bit signed integer.
pub const NLA_S16: u16 = 12;
/// 32-bit signed integer.
pub const NLA_S32: u16 = 13;
/// 64-bit signed integer.
pub const NLA_S64: u16 = 14;

// ---------------------------------------------------------------------------
// Netlink attribute alignment
// ---------------------------------------------------------------------------

/// Netlink attribute alignment (4 bytes).
pub const NLA_ALIGNTO: u16 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nla_flags_no_overlap() {
        assert_eq!(NLA_F_NESTED & NLA_F_NET_BYTEORDER, 0);
    }

    #[test]
    fn test_nla_flags_are_high_bits() {
        assert!(NLA_F_NESTED >= 0x4000);
        assert!(NLA_F_NET_BYTEORDER >= 0x4000);
    }

    #[test]
    fn test_nla_type_mask() {
        // Mask should clear both flag bits
        assert_eq!(NLA_TYPE_MASK & NLA_F_NESTED, 0);
        assert_eq!(NLA_TYPE_MASK & NLA_F_NET_BYTEORDER, 0);
    }

    #[test]
    fn test_nla_data_types_distinct() {
        let types = [
            NLA_UNSPEC, NLA_U8, NLA_U16, NLA_U32, NLA_U64,
            NLA_STRING, NLA_FLAG, NLA_MSECS, NLA_NESTED,
            NLA_NESTED_ARRAY, NLA_BINARY,
            NLA_S8, NLA_S16, NLA_S32, NLA_S64,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_nla_unspec_is_zero() {
        assert_eq!(NLA_UNSPEC, 0);
    }

    #[test]
    fn test_nla_alignment() {
        assert_eq!(NLA_ALIGNTO, 4);
        assert!(NLA_ALIGNTO.is_power_of_two());
    }
}

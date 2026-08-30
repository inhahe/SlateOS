//! `<linux/if_tr.h>` — Token Ring constants.
//!
//! Token Ring (IEEE 802.5) is a LAN technology using token
//! passing for access control.  These constants define frame
//! fields, access control bits, frame control types, and
//! source routing parameters.

// ---------------------------------------------------------------------------
// Token Ring frame sizes
// ---------------------------------------------------------------------------

/// Minimum Token Ring MTU.
pub const TR_MIN_MTU: u32 = 100;
/// Maximum Token Ring MTU (4 Mbps ring).
pub const TR_MAX_MTU_4: u32 = 4464;
/// Maximum Token Ring MTU (16 Mbps ring).
pub const TR_MAX_MTU_16: u32 = 17914;
/// Default Token Ring MTU.
pub const TR_DEFAULT_MTU: u32 = TR_MAX_MTU_4;
/// Token Ring header length (without routing info).
pub const TR_HLEN: u32 = 14;
/// Token Ring address length (MAC).
pub const TR_ALEN: u32 = 6;

// ---------------------------------------------------------------------------
// Access Control byte fields
// ---------------------------------------------------------------------------

/// Priority mask (3 bits).
pub const AC_PRIORITY_MASK: u8 = 0xE0;
/// Token bit (0 = token, 1 = frame).
pub const AC_TOKEN: u8 = 0x10;
/// Monitor count.
pub const AC_MONITOR: u8 = 0x08;
/// Reservation mask.
pub const AC_RESERVATION_MASK: u8 = 0x07;

// ---------------------------------------------------------------------------
// Frame Control byte values
// ---------------------------------------------------------------------------

/// MAC frame.
pub const FC_MAC: u8 = 0x00;
/// LLC frame (data).
pub const FC_LLC: u8 = 0x40;

// ---------------------------------------------------------------------------
// Source Routing parameters
// ---------------------------------------------------------------------------

/// Routing info present indicator (bit in source MAC).
pub const TR_RII: u8 = 0x80;
/// Maximum source route length.
pub const TR_MAX_RIF_LEN: u32 = 18;
/// Routing control: broadcast all routes.
pub const RC_BROADCAST_ALL: u16 = 0x8000;
/// Routing control: broadcast single route.
pub const RC_BROADCAST_SINGLE: u16 = 0xC000;
/// Routing control: length mask.
pub const RC_LEN_MASK: u16 = 0x1F00;
/// Routing control: direction bit.
pub const RC_DIR_BIT: u16 = 0x0080;
/// Routing control: largest frame mask.
pub const RC_LF_MASK: u16 = 0x0070;

// ---------------------------------------------------------------------------
// Source Routing largest frame sizes
// ---------------------------------------------------------------------------

/// 516 bytes.
pub const RC_LF_516: u16 = 0x0000;
/// 1500 bytes.
pub const RC_LF_1500: u16 = 0x0010;
/// 2052 bytes.
pub const RC_LF_2052: u16 = 0x0020;
/// 4472 bytes.
pub const RC_LF_4472: u16 = 0x0030;
/// 8191 bytes.
pub const RC_LF_8191: u16 = 0x0040;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mtu_ordering() {
        assert!(TR_MIN_MTU < TR_MAX_MTU_4);
        assert!(TR_MAX_MTU_4 < TR_MAX_MTU_16);
    }

    #[test]
    fn test_default_mtu() {
        assert_eq!(TR_DEFAULT_MTU, TR_MAX_MTU_4);
    }

    #[test]
    fn test_hlen() {
        assert_eq!(TR_HLEN, 14);
    }

    #[test]
    fn test_alen() {
        assert_eq!(TR_ALEN, 6);
    }

    #[test]
    fn test_fc_types_distinct() {
        assert_ne!(FC_MAC, FC_LLC);
    }

    #[test]
    fn test_ac_fields_distinct() {
        let fields: [u8; 4] = [AC_PRIORITY_MASK, AC_TOKEN, AC_MONITOR, AC_RESERVATION_MASK];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }

    #[test]
    fn test_rc_broadcast_distinct() {
        assert_ne!(RC_BROADCAST_ALL, RC_BROADCAST_SINGLE);
    }

    #[test]
    fn test_lf_sizes_distinct() {
        let sizes = [RC_LF_516, RC_LF_1500, RC_LF_2052, RC_LF_4472, RC_LF_8191];
        for i in 0..sizes.len() {
            for j in (i + 1)..sizes.len() {
                assert_ne!(sizes[i], sizes[j]);
            }
        }
    }

    #[test]
    fn test_rif_max_len() {
        assert_eq!(TR_MAX_RIF_LEN, 18);
    }

    #[test]
    fn test_rii_bit() {
        assert_eq!(TR_RII, 0x80);
    }
}

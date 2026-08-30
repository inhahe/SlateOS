//! `<linux/if_macsec.h>` — MACsec (IEEE 802.1AE) netlink ABI.
//!
//! MACsec is the Layer-2 encryption protocol used in datacenter
//! switches and modern enterprise NICs. `iproute2`'s `ip macsec`
//! and `wpa_supplicant`'s MKA daemon configure transmit/receive
//! security channels through the genetlink commands below.

// ---------------------------------------------------------------------------
// Genetlink family
// ---------------------------------------------------------------------------

pub const MACSEC_GENL_NAME: &str = "macsec";
pub const MACSEC_GENL_VERSION: u32 = 1;
pub const MACSEC_GENL_MCGROUP: &str = "config";

// ---------------------------------------------------------------------------
// `enum macsec_nl_commands`
// ---------------------------------------------------------------------------

pub const MACSEC_CMD_GET_TXSC: u32 = 0;
pub const MACSEC_CMD_ADD_RXSC: u32 = 1;
pub const MACSEC_CMD_DEL_RXSC: u32 = 2;
pub const MACSEC_CMD_UPD_RXSC: u32 = 3;
pub const MACSEC_CMD_ADD_TXSA: u32 = 4;
pub const MACSEC_CMD_DEL_TXSA: u32 = 5;
pub const MACSEC_CMD_UPD_TXSA: u32 = 6;
pub const MACSEC_CMD_ADD_RXSA: u32 = 7;
pub const MACSEC_CMD_DEL_RXSA: u32 = 8;
pub const MACSEC_CMD_UPD_RXSA: u32 = 9;
pub const MACSEC_CMD_UPD_OFFLOAD: u32 = 10;

// ---------------------------------------------------------------------------
// Validation modes
// ---------------------------------------------------------------------------

pub const MACSEC_VALIDATE_DISABLED: u32 = 0;
pub const MACSEC_VALIDATE_CHECK: u32 = 1;
pub const MACSEC_VALIDATE_STRICT: u32 = 2;
pub const MACSEC_VALIDATE_MAX: u32 = MACSEC_VALIDATE_STRICT;

// ---------------------------------------------------------------------------
// Confidentiality offset (per IEEE 802.1AEbn)
// ---------------------------------------------------------------------------

pub const MACSEC_OFFLOAD_OFF: u32 = 0;
pub const MACSEC_OFFLOAD_PHY: u32 = 1;
pub const MACSEC_OFFLOAD_MAC: u32 = 2;
pub const MACSEC_OFFLOAD_MAX: u32 = MACSEC_OFFLOAD_MAC;

// ---------------------------------------------------------------------------
// Default ciphersuite IDs (IEEE 802.1AE-2018 Table 14-1)
// ---------------------------------------------------------------------------

/// GCM-AES-128 default.
pub const MACSEC_DEFAULT_CIPHER_ID: u64 = 0x0080_C200_0000_0001;
/// GCM-AES-256 default.
pub const MACSEC_GCM_AES_256: u64 = 0x0080_C200_0000_0002;
/// GCM-AES-XPN-128 (extended packet number).
pub const MACSEC_GCM_AES_XPN_128: u64 = 0x0080_C200_0000_0003;
/// GCM-AES-XPN-256 (extended packet number).
pub const MACSEC_GCM_AES_XPN_256: u64 = 0x0080_C200_0000_0004;

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// MACsec SAK (Secure Association Key) AES-128 length.
pub const MACSEC_KEYID_LEN: usize = 16;
/// EtherType for MACsec frames.
pub const ETH_P_MACSEC: u16 = 0x88E5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genl_identity() {
        assert_eq!(MACSEC_GENL_NAME, "macsec");
        assert_eq!(MACSEC_GENL_VERSION, 1);
        assert_eq!(MACSEC_GENL_MCGROUP, "config");
    }

    #[test]
    fn test_commands_dense_0_to_10() {
        let c = [
            MACSEC_CMD_GET_TXSC,
            MACSEC_CMD_ADD_RXSC,
            MACSEC_CMD_DEL_RXSC,
            MACSEC_CMD_UPD_RXSC,
            MACSEC_CMD_ADD_TXSA,
            MACSEC_CMD_DEL_TXSA,
            MACSEC_CMD_UPD_TXSA,
            MACSEC_CMD_ADD_RXSA,
            MACSEC_CMD_DEL_RXSA,
            MACSEC_CMD_UPD_RXSA,
            MACSEC_CMD_UPD_OFFLOAD,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_validate_modes_dense() {
        assert_eq!(MACSEC_VALIDATE_DISABLED, 0);
        assert_eq!(MACSEC_VALIDATE_CHECK, 1);
        assert_eq!(MACSEC_VALIDATE_STRICT, 2);
        assert_eq!(MACSEC_VALIDATE_MAX, MACSEC_VALIDATE_STRICT);
    }

    #[test]
    fn test_offload_modes_dense() {
        assert_eq!(MACSEC_OFFLOAD_OFF, 0);
        assert_eq!(MACSEC_OFFLOAD_PHY, 1);
        assert_eq!(MACSEC_OFFLOAD_MAC, 2);
        assert_eq!(MACSEC_OFFLOAD_MAX, MACSEC_OFFLOAD_MAC);
    }

    #[test]
    fn test_cipher_ids_share_oui_and_differ_in_low_byte() {
        let cs = [
            MACSEC_DEFAULT_CIPHER_ID,
            MACSEC_GCM_AES_256,
            MACSEC_GCM_AES_XPN_128,
            MACSEC_GCM_AES_XPN_256,
        ];
        // Top 48 bits are the same 802.1 OUI (00:80:C2 + 24 zero bits).
        for c in cs {
            assert_eq!(c >> 16, 0x0080_C200_0000);
        }
        // Low bytes are 1,2,3,4 — dense.
        for (i, &c) in cs.iter().enumerate() {
            assert_eq!(c & 0xFFFF, i as u64 + 1);
        }
    }

    #[test]
    fn test_sizes_and_ethertype() {
        // AES-128 key ID is 128 bits = 16 bytes.
        assert_eq!(MACSEC_KEYID_LEN, 16);
        // IEEE-registered MACsec EtherType.
        assert_eq!(ETH_P_MACSEC, 0x88E5);
    }
}

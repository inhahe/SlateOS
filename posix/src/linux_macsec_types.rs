//! `<linux/if_macsec.h>` — MACsec (802.1AE) constants.
//!
//! MACsec provides hop-by-hop encryption at the Ethernet layer
//! (Layer 2). It encrypts and authenticates all traffic between
//! directly connected nodes. Used in data center interconnects,
//! carrier Ethernet, and enterprise LANs requiring confidentiality.
//! The kernel MACsec implementation uses GCM-AES-128 or GCM-AES-256.

// ---------------------------------------------------------------------------
// MACsec cipher suites
// ---------------------------------------------------------------------------

/// GCM-AES-128 cipher suite ID.
pub const MACSEC_CIPHER_ID_GCM_AES_128: u64 = 0x0080_C201_0001_0001;
/// GCM-AES-256 cipher suite ID.
pub const MACSEC_CIPHER_ID_GCM_AES_256: u64 = 0x0080_C201_0001_0002;
/// GCM-AES-XPN-128 cipher suite ID (extended packet number).
pub const MACSEC_CIPHER_ID_GCM_AES_XPN_128: u64 = 0x0080_C201_0001_0003;
/// GCM-AES-XPN-256 cipher suite ID (extended packet number).
pub const MACSEC_CIPHER_ID_GCM_AES_XPN_256: u64 = 0x0080_C201_0001_0004;

// ---------------------------------------------------------------------------
// MACsec validation modes
// ---------------------------------------------------------------------------

/// Disabled: don't validate incoming frames.
pub const MACSEC_VALIDATE_DISABLED: u32 = 0;
/// Check: validate but allow invalid frames through.
pub const MACSEC_VALIDATE_CHECK: u32 = 1;
/// Strict: drop invalid frames.
pub const MACSEC_VALIDATE_STRICT: u32 = 2;

// ---------------------------------------------------------------------------
// MACsec offload modes
// ---------------------------------------------------------------------------

/// No offload (software only).
pub const MACSEC_OFFLOAD_OFF: u32 = 0;
/// PHY offload.
pub const MACSEC_OFFLOAD_PHY: u32 = 1;
/// MAC offload.
pub const MACSEC_OFFLOAD_MAC: u32 = 2;

// ---------------------------------------------------------------------------
// MACsec constants
// ---------------------------------------------------------------------------

/// MACsec ethertype (SecTAG).
pub const ETH_P_MACSEC: u16 = 0x88E5;
/// SecTAG header length (without optional SCI).
pub const MACSEC_TAG_LEN: u32 = 6;
/// SCI (Secure Channel Identifier) length.
pub const MACSEC_SCI_LEN: u32 = 8;
/// ICV (Integrity Check Value) default length for GCM-AES.
pub const MACSEC_ICV_LEN: u32 = 16;
/// Maximum key length.
pub const MACSEC_MAX_KEY_LEN: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cipher_ids_distinct() {
        let ciphers = [
            MACSEC_CIPHER_ID_GCM_AES_128,
            MACSEC_CIPHER_ID_GCM_AES_256,
            MACSEC_CIPHER_ID_GCM_AES_XPN_128,
            MACSEC_CIPHER_ID_GCM_AES_XPN_256,
        ];
        for i in 0..ciphers.len() {
            for j in (i + 1)..ciphers.len() {
                assert_ne!(ciphers[i], ciphers[j]);
            }
        }
    }

    #[test]
    fn test_validation_modes_distinct() {
        let modes = [
            MACSEC_VALIDATE_DISABLED,
            MACSEC_VALIDATE_CHECK,
            MACSEC_VALIDATE_STRICT,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_offload_modes_distinct() {
        let modes = [MACSEC_OFFLOAD_OFF, MACSEC_OFFLOAD_PHY, MACSEC_OFFLOAD_MAC];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_lengths() {
        assert!(MACSEC_TAG_LEN > 0);
        assert_eq!(MACSEC_SCI_LEN, 8);
        assert_eq!(MACSEC_ICV_LEN, 16);
        assert_eq!(MACSEC_MAX_KEY_LEN, 32);
    }
}

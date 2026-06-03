//! `<linux/if_macsec.h>` — Additional MACsec constants.
//!
//! Supplementary MACsec constants covering cipher suites,
//! validation modes, and offload types.

// ---------------------------------------------------------------------------
// MACsec cipher suite IDs
// ---------------------------------------------------------------------------

/// GCM-AES-128.
pub const MACSEC_CIPHER_ID_GCM_AES_128: u64 = 0x0080C20001000001;
/// GCM-AES-256.
pub const MACSEC_CIPHER_ID_GCM_AES_256: u64 = 0x0080C20001000002;
/// GCM-AES-XPN-128.
pub const MACSEC_CIPHER_ID_GCM_AES_XPN_128: u64 = 0x0080C20001000003;
/// GCM-AES-XPN-256.
pub const MACSEC_CIPHER_ID_GCM_AES_XPN_256: u64 = 0x0080C20001000004;

// ---------------------------------------------------------------------------
// MACsec validation modes
// ---------------------------------------------------------------------------

/// Disabled validation.
pub const MACSEC_VALIDATE_DISABLED: u32 = 0;
/// Check validation (accept invalid frames).
pub const MACSEC_VALIDATE_CHECK: u32 = 1;
/// Strict validation (drop invalid frames).
pub const MACSEC_VALIDATE_STRICT: u32 = 2;

// ---------------------------------------------------------------------------
// MACsec offload types
// ---------------------------------------------------------------------------

/// No offload.
pub const MACSEC_OFFLOAD_OFF: u32 = 0;
/// PHY offload.
pub const MACSEC_OFFLOAD_PHY: u32 = 1;
/// MAC offload.
pub const MACSEC_OFFLOAD_MAC: u32 = 2;

// ---------------------------------------------------------------------------
// MACsec constants
// ---------------------------------------------------------------------------

/// Default ICV (Integrity Check Value) length.
pub const MACSEC_DEFAULT_ICV_LEN: u32 = 16;
/// SecTAG (Security Tag) length.
pub const MACSEC_SECTAG_LEN: u32 = 16;
/// Maximum SCI (Secure Channel Identifier) value.
pub const MACSEC_SCI_LEN: u32 = 8;
/// Maximum secure associations per SC.
pub const MACSEC_MAX_SA: u32 = 4;
/// AN (Association Number) bit width.
pub const MACSEC_AN_BITS: u32 = 2;
/// AN mask.
pub const MACSEC_AN_MASK: u32 = 0x03;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cipher_suites_distinct() {
        let suites = [
            MACSEC_CIPHER_ID_GCM_AES_128,
            MACSEC_CIPHER_ID_GCM_AES_256,
            MACSEC_CIPHER_ID_GCM_AES_XPN_128,
            MACSEC_CIPHER_ID_GCM_AES_XPN_256,
        ];
        for i in 0..suites.len() {
            for j in (i + 1)..suites.len() {
                assert_ne!(suites[i], suites[j]);
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
    fn test_offload_types_distinct() {
        let types = [MACSEC_OFFLOAD_OFF, MACSEC_OFFLOAD_PHY, MACSEC_OFFLOAD_MAC];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_constants() {
        assert_eq!(MACSEC_DEFAULT_ICV_LEN, 16);
        assert_eq!(MACSEC_SCI_LEN, 8);
        assert_eq!(MACSEC_MAX_SA, 4);
    }

    #[test]
    fn test_an_mask() {
        assert_eq!(MACSEC_AN_MASK, (1 << MACSEC_AN_BITS) - 1);
    }
}

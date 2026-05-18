//! `<linux/hdlc.h>` — HDLC (High-Level Data Link Control) constants.
//!
//! HDLC is a synchronous data link layer protocol.  These
//! constants define HDLC encoding types, frame formats,
//! IOCTL commands, and interface modes.

// ---------------------------------------------------------------------------
// HDLC encoding types
// ---------------------------------------------------------------------------

/// NRZ (Non-Return-to-Zero).
pub const ENCODING_NRZ: u32 = 0;
/// NRZI (Non-Return-to-Zero Inverted).
pub const ENCODING_NRZI: u32 = 1;
/// FM (Frequency Modulation) mark.
pub const ENCODING_FM_MARK: u32 = 2;
/// FM space.
pub const ENCODING_FM_SPACE: u32 = 3;
/// Manchester.
pub const ENCODING_MANCHESTER: u32 = 4;

// ---------------------------------------------------------------------------
// HDLC parity types
// ---------------------------------------------------------------------------

/// No parity (CRC-16 default).
pub const PARITY_DEFAULT: u32 = 0;
/// CRC-16-CCITT.
pub const PARITY_CRC16_PR1: u32 = 1;
/// CRC-16 preset to 0.
pub const PARITY_CRC16_PR0: u32 = 2;
/// CRC-32-CCITT.
pub const PARITY_CRC32_PR1_CCITT: u32 = 3;
/// CRC-16 preset to FFFF.
pub const PARITY_CRC16_PR1_CCITT: u32 = 4;
/// No CRC.
pub const PARITY_NONE: u32 = 5;

// ---------------------------------------------------------------------------
// HDLC interface types
// ---------------------------------------------------------------------------

/// Generic HDLC.
pub const IF_PROTO_HDLC: u32 = 0x0800;
/// Cisco HDLC.
pub const IF_PROTO_CISCO: u32 = 0x0801;
/// Frame Relay (FR).
pub const IF_PROTO_FR: u32 = 0x0802;
/// Frame Relay (Annex D, PVC).
pub const IF_PROTO_FR_ADD_PVC: u32 = 0x0803;
/// Frame Relay (delete PVC).
pub const IF_PROTO_FR_DEL_PVC: u32 = 0x0804;
/// X.25.
pub const IF_PROTO_X25: u32 = 0x0805;
/// HDLC Ethernet.
pub const IF_PROTO_HDLC_ETH: u32 = 0x0806;
/// Frame Relay (Annex A, PVC).
pub const IF_PROTO_FR_ADD_ETH_PVC: u32 = 0x0807;
/// Frame Relay (delete Ethernet PVC).
pub const IF_PROTO_FR_DEL_ETH_PVC: u32 = 0x0808;
/// Frame Relay (ETH switch PVC).
pub const IF_PROTO_FR_PVC: u32 = 0x0809;
/// Frame Relay ETH PVC.
pub const IF_PROTO_FR_ETH_PVC: u32 = 0x080A;
/// PPP over HDLC.
pub const IF_PROTO_PPP: u32 = 0x080B;
/// Raw HDLC.
pub const IF_PROTO_RAW: u32 = 0x080C;

// ---------------------------------------------------------------------------
// HDLC IOCTL commands
// ---------------------------------------------------------------------------

/// Set HDLC interface.
pub const HDLCSETIF: u32 = 0x89E0;
/// Get HDLC interface.
pub const HDLCGETIF: u32 = 0x89E1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encodings_distinct() {
        let encs = [
            ENCODING_NRZ, ENCODING_NRZI, ENCODING_FM_MARK,
            ENCODING_FM_SPACE, ENCODING_MANCHESTER,
        ];
        for i in 0..encs.len() {
            for j in (i + 1)..encs.len() {
                assert_ne!(encs[i], encs[j]);
            }
        }
    }

    #[test]
    fn test_parities_distinct() {
        let pars = [
            PARITY_DEFAULT, PARITY_CRC16_PR1, PARITY_CRC16_PR0,
            PARITY_CRC32_PR1_CCITT, PARITY_CRC16_PR1_CCITT,
            PARITY_NONE,
        ];
        for i in 0..pars.len() {
            for j in (i + 1)..pars.len() {
                assert_ne!(pars[i], pars[j]);
            }
        }
    }

    #[test]
    fn test_protos_distinct() {
        let protos = [
            IF_PROTO_HDLC, IF_PROTO_CISCO, IF_PROTO_FR,
            IF_PROTO_FR_ADD_PVC, IF_PROTO_FR_DEL_PVC,
            IF_PROTO_X25, IF_PROTO_HDLC_ETH,
            IF_PROTO_FR_ADD_ETH_PVC, IF_PROTO_FR_DEL_ETH_PVC,
            IF_PROTO_FR_PVC, IF_PROTO_FR_ETH_PVC,
            IF_PROTO_PPP, IF_PROTO_RAW,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_nrz_is_zero() {
        assert_eq!(ENCODING_NRZ, 0);
    }

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(HDLCSETIF, HDLCGETIF);
    }
}

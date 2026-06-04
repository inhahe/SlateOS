//! `<linux/atmsap.h>` — ATM Service-Access-Point signalling structures.
//!
//! When signalling SVC connection setup the calling application
//! supplies B-LLI (Lower-Layer Information) and B-HLI (Higher-Layer)
//! "SAP" octets that mirror ITU Q.2931. The kernel passes these
//! through to the switch as part of the SETUP message.

// ---------------------------------------------------------------------------
// Field lengths
// ---------------------------------------------------------------------------

pub const ATM_MAX_HLI: usize = 8;
pub const ATM_ESI_LEN: usize = 6;
pub const ATM_E164_LEN: usize = 20;
pub const ATM_AESA_LEN: usize = 20;

// ---------------------------------------------------------------------------
// B-LLI Layer-2 protocol identifiers (`bhli.hi_type`)
// ---------------------------------------------------------------------------

pub const ATM_HL_NONE: u8 = 0;
pub const ATM_HL_ISO: u8 = 1;
pub const ATM_HL_USER: u8 = 2;
pub const ATM_HL_HLP: u8 = 3;
pub const ATM_HL_VENDOR: u8 = 4;

// ---------------------------------------------------------------------------
// B-LLI Layer-2 protocol identifiers (`blli.l2_proto`)
// ---------------------------------------------------------------------------

pub const ATM_L2_NONE: u8 = 0;
pub const ATM_L2_ISO1745: u8 = 0x01;
pub const ATM_L2_Q291: u8 = 0x02;
pub const ATM_L2_X25_LL: u8 = 0x06;
pub const ATM_L2_X25_ML: u8 = 0x07;
pub const ATM_L2_LAPB: u8 = 0x08;
pub const ATM_L2_HDLC_ARM: u8 = 0x09;
pub const ATM_L2_HDLC_NRM: u8 = 0x0A;
pub const ATM_L2_HDLC_ABM: u8 = 0x0B;
pub const ATM_L2_ISO8802: u8 = 0x0C;
pub const ATM_L2_X75: u8 = 0x0D;
pub const ATM_L2_Q922: u8 = 0x0E;
pub const ATM_L2_USER: u8 = 0x10;
pub const ATM_L2_ISO7776: u8 = 0x11;

// ---------------------------------------------------------------------------
// B-LLI Layer-3 protocol identifiers (`blli.l3_proto`)
// ---------------------------------------------------------------------------

pub const ATM_L3_NONE: u8 = 0;
pub const ATM_L3_X25: u8 = 0x06;
pub const ATM_L3_ISO8208: u8 = 0x07;
pub const ATM_L3_X223: u8 = 0x08;
pub const ATM_L3_ISO8473: u8 = 0x09;
pub const ATM_L3_T70: u8 = 0x0A;
pub const ATM_L3_TR9577: u8 = 0x0B;
pub const ATM_L3_H310: u8 = 0x0C;
pub const ATM_L3_H321: u8 = 0x0D;
pub const ATM_L3_USER: u8 = 0x10;

// ---------------------------------------------------------------------------
// AAL5 mode field (`atm_aal_parm.aal5.mode`)
// ---------------------------------------------------------------------------

pub const ATM_AAL5_MODE_MESSAGE: u8 = 1;
pub const ATM_AAL5_MODE_STREAMING: u8 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_lengths() {
        // E.164 and AESA are both 20 octets in ITU encoding.
        assert_eq!(ATM_E164_LEN, 20);
        assert_eq!(ATM_AESA_LEN, 20);
        assert_eq!(ATM_E164_LEN, ATM_AESA_LEN);
        // ESI (End-System Identifier) is the 6-byte trailing MAC-like ID.
        assert_eq!(ATM_ESI_LEN, 6);
        assert_eq!(ATM_MAX_HLI, 8);
    }

    #[test]
    fn test_hli_types_dense_0_to_4() {
        let h = [ATM_HL_NONE, ATM_HL_ISO, ATM_HL_USER, ATM_HL_HLP, ATM_HL_VENDOR];
        for (i, &v) in h.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_l2_protocols_distinct_and_below_0x12() {
        let l = [
            ATM_L2_NONE,
            ATM_L2_ISO1745,
            ATM_L2_Q291,
            ATM_L2_X25_LL,
            ATM_L2_X25_ML,
            ATM_L2_LAPB,
            ATM_L2_HDLC_ARM,
            ATM_L2_HDLC_NRM,
            ATM_L2_HDLC_ABM,
            ATM_L2_ISO8802,
            ATM_L2_X75,
            ATM_L2_Q922,
            ATM_L2_USER,
            ATM_L2_ISO7776,
        ];
        for &v in &l {
            assert!(v < 0x12);
        }
        for (i, &a) in l.iter().enumerate() {
            for &b in &l[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // HDLC family is dense 0x09..=0x0B.
        assert_eq!(ATM_L2_HDLC_ARM, 0x09);
        assert_eq!(ATM_L2_HDLC_ABM, 0x0B);
        // X.25 LL/ML form a consecutive pair.
        assert_eq!(ATM_L2_X25_ML, ATM_L2_X25_LL + 1);
    }

    #[test]
    fn test_l3_protocols_distinct_and_below_0x12() {
        let l = [
            ATM_L3_NONE,
            ATM_L3_X25,
            ATM_L3_ISO8208,
            ATM_L3_X223,
            ATM_L3_ISO8473,
            ATM_L3_T70,
            ATM_L3_TR9577,
            ATM_L3_H310,
            ATM_L3_H321,
            ATM_L3_USER,
        ];
        for &v in &l {
            assert!(v < 0x12);
        }
        for (i, &a) in l.iter().enumerate() {
            for &b in &l[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // H.310 and H.321 (videoconf) form a consecutive pair.
        assert_eq!(ATM_L3_H321, ATM_L3_H310 + 1);
    }

    #[test]
    fn test_aal5_mode_pair() {
        // MESSAGE=1, STREAMING=2 — boolean-like pair.
        assert_eq!(ATM_AAL5_MODE_STREAMING, ATM_AAL5_MODE_MESSAGE + 1);
        assert_ne!(ATM_AAL5_MODE_MESSAGE, ATM_AAL5_MODE_STREAMING);
    }
}

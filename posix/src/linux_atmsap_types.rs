//! `<linux/atmsap.h>` — ATM Service Access Point (SAP) constants.
//!
//! ATM signalling carries B-LLI/B-HLI Service Access Point info in
//! Q.2931 setup messages. These constants cover the BLLI layer-2/3
//! protocol identifiers and the high-layer-info type/profile tags
//! used by the ATM stack's signalling and `atmsigd` userspace daemon.

// ---------------------------------------------------------------------------
// B-LLI layer 2 protocols (ATM Forum / Q.2931 §4.5.18)
// ---------------------------------------------------------------------------

/// ISO/IEC 1745 (basic mode, BSC).
pub const ATM_L2_ISO_1745: u8 = 0x01;
/// Q.921 (LAPD).
pub const ATM_L2_Q291: u8 = 0x02;
/// HDLC ARM.
pub const ATM_L2_X25_LL: u8 = 0x06;
/// HDLC NRM.
pub const ATM_L2_X25_ML: u8 = 0x07;
/// LAPB.
pub const ATM_L2_LAPB: u8 = 0x08;
/// HDLC ABM.
pub const ATM_L2_HDLC_ARM: u8 = 0x09;
/// HDLC ARM mode.
pub const ATM_L2_HDLC_NRM: u8 = 0x0a;
/// LLC.
pub const ATM_L2_LLC: u8 = 0x0c;
/// X.75.
pub const ATM_L2_X75: u8 = 0x0d;
/// Q.922.
pub const ATM_L2_Q922: u8 = 0x0e;
/// User-specified layer 2.
pub const ATM_L2_USER: u8 = 0x10;
/// ISO 7776.
pub const ATM_L2_ISO_7776: u8 = 0x11;

// ---------------------------------------------------------------------------
// B-LLI layer 3 protocols
// ---------------------------------------------------------------------------

/// X.25 packet layer.
pub const ATM_L3_X25: u8 = 0x06;
/// ISO/IEC 8208.
pub const ATM_L3_ISO_8208: u8 = 0x07;
/// X.223 / ISO 8878.
pub const ATM_L3_X223: u8 = 0x08;
/// ISO/IEC 8473 (CLNP).
pub const ATM_L3_ISO_8473: u8 = 0x09;
/// T.70.
pub const ATM_L3_T70: u8 = 0x0a;
/// User-specified.
pub const ATM_L3_USER: u8 = 0x10;
/// TR-9577 (multiprotocol).
pub const ATM_L3_TR9577: u8 = 0x0b;
/// H.310.
pub const ATM_L3_H310: u8 = 0x0c;
/// H.321.
pub const ATM_L3_H321: u8 = 0x0d;

// ---------------------------------------------------------------------------
// B-HLI high layer-info types
// ---------------------------------------------------------------------------

/// ISO/IEC TR-9577.
pub const ATM_HL_NONE: u8 = 0x00;
/// ISO high-layer profile.
pub const ATM_HL_ISO: u8 = 0x01;
/// User-specified high-layer information.
pub const ATM_HL_USER: u8 = 0x02;
/// HLI based on a vendor (manufacturer) code.
pub const ATM_HL_HLP: u8 = 0x03;
/// HLI tagged as a registered HLP.
pub const ATM_HL_VENDOR: u8 = 0x04;

// ---------------------------------------------------------------------------
// SAP buffer / field sizes
// ---------------------------------------------------------------------------

/// Maximum BLLI element length (Q.2931 limit).
pub const ATM_MAX_BLLI: u32 = 13;
/// Maximum BHLI element length.
pub const ATM_MAX_BHLI: u32 = 8;
/// Maximum number of BLLIs that may be negotiated in one setup.
pub const ATM_MAX_BLLI_COUNT: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_l2_protocols_distinct() {
        let l2 = [
            ATM_L2_ISO_1745,
            ATM_L2_Q291,
            ATM_L2_X25_LL,
            ATM_L2_X25_ML,
            ATM_L2_LAPB,
            ATM_L2_HDLC_ARM,
            ATM_L2_HDLC_NRM,
            ATM_L2_LLC,
            ATM_L2_X75,
            ATM_L2_Q922,
            ATM_L2_USER,
            ATM_L2_ISO_7776,
        ];
        for i in 0..l2.len() {
            for j in (i + 1)..l2.len() {
                assert_ne!(l2[i], l2[j]);
            }
        }
    }

    #[test]
    fn test_l3_protocols_distinct() {
        let l3 = [
            ATM_L3_X25,
            ATM_L3_ISO_8208,
            ATM_L3_X223,
            ATM_L3_ISO_8473,
            ATM_L3_T70,
            ATM_L3_USER,
            ATM_L3_TR9577,
            ATM_L3_H310,
            ATM_L3_H321,
        ];
        for i in 0..l3.len() {
            for j in (i + 1)..l3.len() {
                assert_ne!(l3[i], l3[j]);
            }
        }
    }

    #[test]
    fn test_hli_types_distinct() {
        let h = [
            ATM_HL_NONE,
            ATM_HL_ISO,
            ATM_HL_USER,
            ATM_HL_HLP,
            ATM_HL_VENDOR,
        ];
        for i in 0..h.len() {
            for j in (i + 1)..h.len() {
                assert_ne!(h[i], h[j]);
            }
        }
        // "None" must be 0 so a freshly-zeroed BHLI struct reads as
        // unset.
        assert_eq!(ATM_HL_NONE, 0);
    }

    #[test]
    fn test_buffer_sizes_within_q2931_limits() {
        // Q.2931 caps BLLI at 13 octets and BHLI at 8 octets.
        assert_eq!(ATM_MAX_BLLI, 13);
        assert_eq!(ATM_MAX_BHLI, 8);
        // At most 3 BLLI alternatives may appear in one setup.
        assert_eq!(ATM_MAX_BLLI_COUNT, 3);
    }
}

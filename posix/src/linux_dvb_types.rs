//! `<linux/dvb/frontend.h>` — DVB (Digital Video Broadcasting) constants.
//!
//! DVB constants for digital TV frontend types,
//! delivery systems, and modulation schemes.

// ---------------------------------------------------------------------------
// Frontend types (deprecated, kept for compat)
// ---------------------------------------------------------------------------

/// QPSK frontend (satellite).
pub const FE_QPSK: u32 = 0;
/// QAM frontend (cable).
pub const FE_QAM: u32 = 1;
/// OFDM frontend (terrestrial).
pub const FE_OFDM: u32 = 2;
/// ATSC frontend.
pub const FE_ATSC: u32 = 3;

// ---------------------------------------------------------------------------
// Delivery systems (SYS_*)
// ---------------------------------------------------------------------------

/// Undefined.
pub const SYS_UNDEFINED: u32 = 0;
/// DVB-C Annex A.
pub const SYS_DVBC_ANNEX_A: u32 = 1;
/// DVB-C Annex B.
pub const SYS_DVBC_ANNEX_B: u32 = 2;
/// DVB-T.
pub const SYS_DVBT: u32 = 3;
/// DSS.
pub const SYS_DSS: u32 = 4;
/// DVB-S.
pub const SYS_DVBS: u32 = 5;
/// DVB-S2.
pub const SYS_DVBS2: u32 = 6;
/// DVB-H.
pub const SYS_DVBH: u32 = 7;
/// ISDB-T.
pub const SYS_ISDBT: u32 = 8;
/// ISDB-S.
pub const SYS_ISDBS: u32 = 9;
/// ISDB-C.
pub const SYS_ISDBC: u32 = 10;
/// ATSC.
pub const SYS_ATSC: u32 = 11;
/// ATSC-MH.
pub const SYS_ATSCMH: u32 = 12;
/// DTMB.
pub const SYS_DTMB: u32 = 13;
/// CMMB.
pub const SYS_CMMB: u32 = 14;
/// DAB.
pub const SYS_DAB: u32 = 15;
/// DVB-T2.
pub const SYS_DVBT2: u32 = 16;
/// Turbo FEC.
pub const SYS_TURBO: u32 = 17;
/// DVB-C Annex C.
pub const SYS_DVBC_ANNEX_C: u32 = 18;
/// DVB-S2X.
pub const SYS_DVBS2X: u32 = 19;

// ---------------------------------------------------------------------------
// Modulation schemes
// ---------------------------------------------------------------------------

/// QPSK.
pub const QPSK: u32 = 0;
/// QAM 16.
pub const QAM_16: u32 = 1;
/// QAM 32.
pub const QAM_32: u32 = 2;
/// QAM 64.
pub const QAM_64: u32 = 3;
/// QAM 128.
pub const QAM_128: u32 = 4;
/// QAM 256.
pub const QAM_256: u32 = 5;
/// QAM auto.
pub const QAM_AUTO: u32 = 6;
/// VSB 8.
pub const VSB_8: u32 = 7;
/// VSB 16.
pub const VSB_16: u32 = 8;
/// PSK 8.
pub const PSK_8: u32 = 9;
/// APSK 16.
pub const APSK_16: u32 = 10;
/// APSK 32.
pub const APSK_32: u32 = 11;
/// DQPSK.
pub const DQPSK: u32 = 12;
/// QAM 4 NR.
pub const QAM_4_NR: u32 = 13;

// ---------------------------------------------------------------------------
// Frontend capabilities
// ---------------------------------------------------------------------------

/// Can FEC auto.
pub const FE_CAN_FEC_AUTO: u32 = 0x200;
/// Can QPSK.
pub const FE_CAN_QPSK: u32 = 0x400;
/// Can QAM auto.
pub const FE_CAN_QAM_AUTO: u32 = 0x10000;
/// Can mute TS.
pub const FE_CAN_MUTE_TS: u32 = 0x80000000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontend_types_sequential() {
        assert_eq!(FE_QPSK, 0);
        assert_eq!(FE_QAM, 1);
        assert_eq!(FE_OFDM, 2);
        assert_eq!(FE_ATSC, 3);
    }

    #[test]
    fn test_delivery_systems_sequential() {
        assert_eq!(SYS_UNDEFINED, 0);
        assert_eq!(SYS_DVBC_ANNEX_A, 1);
        assert_eq!(SYS_DVBS2X, 19);
    }

    #[test]
    fn test_delivery_systems_distinct() {
        let systems = [
            SYS_UNDEFINED,
            SYS_DVBC_ANNEX_A,
            SYS_DVBC_ANNEX_B,
            SYS_DVBT,
            SYS_DSS,
            SYS_DVBS,
            SYS_DVBS2,
            SYS_DVBH,
            SYS_ISDBT,
            SYS_ISDBS,
            SYS_ISDBC,
            SYS_ATSC,
            SYS_ATSCMH,
            SYS_DTMB,
            SYS_CMMB,
            SYS_DAB,
            SYS_DVBT2,
            SYS_TURBO,
            SYS_DVBC_ANNEX_C,
            SYS_DVBS2X,
        ];
        for i in 0..systems.len() {
            for j in (i + 1)..systems.len() {
                assert_ne!(systems[i], systems[j]);
            }
        }
    }

    #[test]
    fn test_modulation_sequential() {
        assert_eq!(QPSK, 0);
        assert_eq!(QAM_16, 1);
        assert_eq!(QAM_4_NR, 13);
    }

    #[test]
    fn test_capabilities_distinct() {
        let caps = [
            FE_CAN_FEC_AUTO,
            FE_CAN_QPSK,
            FE_CAN_QAM_AUTO,
            FE_CAN_MUTE_TS,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }
}

//! `<linux/dvb/frontend.h>` + `<linux/dvb/dmx.h>` — Digital Video Broadcasting constants.
//!
//! The DVB subsystem handles digital TV reception (DVB-T/T2, DVB-S/S2,
//! DVB-C, ATSC, ISDB-T). Accessed via /dev/dvb/adapterN/{frontend,demux,dvr}.

// ---------------------------------------------------------------------------
// Frontend types (delivery systems)
// ---------------------------------------------------------------------------

/// Undefined.
pub const SYS_UNDEFINED: u32 = 0;
/// DVB-C Annex A (cable).
pub const SYS_DVBC_ANNEX_A: u32 = 1;
/// DVB-C Annex B.
pub const SYS_DVBC_ANNEX_B: u32 = 2;
/// DVB-T (terrestrial).
pub const SYS_DVBT: u32 = 3;
/// DSS (DigiCipher).
pub const SYS_DSS: u32 = 4;
/// DVB-S (satellite).
pub const SYS_DVBS: u32 = 5;
/// DVB-S2.
pub const SYS_DVBS2: u32 = 6;
/// DVB-H (handheld).
pub const SYS_DVBH: u32 = 7;
/// ISDB-T (Japanese terrestrial).
pub const SYS_ISDBT: u32 = 8;
/// ISDB-S.
pub const SYS_ISDBS: u32 = 9;
/// ISDB-C.
pub const SYS_ISDBC: u32 = 10;
/// ATSC (North American).
pub const SYS_ATSC: u32 = 11;
/// ATSC-MH (mobile).
pub const SYS_ATSCMH: u32 = 12;
/// DTMB (Chinese terrestrial).
pub const SYS_DTMB: u32 = 13;
/// CMMB (Chinese mobile multimedia).
pub const SYS_CMMB: u32 = 14;
/// DAB (Digital Audio Broadcasting).
pub const SYS_DAB: u32 = 15;
/// DVB-T2.
pub const SYS_DVBT2: u32 = 16;
/// Turbo FEC.
pub const SYS_TURBO: u32 = 17;
/// DVB-C Annex C.
pub const SYS_DVBC_ANNEX_C: u32 = 18;

// ---------------------------------------------------------------------------
// Frontend status flags
// ---------------------------------------------------------------------------

/// Has signal.
pub const FE_HAS_SIGNAL: u32 = 0x01;
/// Has carrier.
pub const FE_HAS_CARRIER: u32 = 0x02;
/// Has viterbi lock.
pub const FE_HAS_VITERBI: u32 = 0x04;
/// Has sync.
pub const FE_HAS_SYNC: u32 = 0x08;
/// Has lock (fully tuned).
pub const FE_HAS_LOCK: u32 = 0x10;
/// Timed out.
pub const FE_TIMEDOUT: u32 = 0x20;
/// Re-init.
pub const FE_REINIT: u32 = 0x40;

// ---------------------------------------------------------------------------
// Demux filter types
// ---------------------------------------------------------------------------

/// Section filter.
pub const DMX_FILTER_SECTION: u32 = 1;
/// PES filter.
pub const DMX_FILTER_PES: u32 = 2;

// ---------------------------------------------------------------------------
// Demux output types
// ---------------------------------------------------------------------------

/// Output to decoder.
pub const DMX_OUT_DECODER: u32 = 0;
/// Output to DVR device.
pub const DMX_OUT_TAP: u32 = 1;
/// Output to TS tap.
pub const DMX_OUT_TS_TAP: u32 = 2;
/// Output to tsdemux.
pub const DMX_OUT_TSDEMUX_TAP: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        ];
        for i in 0..systems.len() {
            for j in (i + 1)..systems.len() {
                assert_ne!(systems[i], systems[j]);
            }
        }
    }

    #[test]
    fn test_status_flags_are_powers_of_two() {
        let flags = [
            FE_HAS_SIGNAL,
            FE_HAS_CARRIER,
            FE_HAS_VITERBI,
            FE_HAS_SYNC,
            FE_HAS_LOCK,
            FE_TIMEDOUT,
            FE_REINIT,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_dmx_output_distinct() {
        let outs = [
            DMX_OUT_DECODER,
            DMX_OUT_TAP,
            DMX_OUT_TS_TAP,
            DMX_OUT_TSDEMUX_TAP,
        ];
        for i in 0..outs.len() {
            for j in (i + 1)..outs.len() {
                assert_ne!(outs[i], outs[j]);
            }
        }
    }

    #[test]
    fn test_filter_types() {
        assert_ne!(DMX_FILTER_SECTION, DMX_FILTER_PES);
    }
}

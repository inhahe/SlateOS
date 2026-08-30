//! `<linux/dvb/*>` — Digital Video Broadcasting frontend/demux ABI.
//!
//! TV-tuner stacks (tvheadend, dvblast, libdvbv5) use these constants
//! to drive `/dev/dvb/adapterN/frontendM`, `/dev/dvb/adapterN/demuxM`,
//! and the DVR ring. Status bits and PES filter types are also part
//! of the Live-TV pipeline in MythTV and Jellyfin.

// ---------------------------------------------------------------------------
// Device-node paths
// ---------------------------------------------------------------------------

/// Adapter directory base.
pub const DVB_DEV_DIR: &str = "/dev/dvb";
/// Frontend device basename.
pub const DVB_DEV_FRONTEND: &str = "frontend";
/// Demux device basename.
pub const DVB_DEV_DEMUX: &str = "demux";
/// DVR (digital video recorder) basename.
pub const DVB_DEV_DVR: &str = "dvr";
/// CA (conditional access) basename.
pub const DVB_DEV_CA: &str = "ca";
/// Network basename.
pub const DVB_DEV_NET: &str = "net";

// ---------------------------------------------------------------------------
// Frontend status bits (fe_status_t)
// ---------------------------------------------------------------------------

/// Signal present.
pub const FE_HAS_SIGNAL: u32 = 0x01;
/// Carrier detected.
pub const FE_HAS_CARRIER: u32 = 0x02;
/// FEC code locked.
pub const FE_HAS_VITERBI: u32 = 0x04;
/// Sync byte found.
pub const FE_HAS_SYNC: u32 = 0x08;
/// Full lock (typically the user-visible signal).
pub const FE_HAS_LOCK: u32 = 0x10;
/// Tuning timed out.
pub const FE_TIMEDOUT: u32 = 0x20;
/// Frontend lost lock since last poll.
pub const FE_REINIT: u32 = 0x40;

// ---------------------------------------------------------------------------
// Demux PES types
// ---------------------------------------------------------------------------

/// Audio PES filter.
pub const DMX_PES_AUDIO0: u32 = 0;
/// Video PES filter.
pub const DMX_PES_VIDEO0: u32 = 1;
/// Teletext PES filter.
pub const DMX_PES_TELETEXT0: u32 = 2;
/// Subtitle PES filter.
pub const DMX_PES_SUBTITLE0: u32 = 3;
/// PCR (program clock reference) filter.
pub const DMX_PES_PCR0: u32 = 4;

/// Default audio alias (audio0).
pub const DMX_PES_AUDIO: u32 = DMX_PES_AUDIO0;
/// Default video alias (video0).
pub const DMX_PES_VIDEO: u32 = DMX_PES_VIDEO0;

// ---------------------------------------------------------------------------
// Demux input/output (dmx_input_t / dmx_output_t)
// ---------------------------------------------------------------------------

/// Input from frontend.
pub const DMX_IN_FRONTEND: u32 = 0;
/// Input from DVR (replay).
pub const DMX_IN_DVR: u32 = 1;

/// Discard output.
pub const DMX_OUT_DECODER: u32 = 0;
/// Output to userland.
pub const DMX_OUT_TAP: u32 = 1;
/// Output to TS-format tap.
pub const DMX_OUT_TS_TAP: u32 = 2;
/// TS demux tap.
pub const DMX_OUT_TSDEMUX_TAP: u32 = 3;

// ---------------------------------------------------------------------------
// Filter buffer size limits
// ---------------------------------------------------------------------------

/// Maximum filter size (PSI/SI section).
pub const DMX_FILTER_SIZE: u32 = 16;
/// Maximum bytes per section read.
pub const DMX_MAX_FILTER_SIZE: u32 = 18;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_paths() {
        assert!(DVB_DEV_DIR.starts_with("/dev/"));
        let n = [
            DVB_DEV_FRONTEND,
            DVB_DEV_DEMUX,
            DVB_DEV_DVR,
            DVB_DEV_CA,
            DVB_DEV_NET,
        ];
        for i in 0..n.len() {
            for j in (i + 1)..n.len() {
                assert_ne!(n[i], n[j]);
            }
        }
    }

    #[test]
    fn test_fe_status_bits_pow2() {
        // Frontend status is a bitmask — every bit must be a power of
        // two so callers can OR them together cleanly.
        for &b in &[
            FE_HAS_SIGNAL,
            FE_HAS_CARRIER,
            FE_HAS_VITERBI,
            FE_HAS_SYNC,
            FE_HAS_LOCK,
            FE_TIMEDOUT,
            FE_REINIT,
        ] {
            assert!(b.is_power_of_two());
        }
        // LOCK == 0x10 is what userspace polls for tuner-ready.
        assert_eq!(FE_HAS_LOCK, 0x10);
    }

    #[test]
    fn test_pes_types_dense() {
        let p = [
            DMX_PES_AUDIO0,
            DMX_PES_VIDEO0,
            DMX_PES_TELETEXT0,
            DMX_PES_SUBTITLE0,
            DMX_PES_PCR0,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // Aliases must map to the index-0 variants.
        assert_eq!(DMX_PES_AUDIO, DMX_PES_AUDIO0);
        assert_eq!(DMX_PES_VIDEO, DMX_PES_VIDEO0);
    }

    #[test]
    fn test_input_output_distinct() {
        assert_ne!(DMX_IN_FRONTEND, DMX_IN_DVR);
        let o = [
            DMX_OUT_DECODER,
            DMX_OUT_TAP,
            DMX_OUT_TS_TAP,
            DMX_OUT_TSDEMUX_TAP,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_filter_buffer_limits() {
        // PSI section filter is 16 bytes of pattern + 2 byte mask
        // extension; cannot change without ABI breakage.
        assert_eq!(DMX_FILTER_SIZE, 16);
        assert!(DMX_MAX_FILTER_SIZE >= DMX_FILTER_SIZE);
    }
}

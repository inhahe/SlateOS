//! `<alsa/mixer.h>` — high-level mixer-element selectors and names.
//!
//! The ALSA mixer abstraction (above the raw control API) groups
//! controls into "simple elements" identified by a canonical name plus
//! an enumerated channel ID. These identifiers are stable enough to
//! be referenced from configuration files and CLI tools (`amixer`).

// ---------------------------------------------------------------------------
// `snd_mixer_selem_channel_id_t` — channel selectors
// ---------------------------------------------------------------------------

pub const SND_MIXER_SCHN_UNKNOWN: i32 = -1;
pub const SND_MIXER_SCHN_FRONT_LEFT: i32 = 0;
pub const SND_MIXER_SCHN_FRONT_RIGHT: i32 = 1;
pub const SND_MIXER_SCHN_REAR_LEFT: i32 = 2;
pub const SND_MIXER_SCHN_REAR_RIGHT: i32 = 3;
pub const SND_MIXER_SCHN_FRONT_CENTER: i32 = 4;
pub const SND_MIXER_SCHN_WOOFER: i32 = 5;
pub const SND_MIXER_SCHN_SIDE_LEFT: i32 = 6;
pub const SND_MIXER_SCHN_SIDE_RIGHT: i32 = 7;
pub const SND_MIXER_SCHN_REAR_CENTER: i32 = 8;
pub const SND_MIXER_SCHN_LAST: i32 = 31;
pub const SND_MIXER_SCHN_MONO: i32 = SND_MIXER_SCHN_FRONT_LEFT;

// ---------------------------------------------------------------------------
// Canonical simple-element names (case-sensitive ASCII)
// ---------------------------------------------------------------------------

pub const SELEM_NAME_MASTER: &str = "Master";
pub const SELEM_NAME_PCM: &str = "PCM";
pub const SELEM_NAME_HEADPHONE: &str = "Headphone";
pub const SELEM_NAME_SPEAKER: &str = "Speaker";
pub const SELEM_NAME_MIC: &str = "Mic";
pub const SELEM_NAME_CAPTURE: &str = "Capture";
pub const SELEM_NAME_LINE: &str = "Line";
pub const SELEM_NAME_DIGITAL: &str = "Digital";
pub const SELEM_NAME_MIC_BOOST: &str = "Mic Boost";
pub const SELEM_NAME_AUTO_MUTE: &str = "Auto-Mute Mode";

// ---------------------------------------------------------------------------
// dB-scale conventions — `SNDRV_CTL_TLVD_*`
// ---------------------------------------------------------------------------

/// Mute sentinel returned by `snd_mixer_selem_get_*_dB`: -∞ dB.
pub const SND_MIXER_DB_MIN_SENTINEL: i32 = -9_999_999;

/// dB values are encoded as hundredths of a decibel (centibels).
pub const SND_MIXER_DB_SCALE: i32 = 100;

// ---------------------------------------------------------------------------
// Channel count constants
// ---------------------------------------------------------------------------

pub const SND_MIXER_CHN_COUNT_STEREO: u32 = 2;
pub const SND_MIXER_CHN_COUNT_5_1: u32 = 6;
pub const SND_MIXER_CHN_COUNT_7_1: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channels_dense_0_to_8() {
        let c = [
            SND_MIXER_SCHN_FRONT_LEFT,
            SND_MIXER_SCHN_FRONT_RIGHT,
            SND_MIXER_SCHN_REAR_LEFT,
            SND_MIXER_SCHN_REAR_RIGHT,
            SND_MIXER_SCHN_FRONT_CENTER,
            SND_MIXER_SCHN_WOOFER,
            SND_MIXER_SCHN_SIDE_LEFT,
            SND_MIXER_SCHN_SIDE_RIGHT,
            SND_MIXER_SCHN_REAR_CENTER,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_unknown_and_mono_aliases() {
        assert_eq!(SND_MIXER_SCHN_UNKNOWN, -1);
        // Mono channel maps to the left channel for libasound APIs.
        assert_eq!(SND_MIXER_SCHN_MONO, SND_MIXER_SCHN_FRONT_LEFT);
        assert_eq!(SND_MIXER_SCHN_LAST, 31);
    }

    #[test]
    fn test_selem_names_distinct() {
        let n = [
            SELEM_NAME_MASTER,
            SELEM_NAME_PCM,
            SELEM_NAME_HEADPHONE,
            SELEM_NAME_SPEAKER,
            SELEM_NAME_MIC,
            SELEM_NAME_CAPTURE,
            SELEM_NAME_LINE,
            SELEM_NAME_DIGITAL,
            SELEM_NAME_MIC_BOOST,
            SELEM_NAME_AUTO_MUTE,
        ];
        // Pairwise distinct.
        for (i, &a) in n.iter().enumerate() {
            for &b in &n[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // No name is empty.
        for s in n {
            assert!(!s.is_empty());
        }
    }

    #[test]
    fn test_db_min_sentinel_very_negative() {
        // Sentinel must be unmistakable as a real dB value.
        assert!(SND_MIXER_DB_MIN_SENTINEL <= -99_999);
        assert_eq!(SND_MIXER_DB_SCALE, 100);
    }

    #[test]
    fn test_channel_count_constants_in_order() {
        assert!(SND_MIXER_CHN_COUNT_STEREO < SND_MIXER_CHN_COUNT_5_1);
        assert!(SND_MIXER_CHN_COUNT_5_1 < SND_MIXER_CHN_COUNT_7_1);
        assert_eq!(SND_MIXER_CHN_COUNT_STEREO, 2);
        assert_eq!(SND_MIXER_CHN_COUNT_5_1, 6);
        assert_eq!(SND_MIXER_CHN_COUNT_7_1, 8);
    }
}

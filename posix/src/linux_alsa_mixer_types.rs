//! `<sound/asound.h>` (mixer subset) — ALSA mixer channel constants.
//!
//! The ALSA mixer maps physical audio controls to logical channel
//! names. Channels identify which audio path a control affects:
//! front speakers, rear speakers, center, subwoofer, headphones, etc.
//! The mixer abstraction layer (in alsa-lib) combines related controls
//! into "simple mixer elements" for easier userspace consumption.

// ---------------------------------------------------------------------------
// Mixer channel positions (speaker map)
// ---------------------------------------------------------------------------

/// Front left.
pub const SNDRV_MIXER_CHANNEL_FL: u32 = 0;
/// Front right.
pub const SNDRV_MIXER_CHANNEL_FR: u32 = 1;
/// Rear left (surround left).
pub const SNDRV_MIXER_CHANNEL_RL: u32 = 2;
/// Rear right (surround right).
pub const SNDRV_MIXER_CHANNEL_RR: u32 = 3;
/// Front center.
pub const SNDRV_MIXER_CHANNEL_FC: u32 = 4;
/// LFE / subwoofer.
pub const SNDRV_MIXER_CHANNEL_LFE: u32 = 5;
/// Side left.
pub const SNDRV_MIXER_CHANNEL_SL: u32 = 6;
/// Side right.
pub const SNDRV_MIXER_CHANNEL_SR: u32 = 7;
/// Rear center.
pub const SNDRV_MIXER_CHANNEL_RC: u32 = 8;
/// Mono (single channel).
pub const SNDRV_MIXER_CHANNEL_MONO: u32 = 0;

// ---------------------------------------------------------------------------
// Common channel configurations
// ---------------------------------------------------------------------------

/// Mono (1 channel).
pub const SNDRV_MIXER_CONFIG_MONO: u32 = 1;
/// Stereo (2 channels).
pub const SNDRV_MIXER_CONFIG_STEREO: u32 = 2;
/// 2.1 (stereo + sub).
pub const SNDRV_MIXER_CONFIG_2_1: u32 = 3;
/// Quadraphonic (4 channels).
pub const SNDRV_MIXER_CONFIG_QUAD: u32 = 4;
/// 5.1 surround.
pub const SNDRV_MIXER_CONFIG_5_1: u32 = 6;
/// 7.1 surround.
pub const SNDRV_MIXER_CONFIG_7_1: u32 = 8;

// ---------------------------------------------------------------------------
// Volume dB scale
// ---------------------------------------------------------------------------

/// Minimum volume (mute, -infinity dB).
pub const SNDRV_MIXER_VOL_MUTE: i32 = -9999999;
/// 0 dB reference level.
pub const SNDRV_MIXER_VOL_0DB: i32 = 0;
/// TLV type for dB linear scale.
pub const SNDRV_CTL_TLV_DB_LINEAR: u32 = 1;
/// TLV type for dB scale (step-based).
pub const SNDRV_CTL_TLV_DB_SCALE: u32 = 2;
/// TLV type for dB range.
pub const SNDRV_CTL_TLV_DB_RANGE: u32 = 3;
/// TLV type for dB minmax.
pub const SNDRV_CTL_TLV_DB_MINMAX: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channels_distinct() {
        // FL and MONO alias to 0, but the named surround channels are distinct
        let channels = [
            SNDRV_MIXER_CHANNEL_FR, SNDRV_MIXER_CHANNEL_RL,
            SNDRV_MIXER_CHANNEL_RR, SNDRV_MIXER_CHANNEL_FC,
            SNDRV_MIXER_CHANNEL_LFE, SNDRV_MIXER_CHANNEL_SL,
            SNDRV_MIXER_CHANNEL_SR, SNDRV_MIXER_CHANNEL_RC,
        ];
        for i in 0..channels.len() {
            for j in (i + 1)..channels.len() {
                assert_ne!(channels[i], channels[j]);
            }
        }
    }

    #[test]
    fn test_configs_ordered() {
        assert!(SNDRV_MIXER_CONFIG_MONO < SNDRV_MIXER_CONFIG_STEREO);
        assert!(SNDRV_MIXER_CONFIG_STEREO < SNDRV_MIXER_CONFIG_5_1);
        assert!(SNDRV_MIXER_CONFIG_5_1 < SNDRV_MIXER_CONFIG_7_1);
    }

    #[test]
    fn test_volume_scale() {
        assert!(SNDRV_MIXER_VOL_MUTE < SNDRV_MIXER_VOL_0DB);
    }

    #[test]
    fn test_tlv_types_distinct() {
        let types = [
            SNDRV_CTL_TLV_DB_LINEAR, SNDRV_CTL_TLV_DB_SCALE,
            SNDRV_CTL_TLV_DB_RANGE, SNDRV_CTL_TLV_DB_MINMAX,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}

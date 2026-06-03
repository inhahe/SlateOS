//! `<sound/asound.h>` — ALSA PCM sample format constants.
//!
//! PCM formats describe the encoding of audio samples: bit depth,
//! signedness, endianness, and whether samples are integer or float.
//! Hardware and software converters use these to set up the audio
//! pipeline correctly.

// ---------------------------------------------------------------------------
// PCM sample formats (snd_pcm_format_t)
// ---------------------------------------------------------------------------

/// Signed 8-bit.
pub const SNDRV_PCM_FORMAT_S8: i32 = 0;
/// Unsigned 8-bit.
pub const SNDRV_PCM_FORMAT_U8: i32 = 1;
/// Signed 16-bit little-endian.
pub const SNDRV_PCM_FORMAT_S16_LE: i32 = 2;
/// Signed 16-bit big-endian.
pub const SNDRV_PCM_FORMAT_S16_BE: i32 = 3;
/// Unsigned 16-bit little-endian.
pub const SNDRV_PCM_FORMAT_U16_LE: i32 = 4;
/// Unsigned 16-bit big-endian.
pub const SNDRV_PCM_FORMAT_U16_BE: i32 = 5;
/// Signed 24-bit little-endian (in 4-byte container).
pub const SNDRV_PCM_FORMAT_S24_LE: i32 = 6;
/// Signed 24-bit big-endian (in 4-byte container).
pub const SNDRV_PCM_FORMAT_S24_BE: i32 = 7;
/// Unsigned 24-bit little-endian (in 4-byte container).
pub const SNDRV_PCM_FORMAT_U24_LE: i32 = 8;
/// Unsigned 24-bit big-endian (in 4-byte container).
pub const SNDRV_PCM_FORMAT_U24_BE: i32 = 9;
/// Signed 32-bit little-endian.
pub const SNDRV_PCM_FORMAT_S32_LE: i32 = 10;
/// Signed 32-bit big-endian.
pub const SNDRV_PCM_FORMAT_S32_BE: i32 = 11;
/// Unsigned 32-bit little-endian.
pub const SNDRV_PCM_FORMAT_U32_LE: i32 = 12;
/// Unsigned 32-bit big-endian.
pub const SNDRV_PCM_FORMAT_U32_BE: i32 = 13;
/// IEEE 32-bit float little-endian.
pub const SNDRV_PCM_FORMAT_FLOAT_LE: i32 = 14;
/// IEEE 32-bit float big-endian.
pub const SNDRV_PCM_FORMAT_FLOAT_BE: i32 = 15;
/// IEEE 64-bit float little-endian.
pub const SNDRV_PCM_FORMAT_FLOAT64_LE: i32 = 16;
/// IEEE 64-bit float big-endian.
pub const SNDRV_PCM_FORMAT_FLOAT64_BE: i32 = 17;
/// IEC 958 subframe little-endian.
pub const SNDRV_PCM_FORMAT_IEC958_SUBFRAME_LE: i32 = 18;
/// IEC 958 subframe big-endian.
pub const SNDRV_PCM_FORMAT_IEC958_SUBFRAME_BE: i32 = 19;
/// µ-law compressed.
pub const SNDRV_PCM_FORMAT_MU_LAW: i32 = 20;
/// A-law compressed.
pub const SNDRV_PCM_FORMAT_A_LAW: i32 = 21;
/// Signed 24-bit packed (3 bytes per sample).
pub const SNDRV_PCM_FORMAT_S24_3LE: i32 = 32;
/// Signed 24-bit packed big-endian.
pub const SNDRV_PCM_FORMAT_S24_3BE: i32 = 33;
/// Signed 20-bit packed (3 bytes per sample).
pub const SNDRV_PCM_FORMAT_S20_3LE: i32 = 38;
/// Signed 20-bit packed big-endian.
pub const SNDRV_PCM_FORMAT_S20_3BE: i32 = 39;

// ---------------------------------------------------------------------------
// Common format aliases for native endian (little-endian x86)
// ---------------------------------------------------------------------------

/// Native 16-bit signed (LE on x86).
pub const SNDRV_PCM_FORMAT_S16: i32 = SNDRV_PCM_FORMAT_S16_LE;
/// Native 32-bit signed (LE on x86).
pub const SNDRV_PCM_FORMAT_S32: i32 = SNDRV_PCM_FORMAT_S32_LE;
/// Native 32-bit float (LE on x86).
pub const SNDRV_PCM_FORMAT_FLOAT: i32 = SNDRV_PCM_FORMAT_FLOAT_LE;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_formats_distinct() {
        let fmts = [
            SNDRV_PCM_FORMAT_S8,
            SNDRV_PCM_FORMAT_U8,
            SNDRV_PCM_FORMAT_S16_LE,
            SNDRV_PCM_FORMAT_S16_BE,
            SNDRV_PCM_FORMAT_U16_LE,
            SNDRV_PCM_FORMAT_U16_BE,
            SNDRV_PCM_FORMAT_S24_LE,
            SNDRV_PCM_FORMAT_S24_BE,
            SNDRV_PCM_FORMAT_U24_LE,
            SNDRV_PCM_FORMAT_U24_BE,
            SNDRV_PCM_FORMAT_S32_LE,
            SNDRV_PCM_FORMAT_S32_BE,
            SNDRV_PCM_FORMAT_U32_LE,
            SNDRV_PCM_FORMAT_U32_BE,
            SNDRV_PCM_FORMAT_FLOAT_LE,
            SNDRV_PCM_FORMAT_FLOAT_BE,
            SNDRV_PCM_FORMAT_FLOAT64_LE,
            SNDRV_PCM_FORMAT_FLOAT64_BE,
            SNDRV_PCM_FORMAT_IEC958_SUBFRAME_LE,
            SNDRV_PCM_FORMAT_IEC958_SUBFRAME_BE,
            SNDRV_PCM_FORMAT_MU_LAW,
            SNDRV_PCM_FORMAT_A_LAW,
            SNDRV_PCM_FORMAT_S24_3LE,
            SNDRV_PCM_FORMAT_S24_3BE,
            SNDRV_PCM_FORMAT_S20_3LE,
            SNDRV_PCM_FORMAT_S20_3BE,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_native_aliases() {
        assert_eq!(SNDRV_PCM_FORMAT_S16, SNDRV_PCM_FORMAT_S16_LE);
        assert_eq!(SNDRV_PCM_FORMAT_S32, SNDRV_PCM_FORMAT_S32_LE);
        assert_eq!(SNDRV_PCM_FORMAT_FLOAT, SNDRV_PCM_FORMAT_FLOAT_LE);
    }

    #[test]
    fn test_s8_is_zero() {
        assert_eq!(SNDRV_PCM_FORMAT_S8, 0);
    }
}

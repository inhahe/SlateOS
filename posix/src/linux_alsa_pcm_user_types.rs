//! `<sound/asound.h>` — ALSA PCM sample formats, access modes, and subformats.
//!
//! These constants describe how PCM audio bytes are laid out in
//! buffers and how userspace requests playback/capture access. They
//! are part of the wire ABI for `hw_params` ioctls.

// ---------------------------------------------------------------------------
// PCM access modes (`snd_pcm_access_t`)
// ---------------------------------------------------------------------------

pub const SNDRV_PCM_ACCESS_MMAP_INTERLEAVED: u32 = 0;
pub const SNDRV_PCM_ACCESS_MMAP_NONINTERLEAVED: u32 = 1;
pub const SNDRV_PCM_ACCESS_MMAP_COMPLEX: u32 = 2;
pub const SNDRV_PCM_ACCESS_RW_INTERLEAVED: u32 = 3;
pub const SNDRV_PCM_ACCESS_RW_NONINTERLEAVED: u32 = 4;
pub const SNDRV_PCM_ACCESS_LAST: u32 = SNDRV_PCM_ACCESS_RW_NONINTERLEAVED;

// ---------------------------------------------------------------------------
// PCM sample formats (`snd_pcm_format_t`) — selected popular values
// ---------------------------------------------------------------------------

pub const SNDRV_PCM_FORMAT_S8: i32 = 0;
pub const SNDRV_PCM_FORMAT_U8: i32 = 1;
pub const SNDRV_PCM_FORMAT_S16_LE: i32 = 2;
pub const SNDRV_PCM_FORMAT_S16_BE: i32 = 3;
pub const SNDRV_PCM_FORMAT_U16_LE: i32 = 4;
pub const SNDRV_PCM_FORMAT_U16_BE: i32 = 5;
pub const SNDRV_PCM_FORMAT_S24_LE: i32 = 6;
pub const SNDRV_PCM_FORMAT_S24_BE: i32 = 7;
pub const SNDRV_PCM_FORMAT_U24_LE: i32 = 8;
pub const SNDRV_PCM_FORMAT_U24_BE: i32 = 9;
pub const SNDRV_PCM_FORMAT_S32_LE: i32 = 10;
pub const SNDRV_PCM_FORMAT_S32_BE: i32 = 11;
pub const SNDRV_PCM_FORMAT_U32_LE: i32 = 12;
pub const SNDRV_PCM_FORMAT_U32_BE: i32 = 13;
pub const SNDRV_PCM_FORMAT_FLOAT_LE: i32 = 14;
pub const SNDRV_PCM_FORMAT_FLOAT_BE: i32 = 15;
pub const SNDRV_PCM_FORMAT_FLOAT64_LE: i32 = 16;
pub const SNDRV_PCM_FORMAT_FLOAT64_BE: i32 = 17;

/// Sentinel for "any/unknown format" returned by drivers.
pub const SNDRV_PCM_FORMAT_UNKNOWN: i32 = -1;

// ---------------------------------------------------------------------------
// PCM subformat (`snd_pcm_subformat_t`) — only one defined today
// ---------------------------------------------------------------------------

pub const SNDRV_PCM_SUBFORMAT_STD: u32 = 0;
pub const SNDRV_PCM_SUBFORMAT_LAST: u32 = SNDRV_PCM_SUBFORMAT_STD;

// ---------------------------------------------------------------------------
// Common sample-rate constants (Hz)
// ---------------------------------------------------------------------------

pub const SNDRV_PCM_RATE_8000: u32 = 8_000;
pub const SNDRV_PCM_RATE_16000: u32 = 16_000;
pub const SNDRV_PCM_RATE_22050: u32 = 22_050;
pub const SNDRV_PCM_RATE_44100: u32 = 44_100;
pub const SNDRV_PCM_RATE_48000: u32 = 48_000;
pub const SNDRV_PCM_RATE_88200: u32 = 88_200;
pub const SNDRV_PCM_RATE_96000: u32 = 96_000;
pub const SNDRV_PCM_RATE_176400: u32 = 176_400;
pub const SNDRV_PCM_RATE_192000: u32 = 192_000;
pub const SNDRV_PCM_RATE_384000: u32 = 384_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_modes_dense_0_to_4() {
        let a = [
            SNDRV_PCM_ACCESS_MMAP_INTERLEAVED,
            SNDRV_PCM_ACCESS_MMAP_NONINTERLEAVED,
            SNDRV_PCM_ACCESS_MMAP_COMPLEX,
            SNDRV_PCM_ACCESS_RW_INTERLEAVED,
            SNDRV_PCM_ACCESS_RW_NONINTERLEAVED,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(SNDRV_PCM_ACCESS_LAST, 4);
    }

    #[test]
    fn test_formats_dense_0_to_17() {
        let f = [
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
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(SNDRV_PCM_FORMAT_UNKNOWN, -1);
    }

    #[test]
    fn test_le_be_pairs_consecutive() {
        // For every endian-dependent format, BE = LE + 1.
        assert_eq!(SNDRV_PCM_FORMAT_S16_BE, SNDRV_PCM_FORMAT_S16_LE + 1);
        assert_eq!(SNDRV_PCM_FORMAT_U16_BE, SNDRV_PCM_FORMAT_U16_LE + 1);
        assert_eq!(SNDRV_PCM_FORMAT_S24_BE, SNDRV_PCM_FORMAT_S24_LE + 1);
        assert_eq!(SNDRV_PCM_FORMAT_U24_BE, SNDRV_PCM_FORMAT_U24_LE + 1);
        assert_eq!(SNDRV_PCM_FORMAT_S32_BE, SNDRV_PCM_FORMAT_S32_LE + 1);
        assert_eq!(SNDRV_PCM_FORMAT_U32_BE, SNDRV_PCM_FORMAT_U32_LE + 1);
        assert_eq!(SNDRV_PCM_FORMAT_FLOAT_BE, SNDRV_PCM_FORMAT_FLOAT_LE + 1);
        assert_eq!(SNDRV_PCM_FORMAT_FLOAT64_BE, SNDRV_PCM_FORMAT_FLOAT64_LE + 1);
    }

    #[test]
    fn test_subformat_only_std_today() {
        assert_eq!(SNDRV_PCM_SUBFORMAT_STD, 0);
        assert_eq!(SNDRV_PCM_SUBFORMAT_LAST, SNDRV_PCM_SUBFORMAT_STD);
    }

    #[test]
    fn test_rates_strictly_increasing() {
        let r = [
            SNDRV_PCM_RATE_8000,
            SNDRV_PCM_RATE_16000,
            SNDRV_PCM_RATE_22050,
            SNDRV_PCM_RATE_44100,
            SNDRV_PCM_RATE_48000,
            SNDRV_PCM_RATE_88200,
            SNDRV_PCM_RATE_96000,
            SNDRV_PCM_RATE_176400,
            SNDRV_PCM_RATE_192000,
            SNDRV_PCM_RATE_384000,
        ];
        for w in r.windows(2) {
            assert!(w[0] < w[1]);
        }
        // Double-rate relationships hold within the 44.1 and 48 kHz families.
        assert_eq!(SNDRV_PCM_RATE_88200, SNDRV_PCM_RATE_44100 * 2);
        assert_eq!(SNDRV_PCM_RATE_96000, SNDRV_PCM_RATE_48000 * 2);
        assert_eq!(SNDRV_PCM_RATE_176400, SNDRV_PCM_RATE_44100 * 4);
        assert_eq!(SNDRV_PCM_RATE_192000, SNDRV_PCM_RATE_48000 * 4);
        assert_eq!(SNDRV_PCM_RATE_384000, SNDRV_PCM_RATE_48000 * 8);
    }
}

//! `<linux/soundcard.h>` / ALSA — Sound subsystem constants.
//!
//! Linux audio is handled by ALSA (Advanced Linux Sound Architecture).
//! ALSA provides PCM (digital audio), mixer (volume controls), MIDI,
//! sequencer, and timer interfaces. Applications access audio through
//! /dev/snd/pcmC*D*p (playback) and /dev/snd/pcmC*D*c (capture) devices.

// ---------------------------------------------------------------------------
// ALSA PCM formats (snd_pcm_format_t)
// ---------------------------------------------------------------------------

/// Signed 8-bit.
pub const SNDRV_PCM_FORMAT_S8: u32 = 0;
/// Unsigned 8-bit.
pub const SNDRV_PCM_FORMAT_U8: u32 = 1;
/// Signed 16-bit little-endian.
pub const SNDRV_PCM_FORMAT_S16_LE: u32 = 2;
/// Signed 16-bit big-endian.
pub const SNDRV_PCM_FORMAT_S16_BE: u32 = 3;
/// Signed 24-bit LE (in 32-bit frame).
pub const SNDRV_PCM_FORMAT_S24_LE: u32 = 6;
/// Signed 32-bit little-endian.
pub const SNDRV_PCM_FORMAT_S32_LE: u32 = 10;
/// 32-bit IEEE float LE.
pub const SNDRV_PCM_FORMAT_FLOAT_LE: u32 = 14;

// ---------------------------------------------------------------------------
// ALSA PCM access types
// ---------------------------------------------------------------------------

/// Interleaved (channels mixed: L R L R ...).
pub const SNDRV_PCM_ACCESS_MMAP_INTERLEAVED: u32 = 0;
/// Non-interleaved (channels separate: LLLL... RRRR...).
pub const SNDRV_PCM_ACCESS_MMAP_NONINTERLEAVED: u32 = 1;
/// Interleaved read/write.
pub const SNDRV_PCM_ACCESS_RW_INTERLEAVED: u32 = 3;
/// Non-interleaved read/write.
pub const SNDRV_PCM_ACCESS_RW_NONINTERLEAVED: u32 = 4;

// ---------------------------------------------------------------------------
// PCM stream directions
// ---------------------------------------------------------------------------

/// Playback stream.
pub const SNDRV_PCM_STREAM_PLAYBACK: u32 = 0;
/// Capture stream.
pub const SNDRV_PCM_STREAM_CAPTURE: u32 = 1;

// ---------------------------------------------------------------------------
// PCM states
// ---------------------------------------------------------------------------

/// Stream is open, not configured.
pub const SNDRV_PCM_STATE_OPEN: u32 = 0;
/// Hardware params set.
pub const SNDRV_PCM_STATE_SETUP: u32 = 1;
/// Stream is prepared (ready to start).
pub const SNDRV_PCM_STATE_PREPARED: u32 = 2;
/// Stream is running.
pub const SNDRV_PCM_STATE_RUNNING: u32 = 3;
/// Buffer underrun (playback) or overrun (capture).
pub const SNDRV_PCM_STATE_XRUN: u32 = 4;
/// Stream is draining (finishing playback).
pub const SNDRV_PCM_STATE_DRAINING: u32 = 5;
/// Stream is paused.
pub const SNDRV_PCM_STATE_PAUSED: u32 = 6;
/// Stream is suspended (power save).
pub const SNDRV_PCM_STATE_SUSPENDED: u32 = 7;

// ---------------------------------------------------------------------------
// Common sample rates (Hz)
// ---------------------------------------------------------------------------

/// CD quality.
pub const SAMPLE_RATE_44100: u32 = 44100;
/// DVD/DAT quality.
pub const SAMPLE_RATE_48000: u32 = 48000;
/// High-res audio.
pub const SAMPLE_RATE_96000: u32 = 96000;
/// Ultra high-res.
pub const SAMPLE_RATE_192000: u32 = 192000;

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
            SNDRV_PCM_FORMAT_S24_LE,
            SNDRV_PCM_FORMAT_S32_LE,
            SNDRV_PCM_FORMAT_FLOAT_LE,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_access_types_distinct() {
        let types = [
            SNDRV_PCM_ACCESS_MMAP_INTERLEAVED,
            SNDRV_PCM_ACCESS_MMAP_NONINTERLEAVED,
            SNDRV_PCM_ACCESS_RW_INTERLEAVED,
            SNDRV_PCM_ACCESS_RW_NONINTERLEAVED,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_stream_directions_distinct() {
        assert_ne!(SNDRV_PCM_STREAM_PLAYBACK, SNDRV_PCM_STREAM_CAPTURE);
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            SNDRV_PCM_STATE_OPEN,
            SNDRV_PCM_STATE_SETUP,
            SNDRV_PCM_STATE_PREPARED,
            SNDRV_PCM_STATE_RUNNING,
            SNDRV_PCM_STATE_XRUN,
            SNDRV_PCM_STATE_DRAINING,
            SNDRV_PCM_STATE_PAUSED,
            SNDRV_PCM_STATE_SUSPENDED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_sample_rates_ascending() {
        assert!(SAMPLE_RATE_44100 < SAMPLE_RATE_48000);
        assert!(SAMPLE_RATE_48000 < SAMPLE_RATE_96000);
        assert!(SAMPLE_RATE_96000 < SAMPLE_RATE_192000);
    }
}

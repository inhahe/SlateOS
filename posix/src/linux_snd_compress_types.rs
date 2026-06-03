//! `<sound/compress_offload.h>` — ALSA compressed audio offload constants.
//!
//! Compressed audio offload allows hardware DSPs to decode compressed
//! audio streams (MP3, AAC, FLAC, etc.) without CPU involvement.
//! The application sends compressed data to the DSP, which decodes
//! and renders directly to the audio output. This saves significant
//! CPU power on mobile/embedded devices. The API provides stream
//! creation, buffer management, codec configuration, and gapless
//! playback support.

// ---------------------------------------------------------------------------
// Compressed stream IOCTLs
// ---------------------------------------------------------------------------

/// Get compressed stream capabilities.
pub const SNDRV_COMPRESS_GET_CAPS: u32 = 0x10;
/// Get codec capabilities.
pub const SNDRV_COMPRESS_GET_CODEC_CAPS: u32 = 0x11;
/// Set parameters (codec, sample rate, channels, etc.).
pub const SNDRV_COMPRESS_SET_PARAMS: u32 = 0x12;
/// Get parameters.
pub const SNDRV_COMPRESS_GET_PARAMS: u32 = 0x13;
/// Set metadata (gapless info, etc.).
pub const SNDRV_COMPRESS_SET_METADATA: u32 = 0x14;
/// Get metadata.
pub const SNDRV_COMPRESS_GET_METADATA: u32 = 0x15;
/// Start the stream.
pub const SNDRV_COMPRESS_START: u32 = 0x20;
/// Stop the stream.
pub const SNDRV_COMPRESS_STOP: u32 = 0x21;
/// Pause the stream.
pub const SNDRV_COMPRESS_PAUSE: u32 = 0x22;
/// Resume the stream.
pub const SNDRV_COMPRESS_RESUME: u32 = 0x23;
/// Drain (play remaining buffered data).
pub const SNDRV_COMPRESS_DRAIN: u32 = 0x24;
/// Partial drain (gapless transition).
pub const SNDRV_COMPRESS_PARTIAL_DRAIN: u32 = 0x25;
/// Get timestamp (current playback position).
pub const SNDRV_COMPRESS_TSTAMP: u32 = 0x30;
/// Get available buffer space.
pub const SNDRV_COMPRESS_AVAIL: u32 = 0x31;

// ---------------------------------------------------------------------------
// Compressed stream states
// ---------------------------------------------------------------------------

/// Stream is open (not yet configured).
pub const SNDRV_COMPRESS_STATE_OPEN: u32 = 0;
/// Stream parameters are set.
pub const SNDRV_COMPRESS_STATE_SETUP: u32 = 1;
/// Stream is prepared (buffers allocated).
pub const SNDRV_COMPRESS_STATE_PREPARED: u32 = 2;
/// Stream is running (playback/capture active).
pub const SNDRV_COMPRESS_STATE_RUNNING: u32 = 3;
/// Stream is paused.
pub const SNDRV_COMPRESS_STATE_PAUSED: u32 = 4;
/// Stream is draining.
pub const SNDRV_COMPRESS_STATE_DRAINING: u32 = 5;

// ---------------------------------------------------------------------------
// Compressed codec IDs
// ---------------------------------------------------------------------------

/// PCM (uncompressed, for passthrough).
pub const SND_AUDIOCODEC_PCM: u32 = 0x0001_0000;
/// MP3.
pub const SND_AUDIOCODEC_MP3: u32 = 0x0002_0000;
/// AAC.
pub const SND_AUDIOCODEC_AAC: u32 = 0x0003_0000;
/// WMA.
pub const SND_AUDIOCODEC_WMA: u32 = 0x0004_0000;
/// Vorbis.
pub const SND_AUDIOCODEC_VORBIS: u32 = 0x0005_0000;
/// FLAC.
pub const SND_AUDIOCODEC_FLAC: u32 = 0x0006_0000;
/// ALAC (Apple Lossless).
pub const SND_AUDIOCODEC_ALAC: u32 = 0x0007_0000;
/// APE (Monkey's Audio).
pub const SND_AUDIOCODEC_APE: u32 = 0x0008_0000;
/// Opus.
pub const SND_AUDIOCODEC_OPUS: u32 = 0x000A_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            SNDRV_COMPRESS_GET_CAPS,
            SNDRV_COMPRESS_GET_CODEC_CAPS,
            SNDRV_COMPRESS_SET_PARAMS,
            SNDRV_COMPRESS_GET_PARAMS,
            SNDRV_COMPRESS_SET_METADATA,
            SNDRV_COMPRESS_GET_METADATA,
            SNDRV_COMPRESS_START,
            SNDRV_COMPRESS_STOP,
            SNDRV_COMPRESS_PAUSE,
            SNDRV_COMPRESS_RESUME,
            SNDRV_COMPRESS_DRAIN,
            SNDRV_COMPRESS_PARTIAL_DRAIN,
            SNDRV_COMPRESS_TSTAMP,
            SNDRV_COMPRESS_AVAIL,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            SNDRV_COMPRESS_STATE_OPEN,
            SNDRV_COMPRESS_STATE_SETUP,
            SNDRV_COMPRESS_STATE_PREPARED,
            SNDRV_COMPRESS_STATE_RUNNING,
            SNDRV_COMPRESS_STATE_PAUSED,
            SNDRV_COMPRESS_STATE_DRAINING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_codecs_distinct() {
        let codecs = [
            SND_AUDIOCODEC_PCM,
            SND_AUDIOCODEC_MP3,
            SND_AUDIOCODEC_AAC,
            SND_AUDIOCODEC_WMA,
            SND_AUDIOCODEC_VORBIS,
            SND_AUDIOCODEC_FLAC,
            SND_AUDIOCODEC_ALAC,
            SND_AUDIOCODEC_APE,
            SND_AUDIOCODEC_OPUS,
        ];
        for i in 0..codecs.len() {
            for j in (i + 1)..codecs.len() {
                assert_ne!(codecs[i], codecs[j]);
            }
        }
    }
}

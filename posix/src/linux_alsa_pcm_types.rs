//! `<sound/asound.h>` (PCM subset) — ALSA PCM (audio stream) constants.
//!
//! PCM (Pulse Code Modulation) is the standard interface for digital
//! audio streams. ALSA PCM provides playback (DAC → speaker) and
//! capture (microphone → ADC) with configurable sample format, rate,
//! channels, buffer size, and period size. The ring buffer design
//! allows the application and hardware to run asynchronously with
//! interrupt-driven period notifications.

// ---------------------------------------------------------------------------
// PCM stream types
// ---------------------------------------------------------------------------

/// Playback stream (application → hardware → speakers).
pub const SNDRV_PCM_STREAM_PLAYBACK: u32 = 0;
/// Capture stream (microphone → hardware → application).
pub const SNDRV_PCM_STREAM_CAPTURE: u32 = 1;

// ---------------------------------------------------------------------------
// PCM sample formats
// ---------------------------------------------------------------------------

/// Signed 8-bit.
pub const SNDRV_PCM_FORMAT_S8: u32 = 0;
/// Unsigned 8-bit.
pub const SNDRV_PCM_FORMAT_U8: u32 = 1;
/// Signed 16-bit little-endian.
pub const SNDRV_PCM_FORMAT_S16_LE: u32 = 2;
/// Signed 16-bit big-endian.
pub const SNDRV_PCM_FORMAT_S16_BE: u32 = 3;
/// Unsigned 16-bit little-endian.
pub const SNDRV_PCM_FORMAT_U16_LE: u32 = 4;
/// Signed 24-bit little-endian (in 4 bytes).
pub const SNDRV_PCM_FORMAT_S24_LE: u32 = 6;
/// Signed 32-bit little-endian.
pub const SNDRV_PCM_FORMAT_S32_LE: u32 = 10;
/// 32-bit float little-endian.
pub const SNDRV_PCM_FORMAT_FLOAT_LE: u32 = 14;
/// 64-bit float little-endian.
pub const SNDRV_PCM_FORMAT_FLOAT64_LE: u32 = 16;
/// IEC 958 (S/PDIF) subframe LE.
pub const SNDRV_PCM_FORMAT_IEC958_SUBFRAME_LE: u32 = 18;
/// Mu-law encoding.
pub const SNDRV_PCM_FORMAT_MU_LAW: u32 = 20;
/// A-law encoding.
pub const SNDRV_PCM_FORMAT_A_LAW: u32 = 21;
/// Signed 24-bit packed (3 bytes per sample).
pub const SNDRV_PCM_FORMAT_S24_3LE: u32 = 32;

// ---------------------------------------------------------------------------
// PCM access types
// ---------------------------------------------------------------------------

/// Interleaved (LRLRLR...).
pub const SNDRV_PCM_ACCESS_MMAP_INTERLEAVED: u32 = 0;
/// Non-interleaved (LLL...RRR...).
pub const SNDRV_PCM_ACCESS_MMAP_NONINTERLEAVED: u32 = 1;
/// Read/write interleaved.
pub const SNDRV_PCM_ACCESS_RW_INTERLEAVED: u32 = 3;
/// Read/write non-interleaved.
pub const SNDRV_PCM_ACCESS_RW_NONINTERLEAVED: u32 = 4;

// ---------------------------------------------------------------------------
// PCM states
// ---------------------------------------------------------------------------

/// Stream is open but not configured.
pub const SNDRV_PCM_STATE_OPEN: u32 = 0;
/// Hardware parameters set.
pub const SNDRV_PCM_STATE_SETUP: u32 = 1;
/// Stream is prepared (ready to start).
pub const SNDRV_PCM_STATE_PREPARED: u32 = 2;
/// Stream is running.
pub const SNDRV_PCM_STATE_RUNNING: u32 = 3;
/// Buffer underrun (playback) or overrun (capture).
pub const SNDRV_PCM_STATE_XRUN: u32 = 4;
/// Stream is draining (finishing remaining data).
pub const SNDRV_PCM_STATE_DRAINING: u32 = 5;
/// Stream is paused.
pub const SNDRV_PCM_STATE_PAUSED: u32 = 6;
/// Stream is suspended (power management).
pub const SNDRV_PCM_STATE_SUSPENDED: u32 = 7;
/// Stream was disconnected (device removed).
pub const SNDRV_PCM_STATE_DISCONNECTED: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_types() {
        assert_ne!(SNDRV_PCM_STREAM_PLAYBACK, SNDRV_PCM_STREAM_CAPTURE);
    }

    #[test]
    fn test_formats_distinct() {
        let fmts = [
            SNDRV_PCM_FORMAT_S8,
            SNDRV_PCM_FORMAT_U8,
            SNDRV_PCM_FORMAT_S16_LE,
            SNDRV_PCM_FORMAT_S16_BE,
            SNDRV_PCM_FORMAT_U16_LE,
            SNDRV_PCM_FORMAT_S24_LE,
            SNDRV_PCM_FORMAT_S32_LE,
            SNDRV_PCM_FORMAT_FLOAT_LE,
            SNDRV_PCM_FORMAT_FLOAT64_LE,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
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
            SNDRV_PCM_STATE_DISCONNECTED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}

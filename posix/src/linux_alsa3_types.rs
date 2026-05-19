//! `<sound/asound.h>` — Additional ALSA constants (part 3).
//!
//! Supplementary ALSA constants covering PCM stream types,
//! access modes, and timer types.

// ---------------------------------------------------------------------------
// ALSA PCM stream types
// ---------------------------------------------------------------------------

/// Playback stream.
pub const SNDRV_PCM_STREAM_PLAYBACK: u32 = 0;
/// Capture stream.
pub const SNDRV_PCM_STREAM_CAPTURE: u32 = 1;

// ---------------------------------------------------------------------------
// ALSA PCM access types
// ---------------------------------------------------------------------------

/// MMAP interleaved.
pub const SNDRV_PCM_ACCESS_MMAP_INTERLEAVED: u32 = 0;
/// MMAP non-interleaved.
pub const SNDRV_PCM_ACCESS_MMAP_NONINTERLEAVED: u32 = 1;
/// MMAP complex.
pub const SNDRV_PCM_ACCESS_MMAP_COMPLEX: u32 = 2;
/// Read/write interleaved.
pub const SNDRV_PCM_ACCESS_RW_INTERLEAVED: u32 = 3;
/// Read/write non-interleaved.
pub const SNDRV_PCM_ACCESS_RW_NONINTERLEAVED: u32 = 4;

// ---------------------------------------------------------------------------
// ALSA PCM subformat types
// ---------------------------------------------------------------------------

/// Standard subformat.
pub const SNDRV_PCM_SUBFORMAT_STD: u32 = 0;
/// MSBITS 20.
pub const SNDRV_PCM_SUBFORMAT_MSBITS_20: u32 = 1;
/// MSBITS 24.
pub const SNDRV_PCM_SUBFORMAT_MSBITS_24: u32 = 2;

// ---------------------------------------------------------------------------
// ALSA PCM states
// ---------------------------------------------------------------------------

/// Open.
pub const SNDRV_PCM_STATE_OPEN: i32 = 0;
/// Setup.
pub const SNDRV_PCM_STATE_SETUP: i32 = 1;
/// Prepared.
pub const SNDRV_PCM_STATE_PREPARED: i32 = 2;
/// Running.
pub const SNDRV_PCM_STATE_RUNNING: i32 = 3;
/// Xrun (overrun/underrun).
pub const SNDRV_PCM_STATE_XRUN: i32 = 4;
/// Draining.
pub const SNDRV_PCM_STATE_DRAINING: i32 = 5;
/// Paused.
pub const SNDRV_PCM_STATE_PAUSED: i32 = 6;
/// Suspended.
pub const SNDRV_PCM_STATE_SUSPENDED: i32 = 7;
/// Disconnected.
pub const SNDRV_PCM_STATE_DISCONNECTED: i32 = 8;

// ---------------------------------------------------------------------------
// ALSA timer types
// ---------------------------------------------------------------------------

/// No timer.
pub const SNDRV_TIMER_TYPE_NONE: u32 = 0;
/// Slave (synced to master).
pub const SNDRV_TIMER_TYPE_SLAVE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_types_distinct() {
        assert_ne!(SNDRV_PCM_STREAM_PLAYBACK, SNDRV_PCM_STREAM_CAPTURE);
    }

    #[test]
    fn test_access_types_distinct() {
        let types = [
            SNDRV_PCM_ACCESS_MMAP_INTERLEAVED,
            SNDRV_PCM_ACCESS_MMAP_NONINTERLEAVED,
            SNDRV_PCM_ACCESS_MMAP_COMPLEX,
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
    fn test_states_distinct() {
        let states = [
            SNDRV_PCM_STATE_OPEN, SNDRV_PCM_STATE_SETUP,
            SNDRV_PCM_STATE_PREPARED, SNDRV_PCM_STATE_RUNNING,
            SNDRV_PCM_STATE_XRUN, SNDRV_PCM_STATE_DRAINING,
            SNDRV_PCM_STATE_PAUSED, SNDRV_PCM_STATE_SUSPENDED,
            SNDRV_PCM_STATE_DISCONNECTED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_subformats_distinct() {
        let fmts = [
            SNDRV_PCM_SUBFORMAT_STD,
            SNDRV_PCM_SUBFORMAT_MSBITS_20,
            SNDRV_PCM_SUBFORMAT_MSBITS_24,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_timer_types_distinct() {
        assert_ne!(SNDRV_TIMER_TYPE_NONE, SNDRV_TIMER_TYPE_SLAVE);
    }
}

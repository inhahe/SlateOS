//! `<sound/asound.h>` — Core ALSA constants and device types.
//!
//! ALSA (Advanced Linux Sound Architecture) is the kernel's audio
//! subsystem. It exposes sound cards as devices with multiple
//! subdevices for PCM playback/capture, mixer controls, MIDI,
//! and hardware-dependent features.

// ---------------------------------------------------------------------------
// ALSA device types (snd_device_type)
// ---------------------------------------------------------------------------

/// Top-level sound card.
pub const SNDRV_DEV_TOPLEVEL: u32 = 0;
/// Control interface device.
pub const SNDRV_DEV_CONTROL: u32 = 1;
/// Low-level device (codec, etc.).
pub const SNDRV_DEV_LOWLEVEL: u32 = 2;
/// PCM device.
pub const SNDRV_DEV_PCM: u32 = 4;
/// Raw MIDI device.
pub const SNDRV_DEV_RAWMIDI: u32 = 5;
/// Timer device.
pub const SNDRV_DEV_TIMER: u32 = 6;
/// Sequencer device.
pub const SNDRV_DEV_SEQUENCER: u32 = 7;
/// Hardware-dependent device.
pub const SNDRV_DEV_HWDEP: u32 = 8;

// ---------------------------------------------------------------------------
// ALSA PCM stream directions
// ---------------------------------------------------------------------------

/// Playback stream (data flows to hardware).
pub const SNDRV_PCM_STREAM_PLAYBACK: u32 = 0;
/// Capture stream (data flows from hardware).
pub const SNDRV_PCM_STREAM_CAPTURE: u32 = 1;

// ---------------------------------------------------------------------------
// ALSA PCM states
// ---------------------------------------------------------------------------

/// Stream is open (not yet set up).
pub const SNDRV_PCM_STATE_OPEN: u32 = 0;
/// Stream is set up (params configured).
pub const SNDRV_PCM_STATE_SETUP: u32 = 1;
/// Stream is prepared (ready to start).
pub const SNDRV_PCM_STATE_PREPARED: u32 = 2;
/// Stream is running (playing/recording).
pub const SNDRV_PCM_STATE_RUNNING: u32 = 3;
/// Stream is in XRUN (buffer over/underrun).
pub const SNDRV_PCM_STATE_XRUN: u32 = 4;
/// Stream is draining (finishing playback).
pub const SNDRV_PCM_STATE_DRAINING: u32 = 5;
/// Stream is paused.
pub const SNDRV_PCM_STATE_PAUSED: u32 = 6;
/// Stream is suspended (power management).
pub const SNDRV_PCM_STATE_SUSPENDED: u32 = 7;
/// Stream is disconnected (device removed).
pub const SNDRV_PCM_STATE_DISCONNECTED: u32 = 8;

// ---------------------------------------------------------------------------
// ALSA PCM access types
// ---------------------------------------------------------------------------

/// MMAP interleaved access.
pub const SNDRV_PCM_ACCESS_MMAP_INTERLEAVED: u32 = 0;
/// MMAP non-interleaved access.
pub const SNDRV_PCM_ACCESS_MMAP_NONINTERLEAVED: u32 = 1;
/// MMAP complex access.
pub const SNDRV_PCM_ACCESS_MMAP_COMPLEX: u32 = 2;
/// Read/Write interleaved access.
pub const SNDRV_PCM_ACCESS_RW_INTERLEAVED: u32 = 3;
/// Read/Write non-interleaved access.
pub const SNDRV_PCM_ACCESS_RW_NONINTERLEAVED: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_types_distinct() {
        let devs = [
            SNDRV_DEV_TOPLEVEL, SNDRV_DEV_CONTROL, SNDRV_DEV_LOWLEVEL,
            SNDRV_DEV_PCM, SNDRV_DEV_RAWMIDI, SNDRV_DEV_TIMER,
            SNDRV_DEV_SEQUENCER, SNDRV_DEV_HWDEP,
        ];
        for i in 0..devs.len() {
            for j in (i + 1)..devs.len() {
                assert_ne!(devs[i], devs[j]);
            }
        }
    }

    #[test]
    fn test_stream_directions() {
        assert_ne!(SNDRV_PCM_STREAM_PLAYBACK, SNDRV_PCM_STREAM_CAPTURE);
    }

    #[test]
    fn test_pcm_states_distinct() {
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
    fn test_access_types_distinct() {
        let access = [
            SNDRV_PCM_ACCESS_MMAP_INTERLEAVED,
            SNDRV_PCM_ACCESS_MMAP_NONINTERLEAVED,
            SNDRV_PCM_ACCESS_MMAP_COMPLEX,
            SNDRV_PCM_ACCESS_RW_INTERLEAVED,
            SNDRV_PCM_ACCESS_RW_NONINTERLEAVED,
        ];
        for i in 0..access.len() {
            for j in (i + 1)..access.len() {
                assert_ne!(access[i], access[j]);
            }
        }
    }
}

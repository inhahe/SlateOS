//! `<sound/rawmidi.h>` — ALSA raw MIDI interface constants.
//!
//! Raw MIDI provides direct byte-stream access to MIDI hardware
//! without interpretation of MIDI protocol (no running status,
//! no timestamp management). Applications read/write raw MIDI
//! bytes to /dev/snd/midiCxDx devices. Used by MIDI applications
//! that implement their own MIDI protocol handling or need minimum
//! latency without the sequencer overhead.

// ---------------------------------------------------------------------------
// Raw MIDI stream directions
// ---------------------------------------------------------------------------

/// Output (playback, host → device).
pub const SNDRV_RAWMIDI_STREAM_OUTPUT: u32 = 0;
/// Input (capture, device → host).
pub const SNDRV_RAWMIDI_STREAM_INPUT: u32 = 1;

// ---------------------------------------------------------------------------
// Raw MIDI IOCTLs
// ---------------------------------------------------------------------------

/// Get device info.
pub const SNDRV_RAWMIDI_IOCTL_INFO: u32 = 0x01;
/// Set parameters.
pub const SNDRV_RAWMIDI_IOCTL_PARAMS: u32 = 0x10;
/// Get status (bytes available, xrun count).
pub const SNDRV_RAWMIDI_IOCTL_STATUS: u32 = 0x20;
/// Drop pending output bytes.
pub const SNDRV_RAWMIDI_IOCTL_DROP: u32 = 0x30;
/// Drain output (wait for all bytes to be transmitted).
pub const SNDRV_RAWMIDI_IOCTL_DRAIN: u32 = 0x31;

// ---------------------------------------------------------------------------
// Raw MIDI open flags
// ---------------------------------------------------------------------------

/// Open for output.
pub const SNDRV_RAWMIDI_FLAG_OUTPUT: u32 = 1 << 0;
/// Open for input.
pub const SNDRV_RAWMIDI_FLAG_INPUT: u32 = 1 << 1;
/// Non-blocking mode.
pub const SNDRV_RAWMIDI_FLAG_NONBLOCK: u32 = 1 << 2;
/// Append mode (don't reset on open).
pub const SNDRV_RAWMIDI_FLAG_APPEND: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Raw MIDI info flags
// ---------------------------------------------------------------------------

/// Device supports output.
pub const SNDRV_RAWMIDI_INFO_OUTPUT: u32 = 1 << 0;
/// Device supports input.
pub const SNDRV_RAWMIDI_INFO_INPUT: u32 = 1 << 1;
/// Device supports duplex (simultaneous input+output).
pub const SNDRV_RAWMIDI_INFO_DUPLEX: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streams_distinct() {
        assert_ne!(SNDRV_RAWMIDI_STREAM_OUTPUT, SNDRV_RAWMIDI_STREAM_INPUT);
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            SNDRV_RAWMIDI_IOCTL_INFO, SNDRV_RAWMIDI_IOCTL_PARAMS,
            SNDRV_RAWMIDI_IOCTL_STATUS, SNDRV_RAWMIDI_IOCTL_DROP,
            SNDRV_RAWMIDI_IOCTL_DRAIN,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_open_flags_no_overlap() {
        let flags = [
            SNDRV_RAWMIDI_FLAG_OUTPUT, SNDRV_RAWMIDI_FLAG_INPUT,
            SNDRV_RAWMIDI_FLAG_NONBLOCK, SNDRV_RAWMIDI_FLAG_APPEND,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_info_flags_no_overlap() {
        let flags = [
            SNDRV_RAWMIDI_INFO_OUTPUT, SNDRV_RAWMIDI_INFO_INPUT,
            SNDRV_RAWMIDI_INFO_DUPLEX,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

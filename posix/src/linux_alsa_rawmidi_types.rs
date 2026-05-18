//! `<sound/rawmidi.h>` — ALSA raw MIDI interface constants.
//!
//! The raw MIDI interface provides byte-level access to MIDI ports
//! without sequencer processing. Applications send/receive raw MIDI
//! bytes directly. This is used for MIDI controllers, synthesizers,
//! and MIDI-over-USB devices.

// ---------------------------------------------------------------------------
// Raw MIDI stream directions
// ---------------------------------------------------------------------------

/// Output stream (host → device, MIDI out).
pub const SNDRV_RAWMIDI_STREAM_OUTPUT: u32 = 0;
/// Input stream (device → host, MIDI in).
pub const SNDRV_RAWMIDI_STREAM_INPUT: u32 = 1;

// ---------------------------------------------------------------------------
// Raw MIDI info flags
// ---------------------------------------------------------------------------

/// Device supports output.
pub const SNDRV_RAWMIDI_INFO_OUTPUT: u32 = 0x0000_0001;
/// Device supports input.
pub const SNDRV_RAWMIDI_INFO_INPUT: u32 = 0x0000_0002;
/// Device supports duplex (simultaneous in/out).
pub const SNDRV_RAWMIDI_INFO_DUPLEX: u32 = 0x0000_0004;

// ---------------------------------------------------------------------------
// Raw MIDI open flags
// ---------------------------------------------------------------------------

/// Open for output.
pub const SNDRV_RAWMIDI_OPEN_OUTPUT: u32 = 1 << 0;
/// Open for input.
pub const SNDRV_RAWMIDI_OPEN_INPUT: u32 = 1 << 1;
/// Open non-blocking.
pub const SNDRV_RAWMIDI_OPEN_NONBLOCK: u32 = 1 << 2;
/// Append mode (don't flush on open).
pub const SNDRV_RAWMIDI_OPEN_APPEND: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// MIDI message types (status byte high nibble)
// ---------------------------------------------------------------------------

/// Note Off (0x80).
pub const MIDI_MSG_NOTE_OFF: u8 = 0x80;
/// Note On (0x90).
pub const MIDI_MSG_NOTE_ON: u8 = 0x90;
/// Polyphonic Key Pressure (0xA0).
pub const MIDI_MSG_POLY_PRESSURE: u8 = 0xA0;
/// Control Change (0xB0).
pub const MIDI_MSG_CONTROL_CHANGE: u8 = 0xB0;
/// Program Change (0xC0).
pub const MIDI_MSG_PROGRAM_CHANGE: u8 = 0xC0;
/// Channel Pressure (0xD0).
pub const MIDI_MSG_CHANNEL_PRESSURE: u8 = 0xD0;
/// Pitch Bend (0xE0).
pub const MIDI_MSG_PITCH_BEND: u8 = 0xE0;
/// System Exclusive start (0xF0).
pub const MIDI_MSG_SYSEX_START: u8 = 0xF0;
/// System Exclusive end (0xF7).
pub const MIDI_MSG_SYSEX_END: u8 = 0xF7;
/// Active Sensing (0xFE).
pub const MIDI_MSG_ACTIVE_SENSING: u8 = 0xFE;
/// System Reset (0xFF).
pub const MIDI_MSG_SYSTEM_RESET: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_directions() {
        assert_ne!(SNDRV_RAWMIDI_STREAM_OUTPUT, SNDRV_RAWMIDI_STREAM_INPUT);
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

    #[test]
    fn test_open_flags_no_overlap() {
        let flags = [
            SNDRV_RAWMIDI_OPEN_OUTPUT, SNDRV_RAWMIDI_OPEN_INPUT,
            SNDRV_RAWMIDI_OPEN_NONBLOCK, SNDRV_RAWMIDI_OPEN_APPEND,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_midi_messages_distinct() {
        let msgs = [
            MIDI_MSG_NOTE_OFF, MIDI_MSG_NOTE_ON, MIDI_MSG_POLY_PRESSURE,
            MIDI_MSG_CONTROL_CHANGE, MIDI_MSG_PROGRAM_CHANGE,
            MIDI_MSG_CHANNEL_PRESSURE, MIDI_MSG_PITCH_BEND,
            MIDI_MSG_SYSEX_START, MIDI_MSG_SYSEX_END,
            MIDI_MSG_ACTIVE_SENSING, MIDI_MSG_SYSTEM_RESET,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_midi_channel_msg_range() {
        // Channel messages are 0x80-0xEF
        assert!(MIDI_MSG_NOTE_OFF >= 0x80);
        assert!(MIDI_MSG_PITCH_BEND < 0xF0);
    }
}

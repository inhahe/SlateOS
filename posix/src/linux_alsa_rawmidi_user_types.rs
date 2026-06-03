//! `<sound/asound.h>` — ALSA raw-MIDI streams and buffer defaults.
//!
//! The rawmidi interface exposes byte-oriented MIDI ports as
//! character devices. This module collects the userspace-facing
//! constants that aren't in the core `linux_alsa_types`.

// ---------------------------------------------------------------------------
// Stream directions (mirror PCM but distinct ABI values)
// ---------------------------------------------------------------------------

pub const SNDRV_RAWMIDI_STREAM_OUTPUT: u32 = 0;
pub const SNDRV_RAWMIDI_STREAM_INPUT: u32 = 1;
pub const SNDRV_RAWMIDI_STREAM_LAST: u32 = SNDRV_RAWMIDI_STREAM_INPUT;

// ---------------------------------------------------------------------------
// Information flags (`SNDRV_RAWMIDI_INFO_*`)
// ---------------------------------------------------------------------------

pub const SNDRV_RAWMIDI_INFO_OUTPUT: u32 = 1 << 0;
pub const SNDRV_RAWMIDI_INFO_INPUT: u32 = 1 << 1;
pub const SNDRV_RAWMIDI_INFO_DUPLEX: u32 = 1 << 2;
pub const SNDRV_RAWMIDI_INFO_UMP: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Mode flags passed at open time (`SNDRV_RAWMIDI_LFLG_*`)
// ---------------------------------------------------------------------------

pub const SNDRV_RAWMIDI_LFLG_OUTPUT: u32 = 1 << 0;
pub const SNDRV_RAWMIDI_LFLG_INPUT: u32 = 1 << 1;
pub const SNDRV_RAWMIDI_LFLG_OPEN: u32 = 0x0003;
pub const SNDRV_RAWMIDI_LFLG_APPEND: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Default buffer sizing (bytes)
// ---------------------------------------------------------------------------

pub const SNDRV_RAWMIDI_DEFAULT_BUFFER_SIZE: usize = 4_096;
pub const SNDRV_RAWMIDI_MIN_BUFFER_SIZE: usize = 32;
pub const SNDRV_RAWMIDI_DEFAULT_AVAIL_MIN: usize = 1;

// ---------------------------------------------------------------------------
// MIDI 1.0 wire-protocol status bytes (for context)
// ---------------------------------------------------------------------------

pub const MIDI_STATUS_NOTE_OFF: u8 = 0x80;
pub const MIDI_STATUS_NOTE_ON: u8 = 0x90;
pub const MIDI_STATUS_POLY_PRESSURE: u8 = 0xA0;
pub const MIDI_STATUS_CONTROL_CHANGE: u8 = 0xB0;
pub const MIDI_STATUS_PROGRAM_CHANGE: u8 = 0xC0;
pub const MIDI_STATUS_CHANNEL_PRESSURE: u8 = 0xD0;
pub const MIDI_STATUS_PITCH_BEND: u8 = 0xE0;
pub const MIDI_STATUS_SYSEX_START: u8 = 0xF0;
pub const MIDI_STATUS_SYSEX_END: u8 = 0xF7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streams_dense_0_to_1() {
        assert_eq!(SNDRV_RAWMIDI_STREAM_OUTPUT, 0);
        assert_eq!(SNDRV_RAWMIDI_STREAM_INPUT, 1);
        assert_eq!(SNDRV_RAWMIDI_STREAM_LAST, 1);
    }

    #[test]
    fn test_info_flags_each_power_of_two() {
        for v in [
            SNDRV_RAWMIDI_INFO_OUTPUT,
            SNDRV_RAWMIDI_INFO_INPUT,
            SNDRV_RAWMIDI_INFO_DUPLEX,
            SNDRV_RAWMIDI_INFO_UMP,
        ] {
            assert!(v.is_power_of_two());
        }
        // INFO bits 0..3 fit in low nibble.
        let or = SNDRV_RAWMIDI_INFO_OUTPUT
            | SNDRV_RAWMIDI_INFO_INPUT
            | SNDRV_RAWMIDI_INFO_DUPLEX
            | SNDRV_RAWMIDI_INFO_UMP;
        assert_eq!(or, 0x0F);
    }

    #[test]
    fn test_open_lflag_is_union_of_io() {
        assert_eq!(
            SNDRV_RAWMIDI_LFLG_OPEN,
            SNDRV_RAWMIDI_LFLG_OUTPUT | SNDRV_RAWMIDI_LFLG_INPUT
        );
        assert!(SNDRV_RAWMIDI_LFLG_APPEND.is_power_of_two());
        // Append bit is above the open bits.
        assert!(SNDRV_RAWMIDI_LFLG_APPEND > SNDRV_RAWMIDI_LFLG_OPEN);
    }

    #[test]
    fn test_buffer_size_defaults_sane() {
        // Default >= minimum, both powers of two convenient.
        assert!(SNDRV_RAWMIDI_DEFAULT_BUFFER_SIZE >= SNDRV_RAWMIDI_MIN_BUFFER_SIZE);
        assert_eq!(SNDRV_RAWMIDI_DEFAULT_BUFFER_SIZE, 4096);
        assert_eq!(SNDRV_RAWMIDI_MIN_BUFFER_SIZE, 32);
        assert_eq!(SNDRV_RAWMIDI_DEFAULT_AVAIL_MIN, 1);
    }

    #[test]
    fn test_midi_status_channel_msgs_share_high_nibble_pattern() {
        // Channel voice messages 0x80..0xE0 step by 0x10.
        let v = [
            MIDI_STATUS_NOTE_OFF,
            MIDI_STATUS_NOTE_ON,
            MIDI_STATUS_POLY_PRESSURE,
            MIDI_STATUS_CONTROL_CHANGE,
            MIDI_STATUS_PROGRAM_CHANGE,
            MIDI_STATUS_CHANNEL_PRESSURE,
            MIDI_STATUS_PITCH_BEND,
        ];
        for w in v.windows(2) {
            assert_eq!(w[1] - w[0], 0x10);
            assert_eq!(w[0] & 0x0F, 0);
        }
        // All have status bit set.
        for &s in &v {
            assert!(s & 0x80 != 0);
        }
    }

    #[test]
    fn test_sysex_start_end_pair() {
        assert_eq!(MIDI_STATUS_SYSEX_START, 0xF0);
        assert_eq!(MIDI_STATUS_SYSEX_END, 0xF7);
        // SysEx start has the system real-time bit clear in low nibble; end is 7.
        assert_eq!(MIDI_STATUS_SYSEX_START & 0x0F, 0);
        assert_eq!(MIDI_STATUS_SYSEX_END & 0x0F, 7);
    }
}

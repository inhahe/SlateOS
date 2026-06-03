//! `<sound/asequencer.h>` — ALSA sequencer event types and port capabilities.
//!
//! The sequencer delivers timed MIDI-like events between clients.
//! This module covers the event-type enumeration and the bitmask
//! used to declare port capabilities. Reserved client/port IDs are
//! in `linux_alsa3_user_types`.

// ---------------------------------------------------------------------------
// Event types (`snd_seq_event_type_t`)
// ---------------------------------------------------------------------------

pub const SNDRV_SEQ_EVENT_SYSTEM: u8 = 0;
pub const SNDRV_SEQ_EVENT_RESULT: u8 = 1;

pub const SNDRV_SEQ_EVENT_NOTE: u8 = 5;
pub const SNDRV_SEQ_EVENT_NOTEON: u8 = 6;
pub const SNDRV_SEQ_EVENT_NOTEOFF: u8 = 7;
pub const SNDRV_SEQ_EVENT_KEYPRESS: u8 = 8;

pub const SNDRV_SEQ_EVENT_CONTROLLER: u8 = 10;
pub const SNDRV_SEQ_EVENT_PGMCHANGE: u8 = 11;
pub const SNDRV_SEQ_EVENT_CHANPRESS: u8 = 12;
pub const SNDRV_SEQ_EVENT_PITCHBEND: u8 = 13;

pub const SNDRV_SEQ_EVENT_SONGPOS: u8 = 20;
pub const SNDRV_SEQ_EVENT_SONGSEL: u8 = 21;
pub const SNDRV_SEQ_EVENT_QFRAME: u8 = 22;
pub const SNDRV_SEQ_EVENT_TIMESIGN: u8 = 23;
pub const SNDRV_SEQ_EVENT_KEYSIGN: u8 = 24;

pub const SNDRV_SEQ_EVENT_START: u8 = 30;
pub const SNDRV_SEQ_EVENT_CONTINUE: u8 = 31;
pub const SNDRV_SEQ_EVENT_STOP: u8 = 32;
pub const SNDRV_SEQ_EVENT_TEMPO: u8 = 35;
pub const SNDRV_SEQ_EVENT_CLOCK: u8 = 36;
pub const SNDRV_SEQ_EVENT_TICK: u8 = 37;

pub const SNDRV_SEQ_EVENT_SYSEX: u8 = 130;
pub const SNDRV_SEQ_EVENT_NONE: u8 = 255;

// ---------------------------------------------------------------------------
// Port capabilities (`SNDRV_SEQ_PORT_CAP_*`)
// ---------------------------------------------------------------------------

pub const SNDRV_SEQ_PORT_CAP_READ: u32 = 1 << 0;
pub const SNDRV_SEQ_PORT_CAP_WRITE: u32 = 1 << 1;
pub const SNDRV_SEQ_PORT_CAP_SYNC_READ: u32 = 1 << 2;
pub const SNDRV_SEQ_PORT_CAP_SYNC_WRITE: u32 = 1 << 3;
pub const SNDRV_SEQ_PORT_CAP_DUPLEX: u32 = 1 << 4;
pub const SNDRV_SEQ_PORT_CAP_SUBS_READ: u32 = 1 << 5;
pub const SNDRV_SEQ_PORT_CAP_SUBS_WRITE: u32 = 1 << 6;
pub const SNDRV_SEQ_PORT_CAP_NO_EXPORT: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Port types (`SNDRV_SEQ_PORT_TYPE_*`) — categorisation
// ---------------------------------------------------------------------------

pub const SNDRV_SEQ_PORT_TYPE_SPECIFIC: u32 = 1 << 0;
pub const SNDRV_SEQ_PORT_TYPE_MIDI_GENERIC: u32 = 1 << 1;
pub const SNDRV_SEQ_PORT_TYPE_MIDI_GM: u32 = 1 << 2;
pub const SNDRV_SEQ_PORT_TYPE_MIDI_GS: u32 = 1 << 3;
pub const SNDRV_SEQ_PORT_TYPE_MIDI_XG: u32 = 1 << 4;
pub const SNDRV_SEQ_PORT_TYPE_MIDI_MT32: u32 = 1 << 5;
pub const SNDRV_SEQ_PORT_TYPE_MIDI_GM2: u32 = 1 << 6;

pub const SNDRV_SEQ_PORT_TYPE_SYNTH: u32 = 1 << 10;
pub const SNDRV_SEQ_PORT_TYPE_DIRECT_SAMPLE: u32 = 1 << 11;
pub const SNDRV_SEQ_PORT_TYPE_SAMPLE: u32 = 1 << 12;

pub const SNDRV_SEQ_PORT_TYPE_HARDWARE: u32 = 1 << 16;
pub const SNDRV_SEQ_PORT_TYPE_SOFTWARE: u32 = 1 << 17;
pub const SNDRV_SEQ_PORT_TYPE_SYNTHESIZER: u32 = 1 << 18;
pub const SNDRV_SEQ_PORT_TYPE_PORT: u32 = 1 << 19;
pub const SNDRV_SEQ_PORT_TYPE_APPLICATION: u32 = 1 << 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_note_events_consecutive() {
        // NOTE, NOTEON, NOTEOFF, KEYPRESS form a dense block at 5..=8.
        assert_eq!(SNDRV_SEQ_EVENT_NOTE, 5);
        assert_eq!(SNDRV_SEQ_EVENT_NOTEON, 6);
        assert_eq!(SNDRV_SEQ_EVENT_NOTEOFF, 7);
        assert_eq!(SNDRV_SEQ_EVENT_KEYPRESS, 8);
    }

    #[test]
    fn test_channel_voice_events_block() {
        // CC, PGMCHANGE, CHANPRESS, PITCHBEND consecutive 10..=13.
        let cv = [
            SNDRV_SEQ_EVENT_CONTROLLER,
            SNDRV_SEQ_EVENT_PGMCHANGE,
            SNDRV_SEQ_EVENT_CHANPRESS,
            SNDRV_SEQ_EVENT_PITCHBEND,
        ];
        for (i, &v) in cv.iter().enumerate() {
            assert_eq!(v, 10 + i as u8);
        }
    }

    #[test]
    fn test_transport_events_dense_30_32() {
        assert_eq!(SNDRV_SEQ_EVENT_START, 30);
        assert_eq!(SNDRV_SEQ_EVENT_CONTINUE, 31);
        assert_eq!(SNDRV_SEQ_EVENT_STOP, 32);
    }

    #[test]
    fn test_sentinel_events() {
        // SysEx is in the 130-block; NONE is the all-ones sentinel.
        assert_eq!(SNDRV_SEQ_EVENT_SYSEX, 130);
        assert_eq!(SNDRV_SEQ_EVENT_NONE, 255);
        assert_eq!(SNDRV_SEQ_EVENT_SYSTEM, 0);
    }

    #[test]
    fn test_port_caps_low_byte_bits() {
        let c = [
            SNDRV_SEQ_PORT_CAP_READ,
            SNDRV_SEQ_PORT_CAP_WRITE,
            SNDRV_SEQ_PORT_CAP_SYNC_READ,
            SNDRV_SEQ_PORT_CAP_SYNC_WRITE,
            SNDRV_SEQ_PORT_CAP_DUPLEX,
            SNDRV_SEQ_PORT_CAP_SUBS_READ,
            SNDRV_SEQ_PORT_CAP_SUBS_WRITE,
            SNDRV_SEQ_PORT_CAP_NO_EXPORT,
        ];
        let mut or = 0u32;
        for v in c {
            assert!(v.is_power_of_two());
            or |= v;
        }
        assert_eq!(or, 0xFF);
    }

    #[test]
    fn test_port_types_powers_of_two_and_clustered() {
        // MIDI flavour bits 1..6.
        let midi = [
            SNDRV_SEQ_PORT_TYPE_MIDI_GENERIC,
            SNDRV_SEQ_PORT_TYPE_MIDI_GM,
            SNDRV_SEQ_PORT_TYPE_MIDI_GS,
            SNDRV_SEQ_PORT_TYPE_MIDI_XG,
            SNDRV_SEQ_PORT_TYPE_MIDI_MT32,
            SNDRV_SEQ_PORT_TYPE_MIDI_GM2,
        ];
        for v in midi {
            assert!(v.is_power_of_two());
            assert!(v < 1 << 10);
        }
        // Categorical bits (HARDWARE/SOFTWARE/etc.) sit above bit 16.
        let cat = [
            SNDRV_SEQ_PORT_TYPE_HARDWARE,
            SNDRV_SEQ_PORT_TYPE_SOFTWARE,
            SNDRV_SEQ_PORT_TYPE_SYNTHESIZER,
            SNDRV_SEQ_PORT_TYPE_PORT,
            SNDRV_SEQ_PORT_TYPE_APPLICATION,
        ];
        for v in cat {
            assert!(v.is_power_of_two());
            assert!(v >= 1 << 16);
        }
    }
}

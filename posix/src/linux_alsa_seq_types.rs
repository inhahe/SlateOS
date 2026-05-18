//! `<sound/asequencer.h>` — ALSA sequencer event and port constants.
//!
//! The ALSA sequencer provides time-accurate MIDI event routing
//! between ports. Events have type codes indicating the kind of
//! MIDI message (note on/off, controller change, etc.) and ports
//! have capability flags controlling access and direction.

// ---------------------------------------------------------------------------
// Sequencer event types
// ---------------------------------------------------------------------------

/// Note on event.
pub const SNDRV_SEQ_EVENT_NOTEON: u32 = 6;
/// Note off event.
pub const SNDRV_SEQ_EVENT_NOTEOFF: u32 = 7;
/// Key pressure (aftertouch per note).
pub const SNDRV_SEQ_EVENT_KEYPRESS: u32 = 8;
/// Controller change (CC).
pub const SNDRV_SEQ_EVENT_CONTROLLER: u32 = 10;
/// Program change.
pub const SNDRV_SEQ_EVENT_PGMCHANGE: u32 = 11;
/// Channel pressure (aftertouch per channel).
pub const SNDRV_SEQ_EVENT_CHANPRESS: u32 = 12;
/// Pitch bend.
pub const SNDRV_SEQ_EVENT_PITCHBEND: u32 = 13;
/// System exclusive message.
pub const SNDRV_SEQ_EVENT_SYSEX: u32 = 130;
/// Port start event.
pub const SNDRV_SEQ_EVENT_PORT_START: u32 = 63;
/// Port exit event.
pub const SNDRV_SEQ_EVENT_PORT_EXIT: u32 = 64;
/// Port change event.
pub const SNDRV_SEQ_EVENT_PORT_CHANGE: u32 = 65;
/// Client start event.
pub const SNDRV_SEQ_EVENT_CLIENT_START: u32 = 60;
/// Client exit event.
pub const SNDRV_SEQ_EVENT_CLIENT_EXIT: u32 = 61;
/// Subscription change event.
pub const SNDRV_SEQ_EVENT_PORT_SUBSCRIBED: u32 = 66;
/// Timer tick event.
pub const SNDRV_SEQ_EVENT_TICK: u32 = 33;

// ---------------------------------------------------------------------------
// Port capability flags
// ---------------------------------------------------------------------------

/// Port can read (generate events).
pub const SNDRV_SEQ_PORT_CAP_READ: u32 = 1 << 0;
/// Port can write (accept events).
pub const SNDRV_SEQ_PORT_CAP_WRITE: u32 = 1 << 1;
/// Port allows read subscription.
pub const SNDRV_SEQ_PORT_CAP_SUBS_READ: u32 = 1 << 5;
/// Port allows write subscription.
pub const SNDRV_SEQ_PORT_CAP_SUBS_WRITE: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Port type flags
// ---------------------------------------------------------------------------

/// Hardware port.
pub const SNDRV_SEQ_PORT_TYPE_HARDWARE: u32 = 1 << 16;
/// Software port (synthesizer, etc.).
pub const SNDRV_SEQ_PORT_TYPE_SOFTWARE: u32 = 1 << 17;
/// MIDI generic port.
pub const SNDRV_SEQ_PORT_TYPE_MIDI_GENERIC: u32 = 1 << 1;
/// General MIDI compatible.
pub const SNDRV_SEQ_PORT_TYPE_MIDI_GM: u32 = 1 << 2;
/// Synthesizer port.
pub const SNDRV_SEQ_PORT_TYPE_SYNTHESIZER: u32 = 1 << 18;
/// Application port.
pub const SNDRV_SEQ_PORT_TYPE_APPLICATION: u32 = 1 << 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct() {
        let evts = [
            SNDRV_SEQ_EVENT_NOTEON, SNDRV_SEQ_EVENT_NOTEOFF,
            SNDRV_SEQ_EVENT_KEYPRESS, SNDRV_SEQ_EVENT_CONTROLLER,
            SNDRV_SEQ_EVENT_PGMCHANGE, SNDRV_SEQ_EVENT_CHANPRESS,
            SNDRV_SEQ_EVENT_PITCHBEND, SNDRV_SEQ_EVENT_SYSEX,
            SNDRV_SEQ_EVENT_PORT_START, SNDRV_SEQ_EVENT_PORT_EXIT,
            SNDRV_SEQ_EVENT_PORT_CHANGE, SNDRV_SEQ_EVENT_CLIENT_START,
            SNDRV_SEQ_EVENT_CLIENT_EXIT, SNDRV_SEQ_EVENT_PORT_SUBSCRIBED,
            SNDRV_SEQ_EVENT_TICK,
        ];
        for i in 0..evts.len() {
            for j in (i + 1)..evts.len() {
                assert_ne!(evts[i], evts[j]);
            }
        }
    }

    #[test]
    fn test_port_caps_no_overlap() {
        let caps = [
            SNDRV_SEQ_PORT_CAP_READ, SNDRV_SEQ_PORT_CAP_WRITE,
            SNDRV_SEQ_PORT_CAP_SUBS_READ, SNDRV_SEQ_PORT_CAP_SUBS_WRITE,
        ];
        for i in 0..caps.len() {
            assert!(caps[i].is_power_of_two());
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_port_types_no_overlap() {
        let types = [
            SNDRV_SEQ_PORT_TYPE_HARDWARE, SNDRV_SEQ_PORT_TYPE_SOFTWARE,
            SNDRV_SEQ_PORT_TYPE_MIDI_GENERIC, SNDRV_SEQ_PORT_TYPE_MIDI_GM,
            SNDRV_SEQ_PORT_TYPE_SYNTHESIZER, SNDRV_SEQ_PORT_TYPE_APPLICATION,
        ];
        for i in 0..types.len() {
            assert!(types[i].is_power_of_two());
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }
}

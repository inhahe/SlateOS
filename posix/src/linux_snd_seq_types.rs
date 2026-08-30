//! `<sound/asequencer.h>` — ALSA sequencer constants.
//!
//! The ALSA sequencer provides timestamped, scheduled MIDI event
//! routing between clients. Unlike raw MIDI, the sequencer handles
//! timing (both real-time and MIDI tick), event queuing, and
//! port-to-port subscription routing. Clients can be kernel-level
//! (hardware MIDI ports) or user-level (applications). Used by
//! MIDI sequencers, notation software, and virtual instrument hosts.

// ---------------------------------------------------------------------------
// Sequencer event types
// ---------------------------------------------------------------------------

/// Note on.
pub const SND_SEQ_EVENT_NOTEON: u32 = 6;
/// Note off.
pub const SND_SEQ_EVENT_NOTEOFF: u32 = 7;
/// Key pressure (polyphonic aftertouch).
pub const SND_SEQ_EVENT_KEYPRESS: u32 = 8;
/// Controller change (CC).
pub const SND_SEQ_EVENT_CONTROLLER: u32 = 10;
/// Program change.
pub const SND_SEQ_EVENT_PGMCHANGE: u32 = 11;
/// Channel pressure (aftertouch).
pub const SND_SEQ_EVENT_CHANPRESS: u32 = 12;
/// Pitch bend.
pub const SND_SEQ_EVENT_PITCHBEND: u32 = 13;
/// System exclusive (SysEx).
pub const SND_SEQ_EVENT_SYSEX: u32 = 130;
/// Tempo change.
pub const SND_SEQ_EVENT_TEMPO: u32 = 35;
/// Clock tick.
pub const SND_SEQ_EVENT_CLOCK: u32 = 36;
/// Start.
pub const SND_SEQ_EVENT_START: u32 = 37;
/// Continue.
pub const SND_SEQ_EVENT_CONTINUE: u32 = 38;
/// Stop.
pub const SND_SEQ_EVENT_STOP: u32 = 39;
/// Port subscription (connect).
pub const SND_SEQ_EVENT_PORT_SUBSCRIBED: u32 = 66;
/// Port unsubscription (disconnect).
pub const SND_SEQ_EVENT_PORT_UNSUBSCRIBED: u32 = 67;

// ---------------------------------------------------------------------------
// Sequencer client types
// ---------------------------------------------------------------------------

/// Kernel client (hardware MIDI port).
pub const SND_SEQ_CLIENT_KERNEL: u32 = 0;
/// User client (application).
pub const SND_SEQ_CLIENT_USER: u32 = 1;

// ---------------------------------------------------------------------------
// Sequencer port capability flags
// ---------------------------------------------------------------------------

/// Port can receive (write) events.
pub const SND_SEQ_PORT_CAP_WRITE: u32 = 1 << 0;
/// Port can send (read) events.
pub const SND_SEQ_PORT_CAP_READ: u32 = 1 << 1;
/// Port is available for subscription (write direction).
pub const SND_SEQ_PORT_CAP_SUBS_WRITE: u32 = 1 << 2;
/// Port is available for subscription (read direction).
pub const SND_SEQ_PORT_CAP_SUBS_READ: u32 = 1 << 3;
/// Port allows no-export (private).
pub const SND_SEQ_PORT_CAP_NO_EXPORT: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Sequencer port types
// ---------------------------------------------------------------------------

/// MIDI generic port.
pub const SND_SEQ_PORT_TYPE_MIDI_GENERIC: u32 = 1 << 1;
/// Hardware port (physical MIDI).
pub const SND_SEQ_PORT_TYPE_HARDWARE: u32 = 1 << 16;
/// Software port (virtual).
pub const SND_SEQ_PORT_TYPE_SOFTWARE: u32 = 1 << 17;
/// Synthesizer port.
pub const SND_SEQ_PORT_TYPE_SYNTHESIZER: u32 = 1 << 18;
/// Application port.
pub const SND_SEQ_PORT_TYPE_APPLICATION: u32 = 1 << 20;

// ---------------------------------------------------------------------------
// Sequencer queue timer types
// ---------------------------------------------------------------------------

/// Real-time timer (nanoseconds).
pub const SND_SEQ_TIMER_REALTIME: u32 = 0;
/// MIDI tick timer.
pub const SND_SEQ_TIMER_TICK: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct() {
        let events = [
            SND_SEQ_EVENT_NOTEON,
            SND_SEQ_EVENT_NOTEOFF,
            SND_SEQ_EVENT_KEYPRESS,
            SND_SEQ_EVENT_CONTROLLER,
            SND_SEQ_EVENT_PGMCHANGE,
            SND_SEQ_EVENT_CHANPRESS,
            SND_SEQ_EVENT_PITCHBEND,
            SND_SEQ_EVENT_SYSEX,
            SND_SEQ_EVENT_TEMPO,
            SND_SEQ_EVENT_CLOCK,
            SND_SEQ_EVENT_START,
            SND_SEQ_EVENT_CONTINUE,
            SND_SEQ_EVENT_STOP,
            SND_SEQ_EVENT_PORT_SUBSCRIBED,
            SND_SEQ_EVENT_PORT_UNSUBSCRIBED,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_client_types_distinct() {
        assert_ne!(SND_SEQ_CLIENT_KERNEL, SND_SEQ_CLIENT_USER);
    }

    #[test]
    fn test_port_caps_no_overlap() {
        let caps = [
            SND_SEQ_PORT_CAP_WRITE,
            SND_SEQ_PORT_CAP_READ,
            SND_SEQ_PORT_CAP_SUBS_WRITE,
            SND_SEQ_PORT_CAP_SUBS_READ,
            SND_SEQ_PORT_CAP_NO_EXPORT,
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
            SND_SEQ_PORT_TYPE_MIDI_GENERIC,
            SND_SEQ_PORT_TYPE_HARDWARE,
            SND_SEQ_PORT_TYPE_SOFTWARE,
            SND_SEQ_PORT_TYPE_SYNTHESIZER,
            SND_SEQ_PORT_TYPE_APPLICATION,
        ];
        for i in 0..types.len() {
            assert!(types[i].is_power_of_two());
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }

    #[test]
    fn test_timer_types_distinct() {
        assert_ne!(SND_SEQ_TIMER_REALTIME, SND_SEQ_TIMER_TICK);
    }
}

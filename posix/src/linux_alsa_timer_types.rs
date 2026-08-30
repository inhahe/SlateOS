//! `<sound/asound.h>` (timer subset) — ALSA timer constants.
//!
//! ALSA timers provide high-resolution timing for audio applications:
//! MIDI sequencing, sample-accurate synchronization, and period
//! elapsed notifications. The system timer (based on hrtimers),
//! PCM timers (hardware interrupt-driven), and sequencer timers
//! are all accessible through the timer interface.

// ---------------------------------------------------------------------------
// Timer types
// ---------------------------------------------------------------------------

/// System timer (software, hrtimer-based).
pub const SNDRV_TIMER_TYPE_SYSTEM: u32 = 0;
/// PCM timer (hardware-driven, interrupt per period).
pub const SNDRV_TIMER_TYPE_PCM: u32 = 1;
/// Sequencer timer (MIDI tempo-driven).
pub const SNDRV_TIMER_TYPE_SEQ: u32 = 2;

// ---------------------------------------------------------------------------
// Timer global instances
// ---------------------------------------------------------------------------

/// System timer instance 0 (high-resolution).
pub const SNDRV_TIMER_GLOBAL_SYSTEM: u32 = 0;
/// Real-time clock timer.
pub const SNDRV_TIMER_GLOBAL_RTC: u32 = 1;
/// HPET-based timer.
pub const SNDRV_TIMER_GLOBAL_HPET: u32 = 2;
/// HRTIMER-based timer.
pub const SNDRV_TIMER_GLOBAL_HRTIMER: u32 = 3;

// ---------------------------------------------------------------------------
// Timer flags
// ---------------------------------------------------------------------------

/// Timer supports slave mode (can be driven by another timer).
pub const SNDRV_TIMER_FLAG_SLAVE: u32 = 0x01;
/// Timer auto-starts on first read.
pub const SNDRV_TIMER_FLAG_AUTO: u32 = 0x02;
/// Timer generates early events.
pub const SNDRV_TIMER_FLAG_EARLY_EVENT: u32 = 0x04;

// ---------------------------------------------------------------------------
// Timer event types
// ---------------------------------------------------------------------------

/// Timer tick event (period elapsed).
pub const SNDRV_TIMER_EVENT_TICK: u32 = 0;
/// Timer resolution changed.
pub const SNDRV_TIMER_EVENT_RESOLUTION: u32 = 1;
/// Timer started.
pub const SNDRV_TIMER_EVENT_START: u32 = 2;
/// Timer stopped.
pub const SNDRV_TIMER_EVENT_STOP: u32 = 3;
/// Timer paused.
pub const SNDRV_TIMER_EVENT_PAUSE: u32 = 4;
/// Timer continued after pause.
pub const SNDRV_TIMER_EVENT_CONTINUE: u32 = 5;
/// Early event (before actual tick).
pub const SNDRV_TIMER_EVENT_EARLY: u32 = 6;
/// Timer suspended (power management).
pub const SNDRV_TIMER_EVENT_SUSPEND: u32 = 7;
/// Timer resumed.
pub const SNDRV_TIMER_EVENT_RESUME: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            SNDRV_TIMER_TYPE_SYSTEM,
            SNDRV_TIMER_TYPE_PCM,
            SNDRV_TIMER_TYPE_SEQ,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_globals_distinct() {
        let globals = [
            SNDRV_TIMER_GLOBAL_SYSTEM,
            SNDRV_TIMER_GLOBAL_RTC,
            SNDRV_TIMER_GLOBAL_HPET,
            SNDRV_TIMER_GLOBAL_HRTIMER,
        ];
        for i in 0..globals.len() {
            for j in (i + 1)..globals.len() {
                assert_ne!(globals[i], globals[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            SNDRV_TIMER_FLAG_SLAVE,
            SNDRV_TIMER_FLAG_AUTO,
            SNDRV_TIMER_FLAG_EARLY_EVENT,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            SNDRV_TIMER_EVENT_TICK,
            SNDRV_TIMER_EVENT_RESOLUTION,
            SNDRV_TIMER_EVENT_START,
            SNDRV_TIMER_EVENT_STOP,
            SNDRV_TIMER_EVENT_PAUSE,
            SNDRV_TIMER_EVENT_CONTINUE,
            SNDRV_TIMER_EVENT_EARLY,
            SNDRV_TIMER_EVENT_SUSPEND,
            SNDRV_TIMER_EVENT_RESUME,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}

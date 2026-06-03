//! `<sound/asound.h>` — ALSA timer classes, slave classes, and event types.
//!
//! ALSA timers expose periodic ticks (jiffies, HPET, RTC, sound-card
//! sample clocks) as devices that userspace and the sequencer can
//! synchronise to.

// ---------------------------------------------------------------------------
// Timer classes (`SNDRV_TIMER_CLASS_*`)
// ---------------------------------------------------------------------------

pub const SNDRV_TIMER_CLASS_NONE: i32 = -1;
pub const SNDRV_TIMER_CLASS_SLAVE: i32 = 0;
pub const SNDRV_TIMER_CLASS_GLOBAL: i32 = 1;
pub const SNDRV_TIMER_CLASS_CARD: i32 = 2;
pub const SNDRV_TIMER_CLASS_PCM: i32 = 3;
pub const SNDRV_TIMER_CLASS_LAST: i32 = SNDRV_TIMER_CLASS_PCM;

// ---------------------------------------------------------------------------
// Slave classes (`SNDRV_TIMER_SCLASS_*`)
// ---------------------------------------------------------------------------

pub const SNDRV_TIMER_SCLASS_NONE: i32 = 0;
pub const SNDRV_TIMER_SCLASS_APPLICATION: i32 = 1;
pub const SNDRV_TIMER_SCLASS_SEQUENCER: i32 = 2;
pub const SNDRV_TIMER_SCLASS_OSS_SEQUENCER: i32 = 3;
pub const SNDRV_TIMER_SCLASS_LAST: i32 = SNDRV_TIMER_SCLASS_OSS_SEQUENCER;

// ---------------------------------------------------------------------------
// Reserved global timer device numbers (within `CLASS_GLOBAL`)
// ---------------------------------------------------------------------------

pub const SNDRV_TIMER_GLOBAL_SYSTEM: u32 = 0;
pub const SNDRV_TIMER_GLOBAL_RTC: u32 = 1;
pub const SNDRV_TIMER_GLOBAL_HPET: u32 = 2;
pub const SNDRV_TIMER_GLOBAL_HRTIMER: u32 = 3;

// ---------------------------------------------------------------------------
// Timer flags (`SNDRV_TIMER_FLG_*`)
// ---------------------------------------------------------------------------

pub const SNDRV_TIMER_FLG_SLAVE: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Event types (`SNDRV_TIMER_EVENT_*`)
// ---------------------------------------------------------------------------

pub const SNDRV_TIMER_EVENT_RESOLUTION: i32 = 0;
pub const SNDRV_TIMER_EVENT_TICK: i32 = 1;
pub const SNDRV_TIMER_EVENT_START: i32 = 2;
pub const SNDRV_TIMER_EVENT_STOP: i32 = 3;
pub const SNDRV_TIMER_EVENT_CONTINUE: i32 = 4;
pub const SNDRV_TIMER_EVENT_PAUSE: i32 = 5;
pub const SNDRV_TIMER_EVENT_EARLY: i32 = 6;
pub const SNDRV_TIMER_EVENT_SUSPEND: i32 = 7;
pub const SNDRV_TIMER_EVENT_RESUME: i32 = 8;
pub const SNDRV_TIMER_EVENT_MSTART: i32 = 9;
pub const SNDRV_TIMER_EVENT_MSTOP: i32 = 10;
pub const SNDRV_TIMER_EVENT_MCONTINUE: i32 = 11;
pub const SNDRV_TIMER_EVENT_MPAUSE: i32 = 12;
pub const SNDRV_TIMER_EVENT_MSUSPEND: i32 = 13;
pub const SNDRV_TIMER_EVENT_MRESUME: i32 = 14;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timer_classes_dense_minus1_to_3() {
        let c = [
            SNDRV_TIMER_CLASS_NONE,
            SNDRV_TIMER_CLASS_SLAVE,
            SNDRV_TIMER_CLASS_GLOBAL,
            SNDRV_TIMER_CLASS_CARD,
            SNDRV_TIMER_CLASS_PCM,
        ];
        // None=-1; SLAVE..PCM is 0..3.
        assert_eq!(c[0], -1);
        for (i, &v) in c[1..].iter().enumerate() {
            assert_eq!(v, i as i32);
        }
        assert_eq!(SNDRV_TIMER_CLASS_LAST, 3);
    }

    #[test]
    fn test_slave_classes_dense_0_to_3() {
        let s = [
            SNDRV_TIMER_SCLASS_NONE,
            SNDRV_TIMER_SCLASS_APPLICATION,
            SNDRV_TIMER_SCLASS_SEQUENCER,
            SNDRV_TIMER_SCLASS_OSS_SEQUENCER,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v, i as i32);
        }
        assert_eq!(SNDRV_TIMER_SCLASS_LAST, 3);
    }

    #[test]
    fn test_global_timer_ids_dense_0_to_3() {
        let g = [
            SNDRV_TIMER_GLOBAL_SYSTEM,
            SNDRV_TIMER_GLOBAL_RTC,
            SNDRV_TIMER_GLOBAL_HPET,
            SNDRV_TIMER_GLOBAL_HRTIMER,
        ];
        for (i, &v) in g.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_flg_slave_single_bit() {
        assert!(SNDRV_TIMER_FLG_SLAVE.is_power_of_two());
    }

    #[test]
    fn test_event_types_dense_0_to_14() {
        let e = [
            SNDRV_TIMER_EVENT_RESOLUTION,
            SNDRV_TIMER_EVENT_TICK,
            SNDRV_TIMER_EVENT_START,
            SNDRV_TIMER_EVENT_STOP,
            SNDRV_TIMER_EVENT_CONTINUE,
            SNDRV_TIMER_EVENT_PAUSE,
            SNDRV_TIMER_EVENT_EARLY,
            SNDRV_TIMER_EVENT_SUSPEND,
            SNDRV_TIMER_EVENT_RESUME,
            SNDRV_TIMER_EVENT_MSTART,
            SNDRV_TIMER_EVENT_MSTOP,
            SNDRV_TIMER_EVENT_MCONTINUE,
            SNDRV_TIMER_EVENT_MPAUSE,
            SNDRV_TIMER_EVENT_MSUSPEND,
            SNDRV_TIMER_EVENT_MRESUME,
        ];
        for (i, &v) in e.iter().enumerate() {
            assert_eq!(v, i as i32);
        }
    }

    #[test]
    fn test_master_slave_event_pairs_offset() {
        // Master events MSTART..MRESUME (9..=14) mirror six slave events,
        // but `EARLY` (=6) intrudes between PAUSE and SUSPEND with no
        // master counterpart, so the constant offset is +7 for the first
        // four pairs and +6 for the last two.
        assert_eq!(SNDRV_TIMER_EVENT_MSTART - SNDRV_TIMER_EVENT_START, 7);
        assert_eq!(SNDRV_TIMER_EVENT_MSTOP - SNDRV_TIMER_EVENT_STOP, 7);
        assert_eq!(
            SNDRV_TIMER_EVENT_MCONTINUE - SNDRV_TIMER_EVENT_CONTINUE,
            7
        );
        assert_eq!(SNDRV_TIMER_EVENT_MPAUSE - SNDRV_TIMER_EVENT_PAUSE, 7);
        assert_eq!(
            SNDRV_TIMER_EVENT_MSUSPEND - SNDRV_TIMER_EVENT_SUSPEND,
            6
        );
        assert_eq!(SNDRV_TIMER_EVENT_MRESUME - SNDRV_TIMER_EVENT_RESUME, 6);
        // EARLY (=6) sits between PAUSE (5) and SUSPEND (7); has no master pair.
        assert_eq!(SNDRV_TIMER_EVENT_EARLY, 6);
    }
}

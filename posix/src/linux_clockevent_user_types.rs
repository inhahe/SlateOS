//! `<linux/clockchips.h>` — clock event device modes and features.
//!
//! Clock event devices generate timer interrupts at a programmed time.
//! Each device exposes a feature set (oneshot, periodic, frequency
//! programming) and runs in one of several modes (shutdown, periodic,
//! oneshot, oneshot_stopped). Programmers select the best clockevent
//! based on these features.

// ---------------------------------------------------------------------------
// Clock event modes
// ---------------------------------------------------------------------------

pub const CLOCK_EVT_STATE_DETACHED: u32 = 0;
pub const CLOCK_EVT_STATE_SHUTDOWN: u32 = 1;
pub const CLOCK_EVT_STATE_PERIODIC: u32 = 2;
pub const CLOCK_EVT_STATE_ONESHOT: u32 = 3;
pub const CLOCK_EVT_STATE_ONESHOT_STOPPED: u32 = 4;

// ---------------------------------------------------------------------------
// Clock event features
// ---------------------------------------------------------------------------

pub const CLOCK_EVT_FEAT_PERIODIC: u32 = 1 << 0;
pub const CLOCK_EVT_FEAT_ONESHOT: u32 = 1 << 1;
pub const CLOCK_EVT_FEAT_KTIME: u32 = 1 << 2;
pub const CLOCK_EVT_FEAT_C3STOP: u32 = 1 << 3;
pub const CLOCK_EVT_FEAT_DUMMY: u32 = 1 << 4;
pub const CLOCK_EVT_FEAT_PERCPU: u32 = 1 << 5;
pub const CLOCK_EVT_FEAT_HRTIMER: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Default rating bounds (higher = preferred)
// ---------------------------------------------------------------------------

/// Lowest-rated device (fallback only).
pub const CLOCK_EVT_RATING_MIN: u32 = 1;
/// Typical embedded SoC timer.
pub const CLOCK_EVT_RATING_GOOD: u32 = 300;
/// HPET or modern x86 LAPIC timer.
pub const CLOCK_EVT_RATING_BEST: u32 = 400;
/// Maximum rating (theoretical).
pub const CLOCK_EVT_RATING_MAX: u32 = 1000;

// ---------------------------------------------------------------------------
// Tick device kinds
// ---------------------------------------------------------------------------

pub const TICK_DEVICE_MODE_PERIODIC: u32 = 0;
pub const TICK_DEVICE_MODE_ONESHOT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_dense_0_to_4() {
        let s = [
            CLOCK_EVT_STATE_DETACHED,
            CLOCK_EVT_STATE_SHUTDOWN,
            CLOCK_EVT_STATE_PERIODIC,
            CLOCK_EVT_STATE_ONESHOT,
            CLOCK_EVT_STATE_ONESHOT_STOPPED,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_features_distinct_single_bit() {
        let f = [
            CLOCK_EVT_FEAT_PERIODIC,
            CLOCK_EVT_FEAT_ONESHOT,
            CLOCK_EVT_FEAT_KTIME,
            CLOCK_EVT_FEAT_C3STOP,
            CLOCK_EVT_FEAT_DUMMY,
            CLOCK_EVT_FEAT_PERCPU,
            CLOCK_EVT_FEAT_HRTIMER,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
        // OR of all = low 7 bits = 0x7F.
        let or_all = f.iter().fold(0u32, |a, &v| a | v);
        assert_eq!(or_all, 0x7F);
    }

    #[test]
    fn test_rating_ordering() {
        assert!(CLOCK_EVT_RATING_MIN < CLOCK_EVT_RATING_GOOD);
        assert!(CLOCK_EVT_RATING_GOOD < CLOCK_EVT_RATING_BEST);
        assert!(CLOCK_EVT_RATING_BEST < CLOCK_EVT_RATING_MAX);
        assert_eq!(CLOCK_EVT_RATING_MAX, 1000);
    }

    #[test]
    fn test_tick_device_modes_binary() {
        assert_eq!(TICK_DEVICE_MODE_PERIODIC, 0);
        assert_eq!(TICK_DEVICE_MODE_ONESHOT, 1);
    }

    #[test]
    fn test_periodic_and_oneshot_are_independent_features() {
        // A device can support both — they are not mutually exclusive.
        assert_ne!(
            CLOCK_EVT_FEAT_PERIODIC & CLOCK_EVT_FEAT_ONESHOT,
            CLOCK_EVT_FEAT_PERIODIC,
        );
    }
}

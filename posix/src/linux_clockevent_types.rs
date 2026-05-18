//! `<linux/clockchips.h>` — clock event device mode and feature constants.
//!
//! Clock event devices deliver timer interrupts at programmed
//! intervals or one-shot deadlines. The kernel uses them for the
//! scheduler tick, high-resolution timers, and deadline-based
//! wakeups. Each CPU typically has its own local clock event device
//! (e.g. LAPIC timer, ARM architected timer).

// ---------------------------------------------------------------------------
// Clock event device modes
// ---------------------------------------------------------------------------

/// Device is unused / shutdown.
pub const CLOCK_EVT_MODE_UNUSED: u32 = 0;
/// Device is shut down (no interrupts).
pub const CLOCK_EVT_MODE_SHUTDOWN: u32 = 1;
/// Periodic mode: interrupts at fixed intervals.
pub const CLOCK_EVT_MODE_PERIODIC: u32 = 2;
/// One-shot mode: single interrupt at programmed deadline.
pub const CLOCK_EVT_MODE_ONESHOT: u32 = 3;
/// One-shot stopped: armed but will not fire until reprogrammed.
pub const CLOCK_EVT_MODE_ONESHOT_STOPPED: u32 = 4;
/// Resume mode: device is resuming from suspend.
pub const CLOCK_EVT_MODE_RESUME: u32 = 5;

// ---------------------------------------------------------------------------
// Clock event features
// ---------------------------------------------------------------------------

/// Device supports periodic mode.
pub const CLOCK_EVT_FEAT_PERIODIC: u32 = 0x0001;
/// Device supports one-shot mode.
pub const CLOCK_EVT_FEAT_ONESHOT: u32 = 0x0002;
/// Device wraps around (counter is finite width).
pub const CLOCK_EVT_FEAT_KTIME: u32 = 0x0004;
/// Device supports C3-stop (survives deep idle).
pub const CLOCK_EVT_FEAT_C3STOP: u32 = 0x0008;
/// Device supports one-shot stopped state.
pub const CLOCK_EVT_FEAT_ONESHOT_STOPPED: u32 = 0x0010;
/// Device is per-CPU local.
pub const CLOCK_EVT_FEAT_PERCPU: u32 = 0x0020;
/// Device has dynamic IRQ affinity.
pub const CLOCK_EVT_FEAT_DYNIRQ: u32 = 0x0040;
/// Device is a dummy (never fires).
pub const CLOCK_EVT_FEAT_DUMMY: u32 = 0x0080;

// ---------------------------------------------------------------------------
// Clock event state (newer API, replacing modes)
// ---------------------------------------------------------------------------

/// State: detached from framework.
pub const CLOCK_EVT_STATE_DETACHED: u32 = 0;
/// State: shutdown.
pub const CLOCK_EVT_STATE_SHUTDOWN: u32 = 1;
/// State: periodic mode active.
pub const CLOCK_EVT_STATE_PERIODIC: u32 = 2;
/// State: one-shot mode active.
pub const CLOCK_EVT_STATE_ONESHOT: u32 = 3;
/// State: one-shot stopped.
pub const CLOCK_EVT_STATE_ONESHOT_STOPPED: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            CLOCK_EVT_MODE_UNUSED, CLOCK_EVT_MODE_SHUTDOWN,
            CLOCK_EVT_MODE_PERIODIC, CLOCK_EVT_MODE_ONESHOT,
            CLOCK_EVT_MODE_ONESHOT_STOPPED, CLOCK_EVT_MODE_RESUME,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_features_no_overlap() {
        let feats = [
            CLOCK_EVT_FEAT_PERIODIC, CLOCK_EVT_FEAT_ONESHOT,
            CLOCK_EVT_FEAT_KTIME, CLOCK_EVT_FEAT_C3STOP,
            CLOCK_EVT_FEAT_ONESHOT_STOPPED, CLOCK_EVT_FEAT_PERCPU,
            CLOCK_EVT_FEAT_DYNIRQ, CLOCK_EVT_FEAT_DUMMY,
        ];
        for i in 0..feats.len() {
            assert!(feats[i].is_power_of_two());
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_states_sequential() {
        assert_eq!(CLOCK_EVT_STATE_DETACHED, 0);
        assert_eq!(CLOCK_EVT_STATE_SHUTDOWN, 1);
        assert_eq!(CLOCK_EVT_STATE_PERIODIC, 2);
        assert_eq!(CLOCK_EVT_STATE_ONESHOT, 3);
        assert_eq!(CLOCK_EVT_STATE_ONESHOT_STOPPED, 4);
    }
}

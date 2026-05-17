//! `<linux/hrtimer.h>` — High-resolution timer constants.
//!
//! High-resolution timers (hrtimers) provide nanosecond-precision
//! timer infrastructure in the Linux kernel. Unlike the older timer
//! wheel (jiffies-resolution), hrtimers use a red-black tree sorted
//! by expiration time. They drive nanosleep(), POSIX timers, scheduler
//! tick (tickless operation), and epoll/poll timeouts. On hardware
//! that supports it, hrtimers can achieve sub-microsecond precision.

// ---------------------------------------------------------------------------
// hrtimer modes
// ---------------------------------------------------------------------------

/// Absolute time (expire at specific time).
pub const HRTIMER_MODE_ABS: u32 = 0x00;
/// Relative time (expire after duration from now).
pub const HRTIMER_MODE_REL: u32 = 0x01;
/// Pinned: timer fires on the CPU where it was started.
pub const HRTIMER_MODE_PINNED: u32 = 0x02;
/// Soft: timer fires in softirq context (not hard IRQ).
pub const HRTIMER_MODE_SOFT: u32 = 0x04;
/// Hard: timer fires in hard IRQ context.
pub const HRTIMER_MODE_HARD: u32 = 0x08;

// ---------------------------------------------------------------------------
// hrtimer states
// ---------------------------------------------------------------------------

/// Timer is inactive (not enqueued).
pub const HRTIMER_STATE_INACTIVE: u32 = 0x00;
/// Timer is enqueued in the timer tree.
pub const HRTIMER_STATE_ENQUEUED: u32 = 0x01;

// ---------------------------------------------------------------------------
// hrtimer restart return values
// ---------------------------------------------------------------------------

/// Timer should not be restarted.
pub const HRTIMER_NORESTART: u32 = 0;
/// Timer should be restarted (periodic).
pub const HRTIMER_RESTART: u32 = 1;

// ---------------------------------------------------------------------------
// hrtimer clock bases
// ---------------------------------------------------------------------------

/// Monotonic clock base.
pub const HRTIMER_BASE_MONOTONIC: u32 = 0;
/// Realtime clock base.
pub const HRTIMER_BASE_REALTIME: u32 = 1;
/// Boottime clock base.
pub const HRTIMER_BASE_BOOTTIME: u32 = 2;
/// TAI clock base.
pub const HRTIMER_BASE_TAI: u32 = 3;
/// Monotonic soft base.
pub const HRTIMER_BASE_MONOTONIC_SOFT: u32 = 4;
/// Realtime soft base.
pub const HRTIMER_BASE_REALTIME_SOFT: u32 = 5;
/// Boottime soft base.
pub const HRTIMER_BASE_BOOTTIME_SOFT: u32 = 6;
/// TAI soft base.
pub const HRTIMER_BASE_TAI_SOFT: u32 = 7;
/// Number of clock bases.
pub const HRTIMER_MAX_CLOCK_BASES: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_composable() {
        // ABS is 0, so REL | PINNED is valid
        let mode = HRTIMER_MODE_REL | HRTIMER_MODE_PINNED;
        assert_eq!(mode, 0x03);
        // SOFT and HARD should not overlap
        assert_eq!(HRTIMER_MODE_SOFT & HRTIMER_MODE_HARD, 0);
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(HRTIMER_STATE_INACTIVE, HRTIMER_STATE_ENQUEUED);
    }

    #[test]
    fn test_restart_values_distinct() {
        assert_ne!(HRTIMER_NORESTART, HRTIMER_RESTART);
    }

    #[test]
    fn test_clock_bases_distinct() {
        let bases = [
            HRTIMER_BASE_MONOTONIC, HRTIMER_BASE_REALTIME,
            HRTIMER_BASE_BOOTTIME, HRTIMER_BASE_TAI,
            HRTIMER_BASE_MONOTONIC_SOFT, HRTIMER_BASE_REALTIME_SOFT,
            HRTIMER_BASE_BOOTTIME_SOFT, HRTIMER_BASE_TAI_SOFT,
        ];
        assert_eq!(bases.len(), HRTIMER_MAX_CLOCK_BASES as usize);
        for i in 0..bases.len() {
            for j in (i + 1)..bases.len() {
                assert_ne!(bases[i], bases[j]);
            }
        }
    }
}

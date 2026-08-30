//! `<linux/hrtimer.h>` / `<linux/time.h>` — high-resolution timer modes.
//!
//! The kernel's hrtimer infrastructure backs `clock_nanosleep`,
//! `timerfd_create`, POSIX timers, and io_uring's `IORING_OP_TIMEOUT`.
//! Userspace selects relative vs absolute deadlines and wall-clock
//! cancellation behavior with the flag bits below.

// ---------------------------------------------------------------------------
// `enum hrtimer_mode` — kernel-internal selection bits visible to userspace
// via timerfd / clock_nanosleep flags.
// ---------------------------------------------------------------------------

/// Relative timer expiry (deadline = now + expires).
pub const HRTIMER_MODE_REL: u32 = 0x00;
/// Absolute timer expiry (deadline = expires).
pub const HRTIMER_MODE_ABS: u32 = 0x01;
/// Soft-IRQ context delivery (the default; non-hard-IRQ).
pub const HRTIMER_MODE_PINNED: u32 = 0x02;
/// Pinned to the issuing CPU.
pub const HRTIMER_MODE_SOFT: u32 = 0x04;
/// Run the callback in hard-IRQ context.
pub const HRTIMER_MODE_HARD: u32 = 0x08;

// ---------------------------------------------------------------------------
// `clock_nanosleep` / `timerfd_settime` flag (uapi-visible)
// ---------------------------------------------------------------------------

/// `TIMER_ABSTIME` — interpret `it_value` as an absolute time.
pub const TIMER_ABSTIME: u32 = 0x01;

// ---------------------------------------------------------------------------
// `timerfd_create` / `timerfd_settime` flags
// ---------------------------------------------------------------------------

/// `TFD_CLOEXEC` — set close-on-exec on the returned fd.
pub const TFD_CLOEXEC: u32 = 0o2000000;
/// `TFD_NONBLOCK` — set O_NONBLOCK on the returned fd.
pub const TFD_NONBLOCK: u32 = 0o4000;
/// `TFD_TIMER_ABSTIME` — interpret `it_value` as absolute (settime flag).
pub const TFD_TIMER_ABSTIME: u32 = 1 << 0;
/// `TFD_TIMER_CANCEL_ON_SET` — cancel timer if wall clock is discontinuously
/// changed (settime flag).
pub const TFD_TIMER_CANCEL_ON_SET: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Resolution bounds for hrtimer clock sources
// ---------------------------------------------------------------------------

/// One nanosecond — the hrtimer subsystem's tick unit.
pub const NSEC_PER_SEC: u64 = 1_000_000_000;
/// One microsecond, expressed in ns.
pub const NSEC_PER_USEC: u64 = 1_000;
/// One millisecond, expressed in ns.
pub const NSEC_PER_MSEC: u64 = 1_000_000;
/// Default jiffy frequency exposed by HZ (kernel-config-dependent;
/// the 1000 value matches CONFIG_HZ_1000).
pub const HZ_DEFAULT: u32 = 1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_bits_distinct() {
        // REL=0, ABS=1, PINNED=2, SOFT=4, HARD=8. ABS|PINNED|SOFT|HARD
        // is a union of three distinct power-of-two bits.
        for &b in &[HRTIMER_MODE_PINNED, HRTIMER_MODE_SOFT, HRTIMER_MODE_HARD] {
            assert!(b.is_power_of_two());
        }
        assert_eq!(HRTIMER_MODE_REL, 0);
        assert_eq!(HRTIMER_MODE_ABS, 1);
    }

    #[test]
    fn test_timer_abstime_aliases_abs_mode_bit() {
        // TIMER_ABSTIME==1 lines up with HRTIMER_MODE_ABS.
        assert_eq!(TIMER_ABSTIME, HRTIMER_MODE_ABS);
        // And with the timerfd-level ABSTIME flag.
        assert_eq!(TFD_TIMER_ABSTIME, TIMER_ABSTIME);
    }

    #[test]
    fn test_timerfd_create_flags_match_open_flags() {
        // Must match O_CLOEXEC / O_NONBLOCK on Linux.
        assert_eq!(TFD_CLOEXEC, 0o2000000);
        assert_eq!(TFD_NONBLOCK, 0o4000);
    }

    #[test]
    fn test_timerfd_settime_flags_pow2() {
        assert!(TFD_TIMER_ABSTIME.is_power_of_two());
        assert!(TFD_TIMER_CANCEL_ON_SET.is_power_of_two());
        assert_ne!(TFD_TIMER_ABSTIME, TFD_TIMER_CANCEL_ON_SET);
    }

    #[test]
    fn test_time_units_ratio() {
        assert_eq!(NSEC_PER_SEC / NSEC_PER_MSEC, 1_000);
        assert_eq!(NSEC_PER_SEC / NSEC_PER_USEC, 1_000_000);
        assert_eq!(NSEC_PER_MSEC / NSEC_PER_USEC, 1_000);
        // 1000 Hz means a tick per millisecond.
        assert_eq!(NSEC_PER_SEC / u64::from(HZ_DEFAULT), NSEC_PER_MSEC);
    }
}

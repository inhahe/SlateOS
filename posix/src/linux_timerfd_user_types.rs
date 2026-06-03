//! `<sys/timerfd.h>` — POSIX timers exposed as file descriptors.
//!
//! `timerfd_create(2)` returns an fd that becomes readable when the
//! timer expires. Reading consumes the expiration count as a `u64`.
//! Combined with `epoll(7)`, this is how every modern event loop
//! integrates timeouts without `SIGALRM` or `pselect` tricks.

// ---------------------------------------------------------------------------
// `timerfd_create` flags
// ---------------------------------------------------------------------------

pub const TFD_CLOEXEC: u32 = 0o2000000;
pub const TFD_NONBLOCK: u32 = 0o0004000;

// ---------------------------------------------------------------------------
// `timerfd_settime` flags
// ---------------------------------------------------------------------------

pub const TFD_TIMER_ABSTIME: u32 = 1 << 0;
pub const TFD_TIMER_CANCEL_ON_SET: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Clock IDs accepted by `timerfd_create` (subset of `<time.h>` clocks)
// ---------------------------------------------------------------------------

pub const TFD_CLOCK_REALTIME: u32 = 0;
pub const TFD_CLOCK_MONOTONIC: u32 = 1;
pub const TFD_CLOCK_BOOTTIME: u32 = 7;
pub const TFD_CLOCK_REALTIME_ALARM: u32 = 8;
pub const TFD_CLOCK_BOOTTIME_ALARM: u32 = 9;

// ---------------------------------------------------------------------------
// I/O contract — each `read(fd)` returns an 8-byte little-endian u64
// expiration counter.
// ---------------------------------------------------------------------------

pub const TFD_READ_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// Linux x86_64 syscall numbers
// ---------------------------------------------------------------------------

pub const NR_TIMERFD_CREATE: u32 = 283;
pub const NR_TIMERFD_SETTIME: u32 = 286;
pub const NR_TIMERFD_GETTIME: u32 = 287;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_flags_match_o_flags() {
        // TFD_CLOEXEC and TFD_NONBLOCK share their bit values with the
        // generic O_CLOEXEC (0o2000000) / O_NONBLOCK (0o4000) flags —
        // so the kernel can pass them straight to the fd setup.
        assert_eq!(TFD_CLOEXEC, 0o2000000);
        assert_eq!(TFD_NONBLOCK, 0o4000);
        // And they don't collide.
        assert_eq!(TFD_CLOEXEC & TFD_NONBLOCK, 0);
    }

    #[test]
    fn test_settime_flags_low_2_bits() {
        // settime flags occupy bits 0..1 only.
        assert_eq!(TFD_TIMER_ABSTIME, 1);
        assert_eq!(TFD_TIMER_CANCEL_ON_SET, 2);
        assert_eq!(TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET, 0x3);
    }

    #[test]
    fn test_clock_ids_match_time_h() {
        // timerfd clock IDs are exactly the same numbers as <time.h>'s
        // CLOCK_* — that's deliberate, the kernel uses one switch.
        assert_eq!(TFD_CLOCK_REALTIME, 0);
        assert_eq!(TFD_CLOCK_MONOTONIC, 1);
        assert_eq!(TFD_CLOCK_BOOTTIME, 7);
        assert_eq!(TFD_CLOCK_REALTIME_ALARM, 8);
        assert_eq!(TFD_CLOCK_BOOTTIME_ALARM, 9);
        // The two ALARM clocks are adjacent.
        assert_eq!(TFD_CLOCK_BOOTTIME_ALARM, TFD_CLOCK_REALTIME_ALARM + 1);
    }

    #[test]
    fn test_read_size_is_8() {
        // The read() result is always a single u64 expiration count.
        assert_eq!(TFD_READ_SIZE, core::mem::size_of::<u64>());
    }

    #[test]
    fn test_syscall_numbers_x86_64() {
        assert_eq!(NR_TIMERFD_CREATE, 283);
        // _settime and _gettime are adjacent.
        assert_eq!(NR_TIMERFD_SETTIME, 286);
        assert_eq!(NR_TIMERFD_GETTIME, NR_TIMERFD_SETTIME + 1);
    }
}

//! `<time.h>` — POSIX clocks and time syscalls.
//!
//! The `CLOCK_*` IDs are the keys passed to `clock_gettime(2)`,
//! `clock_settime(2)`, `clock_nanosleep(2)`, `timer_create(2)`,
//! and friends. They identify which kernel timekeeper to consult
//! (wall clock, monotonic, per-CPU, etc.).

// ---------------------------------------------------------------------------
// `clockid_t` values (`CLOCK_*`)
// ---------------------------------------------------------------------------

pub const CLOCK_REALTIME: u32 = 0;
pub const CLOCK_MONOTONIC: u32 = 1;
pub const CLOCK_PROCESS_CPUTIME_ID: u32 = 2;
pub const CLOCK_THREAD_CPUTIME_ID: u32 = 3;
pub const CLOCK_MONOTONIC_RAW: u32 = 4;
pub const CLOCK_REALTIME_COARSE: u32 = 5;
pub const CLOCK_MONOTONIC_COARSE: u32 = 6;
pub const CLOCK_BOOTTIME: u32 = 7;
pub const CLOCK_REALTIME_ALARM: u32 = 8;
pub const CLOCK_BOOTTIME_ALARM: u32 = 9;
pub const CLOCK_SGI_CYCLE: u32 = 10;
pub const CLOCK_TAI: u32 = 11;

pub const MAX_CLOCKS: u32 = 16;

// ---------------------------------------------------------------------------
// `clock_nanosleep` / `timer_settime` flags
// ---------------------------------------------------------------------------

pub const TIMER_ABSTIME: u32 = 0x01;

// ---------------------------------------------------------------------------
// Time scale constants
// ---------------------------------------------------------------------------

pub const NSEC_PER_SEC: u64 = 1_000_000_000;
pub const USEC_PER_SEC: u64 = 1_000_000;
pub const MSEC_PER_SEC: u64 = 1_000;
pub const NSEC_PER_USEC: u64 = 1_000;
pub const NSEC_PER_MSEC: u64 = 1_000_000;
pub const USEC_PER_MSEC: u64 = 1_000;

pub const SECS_PER_MIN: u64 = 60;
pub const MINS_PER_HOUR: u64 = 60;
pub const HOURS_PER_DAY: u64 = 24;
pub const SECS_PER_HOUR: u64 = SECS_PER_MIN * MINS_PER_HOUR;
pub const SECS_PER_DAY: u64 = SECS_PER_HOUR * HOURS_PER_DAY;

// ---------------------------------------------------------------------------
// Linux x86_64 syscall numbers for time-related calls
// ---------------------------------------------------------------------------

pub const NR_NANOSLEEP: u32 = 35;
pub const NR_GETTIMEOFDAY: u32 = 96;
pub const NR_SETTIMEOFDAY: u32 = 164;
pub const NR_CLOCK_SETTIME: u32 = 227;
pub const NR_CLOCK_GETTIME: u32 = 228;
pub const NR_CLOCK_GETRES: u32 = 229;
pub const NR_CLOCK_NANOSLEEP: u32 = 230;
pub const NR_TIMER_CREATE: u32 = 222;
pub const NR_TIMER_SETTIME: u32 = 223;
pub const NR_TIMER_GETTIME: u32 = 224;
pub const NR_TIMER_GETOVERRUN: u32 = 225;
pub const NR_TIMER_DELETE: u32 = 226;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clocks_dense_0_to_11() {
        let c = [
            CLOCK_REALTIME,
            CLOCK_MONOTONIC,
            CLOCK_PROCESS_CPUTIME_ID,
            CLOCK_THREAD_CPUTIME_ID,
            CLOCK_MONOTONIC_RAW,
            CLOCK_REALTIME_COARSE,
            CLOCK_MONOTONIC_COARSE,
            CLOCK_BOOTTIME,
            CLOCK_REALTIME_ALARM,
            CLOCK_BOOTTIME_ALARM,
            CLOCK_SGI_CYCLE,
            CLOCK_TAI,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // Linux caps clockid_t at 16 (4 lowest bits — high bits are
        // reused for dynamic clock fd ids).
        assert!(CLOCK_TAI < MAX_CLOCKS);
        assert_eq!(MAX_CLOCKS, 16);
    }

    #[test]
    fn test_timer_abstime_is_bit_zero() {
        // TIMER_ABSTIME is the only flag for clock_nanosleep/timer_settime.
        assert_eq!(TIMER_ABSTIME, 1);
    }

    #[test]
    fn test_time_scales_consistent() {
        // The triangle: NSEC == USEC * NSEC_PER_USEC == MSEC * NSEC_PER_MSEC.
        assert_eq!(NSEC_PER_SEC, USEC_PER_SEC * NSEC_PER_USEC);
        assert_eq!(NSEC_PER_SEC, MSEC_PER_SEC * NSEC_PER_MSEC);
        assert_eq!(USEC_PER_SEC, MSEC_PER_SEC * USEC_PER_MSEC);
        assert_eq!(NSEC_PER_SEC, 1_000_000_000);
    }

    #[test]
    fn test_calendar_math() {
        assert_eq!(SECS_PER_HOUR, 3600);
        assert_eq!(SECS_PER_DAY, 86_400);
        assert_eq!(SECS_PER_DAY, SECS_PER_HOUR * 24);
    }

    #[test]
    fn test_syscall_numbers_x86_64() {
        // Spot-check classic syscall numbers from arch/x86/entry/syscalls/syscall_64.tbl.
        assert_eq!(NR_NANOSLEEP, 35);
        assert_eq!(NR_GETTIMEOFDAY, 96);
        assert_eq!(NR_SETTIMEOFDAY, 164);
        // clock_* syscalls form a dense block 227..230.
        assert_eq!(NR_CLOCK_GETTIME, NR_CLOCK_SETTIME + 1);
        assert_eq!(NR_CLOCK_GETRES, NR_CLOCK_GETTIME + 1);
        assert_eq!(NR_CLOCK_NANOSLEEP, NR_CLOCK_GETRES + 1);
        // timer_* form 222..226.
        assert_eq!(NR_TIMER_SETTIME, NR_TIMER_CREATE + 1);
        assert_eq!(NR_TIMER_GETTIME, NR_TIMER_SETTIME + 1);
        assert_eq!(NR_TIMER_GETOVERRUN, NR_TIMER_GETTIME + 1);
        assert_eq!(NR_TIMER_DELETE, NR_TIMER_GETOVERRUN + 1);
    }
}

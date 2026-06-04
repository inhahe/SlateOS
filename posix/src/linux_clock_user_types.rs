//! `<sys/time.h>` — ITIMER kinds, struct tm layout, and broken-down time.
//!
//! Legacy BSD interval timers (`setitimer`, `getitimer`) come in three
//! flavors based on which clock they consume. `struct tm` represents
//! broken-down calendar time used by `mktime`, `strftime`, and friends.

// ---------------------------------------------------------------------------
// ITIMER which-arg values
// ---------------------------------------------------------------------------

/// Real (wall-clock) interval timer — sends SIGALRM.
pub const ITIMER_REAL: u32 = 0;
/// Virtual interval timer — user CPU time only, sends SIGVTALRM.
pub const ITIMER_VIRTUAL: u32 = 1;
/// Profiling interval timer — user + system CPU, sends SIGPROF.
pub const ITIMER_PROF: u32 = 2;

// ---------------------------------------------------------------------------
// struct tm field offsets (kernel and glibc share this layout on 64-bit)
// ---------------------------------------------------------------------------

/// `int tm_sec` — seconds [0, 60] (61 allows for leap second).
pub const TM_OFF_SEC: usize = 0;
/// `int tm_min` — minutes [0, 59].
pub const TM_OFF_MIN: usize = 4;
/// `int tm_hour` — hours [0, 23].
pub const TM_OFF_HOUR: usize = 8;
/// `int tm_mday` — day of month [1, 31].
pub const TM_OFF_MDAY: usize = 12;
/// `int tm_mon` — month [0, 11].
pub const TM_OFF_MON: usize = 16;
/// `int tm_year` — year - 1900.
pub const TM_OFF_YEAR: usize = 20;
/// `int tm_wday` — day of week [0, 6], Sunday = 0.
pub const TM_OFF_WDAY: usize = 24;
/// `int tm_yday` — day of year [0, 365].
pub const TM_OFF_YDAY: usize = 28;
/// `int tm_isdst` — DST flag (negative = unknown).
pub const TM_OFF_ISDST: usize = 32;

// ---------------------------------------------------------------------------
// Broken-down time field ranges
// ---------------------------------------------------------------------------

pub const TM_SEC_MAX: u32 = 60;
pub const TM_MIN_MAX: u32 = 59;
pub const TM_HOUR_MAX: u32 = 23;
pub const TM_MDAY_MIN: u32 = 1;
pub const TM_MDAY_MAX: u32 = 31;
pub const TM_MON_MAX: u32 = 11;
pub const TM_WDAY_MAX: u32 = 6;
pub const TM_YDAY_MAX: u32 = 365;

// ---------------------------------------------------------------------------
// Year offset for tm_year
// ---------------------------------------------------------------------------

/// `tm_year` is years since 1900. Add this to get the absolute year.
pub const TM_YEAR_BASE: i32 = 1900;

// ---------------------------------------------------------------------------
// Time conversion constants
// ---------------------------------------------------------------------------

pub const SECONDS_PER_MINUTE: u64 = 60;
pub const SECONDS_PER_HOUR: u64 = 60 * 60;
pub const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
pub const DAYS_PER_WEEK: u64 = 7;
pub const DAYS_PER_YEAR_AVG: u64 = 365;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_itimer_kinds_dense_0_to_2() {
        let it = [ITIMER_REAL, ITIMER_VIRTUAL, ITIMER_PROF];
        for (i, &v) in it.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_tm_layout_nine_consecutive_ints() {
        let o = [
            TM_OFF_SEC,
            TM_OFF_MIN,
            TM_OFF_HOUR,
            TM_OFF_MDAY,
            TM_OFF_MON,
            TM_OFF_YEAR,
            TM_OFF_WDAY,
            TM_OFF_YDAY,
            TM_OFF_ISDST,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v, i * 4);
        }
    }

    #[test]
    fn test_field_ranges_sensible() {
        // Seconds allows 60 to accommodate leap seconds.
        assert_eq!(TM_SEC_MAX, 60);
        // Minutes and hours are 0..=N-1.
        assert_eq!(TM_MIN_MAX, 59);
        assert_eq!(TM_HOUR_MAX, 23);
        // Day of month is 1-based.
        assert_eq!(TM_MDAY_MIN, 1);
        assert_eq!(TM_MDAY_MAX, 31);
        // Month is 0-based.
        assert_eq!(TM_MON_MAX, 11);
        // Day-of-week 0..6.
        assert_eq!(TM_WDAY_MAX, 6);
        // Day-of-year up to 365 for leap years.
        assert_eq!(TM_YDAY_MAX, 365);
    }

    #[test]
    fn test_tm_year_base_is_1900() {
        assert_eq!(TM_YEAR_BASE, 1900);
        // tm_year=0 means year 1900; tm_year=125 means 2025.
        assert_eq!(TM_YEAR_BASE + 125, 2025);
    }

    #[test]
    fn test_time_conversions_consistent() {
        assert_eq!(SECONDS_PER_MINUTE, 60);
        assert_eq!(SECONDS_PER_HOUR, 60 * SECONDS_PER_MINUTE);
        assert_eq!(SECONDS_PER_DAY, 24 * SECONDS_PER_HOUR);
        assert_eq!(SECONDS_PER_DAY, 86_400);
        assert_eq!(DAYS_PER_WEEK, 7);
        assert_eq!(DAYS_PER_YEAR_AVG, 365);
    }
}

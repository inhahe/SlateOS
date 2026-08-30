//! `<time.h>` — struct timespec layout and time constants.
//!
//! `struct timespec` is the fundamental time representation
//! used by `clock_gettime()`, `nanosleep()`, `futex()`, and
//! many other POSIX and Linux interfaces.

// ---------------------------------------------------------------------------
// struct timespec field offsets (Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of tv_sec (seconds) in struct timespec.
pub const TIMESPEC_OFF_SEC: u32 = 0;
/// Offset of tv_nsec (nanoseconds) in struct timespec.
pub const TIMESPEC_OFF_NSEC: u32 = 8;
/// Size of struct timespec on x86_64 (bytes).
pub const TIMESPEC_SIZE: u32 = 16;

// ---------------------------------------------------------------------------
// Time conversion constants
// ---------------------------------------------------------------------------

/// Seconds per minute.
pub const SECS_PER_MIN: u64 = 60;
/// Seconds per hour.
pub const SECS_PER_HOUR: u64 = 3600;
/// Seconds per day.
pub const SECS_PER_DAY: u64 = 86400;
/// Days per (non-leap) year.
pub const DAYS_PER_YEAR: u32 = 365;

// ---------------------------------------------------------------------------
// Epoch
// ---------------------------------------------------------------------------

/// Unix epoch year (January 1, 1970).
pub const EPOCH_YEAR: u32 = 1970;
/// Unix epoch day-of-week (Thursday = 4).
pub const EPOCH_WDAY: u32 = 4;

// ---------------------------------------------------------------------------
// Y2038 limits (time_t on 32-bit systems)
// ---------------------------------------------------------------------------

/// Maximum 32-bit time_t value (January 19, 2038 03:14:07 UTC).
pub const TIME32_MAX: i32 = 0x7FFFFFFF;
/// Minimum 32-bit time_t value.
pub const TIME32_MIN: i32 = -0x80000000_i32;
/// Maximum 64-bit time_t value.
pub const TIME64_MAX: i64 = i64::MAX;

// ---------------------------------------------------------------------------
// tm structure field indices
// ---------------------------------------------------------------------------

/// Index of tm_sec in struct tm.
pub const TM_SEC: u32 = 0;
/// Index of tm_min in struct tm.
pub const TM_MIN: u32 = 1;
/// Index of tm_hour in struct tm.
pub const TM_HOUR: u32 = 2;
/// Index of tm_mday in struct tm.
pub const TM_MDAY: u32 = 3;
/// Index of tm_mon in struct tm.
pub const TM_MON: u32 = 4;
/// Index of tm_year in struct tm.
pub const TM_YEAR: u32 = 5;
/// Index of tm_wday in struct tm.
pub const TM_WDAY: u32 = 6;
/// Index of tm_yday in struct tm.
pub const TM_YDAY: u32 = 7;
/// Index of tm_isdst in struct tm.
pub const TM_ISDST: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timespec_layout() {
        assert_eq!(TIMESPEC_OFF_SEC, 0);
        assert_eq!(TIMESPEC_OFF_NSEC, 8);
        assert_eq!(TIMESPEC_SIZE, 16);
    }

    #[test]
    fn test_time_conversions() {
        assert_eq!(SECS_PER_MIN, 60);
        assert_eq!(SECS_PER_HOUR, 3600);
        assert_eq!(SECS_PER_DAY, 86400);
        assert_eq!(SECS_PER_HOUR, SECS_PER_MIN * 60);
        assert_eq!(SECS_PER_DAY, SECS_PER_HOUR * 24);
    }

    #[test]
    fn test_epoch_year() {
        assert_eq!(EPOCH_YEAR, 1970);
    }

    #[test]
    fn test_epoch_wday() {
        assert_eq!(EPOCH_WDAY, 4); // Thursday
    }

    #[test]
    fn test_time32_max() {
        assert_eq!(TIME32_MAX, 0x7FFFFFFF);
    }

    #[test]
    fn test_tm_indices_sequential() {
        assert_eq!(TM_SEC, 0);
        assert_eq!(TM_ISDST, 8);
    }

    #[test]
    fn test_days_per_year() {
        assert_eq!(DAYS_PER_YEAR, 365);
    }
}

//! C standard limits (`<limits.h>`, `<stdint.h>` constants).
//!
//! Exports the numeric limits that C programs expect as external
//! symbols.  These match the x86_64 LP64 data model (ILP32 would
//! need different values for some).
//!
//! Programs typically access these via preprocessor macros in the
//! C headers, but some reference them as external symbols when
//! compiled with certain flags.

// ---------------------------------------------------------------------------
// <limits.h> — character and integer limits
// ---------------------------------------------------------------------------

/// Bits in a char.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static CHAR_BIT: i32 = 8;

/// Minimum value of a signed char.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static SCHAR_MIN: i32 = -128;

/// Maximum value of a signed char.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static SCHAR_MAX: i32 = 127;

/// Maximum value of an unsigned char.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static UCHAR_MAX: i32 = 255;

/// Minimum value of a `char` (signed on x86_64 Linux).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static CHAR_MIN: i32 = -128;

/// Maximum value of a `char` (signed on x86_64 Linux).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static CHAR_MAX: i32 = 127;

/// Minimum value of a short.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static SHRT_MIN: i16 = i16::MIN;

/// Maximum value of a short.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static SHRT_MAX: i16 = i16::MAX;

/// Maximum value of an unsigned short.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static USHRT_MAX: u16 = u16::MAX;

/// Minimum value of an int.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static INT_MIN: i32 = i32::MIN;

/// Maximum value of an int.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static INT_MAX: i32 = i32::MAX;

/// Maximum value of an unsigned int.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static UINT_MAX: u32 = u32::MAX;

/// Minimum value of a long.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static LONG_MIN: i64 = i64::MIN;

/// Maximum value of a long.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static LONG_MAX: i64 = i64::MAX;

/// Maximum value of an unsigned long.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static ULONG_MAX: u64 = u64::MAX;

/// Minimum value of a long long.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static LLONG_MIN: i64 = i64::MIN;

/// Maximum value of a long long.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static LLONG_MAX: i64 = i64::MAX;

/// Maximum value of an unsigned long long.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static ULLONG_MAX: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// <stdint.h> — fixed-width integer limits
// ---------------------------------------------------------------------------

/// Maximum value of int8_t.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static INT8_MAX: i32 = 127;

/// Maximum value of int16_t.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static INT16_MAX: i32 = 32767;

/// Maximum value of int32_t.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static INT32_MAX: i32 = i32::MAX;

/// Maximum value of int64_t.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static INT64_MAX: i64 = i64::MAX;

/// Maximum value of size_t.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static SIZE_MAX: usize = usize::MAX;

/// Maximum value of ssize_t.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static SSIZE_MAX: isize = isize::MAX;

// ---------------------------------------------------------------------------
// POSIX limits
// ---------------------------------------------------------------------------

/// Maximum length of a host name.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static HOST_NAME_MAX: i32 = 255;

/// Maximum length of a login name.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static LOGIN_NAME_MAX: i32 = 256;

/// Maximum length of a terminal device name.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static TTY_NAME_MAX: i32 = 32;

/// Maximum number of bytes in a filename.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static NAME_MAX: i32 = 255;

/// Maximum number of bytes in a pathname.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static PATH_MAX_LIMIT: i32 = 4096;

/// Maximum number of bytes in a pipe buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static PIPE_BUF: i32 = 4096;

/// Maximum number of open files per process.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static OPEN_MAX: i32 = crate::fdtable::MAX_FDS as i32;

/// Maximum number of simultaneous processes per user.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static CHILD_MAX: i32 = 256;

/// Maximum number of I/O vectors for readv/writev.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static IOV_MAX: i32 = 1024;

/// Number of bytes in a line for utilities that process text.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static LINE_MAX: i32 = 2048;

/// Maximum length of arguments to exec.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static ARG_MAX: i32 = 131_072; // 128 KiB.

/// Maximum number of supplementary group IDs.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static NGROUPS_MAX: i32 = 32;

/// POSIX minimum: maximum pathname length.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _POSIX_PATH_MAX: i32 = 256;

/// POSIX minimum: maximum filename length.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _POSIX_NAME_MAX: i32 = 14;

/// POSIX minimum: maximum number of open files.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _POSIX_OPEN_MAX: i32 = 20;

/// POSIX minimum: maximum number of child processes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _POSIX_CHILD_MAX: i32 = 25;

/// POSIX minimum: maximum length of arguments to exec.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _POSIX_ARG_MAX: i32 = 4096;

/// Standard PATH_MAX symbol (same as PATH_MAX_LIMIT).
///
/// Some programs reference `PATH_MAX` directly rather than through
/// the `_LIMIT` suffixed version.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static PATH_MAX: i32 = 4096;

/// Maximum multibyte character length (UTF-8).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static MB_LEN_MAX: i32 = 4;

// ---------------------------------------------------------------------------
// Additional POSIX / system limits
// ---------------------------------------------------------------------------

/// Maximum number of symbolic links to traverse during resolution.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static SYMLOOP_MAX: i32 = 40;

/// Maximum number of bytes in a symbolic link target.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static SYMLINK_MAX: i32 = 255;

/// Maximum number of timer expiration overruns.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static DELAYTIMER_MAX: i32 = i32::MAX;

/// Maximum value a semaphore may have.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static SEM_VALUE_MAX: i32 = i32::MAX;

/// Maximum number of POSIX message queues per process.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static MQ_OPEN_MAX: i32 = 8;

/// Maximum message priority for POSIX message queues.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static MQ_PRIO_MAX: i32 = 32768;

/// Maximum number of repeated occurrences of a regex.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static RE_DUP_MAX: i32 = 255;

/// Maximum number of weights for collation (LC_COLLATE).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static COLL_WEIGHTS_MAX: i32 = 2;

/// Maximum nesting of `(...)` in `expr`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static EXPR_NEST_MAX: i32 = 32;

/// Maximum number of simultaneous timers per process.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static TIMER_MAX: i32 = 256;

/// Number of links a single page can contain.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static LINK_MAX: i32 = 127;

// -- bc utility limits --

/// bc — maximum ibase/obase.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static BC_BASE_MAX: i32 = 99;

/// bc — maximum array size.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static BC_DIM_MAX: i32 = 2048;

/// bc — maximum scale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static BC_SCALE_MAX: i32 = 99;

/// bc — maximum string length.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static BC_STRING_MAX: i32 = 1000;

// -- POSIX minimum values for new limits --

/// POSIX minimum: maximum semaphore value.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _POSIX_SEM_VALUE_MAX: i32 = 32767;

/// POSIX minimum: maximum number of timers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _POSIX_TIMER_MAX: i32 = 32;

/// POSIX minimum: maximum number of thread-specific data keys.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _POSIX_THREAD_KEYS_MAX: i32 = 128;

/// POSIX minimum: number of destructor iterations.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _POSIX_THREAD_DESTRUCTOR_ITERATIONS: i32 = 4;

/// POSIX minimum: maximum symlink loop count.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _POSIX_SYMLOOP_MAX: i32 = 8;

/// POSIX minimum: maximum symlink target length.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _POSIX_SYMLINK_MAX: i32 = 255;

/// POSIX minimum: maximum hard links to a file.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_upper_case_globals)]
pub static _POSIX_LINK_MAX: i32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Character/integer limits (LP64) --

    #[test]
    fn test_char_bit() {
        assert_eq!(CHAR_BIT, 8);
    }

    #[test]
    fn test_schar_range() {
        assert_eq!(SCHAR_MIN, -128);
        assert_eq!(SCHAR_MAX, 127);
    }

    #[test]
    fn test_uchar_max() {
        assert_eq!(UCHAR_MAX, 255);
    }

    #[test]
    fn test_short_range() {
        assert_eq!(SHRT_MIN, -32768);
        assert_eq!(SHRT_MAX, 32767);
        assert_eq!(USHRT_MAX, 65535);
    }

    #[test]
    fn test_int_range() {
        assert_eq!(INT_MIN, -2_147_483_648);
        assert_eq!(INT_MAX, 2_147_483_647);
        assert_eq!(UINT_MAX, 4_294_967_295);
    }

    #[test]
    fn test_long_range() {
        assert_eq!(LONG_MIN, i64::MIN);
        assert_eq!(LONG_MAX, i64::MAX);
        assert_eq!(ULONG_MAX, u64::MAX);
    }

    #[test]
    fn test_llong_range() {
        assert_eq!(LLONG_MIN, i64::MIN);
        assert_eq!(LLONG_MAX, i64::MAX);
        assert_eq!(ULLONG_MAX, u64::MAX);
    }

    // -- Fixed-width integer limits --

    #[test]
    fn test_fixed_width_max() {
        assert_eq!(INT8_MAX, 127);
        assert_eq!(INT16_MAX, 32767);
        assert_eq!(INT32_MAX, i32::MAX);
        assert_eq!(INT64_MAX, i64::MAX);
    }

    #[test]
    fn test_size_max() {
        assert_eq!(SIZE_MAX, usize::MAX);
        assert_eq!(SSIZE_MAX, isize::MAX);
    }

    // -- POSIX limits --

    #[test]
    fn test_name_limits() {
        assert_eq!(HOST_NAME_MAX, 255);
        assert_eq!(LOGIN_NAME_MAX, 256);
        assert_eq!(TTY_NAME_MAX, 32);
        assert_eq!(NAME_MAX, 255);
    }

    #[test]
    fn test_path_max() {
        assert_eq!(PATH_MAX_LIMIT, 4096);
        assert_eq!(PATH_MAX, 4096);
        assert_eq!(PATH_MAX, PATH_MAX_LIMIT);
    }

    #[test]
    fn test_resource_limits() {
        assert_eq!(PIPE_BUF, 4096);
        assert_eq!(OPEN_MAX, crate::fdtable::MAX_FDS as i32);
        assert_eq!(CHILD_MAX, 256);
        assert_eq!(IOV_MAX, 1024);
        assert_eq!(LINE_MAX, 2048);
        assert_eq!(ARG_MAX, 131_072);
        assert_eq!(NGROUPS_MAX, 32);
    }

    // -- POSIX minimum values --

    #[test]
    fn test_posix_minimums() {
        assert_eq!(_POSIX_PATH_MAX, 256);
        assert_eq!(_POSIX_NAME_MAX, 14);
        assert_eq!(_POSIX_OPEN_MAX, 20);
        assert_eq!(_POSIX_CHILD_MAX, 25);
        assert_eq!(_POSIX_ARG_MAX, 4096);
    }

    // -- Limits are at least POSIX minimums --

    #[test]
    fn test_limits_exceed_posix_minimums() {
        assert!(PATH_MAX >= _POSIX_PATH_MAX);
        assert!(NAME_MAX >= _POSIX_NAME_MAX);
        assert!(OPEN_MAX >= _POSIX_OPEN_MAX);
        assert!(CHILD_MAX >= _POSIX_CHILD_MAX);
        assert!(ARG_MAX >= _POSIX_ARG_MAX);
    }

    #[test]
    fn test_mb_len_max() {
        assert_eq!(MB_LEN_MAX, 4); // UTF-8
    }

    // -- char limits (signed on x86_64) --

    #[test]
    fn test_char_range() {
        assert_eq!(CHAR_MIN, -128);
        assert_eq!(CHAR_MAX, 127);
        // CHAR_MIN/MAX must match SCHAR_MIN/MAX on platforms where char is signed.
        assert_eq!(CHAR_MIN, SCHAR_MIN);
        assert_eq!(CHAR_MAX, SCHAR_MAX);
    }

    // -- Additional POSIX limits --

    #[test]
    fn test_symlink_limits() {
        assert_eq!(SYMLOOP_MAX, 40);
        assert_eq!(SYMLINK_MAX, 255);
        assert!(SYMLOOP_MAX >= _POSIX_SYMLOOP_MAX);
        assert!(SYMLINK_MAX >= _POSIX_SYMLINK_MAX);
    }

    #[test]
    fn test_timer_and_semaphore_limits() {
        assert_eq!(DELAYTIMER_MAX, i32::MAX);
        assert_eq!(SEM_VALUE_MAX, i32::MAX);
        assert_eq!(TIMER_MAX, 256);
        assert!(SEM_VALUE_MAX >= _POSIX_SEM_VALUE_MAX);
        assert!(TIMER_MAX >= _POSIX_TIMER_MAX);
    }

    #[test]
    fn test_mq_limits() {
        assert_eq!(MQ_OPEN_MAX, 8);
        assert_eq!(MQ_PRIO_MAX, 32768);
    }

    #[test]
    fn test_regex_and_collation_limits() {
        assert_eq!(RE_DUP_MAX, 255);
        assert_eq!(COLL_WEIGHTS_MAX, 2);
        assert_eq!(EXPR_NEST_MAX, 32);
    }

    #[test]
    fn test_bc_limits() {
        assert_eq!(BC_BASE_MAX, 99);
        assert_eq!(BC_DIM_MAX, 2048);
        assert_eq!(BC_SCALE_MAX, 99);
        assert_eq!(BC_STRING_MAX, 1000);
    }

    #[test]
    fn test_link_max() {
        assert_eq!(LINK_MAX, 127);
        assert!(LINK_MAX >= _POSIX_LINK_MAX);
    }

    #[test]
    fn test_posix_thread_minimums() {
        assert_eq!(_POSIX_THREAD_KEYS_MAX, 128);
        assert_eq!(_POSIX_THREAD_DESTRUCTOR_ITERATIONS, 4);
    }

    // -- Min/max relationships --

    #[test]
    fn test_schar_range_relationship() {
        // SCHAR_MAX must be positive, SCHAR_MIN must be negative,
        // and MAX = -MIN - 1 for two's complement.
        assert!(SCHAR_MAX > 0);
        assert!(SCHAR_MIN < 0);
        assert_eq!(SCHAR_MAX, -SCHAR_MIN - 1);
    }

    #[test]
    fn test_shrt_range_relationship() {
        assert!(SHRT_MAX > 0);
        assert!(SHRT_MIN < 0);
        // Two's complement: MAX = -(MIN + 1).  Avoid negating MIN directly
        // (i16::MIN overflows on negate).
        assert_eq!(SHRT_MAX, -(SHRT_MIN + 1));
    }

    #[test]
    fn test_int_range_relationship() {
        assert!(INT_MAX > 0);
        assert!(INT_MIN < 0);
        // For 32-bit two's complement: MAX = -MIN - 1.
        assert_eq!(INT_MAX as i64, -(INT_MIN as i64) - 1);
    }

    #[test]
    fn test_long_range_relationship() {
        assert!(LONG_MAX > 0);
        assert!(LONG_MIN < 0);
    }

    // -- Unsigned max >= signed max --

    #[test]
    fn test_uchar_max_ge_schar_max() {
        assert!(UCHAR_MAX > SCHAR_MAX);
    }

    #[test]
    fn test_ushrt_max_ge_shrt_max() {
        assert!(USHRT_MAX as i32 > SHRT_MAX as i32);
    }

    #[test]
    fn test_uint_max_ge_int_max() {
        assert!(UINT_MAX as i64 > INT_MAX as i64);
    }

    // -- Positive resource limits --

    #[test]
    fn test_all_resource_limits_positive() {
        assert!(PIPE_BUF > 0);
        assert!(OPEN_MAX > 0);
        assert!(CHILD_MAX > 0);
        assert!(IOV_MAX > 0);
        assert!(LINE_MAX > 0);
        assert!(ARG_MAX > 0);
        assert!(NGROUPS_MAX > 0);
        assert!(LINK_MAX > 0);
        assert!(TIMER_MAX > 0);
        assert!(MQ_OPEN_MAX > 0);
        assert!(MQ_PRIO_MAX > 0);
    }

    // -- Name/path limits are at least 1 --

    #[test]
    fn test_name_limits_positive() {
        assert!(HOST_NAME_MAX > 0);
        assert!(LOGIN_NAME_MAX > 0);
        assert!(TTY_NAME_MAX > 0);
        assert!(NAME_MAX > 0);
        assert!(PATH_MAX > 0);
        assert!(SYMLINK_MAX > 0);
    }

    // -- POSIX minimums are consistent --

    #[test]
    fn test_posix_minimums_positive() {
        assert!(_POSIX_PATH_MAX > 0);
        assert!(_POSIX_NAME_MAX > 0);
        assert!(_POSIX_OPEN_MAX > 0);
        assert!(_POSIX_CHILD_MAX > 0);
        assert!(_POSIX_ARG_MAX > 0);
        assert!(_POSIX_SEM_VALUE_MAX > 0);
        assert!(_POSIX_TIMER_MAX > 0);
        assert!(_POSIX_THREAD_KEYS_MAX > 0);
        assert!(_POSIX_THREAD_DESTRUCTOR_ITERATIONS > 0);
        assert!(_POSIX_SYMLOOP_MAX > 0);
        assert!(_POSIX_SYMLINK_MAX > 0);
        assert!(_POSIX_LINK_MAX > 0);
    }

    // -- Fixed-width integer min values --

    #[test]
    fn test_fixed_width_min_values() {
        assert_eq!(INT8_MAX, 0x7F);
        assert_eq!(INT16_MAX, 0x7FFF);
        assert_eq!(INT32_MAX, 0x7FFF_FFFF);
        assert_eq!(INT64_MAX, 0x7FFF_FFFF_FFFF_FFFF);
    }

    // -- Additional POSIX minimum constraint checks --

    #[test]
    fn test_symloop_exceeds_posix() {
        assert!(
            SYMLOOP_MAX >= _POSIX_SYMLOOP_MAX,
            "SYMLOOP_MAX ({SYMLOOP_MAX}) must be >= _POSIX_SYMLOOP_MAX ({_POSIX_SYMLOOP_MAX})"
        );
    }

    #[test]
    fn test_sem_value_exceeds_posix() {
        assert!(
            SEM_VALUE_MAX >= _POSIX_SEM_VALUE_MAX,
            "SEM_VALUE_MAX must be >= _POSIX_SEM_VALUE_MAX"
        );
    }

    #[test]
    fn test_timer_exceeds_posix() {
        assert!(
            TIMER_MAX >= _POSIX_TIMER_MAX,
            "TIMER_MAX must be >= _POSIX_TIMER_MAX"
        );
    }

    // -- LONG and LLONG identical on LP64 --

    #[test]
    fn test_long_llong_identical_lp64() {
        // On our LP64 target, long and long long are both 64-bit.
        assert_eq!(LONG_MIN, LLONG_MIN);
        assert_eq!(LONG_MAX, LLONG_MAX);
        assert_eq!(ULONG_MAX, ULLONG_MAX);
    }
}

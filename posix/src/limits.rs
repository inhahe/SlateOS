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
#[unsafe(no_mangle)]
pub static CHAR_BIT: i32 = 8;

/// Minimum value of a signed char.
#[unsafe(no_mangle)]
pub static SCHAR_MIN: i32 = -128;

/// Maximum value of a signed char.
#[unsafe(no_mangle)]
pub static SCHAR_MAX: i32 = 127;

/// Maximum value of an unsigned char.
#[unsafe(no_mangle)]
pub static UCHAR_MAX: i32 = 255;

/// Minimum value of a short.
#[unsafe(no_mangle)]
pub static SHRT_MIN: i16 = i16::MIN;

/// Maximum value of a short.
#[unsafe(no_mangle)]
pub static SHRT_MAX: i16 = i16::MAX;

/// Maximum value of an unsigned short.
#[unsafe(no_mangle)]
pub static USHRT_MAX: u16 = u16::MAX;

/// Minimum value of an int.
#[unsafe(no_mangle)]
pub static INT_MIN: i32 = i32::MIN;

/// Maximum value of an int.
#[unsafe(no_mangle)]
pub static INT_MAX: i32 = i32::MAX;

/// Maximum value of an unsigned int.
#[unsafe(no_mangle)]
pub static UINT_MAX: u32 = u32::MAX;

/// Minimum value of a long.
#[unsafe(no_mangle)]
pub static LONG_MIN: i64 = i64::MIN;

/// Maximum value of a long.
#[unsafe(no_mangle)]
pub static LONG_MAX: i64 = i64::MAX;

/// Maximum value of an unsigned long.
#[unsafe(no_mangle)]
pub static ULONG_MAX: u64 = u64::MAX;

/// Minimum value of a long long.
#[unsafe(no_mangle)]
pub static LLONG_MIN: i64 = i64::MIN;

/// Maximum value of a long long.
#[unsafe(no_mangle)]
pub static LLONG_MAX: i64 = i64::MAX;

/// Maximum value of an unsigned long long.
#[unsafe(no_mangle)]
pub static ULLONG_MAX: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// <stdint.h> — fixed-width integer limits
// ---------------------------------------------------------------------------

/// Maximum value of int8_t.
#[unsafe(no_mangle)]
pub static INT8_MAX: i32 = 127;

/// Maximum value of int16_t.
#[unsafe(no_mangle)]
pub static INT16_MAX: i32 = 32767;

/// Maximum value of int32_t.
#[unsafe(no_mangle)]
pub static INT32_MAX: i32 = i32::MAX;

/// Maximum value of int64_t.
#[unsafe(no_mangle)]
pub static INT64_MAX: i64 = i64::MAX;

/// Maximum value of size_t.
#[unsafe(no_mangle)]
pub static SIZE_MAX: usize = usize::MAX;

/// Maximum value of ssize_t.
#[unsafe(no_mangle)]
pub static SSIZE_MAX: isize = isize::MAX;

// ---------------------------------------------------------------------------
// POSIX limits
// ---------------------------------------------------------------------------

/// Maximum length of a host name.
#[unsafe(no_mangle)]
pub static HOST_NAME_MAX: i32 = 255;

/// Maximum length of a login name.
#[unsafe(no_mangle)]
pub static LOGIN_NAME_MAX: i32 = 256;

/// Maximum length of a terminal device name.
#[unsafe(no_mangle)]
pub static TTY_NAME_MAX: i32 = 32;

/// Maximum number of bytes in a filename.
#[unsafe(no_mangle)]
pub static NAME_MAX: i32 = 255;

/// Maximum number of bytes in a pathname.
#[unsafe(no_mangle)]
pub static PATH_MAX_LIMIT: i32 = 4096;

/// Maximum number of bytes in a pipe buffer.
#[unsafe(no_mangle)]
pub static PIPE_BUF: i32 = 4096;

/// Maximum number of open files per process.
#[unsafe(no_mangle)]
pub static OPEN_MAX: i32 = 256;

/// Maximum number of simultaneous processes per user.
#[unsafe(no_mangle)]
pub static CHILD_MAX: i32 = 256;

/// Maximum number of I/O vectors for readv/writev.
#[unsafe(no_mangle)]
pub static IOV_MAX: i32 = 1024;

/// Number of bytes in a line for utilities that process text.
#[unsafe(no_mangle)]
pub static LINE_MAX: i32 = 2048;

/// Maximum length of arguments to exec.
#[unsafe(no_mangle)]
pub static ARG_MAX: i32 = 131_072; // 128 KiB.

/// Maximum number of supplementary group IDs.
#[unsafe(no_mangle)]
pub static NGROUPS_MAX: i32 = 32;

/// POSIX minimum: maximum pathname length.
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static _POSIX_PATH_MAX: i32 = 256;

/// POSIX minimum: maximum filename length.
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static _POSIX_NAME_MAX: i32 = 14;

/// POSIX minimum: maximum number of open files.
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static _POSIX_OPEN_MAX: i32 = 20;

/// POSIX minimum: maximum number of child processes.
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static _POSIX_CHILD_MAX: i32 = 25;

/// POSIX minimum: maximum length of arguments to exec.
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static _POSIX_ARG_MAX: i32 = 4096;

/// Standard PATH_MAX symbol (same as PATH_MAX_LIMIT).
///
/// Some programs reference `PATH_MAX` directly rather than through
/// the `_LIMIT` suffixed version.
#[unsafe(no_mangle)]
pub static PATH_MAX: i32 = 4096;

/// Maximum multibyte character length (UTF-8).
#[unsafe(no_mangle)]
pub static MB_LEN_MAX: i32 = 4;

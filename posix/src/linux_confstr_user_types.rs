//! `<unistd.h>` — POSIX confstr() configuration string keys.
//!
//! `confstr(name, buf, len)` returns string-valued system configuration
//! values that don't fit in sysconf()'s `long`. The most-used key is
//! `_CS_PATH`, which gives the default PATH for `execvp()` lookups.

// ---------------------------------------------------------------------------
// confstr() key constants (matching glibc / Linux)
// ---------------------------------------------------------------------------

pub const CS_PATH: u32 = 0;
pub const CS_GNU_LIBC_VERSION: u32 = 2;
pub const CS_GNU_LIBPTHREAD_VERSION: u32 = 3;

// ---------------------------------------------------------------------------
// POSIX V7 ILP32 / LP64 environment keys
// ---------------------------------------------------------------------------

pub const CS_POSIX_V7_ILP32_OFF32_CFLAGS: u32 = 1116;
pub const CS_POSIX_V7_ILP32_OFF32_LDFLAGS: u32 = 1117;
pub const CS_POSIX_V7_ILP32_OFF32_LIBS: u32 = 1118;
pub const CS_POSIX_V7_ILP32_OFFBIG_CFLAGS: u32 = 1124;
pub const CS_POSIX_V7_ILP32_OFFBIG_LDFLAGS: u32 = 1125;
pub const CS_POSIX_V7_ILP32_OFFBIG_LIBS: u32 = 1126;
pub const CS_POSIX_V7_LP64_OFF64_CFLAGS: u32 = 1140;
pub const CS_POSIX_V7_LP64_OFF64_LDFLAGS: u32 = 1141;
pub const CS_POSIX_V7_LP64_OFF64_LIBS: u32 = 1142;

// ---------------------------------------------------------------------------
// V6 width-restricted environment keys
// ---------------------------------------------------------------------------

pub const CS_POSIX_V6_ILP32_OFF32_CFLAGS: u32 = 1116 - 16;
pub const CS_POSIX_V6_LP64_OFF64_CFLAGS: u32 = 1140 - 16;

// ---------------------------------------------------------------------------
// Default _CS_PATH value (used by execvp when PATH is unset)
// ---------------------------------------------------------------------------

/// Conservative default PATH (matches POSIX recommendation).
pub const CS_PATH_DEFAULT: &str = "/bin:/usr/bin";

// ---------------------------------------------------------------------------
// confstr return-value semantics
// ---------------------------------------------------------------------------

/// confstr returns 0 if the key is invalid.
pub const CS_INVALID_KEY_RETURN: usize = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cs_path_is_zero() {
        // _CS_PATH = 0 historically.
        assert_eq!(CS_PATH, 0);
    }

    #[test]
    fn test_libc_keys_distinct() {
        assert_ne!(CS_GNU_LIBC_VERSION, CS_GNU_LIBPTHREAD_VERSION);
        assert_ne!(CS_GNU_LIBC_VERSION, CS_PATH);
        // glibc places them just above _CS_PATH.
        assert!(CS_GNU_LIBC_VERSION > CS_PATH);
    }

    #[test]
    fn test_v7_ilp32_off32_triple_consecutive() {
        // CFLAGS, LDFLAGS, LIBS form a consecutive triple.
        assert_eq!(
            CS_POSIX_V7_ILP32_OFF32_LDFLAGS,
            CS_POSIX_V7_ILP32_OFF32_CFLAGS + 1,
        );
        assert_eq!(
            CS_POSIX_V7_ILP32_OFF32_LIBS,
            CS_POSIX_V7_ILP32_OFF32_CFLAGS + 2,
        );
    }

    #[test]
    fn test_v7_lp64_off64_triple_consecutive() {
        assert_eq!(
            CS_POSIX_V7_LP64_OFF64_LDFLAGS,
            CS_POSIX_V7_LP64_OFF64_CFLAGS + 1,
        );
        assert_eq!(
            CS_POSIX_V7_LP64_OFF64_LIBS,
            CS_POSIX_V7_LP64_OFF64_CFLAGS + 2,
        );
    }

    #[test]
    fn test_v6_to_v7_offset_is_16() {
        // V7 keys are 16 above V6 (glibc reserved a block per version).
        assert_eq!(
            CS_POSIX_V7_ILP32_OFF32_CFLAGS - CS_POSIX_V6_ILP32_OFF32_CFLAGS,
            16,
        );
        assert_eq!(
            CS_POSIX_V7_LP64_OFF64_CFLAGS - CS_POSIX_V6_LP64_OFF64_CFLAGS,
            16,
        );
    }

    #[test]
    fn test_default_path_two_components() {
        assert_eq!(CS_PATH_DEFAULT, "/bin:/usr/bin");
        // Two colon-separated entries.
        assert_eq!(CS_PATH_DEFAULT.split(':').count(), 2);
        // Both absolute paths.
        for p in CS_PATH_DEFAULT.split(':') {
            assert!(p.starts_with('/'));
        }
    }

    #[test]
    fn test_invalid_key_returns_zero() {
        assert_eq!(CS_INVALID_KEY_RETURN, 0);
    }
}

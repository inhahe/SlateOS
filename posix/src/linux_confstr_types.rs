//! `<unistd.h>` — confstr() name constants.
//!
//! `confstr()` retrieves string-valued system configuration
//! variables at runtime (such as the default PATH).  These
//! constants are the `name` parameter identifying which value
//! to query.

// ---------------------------------------------------------------------------
// POSIX confstr names (_CS_*)
// ---------------------------------------------------------------------------

/// Default PATH for finding executables.
pub const CS_PATH: u32 = 0;
/// GNU libc version string.
pub const CS_GNU_LIBC_VERSION: u32 = 2;
/// GNU libpthread version string.
pub const CS_GNU_LIBPTHREAD_VERSION: u32 = 3;

// ---------------------------------------------------------------------------
// POSIX confstr names for POSIX.2 variables
// ---------------------------------------------------------------------------

/// Value of POSIX_V7_ILP32_OFF32 CFLAGS.
pub const CS_POSIX_V7_ILP32_OFF32_CFLAGS: u32 = 1116;
/// Value of POSIX_V7_ILP32_OFF32 LDFLAGS.
pub const CS_POSIX_V7_ILP32_OFF32_LDFLAGS: u32 = 1117;
/// Value of POSIX_V7_ILP32_OFF32 LIBS.
pub const CS_POSIX_V7_ILP32_OFF32_LIBS: u32 = 1118;
/// Value of POSIX_V7_ILP32_OFFBIG CFLAGS.
pub const CS_POSIX_V7_ILP32_OFFBIG_CFLAGS: u32 = 1120;
/// Value of POSIX_V7_ILP32_OFFBIG LDFLAGS.
pub const CS_POSIX_V7_ILP32_OFFBIG_LDFLAGS: u32 = 1121;
/// Value of POSIX_V7_ILP32_OFFBIG LIBS.
pub const CS_POSIX_V7_ILP32_OFFBIG_LIBS: u32 = 1122;
/// Value of POSIX_V7_LP64_OFF64 CFLAGS.
pub const CS_POSIX_V7_LP64_OFF64_CFLAGS: u32 = 1124;
/// Value of POSIX_V7_LP64_OFF64 LDFLAGS.
pub const CS_POSIX_V7_LP64_OFF64_LDFLAGS: u32 = 1125;
/// Value of POSIX_V7_LP64_OFF64 LIBS.
pub const CS_POSIX_V7_LP64_OFF64_LIBS: u32 = 1126;
/// Value of POSIX_V7_LPBIG_OFFBIG CFLAGS.
pub const CS_POSIX_V7_LPBIG_OFFBIG_CFLAGS: u32 = 1128;
/// Value of POSIX_V7_LPBIG_OFFBIG LDFLAGS.
pub const CS_POSIX_V7_LPBIG_OFFBIG_LDFLAGS: u32 = 1129;
/// Value of POSIX_V7_LPBIG_OFFBIG LIBS.
pub const CS_POSIX_V7_LPBIG_OFFBIG_LIBS: u32 = 1130;
/// Width of POSIX_V7 environment.
pub const CS_POSIX_V7_WIDTH_RESTRICTED_ENVS: u32 = 1131;

// ---------------------------------------------------------------------------
// V6 compatibility confstr names
// ---------------------------------------------------------------------------

/// V6 ILP32_OFF32 CFLAGS.
pub const CS_POSIX_V6_ILP32_OFF32_CFLAGS: u32 = 1100;
/// V6 ILP32_OFF32 LDFLAGS.
pub const CS_POSIX_V6_ILP32_OFF32_LDFLAGS: u32 = 1101;
/// V6 ILP32_OFF32 LIBS.
pub const CS_POSIX_V6_ILP32_OFF32_LIBS: u32 = 1102;
/// V6 ILP32_OFFBIG CFLAGS.
pub const CS_POSIX_V6_ILP32_OFFBIG_CFLAGS: u32 = 1104;
/// V6 ILP32_OFFBIG LDFLAGS.
pub const CS_POSIX_V6_ILP32_OFFBIG_LDFLAGS: u32 = 1105;
/// V6 ILP32_OFFBIG LIBS.
pub const CS_POSIX_V6_ILP32_OFFBIG_LIBS: u32 = 1106;
/// V6 LP64_OFF64 CFLAGS.
pub const CS_POSIX_V6_LP64_OFF64_CFLAGS: u32 = 1108;
/// V6 LP64_OFF64 LDFLAGS.
pub const CS_POSIX_V6_LP64_OFF64_LDFLAGS: u32 = 1109;
/// V6 LP64_OFF64 LIBS.
pub const CS_POSIX_V6_LP64_OFF64_LIBS: u32 = 1110;
/// V6 width-restricted environments.
pub const CS_POSIX_V6_WIDTH_RESTRICTED_ENVS: u32 = 1115;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_names_distinct() {
        let names = [
            CS_PATH, CS_GNU_LIBC_VERSION, CS_GNU_LIBPTHREAD_VERSION,
            CS_POSIX_V7_ILP32_OFF32_CFLAGS, CS_POSIX_V7_ILP32_OFF32_LDFLAGS,
            CS_POSIX_V7_ILP32_OFF32_LIBS,
            CS_POSIX_V7_ILP32_OFFBIG_CFLAGS, CS_POSIX_V7_ILP32_OFFBIG_LDFLAGS,
            CS_POSIX_V7_ILP32_OFFBIG_LIBS,
            CS_POSIX_V7_LP64_OFF64_CFLAGS, CS_POSIX_V7_LP64_OFF64_LDFLAGS,
            CS_POSIX_V7_LP64_OFF64_LIBS,
            CS_POSIX_V7_LPBIG_OFFBIG_CFLAGS, CS_POSIX_V7_LPBIG_OFFBIG_LDFLAGS,
            CS_POSIX_V7_LPBIG_OFFBIG_LIBS,
            CS_POSIX_V7_WIDTH_RESTRICTED_ENVS,
            CS_POSIX_V6_ILP32_OFF32_CFLAGS, CS_POSIX_V6_ILP32_OFF32_LDFLAGS,
            CS_POSIX_V6_ILP32_OFF32_LIBS,
            CS_POSIX_V6_ILP32_OFFBIG_CFLAGS, CS_POSIX_V6_ILP32_OFFBIG_LDFLAGS,
            CS_POSIX_V6_ILP32_OFFBIG_LIBS,
            CS_POSIX_V6_LP64_OFF64_CFLAGS, CS_POSIX_V6_LP64_OFF64_LDFLAGS,
            CS_POSIX_V6_LP64_OFF64_LIBS,
            CS_POSIX_V6_WIDTH_RESTRICTED_ENVS,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_cs_path_is_zero() {
        assert_eq!(CS_PATH, 0);
    }

    #[test]
    fn test_v7_cflags_values() {
        assert_eq!(CS_POSIX_V7_ILP32_OFF32_CFLAGS, 1116);
        assert_eq!(CS_POSIX_V7_LP64_OFF64_CFLAGS, 1124);
    }

    #[test]
    fn test_v6_cflags_values() {
        assert_eq!(CS_POSIX_V6_ILP32_OFF32_CFLAGS, 1100);
        assert_eq!(CS_POSIX_V6_LP64_OFF64_CFLAGS, 1108);
    }

    #[test]
    fn test_gnu_versions() {
        assert_eq!(CS_GNU_LIBC_VERSION, 2);
        assert_eq!(CS_GNU_LIBPTHREAD_VERSION, 3);
    }
}

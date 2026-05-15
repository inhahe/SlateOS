//! `<ulimit.h>` — user resource limits (legacy API).
//!
//! Provides the `ulimit()` function, a simplified legacy interface to
//! resource limits.  Modern programs should use `getrlimit`/`setrlimit`
//! from `<sys/resource.h>` instead, but many older applications and
//! POSIX-conforming programs still use `ulimit()`.

use crate::errno;

// ---------------------------------------------------------------------------
// Command constants
// ---------------------------------------------------------------------------

/// Get the file size writing limit (in 512-byte blocks).
pub const UL_GETFSIZE: i32 = 1;

/// Set the file size writing limit (in 512-byte blocks).
pub const UL_SETFSIZE: i32 = 2;

/// Get the maximum data segment size (non-standard, for compat).
pub const UL_GETMAXBRK: i32 = 3;

/// Get the maximum number of open files (non-standard, for compat).
pub const UL_GETOPENMAX: i32 = 4;

// ---------------------------------------------------------------------------
// ulimit()
// ---------------------------------------------------------------------------

/// Get or set user limits.
///
/// `cmd` selects the operation:
///   - `UL_GETFSIZE` (1): return the file size limit in 512-byte blocks.
///   - `UL_SETFSIZE` (2): set the file size limit to `new_limit` blocks.
///   - `UL_GETMAXBRK` (3): return the data segment size limit in bytes.
///   - `UL_GETOPENMAX` (4): return the maximum number of open files.
///
/// Returns the current (or new) limit on success, or -1 with `errno`
/// set on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ulimit(cmd: i32, new_limit: i64) -> i64 {
    match cmd {
        UL_GETFSIZE => {
            let mut rl = crate::resource::Rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            let ret = crate::resource::getrlimit(
                crate::resource::RLIMIT_FSIZE,
                &mut rl,
            );
            if ret != 0 {
                return -1;
            }
            let limit_bytes = rl.rlim_cur;
            if limit_bytes == crate::resource::RLIM_INFINITY {
                // Return a large value to indicate unlimited.
                return i64::MAX / 512;
            }
            // Convert bytes to 512-byte blocks.
            (limit_bytes as i64) / 512
        }
        UL_SETFSIZE => {
            if new_limit < 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            // Convert 512-byte blocks to bytes.
            let limit_bytes = if new_limit > i64::MAX / 512 {
                crate::resource::RLIM_INFINITY
            } else {
                (new_limit * 512) as u64
            };
            let rl = crate::resource::Rlimit {
                rlim_cur: limit_bytes,
                rlim_max: limit_bytes,
            };
            let ret = crate::resource::setrlimit(
                crate::resource::RLIMIT_FSIZE,
                &rl,
            );
            if ret != 0 {
                return -1;
            }
            new_limit
        }
        UL_GETMAXBRK => {
            let mut rl = crate::resource::Rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            let ret = crate::resource::getrlimit(
                crate::resource::RLIMIT_DATA,
                &mut rl,
            );
            if ret != 0 {
                return -1;
            }
            if rl.rlim_cur == crate::resource::RLIM_INFINITY {
                return i64::MAX;
            }
            rl.rlim_cur as i64
        }
        UL_GETOPENMAX => {
            let mut rl = crate::resource::Rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            let ret = crate::resource::getrlimit(
                crate::resource::RLIMIT_NOFILE,
                &mut rl,
            );
            if ret != 0 {
                return -1;
            }
            if rl.rlim_cur == crate::resource::RLIM_INFINITY {
                return i64::MAX;
            }
            rl.rlim_cur as i64
        }
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_ul_getfsize_value() {
        assert_eq!(UL_GETFSIZE, 1);
    }

    #[test]
    fn test_ul_setfsize_value() {
        assert_eq!(UL_SETFSIZE, 2);
    }

    #[test]
    fn test_ul_getmaxbrk_value() {
        assert_eq!(UL_GETMAXBRK, 3);
    }

    #[test]
    fn test_ul_getopenmax_value() {
        assert_eq!(UL_GETOPENMAX, 4);
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [UL_GETFSIZE, UL_SETFSIZE, UL_GETMAXBRK, UL_GETOPENMAX];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j], "commands must be distinct");
            }
        }
    }

    // -----------------------------------------------------------------------
    // UL_GETFSIZE
    // -----------------------------------------------------------------------

    #[test]
    fn test_getfsize_returns_positive() {
        let result = ulimit(UL_GETFSIZE, 0);
        assert!(result > 0, "file size limit should be positive, got {result}");
    }

    // -----------------------------------------------------------------------
    // UL_SETFSIZE
    // -----------------------------------------------------------------------

    #[test]
    fn test_setfsize_and_readback() {
        // Save original.
        let orig = ulimit(UL_GETFSIZE, 0);
        assert!(orig > 0);

        // Set a new limit (1000 blocks = 512000 bytes).
        let ret = ulimit(UL_SETFSIZE, 1000);
        assert_eq!(ret, 1000);

        // Read it back.
        let readback = ulimit(UL_GETFSIZE, 0);
        assert_eq!(readback, 1000);

        // Restore original.
        ulimit(UL_SETFSIZE, orig);
    }

    #[test]
    fn test_setfsize_negative_fails() {
        let ret = ulimit(UL_SETFSIZE, -1);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // UL_GETMAXBRK
    // -----------------------------------------------------------------------

    #[test]
    fn test_getmaxbrk_returns_positive() {
        let result = ulimit(UL_GETMAXBRK, 0);
        assert!(result > 0, "data segment limit should be positive, got {result}");
    }

    // -----------------------------------------------------------------------
    // UL_GETOPENMAX
    // -----------------------------------------------------------------------

    #[test]
    fn test_getopenmax_returns_positive() {
        let result = ulimit(UL_GETOPENMAX, 0);
        assert!(result > 0, "open file limit should be positive, got {result}");
    }

    #[test]
    fn test_getopenmax_reasonable() {
        let result = ulimit(UL_GETOPENMAX, 0);
        // Should be at least 20 (POSIX minimum for OPEN_MAX).
        assert!(result >= 20, "open file limit {result} too small");
    }

    // -----------------------------------------------------------------------
    // Invalid command
    // -----------------------------------------------------------------------

    #[test]
    fn test_invalid_command_returns_error() {
        let ret = ulimit(0, 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_invalid_negative_command() {
        let ret = ulimit(-1, 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_invalid_large_command() {
        let ret = ulimit(9999, 0);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // Cross-module consistency
    // -----------------------------------------------------------------------

    #[test]
    fn test_getfsize_matches_getrlimit() {
        let mut rl = crate::resource::Rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        let ret = crate::resource::getrlimit(
            crate::resource::RLIMIT_FSIZE,
            &mut rl,
        );
        assert_eq!(ret, 0);

        let ul_val = ulimit(UL_GETFSIZE, 0);
        if rl.rlim_cur == crate::resource::RLIM_INFINITY {
            assert!(ul_val > 0);
        } else {
            assert_eq!(ul_val, (rl.rlim_cur as i64) / 512);
        }
    }

    #[test]
    fn test_getopenmax_matches_getrlimit() {
        let mut rl = crate::resource::Rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        let ret = crate::resource::getrlimit(
            crate::resource::RLIMIT_NOFILE,
            &mut rl,
        );
        assert_eq!(ret, 0);

        let ul_val = ulimit(UL_GETOPENMAX, 0);
        if rl.rlim_cur == crate::resource::RLIM_INFINITY {
            assert_eq!(ul_val, i64::MAX);
        } else {
            assert_eq!(ul_val, rl.rlim_cur as i64);
        }
    }
}

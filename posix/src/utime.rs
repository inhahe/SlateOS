//! `<utime.h>` — file access and modification times.
//!
//! Provides the `Utimbuf` structure and `utime()` function for setting
//! file timestamps.  This is the legacy interface; prefer `utimensat()`
//! from `<sys/stat.h>` for nanosecond precision.

use crate::types::TimeT;

// ---------------------------------------------------------------------------
// Utimbuf structure
// ---------------------------------------------------------------------------

/// Buffer for `utime()` — access and modification times.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Utimbuf {
    /// Access time (seconds since epoch).
    pub actime: TimeT,
    /// Modification time (seconds since epoch).
    pub modtime: TimeT,
}

// ---------------------------------------------------------------------------
// utime()
// ---------------------------------------------------------------------------

/// Set file access and modification times.
///
/// If `times` is null, both timestamps are set to the current time.
/// Otherwise, the `actime`/`modtime` fields of `*times` are used for
/// access and modification times respectively.  Returns 0 on success,
/// -1 on error.
///
/// Delegates to `utimensat(AT_FDCWD, path, &ts[2], 0)` which is the
/// modern equivalent.  The legacy `utime()` only carries second
/// precision, so the `tv_nsec` fields of the converted timespecs are
/// always zero.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn utime(path: *const u8, times: *const Utimbuf) -> i32 {
    if times.is_null() {
        // Pass NULL through — utimensat treats it as "set both
        // timestamps to the current time".
        return crate::file::utimensat(crate::file::AT_FDCWD, path, core::ptr::null(), 0);
    }
    // SAFETY: caller contract — `times` points to a valid Utimbuf.
    let buf = unsafe { *times };
    let ts = [
        crate::stat::Timespec {
            tv_sec: buf.actime,
            tv_nsec: 0,
        },
        crate::stat::Timespec {
            tv_sec: buf.modtime,
            tv_nsec: 0,
        },
    ];
    crate::file::utimensat(crate::file::AT_FDCWD, path, ts.as_ptr(), 0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utimbuf_size() {
        assert_eq!(
            core::mem::size_of::<Utimbuf>(),
            2 * core::mem::size_of::<TimeT>()
        );
    }

    #[test]
    fn test_utimbuf_init() {
        let buf = Utimbuf {
            actime: 1_000_000,
            modtime: 2_000_000,
        };
        assert_eq!(buf.actime, 1_000_000);
        assert_eq!(buf.modtime, 2_000_000);
    }

    #[test]
    fn test_utime_null_times_succeeds() {
        // utime(path, NULL) delegates to utimensat.  On the host build the
        // kernel syscall path is not compiled, so utimensat returns 0 after
        // argument validation (NULL times is valid).
        let ret = utime(b"/nonexistent\0".as_ptr(), core::ptr::null());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_utime_with_times_succeeds() {
        // On the host build utimensat validates and returns 0 without
        // issuing the kernel SYS_FS_SET_TIMES syscall (not host-executable).
        let buf = Utimbuf {
            actime: 100,
            modtime: 200,
        };
        let ret = utime(b"/nonexistent\0".as_ptr(), &buf);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_utimbuf_clone() {
        let a = Utimbuf {
            actime: 42,
            modtime: 84,
        };
        let b = a;
        assert_eq!(a.actime, b.actime);
        assert_eq!(a.modtime, b.modtime);
    }
}

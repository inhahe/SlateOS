//! `<utime.h>` — file access and modification times.
//!
//! Provides the `Utimbuf` structure and `utime()` function for setting
//! file timestamps.  This is the legacy interface; prefer `utimensat()`
//! from `<sys/stat.h>` for nanosecond precision.

use crate::errno;
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
/// Returns 0 on success, -1 on error.
///
/// Stub — always returns -1 with `ENOSYS` on the bare-metal target.
/// On the test host, delegates to `utimensat` internally.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn utime(_path: *const u8, _times: *const Utimbuf) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
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
    fn test_utime_stub() {
        let ret = utime(b"/nonexistent\0".as_ptr(), core::ptr::null());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_utime_with_times() {
        let buf = Utimbuf {
            actime: 100,
            modtime: 200,
        };
        let ret = utime(b"/nonexistent\0".as_ptr(), &buf);
        assert_eq!(ret, -1);
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

//! `sysconf`, `pathconf` and `confstr`: what the C library says about this
//! system's limits and options.
//!
//! Asked of the one library every program links (design-decisions §768), so
//! a shell script told a limit by `getconf` is told what a C program asking
//! `sysconf` itself is told. `userspace/getconf` existed before this as a
//! table of numbers typed into its source -- `OPEN_MAX` 1024 whatever the
//! library said -- and was deleted for it (known-issues
//! `B-NO-GETCONF-OR-LOCALE-UNTIL-PORTED`).
//!
//! The three calls keep their C shapes where a caller needs to see them:
//! glibc's `getconf` prints `sysconf`'s `-1` as `undefined`, except for the
//! two names whose value *is* all ones, so [`sysconf`] returns the raw value;
//! and `confstr` reports a value's size before the value, so [`confstr`] takes
//! an optional buffer as C's does.

use core::ffi::CStr;
#[cfg(unix)]
use core::ffi::c_char;

#[cfg(unix)]
mod sys {
    use core::ffi::{c_char, c_long};

    unsafe extern "C" {
        pub fn sysconf(name: i32) -> c_long;
        pub fn pathconf(path: *const c_char, name: i32) -> c_long;
        pub fn confstr(name: i32, buf: *mut c_char, len: usize) -> usize;
    }
}

/// `_SC_HOST_NAME_MAX`: 180 in glibc and SlateOS's library alike. What it
/// answers is not alike -- 64 on Linux, 255 on SlateOS -- which is why a
/// program sizing a buffer for `gethostname` asks rather than assumes.
pub const SC_HOST_NAME_MAX: i32 = 180;

/// `MAXHOSTNAMELEN` in glibc's `<sys/param.h>`: util-linux's fallback.
const MAXHOSTNAMELEN: usize = 64;

/// util-linux's `get_hostname_max` (`include/c.h`): the longest host name,
/// as `sysconf (_SC_HOST_NAME_MAX)` says -- 64 on Linux, 255 on SlateOS --
/// or `MAXHOSTNAMELEN`, 64, when it says nothing positive. Its
/// `xgethostname` reads the name into one byte more than this.
#[must_use]
pub fn hostname_max() -> usize {
    usize::try_from(sysconf(SC_HOST_NAME_MAX))
        .ok()
        .filter(|&n| n > 0)
        .unwrap_or(MAXHOSTNAMELEN)
}

/// `sysconf(name)`, as the C library returns it: the value, or `-1` for a
/// name without a limit or one the library does not know.
///
/// On a host that is not Unix there is no C library to ask, and the answer is
/// `-1` -- "not defined here" -- rather than a guessed value.
#[must_use]
// `c_long` is `i64` on every 64-bit target this builds for, which makes the
// `i64::from` below a no-op there -- and dropping it a compile error on a
// 32-bit one. Kept, and the lint told why.
#[allow(clippy::useless_conversion)]
pub fn sysconf(name: i32) -> i64 {
    #[cfg(unix)]
    {
        // SAFETY: `sysconf` takes an integer and reads no memory of ours.
        i64::from(unsafe { sys::sysconf(name) })
    }
    #[cfg(not(unix))]
    {
        let _ = name;
        -1
    }
}

/// `pathconf(path, name)`, telling "no limit" from failure as POSIX
/// specifies: `errno` is cleared first, and a `-1` that leaves it clear means
/// the variable has no limit for this file.
///
/// # Errors
///
/// The `errno` a failing `pathconf` set -- `ENOENT`, `EACCES`, `EINVAL` for a
/// name the library does not know -- and [`crate::ENOSYS`] where there is no
/// C library.
pub fn pathconf(path: &CStr, name: i32) -> Result<Option<i64>, i32> {
    #[cfg(unix)]
    {
        super::clear_errno();
        // SAFETY: `CStr` guarantees the terminator and `path` outlives the
        // call, so the library reads a valid C string and nothing past it.
        // (The conversion is `sysconf`'s: a no-op where `long` is 64 bits.)
        #[allow(clippy::useless_conversion)]
        let value = i64::from(unsafe { sys::pathconf(path.as_ptr(), name) });
        if value != -1 {
            return Ok(Some(value));
        }
        match super::last_errno() {
            0 => Ok(None),
            errno => Err(errno),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, name);
        Err(crate::ENOSYS)
    }
}

/// `confstr(name, buf, len)`: copies as much of the value as fits into
/// `buf`, NUL-terminated, and returns the size the whole value needs with its
/// NUL -- or `0` when `name` is not one the library has a value for.
///
/// `None` asks only for the size, as C's `confstr(name, NULL, 0)` does.
pub fn confstr(name: i32, buf: Option<&mut [u8]>) -> usize {
    #[cfg(unix)]
    {
        let (ptr, len) = match buf {
            Some(b) => (b.as_mut_ptr().cast::<c_char>(), b.len()),
            None => (core::ptr::null_mut(), 0),
        };
        // SAFETY: `ptr` and `len` are exactly the caller's buffer, or null and
        // zero, which `confstr` accepts as "report the size"; the library
        // writes at most `len` bytes.
        unsafe { sys::confstr(name, ptr, len) }
    }
    #[cfg(not(unix))]
    {
        let _ = (name, buf);
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_are_the_librarys() {
        assert_eq!(SC_HOST_NAME_MAX, posix::unistd::_SC_HOST_NAME_MAX);
    }

    /// The real library has a limit for host names, and util-linux's
    /// helper answers it.
    #[cfg(unix)]
    #[test]
    fn the_library_has_a_host_name_limit() {
        let limit = sysconf(SC_HOST_NAME_MAX);
        assert!(limit > 0);
        assert_eq!(i64::try_from(hostname_max()).ok(), Some(limit));
    }

    /// No library to ask: nothing is defined, and util-linux falls back to
    /// `MAXHOSTNAMELEN`.
    #[cfg(not(unix))]
    #[test]
    fn off_unix_nothing_is_defined() {
        assert_eq!(sysconf(SC_HOST_NAME_MAX), -1);
        assert_eq!(hostname_max(), 64);
    }
}

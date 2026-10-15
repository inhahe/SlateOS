//! The environment as the C library holds it, read in place.
//!
//! For work done inside a signal handler, which may neither allocate nor
//! wait on a lock the program it interrupted might hold.
//! `std::env::var_os` does both: it builds an `OsString`, and it takes
//! Rust's environment lock -- which a `set_var` the handler interrupted
//! holds for writing, so the handler would wait for good. `getenv` reads
//! `environ` as it stands, which is what a C program's handler does; it is
//! no safer than that against a `setenv` it interrupted, and no less safe.
//!
//! On a host that is not unix there is no such environment to read.

use core::ffi::CStr;

#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        pub fn getenv(name: *const core::ffi::c_char) -> *const core::ffi::c_char;
    }
}

/// `f (getenv (name))`: the variable's value, as the bytes the C library
/// keeps -- borrowed for `f`'s length and not copied -- or `None` when it is
/// not set (and always off unix).
///
/// The bytes are the environment's own, and stay as they are only while
/// nothing sets or unsets a variable: call this where nothing else does --
/// a signal handler, which interrupted the only thread that would.
pub fn with_var<R>(name: &CStr, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
    with_var_impl(name, f)
}

#[cfg(unix)]
fn with_var_impl<R>(name: &CStr, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
    // SAFETY: `name` is a NUL-terminated string, as `getenv` reads it.
    let p = unsafe { sys::getenv(name.as_ptr()) };
    if p.is_null() {
        return None;
    }
    // SAFETY: a non-null `getenv` result is a NUL-terminated string in the
    // environment, which stays put while nothing changes the environment --
    // the caller's contract above -- and `f` is done with it before this
    // returns.
    let value = unsafe { CStr::from_ptr(p) };
    Some(f(value.to_bytes()))
}

#[cfg(not(unix))]
fn with_var_impl<R>(_name: &CStr, _f: impl FnOnce(&[u8]) -> R) -> Option<R> {
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_variable_is_read_in_place() {
        extern crate std;
        // SAFETY: this test's variable alone; nothing else reads or writes it.
        unsafe { std::env::set_var("LIBCALL_ENV_TEST", "42") };
        assert_eq!(with_var(c"LIBCALL_ENV_TEST", <[u8]>::len), Some(2));
        assert_eq!(
            with_var(c"LIBCALL_ENV_TEST", |v| v == b"42"),
            Some(true)
        );
        assert_eq!(with_var(c"LIBCALL_ENV_TEST_UNSET", <[u8]>::len), None);
    }

    #[cfg(not(unix))]
    #[test]
    fn off_unix_nothing_is_set() {
        assert_eq!(with_var(c"PATH", <[u8]>::len), None);
    }
}

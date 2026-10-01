//! The C library calls prlimit makes: `prlimit(2)` itself, and `execvp`.
//!
//! Through the C library, as upstream (built with `HAVE_PRLIMIT`) calls it --
//! SlateOS's provides `prlimit` -- rather than a system-call instruction of
//! our own, which is what the program this replaced did.

use std::ffi::OsString;
use std::io;

/// `prlimit(pid, resource, new, &old)`: sets `new` when given, and returns
/// the limit as it was.
///
/// # Errors
///
/// The call's own: `EPERM` for another user's process or a raised hard
/// limit, `ESRCH` for no such process, `EINVAL`.
pub fn prlimit(pid: i32, resource: i32, new: Option<(u64, u64)>) -> io::Result<(u64, u64)> {
    imp::prlimit(pid, resource, new)
}

/// `execvp(argv[0], argv)`: returns only on failure.
pub fn exec(argv: &[OsString]) -> io::Error {
    imp::exec(argv)
}

#[cfg(unix)]
mod imp {
    use std::ffi::{OsString, c_int};
    use std::io;

    /// `struct rlimit`.
    #[repr(C)]
    #[derive(Default)]
    struct Rlimit {
        rlim_cur: u64,
        rlim_max: u64,
    }

    mod ffi {
        use std::ffi::c_int;

        unsafe extern "C" {
            pub fn prlimit(
                pid: c_int,
                resource: c_int,
                new_limit: *const super::Rlimit,
                old_limit: *mut super::Rlimit,
            ) -> c_int;
        }
    }

    pub fn prlimit(pid: i32, resource: i32, new: Option<(u64, u64)>) -> io::Result<(u64, u64)> {
        let new = new.map(|(rlim_cur, rlim_max)| Rlimit { rlim_cur, rlim_max });
        let new_ptr = new.as_ref().map_or(std::ptr::null(), std::ptr::from_ref);
        let mut old = Rlimit::default();
        // SAFETY: `new_ptr` is null or points at a live `Rlimit`, which the
        // call only reads; `old` is a live, writable `Rlimit` the call fills.
        // Both have `struct rlimit`'s layout: two 64-bit `rlim_t`.
        let rc = unsafe {
            ffi::prlimit(
                c_int::from(pid),
                c_int::from(resource),
                new_ptr,
                &raw mut old,
            )
        };
        if rc == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok((old.rlim_cur, old.rlim_max))
    }

    pub fn exec(argv: &[OsString]) -> io::Error {
        use std::os::unix::process::CommandExt;
        let Some((program, args)) = argv.split_first() else {
            return io::Error::from(io::ErrorKind::InvalidInput);
        };
        std::process::Command::new(program).args(args).exec()
    }
}

#[cfg(not(unix))]
mod imp {
    use std::ffi::OsString;
    use std::io;

    /// The Windows host the unit tests run on has no resource limits.
    pub fn prlimit(_pid: i32, _resource: i32, _new: Option<(u64, u64)>) -> io::Result<(u64, u64)> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn exec(_argv: &[OsString]) -> io::Error {
        io::Error::from(io::ErrorKind::Unsupported)
    }
}

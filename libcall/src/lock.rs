//! POSIX record locks: `fcntl (fd, F_SETLK, &lock)`.
//!
//! The other advisory lock, `flock`, needs nothing here: the standard
//! library's `File::try_lock_shared` is `flock (fd, LOCK_SH | LOCK_NB)` and
//! says `WouldBlock` for exactly the `EWOULDBLOCK` a caller tests for. Record
//! locks have no such method, and a daemon may hold either kind -- which is
//! why `pgrep -L` tries both before deciding a PID file is unlocked.

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;

/// `fcntl`'s command for taking a record lock without waiting.
#[cfg(any(unix, test))]
const F_SETLK: i32 = 6;
/// A shared (read) lock.
#[cfg(any(unix, test))]
const F_RDLCK: i16 = 0;
/// `l_start` counts from the start of the file.
#[cfg(any(unix, test))]
const SEEK_SET: i16 = 0;

/// `struct flock`, as the Linux headers lay it out on x86-64.
#[cfg(unix)]
#[repr(C)]
struct Flock {
    l_type: i16,
    l_whence: i16,
    l_start: i64,
    l_len: i64,
    l_pid: i32,
}

#[cfg(unix)]
mod sys {
    use super::Flock;

    unsafe extern "C" {
        // Variadic, as C declares it; the third argument here is a pointer.
        pub fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }

    /// The one call this module makes, with its pointer argument spelled out.
    ///
    /// # Safety
    ///
    /// `lock` must point at a live `struct flock` for the call's duration.
    pub unsafe fn set_lock(fd: i32, cmd: i32, lock: *mut Flock) -> i32 {
        // SAFETY: the caller's contract: `lock` is valid for the call, and
        // `F_SETLK` reads (and does not keep) the structure it points at.
        unsafe { fcntl(fd, cmd, lock) }
    }
}

/// Take a shared record lock on the whole of `fd`'s file, without waiting:
/// `fcntl (fd, F_SETLK, &(struct flock){F_RDLCK, SEEK_SET, 0, 0})`.
///
/// The lock is held until any descriptor this process has for the file is
/// closed -- record locks belong to the process and the file, not to the
/// descriptor.
///
/// # Errors
///
/// The `errno` from `fcntl`: `EACCES` or `EAGAIN` when another process holds
/// a conflicting (write) lock -- POSIX allows either -- `EBADF` for a
/// descriptor not open for reading, `EINVAL`, `ENOLCK`;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_read_lock(fd: i32) -> Result<(), i32> {
    set_read_lock_one(fd)
}

#[cfg(unix)]
fn set_read_lock_one(fd: i32) -> Result<(), i32> {
    let mut lock = Flock {
        l_type: F_RDLCK,
        l_whence: SEEK_SET,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    // SAFETY: `lock` is a live, correctly laid out `struct flock` for the
    // whole call.
    let rc = unsafe { sys::set_lock(fd, F_SETLK, &raw mut lock) };
    if rc == -1 { Err(last_errno()) } else { Ok(()) }
}

#[cfg(not(unix))]
fn set_read_lock_one(_fd: i32) -> Result<(), i32> {
    Err(ENOSYS)
}

#[cfg(test)]
mod tests {
    #[test]
    fn constants_agree_with_posix() {
        assert_eq!(super::F_SETLK, posix::fcntl_ops::F_SETLK);
        assert_eq!(super::F_RDLCK, posix::fcntl_ops::F_RDLCK);
        assert_eq!(i32::from(super::SEEK_SET), posix::fcntl::SEEK_SET);
    }

    /// The structure only exists where it is passed to a C library.
    #[cfg(unix)]
    #[test]
    fn the_structure_is_laid_out_as_the_headers_have_it() {
        assert_eq!(
            core::mem::size_of::<super::Flock>(),
            core::mem::size_of::<posix::fcntl_ops::Flock>()
        );
        assert_eq!(core::mem::size_of::<super::Flock>(), 32);
    }

    #[cfg(not(unix))]
    #[test]
    fn off_unix_the_call_declines() {
        assert_eq!(super::set_read_lock(3), Err(crate::ENOSYS));
    }
}

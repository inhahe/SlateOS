//! `<pty.h>` / `<utmp.h>` — pseudo-terminal pair helpers.
//!
//! These three functions are the BSD-derived convenience layer that sits
//! on top of the POSIX pty primitives in [`crate::ioctl`]
//! (`posix_openpt`, `grantpt`, `unlockpt`, `ptsname_r`).  They are what
//! terminal emulators, `script(1)`, `expect`, `ssh`'s server side and —
//! the reason they are here — CPython's `pty`/`os` modules actually
//! call.  The CPython link spike (`scripts/cpython-spike/`) found
//! `openpty`, `forkpty` and `login_tty` among the thirteen libc symbols
//! CPython 3.12 references that SlateOS did not define.
//!
//! ## Composed, not stubbed
//!
//! Every function here is written as the real algorithm over the real
//! primitives, exactly as glibc (`sysdeps/unix/bsd/openpty.c`,
//! `login_tty.c`, `forkpty.c`, checked against glibc 2.39) and musl
//! (`src/misc/pty.c`, `src/misc/login_tty.c`, `src/misc/forkpty.c`) do.
//! It would have been shorter to return `ENOSYS` from all three, since
//! `posix_openpt` returns `ENOSYS` today and therefore `openpty` cannot
//! presently succeed.  Composing instead buys two things:
//!
//! 1. **The errno a caller sees is the truthful one.** `openpty` fails
//!    with whatever `posix_openpt` failed with — `ENOSYS` now, `EMFILE`
//!    or `ENOSPC` later — rather than a hardcoded verdict that would
//!    become a lie the moment the pty layer lands.
//! 2. **They start working with no edit.** When the kernel grows
//!    `/dev/ptmx` and `posix_openpt` begins returning a master fd, these
//!    functions become correct by themselves.  A stub would have to be
//!    found and rewritten, and stubs that must be found later are the
//!    ones that are not.
//!
//! `login_tty` is the exception that is already fully functional: it is
//! `setsid` + `ioctl(TIOCSCTTY)` + three `dup2`s + a `close`, all of
//! which exist, and it works on any tty fd, not just a pty slave.

use crate::errno;
use crate::fcntl::{O_CLOEXEC, O_NOCTTY, O_RDWR};
use crate::ioctl::{TCSAFLUSH, TIOCSCTTY, TIOCSWINSZ, Termios, Winsize};
use crate::types::PidT;

/// Upper bound on a slave pty pathname.
///
/// Linux's slave names are `/dev/pts/<n>` where `<n>` is bounded by
/// `/proc/sys/kernel/pty/max` (4096 by default, 1048576 at the ceiling),
/// so 20 bytes is enough in practice and is exactly what musl uses.  We
/// take 64 because the extra 44 bytes of stack cost nothing and remove
/// the need to reason about the bound again if the naming scheme ever
/// changes.
const PTS_NAME_MAX: usize = 64;

/// Open an available pseudo-terminal pair.
///
/// ```c
/// int openpty(int *amaster, int *aslave, char *name,
///             const struct termios *termp, const struct winsize *winp);
/// ```
///
/// Returns 0 on success (with the master fd in `*amaster` and the slave
/// fd in `*aslave`), or -1 with `errno` set.
///
/// The sequence is glibc's and musl's, in their order, because the order
/// is load-bearing: `grantpt` and `unlockpt` must both run before the
/// slave can be opened, and the slave must be opened before `termp`/
/// `winp` can be applied to it.
///
/// 1. `posix_openpt(O_RDWR | O_NOCTTY)` — `O_NOCTTY` because acquiring
///    the master must not make it the caller's controlling terminal;
///    that is `login_tty`'s job, in the child, on the *slave*.
/// 2. `grantpt`, `unlockpt`.
/// 3. `ptsname_r` into a local buffer.
/// 4. `open(slave, O_RDWR | O_NOCTTY)`.
/// 5. optional `tcsetattr(slave, TCSAFLUSH, termp)` and
///    `ioctl(slave, TIOCSWINSZ, winp)`.
///
/// ## `name` is unbounded, and that is the documented ABI
///
/// `name` has no length parameter, so this function cannot check it.
/// That is a genuine defect of the BSD interface — `openpty(3)` says so
/// in its own BUGS section — and it is why callers are advised to pass
/// NULL and use `ptsname_r` themselves.  We copy at most
/// `PTS_NAME_MAX` bytes including the terminator, which is the same
/// bound glibc and musl impose, so a caller sized by either of their
/// manual pages is not made *more* unsafe by us.
///
/// ## Cleanup preserves errno
///
/// Every failure path after the master is open closes the master (and
/// the slave, if it got that far) around a saved errno, so the caller
/// sees the errno of the operation that actually failed rather than
/// whatever `close` last set.  musl does the same
/// (`src/misc/pty.c`: `int e = errno; ... errno = e;`); a naive
/// implementation that closes first reports `EBADF` for everything.
///
/// # Errors
///
/// Whatever `posix_openpt`, `grantpt`, `unlockpt`, `ptsname_r`, `open`,
/// `tcsetattr` or `ioctl` set.  `EINVAL` if `amaster` or `aslave` is
/// NULL — glibc dereferences them unconditionally and crashes, which is
/// not behaviour worth copying.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn openpty(
    amaster: *mut i32,
    aslave: *mut i32,
    name: *mut u8,
    termp: *const Termios,
    winp: *const Winsize,
) -> i32 {
    if amaster.is_null() || aslave.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let master = crate::ioctl::posix_openpt(O_RDWR | O_NOCTTY);
    if master < 0 {
        return -1;
    }

    let mut buf = [0u8; PTS_NAME_MAX];

    // Everything from here on must close the master before returning -1.
    let slave = 'setup: {
        if crate::ioctl::grantpt(master) != 0 {
            break 'setup -1;
        }
        if crate::ioctl::unlockpt(master) != 0 {
            break 'setup -1;
        }
        // Note the local convention: our `ptsname_r` returns -1 with
        // errno set, not the positive errno strict POSIX specifies.  See
        // the note on `ptsname_r` in ioctl.rs; if that convention is ever
        // switched, this test must switch with it.
        if crate::ioctl::ptsname_r(master, buf.as_mut_ptr(), buf.len()) != 0 {
            break 'setup -1;
        }
        crate::file::open(buf.as_ptr(), O_RDWR | O_NOCTTY, 0)
    };

    if slave < 0 {
        let e = errno::get_errno();
        crate::file::close(master);
        errno::set_errno(e);
        return -1;
    }

    if !termp.is_null() && crate::ioctl::tcsetattr(slave, TCSAFLUSH, termp) != 0 {
        let e = errno::get_errno();
        crate::file::close(slave);
        crate::file::close(master);
        errno::set_errno(e);
        return -1;
    }
    if !winp.is_null() && crate::ioctl::ioctl(slave, TIOCSWINSZ, winp.cast::<u8>().cast_mut()) != 0
    {
        let e = errno::get_errno();
        crate::file::close(slave);
        crate::file::close(master);
        errno::set_errno(e);
        return -1;
    }

    if !name.is_null() {
        // `buf` is NUL-terminated by construction (zero-initialised, and
        // `ptsname_r` cannot fill the final byte without reporting
        // ERANGE), so the copy terminates within bounds.
        let mut i = 0usize;
        while i < buf.len() {
            // SAFETY: `i < buf.len()`, and the caller's `name` is
            // documented to hold at least as much as glibc's openpty
            // would write — see the note above.
            unsafe {
                let b = *buf.get_unchecked(i);
                *name.add(i) = b;
                if b == 0 {
                    break;
                }
            }
            i = i.wrapping_add(1);
        }
    }

    // SAFETY: both pointers were NULL-checked at entry.
    unsafe {
        core::ptr::write_unaligned(amaster, master);
        core::ptr::write_unaligned(aslave, slave);
    }
    0
}

/// Make `fd` the controlling terminal of the calling process and its
/// stdin/stdout/stderr.
///
/// ```c
/// int login_tty(int fd);
/// ```
///
/// Returns 0 on success, -1 with `errno` set.  This is the whole of
/// "become a session leader on this terminal", and it is what every
/// forked pty child does immediately after `fork`.
///
/// The steps, in musl's order (`src/misc/login_tty.c`):
///
/// 1. `setsid()` — a process cannot acquire a controlling terminal
///    unless it is a session leader without one.  Its return value is
///    deliberately ignored: it fails with `EPERM` when the caller is
///    *already* a process-group leader, which for the common
///    `fork` + `login_tty` sequence cannot happen, and when it can
///    (a caller that already called `setsid`) the process is already in
///    the state we wanted.  The `TIOCSCTTY` below is the real test, and
///    it reports the failure that matters.
/// 2. `ioctl(fd, TIOCSCTTY, 0)` — the acquisition itself, and the only
///    step whose failure is propagated.
/// 3. `dup2(fd, 0/1/2)`.
/// 4. `close(fd)` if `fd > 2`, since it is now redundant.  The guard
///    matters: closing unconditionally would close a standard descriptor
///    that `dup2` had just made an alias of itself.
///
/// # Errors
///
/// `ENOTTY` or `EPERM` from `TIOCSCTTY` (the fd is not a terminal, or
/// another session already owns it); `EBADF` from `dup2`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn login_tty(fd: i32) -> i32 {
    crate::process::setsid();

    if crate::ioctl::ioctl(fd, TIOCSCTTY, core::ptr::null_mut()) != 0 {
        return -1;
    }

    let mut target = 0i32;
    while target < 3 {
        if crate::file::dup2(fd, target) < 0 {
            return -1;
        }
        target = target.wrapping_add(1);
    }

    if fd > 2 {
        crate::file::close(fd);
    }
    0
}

/// Create a child process attached to a new pseudo-terminal.
///
/// ```c
/// pid_t forkpty(int *amaster, char *name,
///               const struct termios *termp, const struct winsize *winp);
/// ```
///
/// Returns 0 in the child, the child's pid in the parent, or -1 with
/// `errno` set.  On success in the parent, `*amaster` holds the master
/// fd; the child's stdin/stdout/stderr are the slave.
///
/// ## The synchronisation pipe
///
/// The obvious implementation — `openpty`, `fork`, `login_tty` in the
/// child — has a hole: if `login_tty` fails in the child, the parent has
/// already been told the fork succeeded and cannot find out.  It ends up
/// talking to a child whose stdio is not the pty at all.  musl closes
/// this (`src/misc/forkpty.c`) with an `O_CLOEXEC` pipe: the child writes
/// its errno into it on failure and `_exit`s, and the parent reads.  A
/// successful `login_tty` is followed by `execve` or by the child simply
/// running, and in either case the write end is closed — by `O_CLOEXEC`
/// on exec, or explicitly — so the parent's read returns 0.
///
/// We reproduce that exactly.  It is the difference between "forkpty
/// failed" and "forkpty returned a child that silently is not on a
/// terminal", and the latter is very hard to debug from the outside.
///
/// If the child failed, the parent reaps it with `waitpid` before
/// returning -1, so a failed `forkpty` leaves no zombie.
///
/// ## `vfork` is not usable here
///
/// The child does real work (`login_tty`, `close`, possibly `write`)
/// before exec, which `vfork` forbids — its child shares the parent's
/// address space until exec.  `fork` is required.
///
/// # Errors
///
/// Whatever `openpty`, `pipe2` or `fork` set, or the errno the child
/// reported from `login_tty`.  `EINVAL` if `amaster` is NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn forkpty(
    amaster: *mut i32,
    name: *mut u8,
    termp: *const Termios,
    winp: *const Winsize,
) -> PidT {
    if amaster.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let mut master: i32 = -1;
    let mut slave: i32 = -1;
    if openpty(&raw mut master, &raw mut slave, name, termp, winp) != 0 {
        return -1;
    }

    let mut fds: [i32; 2] = [-1, -1];
    if crate::pipe::pipe2(fds.as_mut_ptr(), O_CLOEXEC) != 0 {
        let e = errno::get_errno();
        crate::file::close(slave);
        crate::file::close(master);
        errno::set_errno(e);
        return -1;
    }
    let (rd, wr) = (fds[0], fds[1]);

    let pid = crate::process::fork();
    if pid == 0 {
        // Child.  The master and the read end belong to the parent.
        crate::file::close(master);
        crate::file::close(rd);
        if login_tty(slave) != 0 {
            let e = errno::get_errno();
            let bytes = e.to_ne_bytes();
            // Best-effort: if this write fails the parent simply sees a
            // zero-length read and believes the child succeeded, which is
            // no worse than not having the pipe at all.  There is nothing
            // further a dying child can do about it.
            crate::file::write(wr, bytes.as_ptr(), bytes.len());
            crate::process::_exit(127);
        }
        crate::file::close(wr);
        return 0;
    }

    if pid < 0 {
        let e = errno::get_errno();
        crate::file::close(rd);
        crate::file::close(wr);
        crate::file::close(slave);
        crate::file::close(master);
        errno::set_errno(e);
        return -1;
    }

    // Parent.  The slave and the write end belong to the child; holding
    // the write end open here would make the read below block forever.
    crate::file::close(slave);
    crate::file::close(wr);

    let mut child_errno_buf = [0u8; 4];
    let got = crate::file::read(
        rd,
        child_errno_buf.as_mut_ptr(),
        core::mem::size_of_val(&child_errno_buf),
    );
    crate::file::close(rd);

    if got > 0 {
        // The child failed in login_tty.  Reap it, discard the pty, and
        // report the child's errno as our own.
        let mut status: i32 = 0;
        crate::process::waitpid(pid, &raw mut status, 0);
        crate::file::close(master);
        let e = i32::from_ne_bytes(child_errno_buf);
        errno::set_errno(if e == 0 { errno::EIO } else { e });
        return -1;
    }

    // SAFETY: NULL-checked at entry.
    unsafe { core::ptr::write_unaligned(amaster, master) };
    pid
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// `openpty` must reject NULL output pointers rather than write
    /// through them, which is the one place we deliberately differ from
    /// glibc (it crashes).
    #[test]
    fn test_openpty_rejects_null_outputs() {
        let mut m: i32 = 0;
        let mut s: i32 = 0;

        errno::set_errno(0);
        assert_eq!(
            openpty(
                core::ptr::null_mut(),
                &raw mut s,
                core::ptr::null_mut(),
                core::ptr::null(),
                core::ptr::null()
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        assert_eq!(
            openpty(
                &raw mut m,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null(),
                core::ptr::null()
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// With no pty layer in the kernel, `openpty` must fail with exactly
    /// the errno `posix_openpt` produced — not a hardcoded verdict of its
    /// own.  This is the test that keeps the composition honest: if
    /// someone replaces the body with a stub returning a fixed errno, and
    /// `posix_openpt` later starts returning something else, this fails.
    #[test]
    fn test_openpty_propagates_posix_openpt_errno() {
        // What posix_openpt reports today, whatever that is.
        errno::set_errno(0);
        assert!(crate::ioctl::posix_openpt(O_RDWR | O_NOCTTY) < 0);
        let expected = errno::get_errno();

        let mut m: i32 = -7;
        let mut s: i32 = -7;
        errno::set_errno(0);
        assert_eq!(
            openpty(
                &raw mut m,
                &raw mut s,
                core::ptr::null_mut(),
                core::ptr::null(),
                core::ptr::null()
            ),
            -1
        );
        assert_eq!(errno::get_errno(), expected);
        // Nothing may be written to the outputs on failure.
        assert_eq!(m, -7);
        assert_eq!(s, -7);
    }

    /// `forkpty` must reject a NULL master pointer, and must do so
    /// *before* forking — a fork whose result cannot be reported is a
    /// leaked process.
    #[test]
    fn test_forkpty_rejects_null_master() {
        errno::set_errno(0);
        assert_eq!(
            forkpty(
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null(),
                core::ptr::null()
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// `forkpty` fails at `openpty` today, so it must report that errno
    /// and must not have forked.
    #[test]
    fn test_forkpty_propagates_openpty_failure() {
        errno::set_errno(0);
        assert!(crate::ioctl::posix_openpt(O_RDWR | O_NOCTTY) < 0);
        let expected = errno::get_errno();

        let mut m: i32 = -7;
        errno::set_errno(0);
        assert_eq!(
            forkpty(
                &raw mut m,
                core::ptr::null_mut(),
                core::ptr::null(),
                core::ptr::null()
            ),
            -1
        );
        assert_eq!(errno::get_errno(), expected);
        assert_eq!(m, -7);
    }

    /// `login_tty` on a descriptor that is not a terminal must fail at
    /// the `TIOCSCTTY` step and leave the standard descriptors alone.
    /// (A negative fd cannot be a terminal on any system, so this holds
    /// whether or not a tty layer exists.)
    #[test]
    fn test_login_tty_rejects_non_terminal() {
        assert_eq!(login_tty(-1), -1);
    }
}

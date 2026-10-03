//! Making, replacing and waiting for processes, the process group they run
//! in, and the root and credentials they run with: `fork`, `execvp`,
//! `waitpid`, `setpgid`, `prctl (PR_SET_DUMPABLE)`, `_exit`, `chroot`,
//! `setgroups`, `setgid` and `setuid`. The last four are what GNU `chroot`
//! changes before it becomes its command.
//!
//! What GNU `timeout` starts its command with
//! (`userspace/coreutils/src/bin/timeout.rs`), and the reason it does not use
//! `std::process::Command` is the reason these exist: `Command` is a good
//! spawner and a different one. It reports a failed exec to the parent, where
//! upstream's child reports its own and exits 126 or 127; it restores the
//! child's `SIGPIPE` to the default, where upstream passes on whatever it was
//! given; and between its fork and its exec it runs code of its own, while
//! upstream's signal handlers -- installed *before* the fork so that no signal
//! is missed -- stay live in the child until the exec replaces them. A port
//! that is to behave as upstream does has to make the calls upstream makes.

use core::ffi::CStr;

use crate::EINVAL;
#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;

/// `waitpid`'s option for "say so if it is still running, rather than wait".
#[cfg(any(unix, test))]
const WNOHANG: i32 = 1;
/// `prctl`'s option for whether this process may leave a core image.
#[cfg(any(unix, test))]
const PR_SET_DUMPABLE: i32 = 4;

/// The library's symbols, declared once. `waitpid` and `_exit` are declared
/// exactly as `pty` declares them, so that one symbol is not declared two ways
/// in one crate.
#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        pub fn fork() -> i32;
        pub fn execvp(file: *const u8, argv: *const *const u8) -> i32;
        pub fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        pub fn setpgid(pid: i32, pgid: i32) -> i32;
        // Variadic, as C declares it: glibc's reads its four further
        // arguments as `unsigned long`s, and SlateOS's names them as `u64`s.
        // On x86-64 both find them in the same registers.
        pub fn prctl(option: i32, ...) -> i32;
        pub fn _exit(status: i32) -> !;
        pub fn chroot(path: *const u8) -> i32;
        pub fn setgroups(size: usize, list: *const u32) -> i32;
        pub fn setgid(gid: u32) -> i32;
        pub fn setuid(uid: u32) -> i32;
        pub fn getpgrp() -> i32;
        pub fn getsid(pid: i32) -> i32;
        pub fn pidfd_open(pid: i32, flags: u32) -> i32;
    }
}

/// What [`fork`] returned, in the process that is reading it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forked {
    /// This is the new process.
    Child,
    /// This is the original; the new one has this process id.
    Parent(i32),
}

/// Copy this process: `fork`.
///
/// # Safety
///
/// The child has only the thread that called this. Any other thread the
/// process had may have held a lock at that instant -- the allocator's among
/// them -- and it stays held in the child for good, so the child of a
/// multi-threaded caller may make only async-signal-safe calls (nothing that
/// allocates) until it reaches [`execvp`] or [`exit_immediately`]. A
/// single-threaded caller, which is what `timeout` is, is under no such
/// constraint; the compiler cannot see which kind it is, hence `unsafe`.
///
/// # Errors
///
/// `EAGAIN` or `ENOMEM` when no process can be made; [`ENOSYS`](crate::ENOSYS) off Unix.
pub unsafe fn fork() -> Result<Forked, i32> {
    fork_one()
}

#[cfg(unix)]
fn fork_one() -> Result<Forked, i32> {
    // SAFETY: `fork` takes nothing and touches no memory of ours; what the
    // child may do afterwards is the caller's contract, stated on `fork`.
    let pid = unsafe { sys::fork() };
    match pid {
        0 => Ok(Forked::Child),
        p if p > 0 => Ok(Forked::Parent(p)),
        _ => Err(last_errno()),
    }
}

#[cfg(not(unix))]
fn fork_one() -> Result<Forked, i32> {
    Err(ENOSYS)
}

/// Become the program `argv[0]` names -- looked for on `PATH` when the name
/// has no `/` in it, as a shell looks -- with `argv` as its arguments:
/// `execvp`.
///
/// Returns only if that failed, with the `errno`: `ENOENT` when there is no
/// such program, `EACCES` when it may not be run, `ENOEXEC` when it is not a
/// program at all.
///
/// `slots` is where the C argument vector is put together -- a pointer per
/// argument and the null that ends them -- so it needs room for
/// `argv.len() + 1`. It is the caller's so that this allocates nothing, which
/// is what lets a child call it between [`fork`] and the exec.
///
/// `EINVAL` when `argv` is empty or `slots` too short; [`ENOSYS`](crate::ENOSYS) off Unix.
#[must_use = "it returns only on failure, and the errno is the reason"]
pub fn execvp(argv: &[&CStr], slots: &mut [*const u8]) -> i32 {
    let Some(program) = argv.first() else {
        return EINVAL;
    };
    let Some(vector) = slots.get_mut(..=argv.len()) else {
        return EINVAL;
    };
    for (slot, arg) in vector.iter_mut().zip(argv) {
        *slot = arg.as_ptr().cast();
    }
    if let Some(end) = vector.last_mut() {
        *end = core::ptr::null();
    }
    execvp_one(program, vector)
}

#[cfg(unix)]
fn execvp_one(program: &CStr, vector: &[*const u8]) -> i32 {
    // SAFETY: `program` is NUL-terminated, and `vector` is a null-terminated
    // array of pointers to NUL-terminated strings -- `execvp` built it from
    // `&CStr`s that outlive this call. On success the call does not return;
    // on failure it has finished reading them.
    unsafe { sys::execvp(program.as_ptr().cast(), vector.as_ptr()) };
    last_errno()
}

#[cfg(not(unix))]
fn execvp_one(_program: &CStr, _vector: &[*const u8]) -> i32 {
    ENOSYS
}

/// What `waitpid` said about a child that has finished: its status word, in
/// the Linux encoding that glibc and SlateOS's library both use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitStatus(i32);

impl WaitStatus {
    /// Its exit status, if it exited: `WIFEXITED`, then `WEXITSTATUS`.
    #[must_use]
    pub const fn exit_code(self) -> Option<i32> {
        if self.0 & 0x7f == 0 {
            Some((self.0 >> 8) & 0xff)
        } else {
            None
        }
    }

    /// The signal that ended it, if one did: `WIFSIGNALED`, then `WTERMSIG`.
    #[must_use]
    pub const fn signal(self) -> Option<i32> {
        let low = self.0 & 0x7f;
        if low != 0 && low != 0x7f {
            Some(low)
        } else {
            None
        }
    }

    /// Whether a signal ended it and it left a core image: `WCOREDUMP`.
    #[must_use]
    pub const fn core_dumped(self) -> bool {
        self.signal().is_some() && self.0 & 0x80 != 0
    }

    /// The word as `waitpid` returned it, for a message with nothing better to
    /// say about one this cannot read.
    #[must_use]
    pub const fn raw(self) -> i32 {
        self.0
    }
}

/// Whether the child `pid` has finished, without waiting for it: `waitpid
/// (pid, &status, WNOHANG)`. `None` while it is still running.
///
/// A finished child is *reaped* by this -- collected, and its process id
/// released -- so its status is reported once, and asking again is `ECHILD`.
///
/// One child only, for the reason [`crate::pty::try_wait`] gives: the wider
/// forms reap whichever child finished, including one some other part of the
/// program is waiting for.
///
/// # Errors
///
/// `EINVAL` when `pid` does not name a single process, `ECHILD` when it is not
/// an unreaped child of this one, and [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn wait_nohang(pid: i32) -> Result<Option<WaitStatus>, i32> {
    if pid <= 0 {
        return Err(EINVAL);
    }
    wait_nohang_one(pid)
}

#[cfg(unix)]
fn wait_nohang_one(pid: i32) -> Result<Option<WaitStatus>, i32> {
    let mut status = 0i32;
    // SAFETY: `status` is a live `int` that the call writes at most once.
    let rc = unsafe { sys::waitpid(pid, &raw mut status, WNOHANG) };
    match rc {
        0 => Ok(None),
        r if r > 0 => Ok(Some(WaitStatus(status))),
        _ => Err(last_errno()),
    }
}

#[cfg(not(unix))]
fn wait_nohang_one(_pid: i32) -> Result<Option<WaitStatus>, i32> {
    Err(ENOSYS)
}

/// Move process `pid` into process group `pgid`: `setpgid`. `(0, 0)` makes
/// the caller the leader of a new group whose id is its own -- what a shell
/// does for each job, and what `timeout` does so that one `kill` reaches
/// everything its command starts.
///
/// # Errors
///
/// `EPERM` when the caller already leads a session, `ESRCH` or `EACCES` for a
/// `pid` it may not move, `EINVAL` for a negative `pgid`, and [`ENOSYS`](crate::ENOSYS) off
/// Unix.
pub fn set_process_group(pid: i32, pgid: i32) -> Result<(), i32> {
    set_process_group_one(pid, pgid)
}

#[cfg(unix)]
fn set_process_group_one(pid: i32, pgid: i32) -> Result<(), i32> {
    // SAFETY: two numbers, and no memory of ours read or written.
    let rc = unsafe { sys::setpgid(pid, pgid) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn set_process_group_one(_pid: i32, _pgid: i32) -> Result<(), i32> {
    Err(ENOSYS)
}

/// Make this process leave no core image if a signal ends it: `prctl
/// (PR_SET_DUMPABLE, 0)`.
///
/// For a program about to end itself with the signal its child died of --
/// `timeout`, passing a child's death on in the one form a shell can see --
/// which must not leave a core file of its own on top of the child's.
///
/// # Errors
///
/// The `errno` from `prctl`. Under SlateOS's library that is `EINVAL` today:
/// its native `prctl` does not take this option yet, though the kernel keeps
/// the flag (`requests/b-d-two-calls-gnu-timeout-makes-are-not-native-yet.md`).
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn disable_core_dumps() -> Result<(), i32> {
    disable_core_dumps_one()
}

#[cfg(unix)]
fn disable_core_dumps_one() -> Result<(), i32> {
    // SAFETY: `PR_SET_DUMPABLE` reads its second argument as a number and
    // touches no memory; the three after it are unused and passed as zero.
    let rc = unsafe { sys::prctl(PR_SET_DUMPABLE, 0u64, 0u64, 0u64, 0u64) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn disable_core_dumps_one() -> Result<(), i32> {
    Err(ENOSYS)
}

/// Make `path` this process's root directory: `chroot`.
///
/// Every path this process resolves from now on starts there, the ones that
/// look absolute included. The working directory is *not* moved -- it may now
/// lie outside the root, which is why `chroot (1)` changes to `/` straight
/// after, unless told not to.
///
/// # Errors
///
/// `EPERM` without the privilege (`CAP_SYS_CHROOT`), `ENOENT` or `ENOTDIR` for
/// a path that names no directory, and [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn change_root(path: &CStr) -> Result<(), i32> {
    change_root_one(path)
}

#[cfg(unix)]
fn change_root_one(path: &CStr) -> Result<(), i32> {
    // SAFETY: `path` is NUL-terminated and borrowed for the call, which reads
    // it and keeps nothing.
    let rc = unsafe { sys::chroot(path.as_ptr().cast()) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn change_root_one(_path: &CStr) -> Result<(), i32> {
    Err(ENOSYS)
}

/// Make `groups` this process's supplementary groups, replacing all of them:
/// `setgroups`. An empty list clears them.
///
/// # Errors
///
/// `EPERM` without the privilege (`CAP_SETGID`) -- and always, in a user
/// namespace whose `setgroups` is denied -- `EINVAL` for more groups than the
/// system keeps, and [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_groups(groups: &[u32]) -> Result<(), i32> {
    set_groups_one(groups)
}

#[cfg(unix)]
fn set_groups_one(groups: &[u32]) -> Result<(), i32> {
    // SAFETY: `groups` is a live slice of `gid_t`s, `groups.len()` long, which
    // is exactly what the call reads.
    let rc = unsafe { sys::setgroups(groups.len(), groups.as_ptr()) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn set_groups_one(_groups: &[u32]) -> Result<(), i32> {
    Err(ENOSYS)
}

/// Set this process's group id: `setgid`.
///
/// # Errors
///
/// `EPERM` without the privilege, `EINVAL` for a gid this system -- or this
/// user namespace -- cannot represent, and [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_gid(gid: u32) -> Result<(), i32> {
    set_gid_one(gid)
}

#[cfg(unix)]
fn set_gid_one(gid: u32) -> Result<(), i32> {
    // SAFETY: a number, and no memory of ours read or written.
    let rc = unsafe { sys::setgid(gid) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn set_gid_one(_gid: u32) -> Result<(), i32> {
    Err(ENOSYS)
}

/// Set this process's user id: `setuid`. For a privileged process it is a
/// one-way step -- the privilege goes with the old id -- which is why it is
/// the last of the three credentials a program sets.
///
/// # Errors
///
/// `EPERM` without the privilege, `EINVAL` for a uid this system -- or this
/// user namespace -- cannot represent, `EAGAIN` when the new user is at its
/// limit of processes, and [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_uid(uid: u32) -> Result<(), i32> {
    set_uid_one(uid)
}

#[cfg(unix)]
fn set_uid_one(uid: u32) -> Result<(), i32> {
    // SAFETY: a number, and no memory of ours read or written.
    let rc = unsafe { sys::setuid(uid) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn set_uid_one(_uid: u32) -> Result<(), i32> {
    Err(ENOSYS)
}

/// This process's process group: `getpgrp`, which cannot fail.
///
/// # Errors
///
/// [`ENOSYS`](crate::ENOSYS) off Unix, where there are no process groups to
/// be in. On Unix, never.
pub fn process_group() -> Result<i32, i32> {
    process_group_one()
}

#[cfg(unix)]
// One signature for both arms; only the host one can fail.
#[allow(clippy::unnecessary_wraps)]
fn process_group_one() -> Result<i32, i32> {
    // SAFETY: no arguments, and no memory of ours read or written.
    Ok(unsafe { sys::getpgrp() })
}

#[cfg(not(unix))]
fn process_group_one() -> Result<i32, i32> {
    Err(ENOSYS)
}

/// The session process `pid` belongs to -- 0 for this one: `getsid`.
///
/// # Errors
///
/// `ESRCH` for no such process, `EPERM` for one in another session that the
/// system will not describe; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn session_of(pid: i32) -> Result<i32, i32> {
    session_of_one(pid)
}

#[cfg(unix)]
fn session_of_one(pid: i32) -> Result<i32, i32> {
    // SAFETY: one number in, one out; no memory of ours.
    let sid = unsafe { sys::getsid(pid) };
    if sid >= 0 { Ok(sid) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn session_of_one(_pid: i32) -> Result<i32, i32> {
    Err(ENOSYS)
}

/// A descriptor that refers to process `pid` itself rather than to its
/// number, and becomes readable when the process exits: `pidfd_open`.
///
/// The descriptor is the caller's to close.
///
/// # Errors
///
/// The `errno` from `pidfd_open`: `ESRCH` for no such process, `EINVAL` for a
/// `pid` below 1 or unknown `flags`, `EMFILE`, and `ENOSYS` where the system
/// cannot do it -- which today includes SlateOS's own library: its native
/// system-call table has no number for the call the kernel implements for
/// Linux programs (`known-issues/B-THE-NATIVE-LIBC-AND-THE-LINUX-ABI-DISAGREE-ABOUT-WHAT-EXISTS.md`).
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn pidfd_open(pid: i32, flags: u32) -> Result<i32, i32> {
    pidfd_open_one(pid, flags)
}

#[cfg(unix)]
fn pidfd_open_one(pid: i32, flags: u32) -> Result<i32, i32> {
    // SAFETY: two numbers in, a descriptor or -1 out; no memory of ours.
    let fd = unsafe { sys::pidfd_open(pid, flags) };
    if fd >= 0 { Ok(fd) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn pidfd_open_one(_pid: i32, _flags: u32) -> Result<i32, i32> {
    Err(ENOSYS)
}

/// End this process now, with `status`: `_exit`.
///
/// Nothing else runs -- no `atexit` handler, no flush of any buffer -- which
/// is what makes it the way out of a signal handler, and out of a child that
/// must not flush the copy of its parent's buffers it was born with.
///
/// Unix only. There is nothing for a host arm to do instead, and no failure it
/// could report: `_exit` does not return.
#[cfg(unix)]
pub fn exit_immediately(status: i32) -> ! {
    // SAFETY: `_exit` takes a number and does not return.
    unsafe { sys::_exit(status) }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    /// The restated numbers are the library's.
    #[test]
    fn the_numbers_are_the_librarys() {
        assert_eq!(WNOHANG, posix::process::WNOHANG);
        assert_eq!(PR_SET_DUMPABLE, posix::sys_prctl::PR_SET_DUMPABLE);
    }

    /// A status word reads the way the C macros read it -- compared with
    /// `posix`'s own, so the two cannot drift apart.
    #[test]
    fn status_words_read_as_the_c_macros_read_them() {
        use posix::sys_wait::{wcoredump, wexitstatus, wifexited, wifsignaled, wtermsig};
        for raw in [
            0, 0x100, 0x7f00, 0xff00, 9, 15, 0x8b, 0x0f, 0x86, 0x137f, 0xffff,
        ] {
            let status = WaitStatus(raw);
            let exited = wifexited(raw).then(|| wexitstatus(raw));
            let signaled = wifsignaled(raw).then(|| wtermsig(raw));
            assert_eq!(status.exit_code(), exited, "{raw:#x}");
            assert_eq!(status.signal(), signaled, "{raw:#x}");
            assert_eq!(
                status.core_dumped(),
                wifsignaled(raw) && wcoredump(raw),
                "{raw:#x}"
            );
            assert_eq!(status.raw(), raw);
        }
        assert_eq!(WaitStatus(0x0300).exit_code(), Some(3));
        assert_eq!(WaitStatus(0x89).signal(), Some(9));
        assert!(WaitStatus(0x89).core_dumped());
        // Stopped is neither an exit nor a death.
        assert_eq!(WaitStatus(0x137f).exit_code(), None);
        assert_eq!(WaitStatus(0x137f).signal(), None);
    }

    /// A pid that is not one process is refused before it can reach the
    /// library, on every build.
    #[test]
    fn a_wider_wait_is_refused() {
        assert_eq!(wait_nohang(0), Err(EINVAL));
        assert_eq!(wait_nohang(-1), Err(EINVAL));
    }

    /// An empty argument vector, or too little room to build one, is refused
    /// before anything is run.
    #[test]
    fn an_argument_vector_that_cannot_be_built_is_refused() {
        let mut slots = [core::ptr::null(); 2];
        assert_eq!(execvp(&[], &mut slots), EINVAL);
        let words = [c"true", c"x"];
        assert_eq!(
            execvp(&words, &mut slots),
            EINVAL,
            "two words need three slots"
        );
    }

    /// Off Unix every call declines.
    #[cfg(not(unix))]
    #[test]
    fn off_unix_every_call_declines() {
        // SAFETY: the host arm makes no process at all.
        assert_eq!(unsafe { fork() }, Err(ENOSYS));
        assert_eq!(wait_nohang(1), Err(ENOSYS));
        assert_eq!(set_process_group(0, 0), Err(ENOSYS));
        assert_eq!(disable_core_dumps(), Err(ENOSYS));
        let mut slots = [core::ptr::null(); 2];
        assert_eq!(execvp(&[c"true"], &mut slots), ENOSYS);
        assert_eq!(change_root(c"/"), Err(ENOSYS));
        assert_eq!(set_groups(&[]), Err(ENOSYS));
        assert_eq!(set_gid(0), Err(ENOSYS));
        assert_eq!(set_uid(0), Err(ENOSYS));
    }

    /// The real library's refusals, chosen so that no outcome changes this
    /// test process's root or credentials: a root that does not exist, and
    /// the one id no process can have, `(uid_t) -1`.
    #[cfg(unix)]
    #[test]
    fn credentials_and_root_are_refused_as_the_library_refuses_them() {
        assert_eq!(
            change_root(c"/nonexistent/libcall-test"),
            Err(crate::ENOENT)
        );
        assert_eq!(set_uid(u32::MAX), Err(posix::errno::EINVAL));
        assert_eq!(set_gid(u32::MAX), Err(posix::errno::EINVAL));
        // Too many groups: `EPERM` for a caller without the privilege, which
        // the kernel checks first, `EINVAL` for one with it. Neither changes
        // anything.
        let too_many = [0u32; 65537];
        let refused = set_groups(&too_many);
        assert!(
            refused == Err(posix::errno::EPERM) || refused == Err(posix::errno::EINVAL),
            "{refused:?}"
        );
    }

    /// The real library: a child is made, exits with a status of its own,
    /// and the parent reads it back -- and a failed exec reports its errno.
    ///
    /// The child makes no call but `execvp` and `_exit`, both
    /// async-signal-safe, because the test harness is multi-threaded: exactly
    /// the case `fork`'s safety contract is about.
    #[cfg(unix)]
    #[test]
    fn a_child_is_made_and_its_status_read_back() {
        extern crate std;
        use std::time::{Duration, Instant};

        let mut slots = [core::ptr::null(); 2];
        // SAFETY: the child calls only `execvp` and `exit_immediately`.
        let pid = match unsafe { fork() }.unwrap() {
            Forked::Child => {
                let errno = execvp(&[c"/nonexistent/libcall-test"], &mut slots);
                exit_immediately(if errno == crate::ENOENT { 7 } else { 8 });
            }
            Forked::Parent(pid) => pid,
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(status) = wait_nohang(pid).unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "the child never finished");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(status.exit_code(), Some(7), "{:#x}", status.raw());
        assert_eq!(wait_nohang(pid), Err(posix::errno::ECHILD), "reaped once");
    }
}

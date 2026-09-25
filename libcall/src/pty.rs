//! A program running on a new pseudo-terminal: what a terminal emulator needs
//! from the C library, and nothing more.
//!
//! # Why this is here
//!
//! `apps/terminal` is a terminal emulator, which is the *master* side of a
//! pseudo-terminal: it writes what the user types into the master and draws
//! what comes back out of it. Everything between the two ends — echo, line
//! editing in cooked mode, turning `^C` into `SIGINT` for the foreground job,
//! telling a full-screen program its window changed size — is the kernel's
//! line discipline (`kernel/src/tty/pty.rs`), and the shell on the slave side
//! finds out it is on a terminal because `isatty(0)` is true.
//!
//! None of that is reachable from `std`. `std::process::Command` can give a
//! child pipes, and a shell on pipes is not interactive: no prompt, no job
//! control, no `^C`, and every full-screen program refuses to start. The call
//! that puts a child on a terminal is `forkpty`, which `posix` implements
//! (`posix/src/pty.rs`) and which, being stateful, `design-decisions.md` §768
//! says a program reaches through the C ABI — this crate — rather than by
//! naming `posix`. Asked for in
//! `requests/c-b-a-terminal-needs-a-shell-on-the-other-end-of-its-pty.md`.
//!
//! # One call, not `fork` then `exec`
//!
//! Between the fork and the exec the child is a copy of a possibly
//! multi-threaded process in which only the forking thread survived. A lock
//! that any other thread held at the instant of the fork — the allocator's
//! included — is held for ever in the child. So the child may do nothing but
//! async-signal-safe system calls until it execs, and that is not a rule a
//! caller can be trusted to keep from inside a closure, so the API offers no
//! closure. [`spawn`] builds everything the child will need *before* forking —
//! the argument and environment vectors live on this function's stack, which
//! the child inherits a copy of — and the child's entire remaining life is:
//! restore `SIGPIPE`, unblock every signal, `execve`, and on failure report
//! `errno` and `_exit`. That is the request's own reasoning ("a rule I would
//! be enforcing from the wrong side of the ABI"), carried to its end.
//!
//! # A failed `exec` is an error, not an exit code
//!
//! Without help, a parent learns that `execve` failed only when the child
//! exits with status 127, which is indistinguishable from a shell that ran and
//! exited 127 on its own. [`spawn`] opens a close-on-exec pipe first: a
//! successful `execve` closes the child's end without writing, so the parent's
//! read returns zero bytes; a failed one writes its `errno` there before
//! `_exit`. So `spawn("/bin/zsh", …)` on a machine without zsh returns
//! `Err(ENOENT)` and leaves no child behind, which is what lets a terminal say
//! *which* shell could not be started, and why. It is the same device
//! `forkpty` itself uses one level down for `login_tty`, and the same one
//! `std::process::Command` uses for its own spawns.
//!
//! # What the child inherits, deliberately
//!
//! * **`SIGPIPE` is put back to its default.** Rust's runtime ignores
//!   `SIGPIPE` in every program it starts, and an ignored disposition survives
//!   `execve`. A shell started without resetting it runs every pipeline with
//!   `SIGPIPE` ignored, so `yes | head -1` makes `yes` print "Broken pipe" and
//!   loop on `EPIPE` instead of dying quietly. `std::process::Command` resets it
//!   for the same reason.
//! * **The signal mask is emptied.** A mask is inherited across both `fork` and
//!   `execve`, and a shell that starts with `SIGINT` blocked cannot be
//!   interrupted.
//! * **The master is marked close-on-exec in the parent.** Otherwise the next
//!   program this process starts — the second tab's shell — inherits the first
//!   tab's master, and closing the first tab no longer hangs up its shell,
//!   because a master still open anywhere is a terminal still connected.
//!
//! # Host builds
//!
//! On a non-`unix` host there are no pseudo-terminals, and every call returns
//! [`ENOSYS`](crate::ENOSYS) after the argument checks — the same choice the
//! rest of this crate makes, and for the same reason: a terminal on a host
//! must be able to *say* it has nothing to connect to, which it cannot do if
//! the call pretends to succeed. The checks run first on every build so that
//! the host test binary proves them.

use core::ffi::CStr;

use crate::EINVAL;

/// Argument list too long: more arguments or environment entries than
/// [`MAX_ARGS`] or [`MAX_ENV`].
pub const E2BIG: i32 = 7;
/// Bad file descriptor.
pub const EBADF: i32 = 9;
/// An interrupted system call. Retried here, never reported.
pub const EINTR: i32 = 4;
/// Input/output error. What a master reports once every slave is closed.
pub const EIO: i32 = 5;

// The rest of what `spawn` can report, so that a caller can say *why* a
// program would not start without depending on `posix` for the numbers.
// Checked against `posix` in `constants_agree_with_posix`.

/// Not an executable format the kernel knows.
pub const ENOEXEC: i32 = 8;
/// No such child -- asked about a pid that is not ours, or already reaped.
pub const ECHILD: i32 = 10;
/// Out of processes or some other resource that may free up; try again.
pub const EAGAIN: i32 = 11;
/// Out of memory.
pub const ENOMEM: i32 = 12;
/// Permission denied: the program is not executable, or a directory on the
/// way to it is not searchable.
pub const EACCES: i32 = 13;
/// A component of the path is not a directory.
pub const ENOTDIR: i32 = 20;
/// The system-wide descriptor or terminal table is full.
pub const ENFILE: i32 = 23;
/// This process has run out of descriptors.
pub const EMFILE: i32 = 24;
/// Not a terminal.
pub const ENOTTY: i32 = 25;
/// The program is open for writing and so may not be run.
pub const ETXTBSY: i32 = 26;
/// Too many symbolic links on the way to the program.
pub const ELOOP: i32 = 40;

/// Hang up. What a terminal sends the program on its slave when the window
/// closes — the one signal every shell interprets as "your terminal is gone",
/// and passes on to the jobs it started.
pub const SIGHUP: i32 = 1;

/// The largest argument vector [`spawn`] accepts, not counting the
/// terminating null.
///
/// A bound rather than a heap allocation because the vector must exist before
/// the fork and must not be built after it, and this crate has no allocator.
/// 256 arguments is far beyond anything a terminal passes to a shell.
pub const MAX_ARGS: usize = 256;

/// The largest environment [`spawn`] accepts, not counting the terminating
/// null.
///
/// A login environment is a few dozen entries; a thousand leaves room for a
/// user who exports a great deal without putting a variable-length array on
/// the stack.
pub const MAX_ENV: usize = 1024;

/// A terminal's size, in character cells and, optionally, in pixels.
///
/// The same four fields as C's `struct winsize`, named for what they mean
/// rather than with its `ws_` prefixes. A pixel size of zero means "unknown",
/// which is what most terminals report and what programs expect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WinSize {
    /// Rows of character cells.
    pub rows: u16,
    /// Columns of character cells.
    pub cols: u16,
    /// Width of the text area in pixels, or zero.
    pub xpixel: u16,
    /// Height of the text area in pixels, or zero.
    pub ypixel: u16,
}

/// A program started by [`spawn`].
///
/// Two integers the caller now owns: a process to reap with [`try_wait`] and
/// a descriptor to close. Deliberately not an RAII type — this crate is
/// `no_std` and has no `OwnedFd` to offer — so the caller wraps `master` in
/// whatever owns descriptors in its world (`std::fs::File`, usually) at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spawned {
    /// The child's process id. It is a session leader, so this is also its
    /// process group id and its session id.
    pub pid: i32,
    /// The master side of the child's terminal: write keystrokes here, read
    /// its output from here. Close-on-exec.
    pub master: i32,
}

/// Where a child started by [`spawn`] stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildState {
    /// Still running (or stopped — a stop is not an exit).
    Running,
    /// It called `exit` with this status. Reaped: the pid is free again.
    Exited(i32),
    /// A signal ended it — this one. Reaped: the pid is free again.
    Signaled(i32),
}

/// Start `path` on the slave side of a new pseudo-terminal of `size`.
///
/// `argv` is the argument vector, starting with the program's own name, and
/// `envp` is the entire environment, as `NAME=value` entries — nothing is
/// inherited implicitly, so a caller that wants its own environment passed on
/// must pass it. `path` is not searched for on `PATH`; `execve` takes it as
/// given.
///
/// The child is a session leader whose controlling terminal is the new slave,
/// with the slave on its standard input, output and error. See the module
/// documentation for everything else it inherits, and why.
///
/// # Errors
///
/// * [`EINVAL`] — `argv` is empty. Every program is entitled to find its own
///   name in `argv[0]`, and a shell uses it to decide whether it is a login
///   shell.
/// * [`E2BIG`] — more than [`MAX_ARGS`] arguments or [`MAX_ENV`] environment
///   entries.
/// * The `errno` from creating the terminal or forking: `EMFILE`/`ENFILE` when
///   descriptors or terminals have run out, `EAGAIN`/`ENOMEM` from `fork`.
/// * The `errno` that `execve` failed with in the child — `ENOENT` for a
///   program that does not exist, `EACCES` for one that may not be run,
///   `ENOEXEC` for a file that is not a program. The child has been reaped and
///   the terminal closed; nothing is left to clean up.
/// * [`ENOSYS`](crate::ENOSYS) — built for a host with no pseudo-terminals.
pub fn spawn(path: &CStr, argv: &[&CStr], envp: &[&CStr], size: WinSize) -> Result<Spawned, i32> {
    // Checked before the target split, so that the host test binary proves
    // them: a check compiled only for the target is a check nothing here runs.
    if argv.is_empty() {
        return Err(EINVAL);
    }
    if argv.len() > MAX_ARGS || envp.len() > MAX_ENV {
        return Err(E2BIG);
    }
    spawn_one(path, argv, envp, size)
}

/// Tell the program on `master`'s terminal that the terminal is now `size`.
///
/// This is `ioctl(master, TIOCSWINSZ)`: the kernel records the size and
/// raises `SIGWINCH` for the terminal's foreground process group if it
/// changed, which is how a full-screen program learns to redraw at the new
/// width. A terminal that resizes its grid without calling this leaves the
/// shell wrapping its prompt at the old width.
///
/// # Errors
///
/// * [`EBADF`] — `master` is negative, or not an open descriptor.
/// * `ENOTTY` — `master` is not a terminal.
/// * [`ENOSYS`](crate::ENOSYS) — built for a host with no pseudo-terminals.
pub fn set_window_size(master: i32, size: WinSize) -> Result<(), i32> {
    if master < 0 {
        return Err(EBADF);
    }
    set_window_size_one(master, size)
}

/// Whether the child `pid` has finished, without waiting for it to.
///
/// `waitpid(pid, WNOHANG)`. A child that has finished is *reaped* by this
/// call — its exit status is collected and its process id released — so the
/// answer [`ChildState::Exited`] or [`ChildState::Signaled`] is given once,
/// and asking again afterwards is an error (`ECHILD`), not a second answer.
///
/// # A single child, deliberately
///
/// `waitpid` gives `pid <= 0` three wider meanings: any child at all, any
/// child in the caller's process group, any child in a named group. Each one
/// reaps whichever child happens to have finished — including one that some
/// other part of the program started and is waiting for itself, whose exit
/// status is then gone for good. So this refuses them with [`EINVAL`], for
/// the reason [`crate::kill`] refuses its broadcast forms: every caller here
/// means one child.
///
/// # Errors
///
/// * [`EINVAL`] — `pid` does not name a single process.
/// * `ECHILD` — `pid` is not a child of this process, or was already reaped.
/// * [`ENOSYS`](crate::ENOSYS) — built for a host with no processes of ours to wait for.
pub fn try_wait(pid: i32) -> Result<ChildState, i32> {
    if pid <= 0 {
        return Err(EINVAL);
    }
    try_wait_one(pid)
}

/// What a `waitpid` status word says, in the Linux encoding our `posix` and
/// glibc both use.
///
/// Pure, so the host test binary checks the decoding even though it can never
/// produce a status of its own.
///
/// The low seven bits are the terminating signal, zero for a normal exit;
/// `0x7f` there means stopped, and `0xffff` as a whole means continued —
/// neither of which `waitpid` reports without `WUNTRACED`/`WCONTINUED`, which
/// [`try_wait`] does not pass, so both read as still running.
///
/// Private: [`try_wait`] is its one caller, and a status word from anywhere
/// else is a status word from a `waitpid` this crate did not make. Compiled
/// where that caller is, and for the tests on every build.
#[cfg(any(unix, test))]
#[must_use]
const fn decode_status(status: i32) -> ChildState {
    let low = status & 0x7f;
    if low == 0 {
        ChildState::Exited((status >> 8) & 0xff)
    } else if low != 0x7f && status != 0xffff {
        ChildState::Signaled(low)
    } else {
        ChildState::Running
    }
}

// ---------------------------------------------------------------------------
// The C library
// ---------------------------------------------------------------------------

/// The symbols `spawn` and friends call, declared once.
///
/// `ioctl` and `fcntl` are declared variadic because that is what they are in
/// C. Our `posix` defines them with a fixed third argument, and on x86-64 the
/// two conventions put that argument in the same register, so one declaration
/// serves both our library and glibc — which is what the `unix` test run on a
/// Linux host links against.
#[cfg(unix)]
mod sys {
    /// C's `struct winsize`.
    #[repr(C)]
    pub struct Winsize {
        pub ws_row: u16,
        pub ws_col: u16,
        pub ws_xpixel: u16,
        pub ws_ypixel: u16,
    }

    unsafe extern "C" {
        pub fn forkpty(
            amaster: *mut i32,
            name: *mut u8,
            termp: *const u8,
            winp: *const Winsize,
        ) -> i32;
        pub fn execve(path: *const u8, argv: *const *const u8, envp: *const *const u8) -> i32;
        pub fn pipe2(fds: *mut i32, flags: i32) -> i32;
        pub fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
        pub fn write(fd: i32, buf: *const u8, count: usize) -> isize;
        pub fn close(fd: i32) -> i32;
        pub fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        pub fn signal(signum: i32, handler: usize) -> usize;
        pub fn sigprocmask(how: i32, set: *const u64, oldset: *mut u64) -> i32;
        pub fn ioctl(fd: i32, request: u64, ...) -> i32;
        pub fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        pub fn _exit(status: i32) -> !;
    }

    /// `O_CLOEXEC`, for `pipe2`.
    pub const O_CLOEXEC: i32 = 0o2_000_000;
    /// `fcntl`: set the descriptor flags.
    pub const F_SETFD: i32 = 2;
    /// The one descriptor flag: close this descriptor on `execve`.
    pub const FD_CLOEXEC: i64 = 1;
    /// `TIOCSWINSZ`: set a terminal's window size.
    pub const TIOCSWINSZ: u64 = 0x5414;
    /// `waitpid`: do not block.
    pub const WNOHANG: i32 = 1;
    /// `sigprocmask`: replace the mask outright.
    pub const SIG_SETMASK: i32 = 2;
    /// The signal a write to a pipe with no reader raises.
    pub const SIGPIPE: i32 = 13;
    /// The default disposition.
    pub const SIG_DFL: usize = 0;
    /// Words in a signal set: 1024 signals, the size glibc's `sigset_t` and
    /// our `SigsetT` both have. An empty set of the larger size is an empty
    /// set of any smaller one.
    pub const SIGSET_WORDS: usize = 16;
}

#[cfg(unix)]
fn spawn_one(path: &CStr, argv: &[&CStr], envp: &[&CStr], size: WinSize) -> Result<Spawned, i32> {
    use core::ptr;

    // Both vectors are built now, in the parent, because the child may not
    // build anything. One slot longer than the limit and filled with null, so
    // the terminating null `execve` needs is already there whatever the length
    // -- the length was checked against the limit before we got here.
    let mut argv_ptrs: [*const u8; MAX_ARGS + 1] = [ptr::null(); MAX_ARGS + 1];
    for (slot, arg) in argv_ptrs.iter_mut().zip(argv) {
        *slot = arg.as_ptr().cast::<u8>();
    }
    let mut envp_ptrs: [*const u8; MAX_ENV + 1] = [ptr::null(); MAX_ENV + 1];
    for (slot, var) in envp_ptrs.iter_mut().zip(envp) {
        *slot = var.as_ptr().cast::<u8>();
    }
    let winsize = sys::Winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: size.xpixel,
        ws_ypixel: size.ypixel,
    };
    let empty_mask = [0u64; sys::SIGSET_WORDS];

    // The exec report, before the fork so both sides have it. Close-on-exec on
    // both ends: the child's write end must vanish at a successful `execve`
    // (that is the success signal), and neither end may leak into anything
    // else this process ever starts.
    let mut report = [-1i32; 2];
    // SAFETY: `report` is two writable `i32`s, which is what `pipe2` fills.
    if unsafe { sys::pipe2(report.as_mut_ptr(), sys::O_CLOEXEC) } != 0 {
        return Err(crate::last_errno());
    }
    let [report_rd, report_wr] = report;

    let mut master: i32 = -1;
    // SAFETY: `master` is a writable `i32`; a null `name` and a null `termp`
    // are documented as "do not return the name" and "keep the default
    // termios"; `winsize` is a live `struct winsize` for the whole call.
    let pid = unsafe {
        sys::forkpty(
            &raw mut master,
            ptr::null_mut(),
            ptr::null(),
            &raw const winsize,
        )
    };

    if pid == 0 {
        // The child. Only async-signal-safe system calls from here on -- see
        // the module documentation -- and every value used below was computed
        // before the fork. Nothing here returns: it execs or it exits.
        //
        // SAFETY (the whole block): each call receives descriptors this process
        // owns, or pointers into `argv_ptrs`/`envp_ptrs`/`empty_mask`, which the
        // fork copied into this child and which outlive every call. `path` and
        // every string the vectors point at were borrowed by the parent for the
        // duration of `spawn`, and the child's copy of the parent's memory holds
        // them unchanged.
        unsafe {
            sys::close(report_rd);
            // Failure of either reset is not reported: the child would have to
            // choose between running with the wrong disposition and not running
            // at all, and a shell that runs is the better of the two. Neither
            // can fail for a valid signal number and a valid set, which these
            // are.
            sys::signal(sys::SIGPIPE, sys::SIG_DFL);
            sys::sigprocmask(sys::SIG_SETMASK, empty_mask.as_ptr(), ptr::null_mut());
            sys::execve(
                path.as_ptr().cast::<u8>(),
                argv_ptrs.as_ptr(),
                envp_ptrs.as_ptr(),
            );
            // Still here: `execve` failed. Tell the parent why. A failed write
            // leaves the parent reading zero bytes -- "it exec'd" -- and then
            // seeing an exit status of 127, which is wrong but not dangerous;
            // there is nothing more a child in this state can do.
            let bytes = crate::last_errno().to_ne_bytes();
            sys::write(report_wr, bytes.as_ptr(), bytes.len());
            sys::_exit(127);
        }
    }

    // The parent. Its copy of the write end must go before the read below,
    // or the read would wait for a writer that is this process.
    // SAFETY: `report_wr` is a descriptor this process opened above.
    unsafe { sys::close(report_wr) };

    if pid < 0 {
        let e = crate::last_errno();
        // SAFETY: as above, `report_rd` is ours.
        unsafe { sys::close(report_rd) };
        return Err(e);
    }

    let mut buf = [0u8; 4];
    let got = read_all_retrying(report_rd, &mut buf);
    // SAFETY: as above.
    unsafe { sys::close(report_rd) };

    if got > 0 {
        // The child reported an `execve` failure. Reap it so it does not
        // linger as a zombie, and discard the terminal nobody will use.
        let e = i32::from_ne_bytes(buf);
        reap(pid);
        // SAFETY: `master` was filled in by a successful `forkpty`.
        unsafe { sys::close(master) };
        return Err(if e > 0 { e } else { EIO });
    }

    // SAFETY: `master` is a descriptor this process owns; `F_SETFD` takes an
    // integer flag word and touches no memory of ours.
    if unsafe { sys::fcntl(master, sys::F_SETFD, sys::FD_CLOEXEC) } != 0 {
        // A master that would leak into the next program this process starts
        // is a terminal that cannot be hung up by closing it -- see the module
        // documentation. Refuse rather than hand one out: kill the child we
        // just started, reap it, and report why.
        let e = crate::last_errno();
        // The child is ours and alive, so this cannot fail in a way that
        // leaves anything to do: at worst it has already exited, and the reap
        // below collects it either way.
        let _ = crate::kill(pid, crate::SIGKILL);
        reap(pid);
        // SAFETY: as above.
        unsafe { sys::close(master) };
        return Err(e);
    }

    Ok(Spawned { pid, master })
}

/// Read until `buf` is full or the writer has gone, retrying `EINTR`.
///
/// Returns the number of bytes read; a read error counts as the writer having
/// gone, which from the exec report's point of view it has. Zero is "the
/// child exec'd"; anything else is "the child is reporting".
#[cfg(unix)]
fn read_all_retrying(fd: i32, buf: &mut [u8]) -> usize {
    let mut got = 0usize;
    while let Some(rest) = buf.get_mut(got..) {
        if rest.is_empty() {
            break;
        }
        // SAFETY: `rest` is a live, writable slice and the length handed over
        // is its own.
        let n = unsafe { sys::read(fd, rest.as_mut_ptr(), rest.len()) };
        match usize::try_from(n) {
            Ok(0) => break,
            Ok(n) => got = got.saturating_add(n),
            Err(_) if crate::last_errno() == EINTR => {}
            Err(_) => break,
        }
    }
    got
}

/// Wait for `pid` to finish and discard its status, retrying `EINTR`.
///
/// Used only for a child this module has just started and knows is exiting
/// or killed, so blocking here is bounded by that child's last few system
/// calls.
#[cfg(unix)]
fn reap(pid: i32) {
    let mut status = 0i32;
    loop {
        // SAFETY: `status` is a writable `i32`.
        let rc = unsafe { sys::waitpid(pid, &raw mut status, 0) };
        if rc >= 0 || crate::last_errno() != EINTR {
            return;
        }
    }
}

#[cfg(unix)]
fn set_window_size_one(master: i32, size: WinSize) -> Result<(), i32> {
    let winsize = sys::Winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: size.xpixel,
        ws_ypixel: size.ypixel,
    };
    // SAFETY: `TIOCSWINSZ` reads one `struct winsize` through the pointer,
    // which is `winsize`, live for the whole call; nothing is written back.
    let rc = unsafe { sys::ioctl(master, sys::TIOCSWINSZ, &raw const winsize) };
    if rc == 0 {
        Ok(())
    } else {
        Err(crate::last_errno())
    }
}

#[cfg(unix)]
fn try_wait_one(pid: i32) -> Result<ChildState, i32> {
    let mut status = 0i32;
    loop {
        // SAFETY: `status` is a writable `i32`.
        let rc = unsafe { sys::waitpid(pid, &raw mut status, sys::WNOHANG) };
        match rc {
            0 => return Ok(ChildState::Running),
            r if r > 0 => return Ok(decode_status(status)),
            _ => {
                let e = crate::last_errno();
                if e != EINTR {
                    return Err(e);
                }
            }
        }
    }
}

#[cfg(not(unix))]
fn spawn_one(
    _path: &CStr,
    _argv: &[&CStr],
    _envp: &[&CStr],
    _size: WinSize,
) -> Result<Spawned, i32> {
    Err(crate::ENOSYS)
}

#[cfg(not(unix))]
fn set_window_size_one(_master: i32, _size: WinSize) -> Result<(), i32> {
    Err(crate::ENOSYS)
}

#[cfg(not(unix))]
fn try_wait_one(_pid: i32) -> Result<ChildState, i32> {
    Err(crate::ENOSYS)
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it; the defensive lints exist for code that runs on a
    // user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    #[cfg(not(unix))]
    use crate::ENOSYS;

    const SH: &CStr = c"/bin/sh";

    fn size(rows: u16, cols: u16) -> WinSize {
        WinSize {
            rows,
            cols,
            xpixel: 0,
            ypixel: 0,
        }
    }

    /// Every restated value equals `posix`'s. See `constants_agree_with_posix`
    /// in the crate root for why a restatement needs a comparison.
    #[test]
    fn constants_agree_with_posix() {
        assert_eq!(E2BIG, posix::errno::E2BIG);
        assert_eq!(EBADF, posix::errno::EBADF);
        assert_eq!(EINTR, posix::errno::EINTR);
        assert_eq!(EIO, posix::errno::EIO);
        assert_eq!(ENOEXEC, posix::errno::ENOEXEC);
        assert_eq!(ECHILD, posix::errno::ECHILD);
        assert_eq!(EAGAIN, posix::errno::EAGAIN);
        assert_eq!(ENOMEM, posix::errno::ENOMEM);
        assert_eq!(EACCES, posix::errno::EACCES);
        assert_eq!(ENOTDIR, posix::errno::ENOTDIR);
        assert_eq!(ENFILE, posix::errno::ENFILE);
        assert_eq!(EMFILE, posix::errno::EMFILE);
        assert_eq!(ENOTTY, posix::errno::ENOTTY);
        assert_eq!(ETXTBSY, posix::errno::ETXTBSY);
        assert_eq!(ELOOP, posix::errno::ELOOP);
        assert_eq!(SIGHUP, posix::signal::SIGHUP);
    }

    /// The `unix` arm's private copies, against the same source.
    #[cfg(unix)]
    #[test]
    fn the_c_constants_agree_with_posix() {
        assert_eq!(sys::O_CLOEXEC, posix::fcntl::O_CLOEXEC);
        assert_eq!(sys::F_SETFD, posix::fcntl_ops::F_SETFD);
        assert_eq!(sys::FD_CLOEXEC, i64::from(posix::fdtable::FD_CLOEXEC));
        assert_eq!(sys::TIOCSWINSZ, posix::ioctl::TIOCSWINSZ);
        assert_eq!(sys::WNOHANG, posix::process::WNOHANG);
        assert_eq!(sys::SIG_SETMASK, posix::signal::SIG_SETMASK);
        assert_eq!(sys::SIGPIPE, posix::signal::SIGPIPE);
        assert_eq!(sys::SIG_DFL, posix::signal::SIG_DFL);
        assert_eq!(
            core::mem::size_of::<[u64; sys::SIGSET_WORDS]>(),
            core::mem::size_of::<posix::signal::SigsetT>()
        );
    }

    /// A status word decodes the way `posix`'s own macros read it.
    ///
    /// Checked against `posix::process`'s `wifexited` family rather than
    /// against literals alone, so that the two decodings cannot drift apart.
    #[test]
    fn status_words_decode_like_the_c_macros() {
        use posix::process::{wexitstatus, wifexited, wifsignaled, wtermsig};
        for status in [0, 0x100, 0x7f00, 0xff00, 9, 15, 0x8b, 0x0f] {
            let expected = if wifexited(status) {
                ChildState::Exited(wexitstatus(status))
            } else if wifsignaled(status) {
                ChildState::Signaled(wtermsig(status))
            } else {
                ChildState::Running
            };
            assert_eq!(decode_status(status), expected, "status {status:#x}");
        }
        assert_eq!(decode_status(0), ChildState::Exited(0));
        assert_eq!(decode_status(0x0300), ChildState::Exited(3));
        // Signal 9 with the core-dump bit (0x80) still decodes as signal 9.
        assert_eq!(decode_status(0x89), ChildState::Signaled(9));
        // Stopped and continued are not exits.
        assert_eq!(decode_status(0x137f), ChildState::Running);
        assert_eq!(decode_status(0xffff), ChildState::Running);
    }

    /// No argument vector at all is refused before anything is created.
    #[test]
    fn an_empty_argument_vector_is_refused() {
        assert_eq!(spawn(SH, &[], &[], size(24, 80)), Err(EINVAL));
    }

    /// One argument too many, in either vector, is refused -- and exactly at
    /// the limit is not (it reaches the arm below the checks instead).
    // `MAX_ENV + 1` references is 16 KiB, just over the lint's line, on a
    // test thread's stack of megabytes; and this crate has no allocator to put
    // them on the heap with.
    #[allow(clippy::large_stack_arrays)]
    #[test]
    fn oversized_vectors_are_refused_at_exactly_the_limit() {
        let many = [c"x"; MAX_ARGS + 1];
        assert_eq!(spawn(SH, &many, &[], size(24, 80)), Err(E2BIG));
        let env = [c"A=1"; MAX_ENV + 1];
        assert_eq!(spawn(SH, &[SH], &env, size(24, 80)), Err(E2BIG));

        // At the limit the checks pass. On a host that is the ENOSYS arm; on
        // a unix host the program is one that does not exist, so the answer
        // is the child's report rather than the guard's.
        let at_limit = [c"x"; MAX_ARGS];
        let missing = c"/nonexistent/slateos-libcall-test";
        let rc = spawn(missing, &at_limit, &[], size(24, 80));
        assert_ne!(rc, Err(E2BIG), "the limit itself was refused");
        assert!(rc.is_err());
    }

    /// A broadcast pid never reaches `waitpid`.
    ///
    /// On both builds, because the guard sits above the split -- which is the
    /// only reason this test can run on the host at all.
    #[test]
    fn try_wait_refuses_anything_but_one_child() {
        assert_eq!(try_wait(0), Err(EINVAL));
        assert_eq!(try_wait(-1), Err(EINVAL));
        assert_eq!(try_wait(-42), Err(EINVAL));
    }

    /// A negative descriptor is refused as a bad descriptor, not passed on.
    #[test]
    fn set_window_size_refuses_a_negative_descriptor() {
        assert_eq!(set_window_size(-1, size(24, 80)), Err(EBADF));
    }

    /// On a host every call declines once the checks have passed, rather than
    /// pretending a terminal exists.
    #[cfg(not(unix))]
    #[test]
    fn the_host_arms_decline() {
        assert_eq!(spawn(SH, &[SH], &[], size(24, 80)), Err(ENOSYS));
        assert_eq!(set_window_size(3, size(24, 80)), Err(ENOSYS));
        assert_eq!(try_wait(1), Err(ENOSYS));
    }

    /// Everything below starts real programs on real pseudo-terminals, so it
    /// runs only where there are some: a Linux host, against glibc's
    /// `forkpty`. It is the test of *this* module's half -- the vectors, the
    /// exec report, the signal resets, close-on-exec, the size -- not of
    /// `posix`'s, which the ring-3 fixture `services/ctest-python-repl` covers
    /// on SlateOS itself.
    #[cfg(unix)]
    mod on_a_real_terminal {
        extern crate std;

        use super::super::*;
        use super::{SH, size};
        use std::string::String;
        use std::time::{Duration, Instant};
        use std::vec::Vec;

        /// Everything the child writes until its terminal closes, or until
        /// `limit` passes.
        ///
        /// Non-blocking so that a child that never finishes fails the test
        /// instead of hanging it. `EIO` is how a master reports that the last
        /// slave descriptor has closed -- the ordinary end of a terminal
        /// session, not an error.
        fn read_until_closed(master: i32, limit: Duration) -> String {
            const F_GETFL: i32 = 3;
            const F_SETFL: i32 = 4;
            const O_NONBLOCK: i64 = 0o4000;
            const EAGAIN: i32 = 11;
            // SAFETY: flag-word `fcntl`s on a descriptor the test owns.
            unsafe {
                let flags = sys::fcntl(master, F_GETFL);
                assert!(flags >= 0);
                assert_eq!(
                    sys::fcntl(master, F_SETFL, i64::from(flags) | O_NONBLOCK),
                    0
                );
            }
            let deadline = Instant::now() + limit;
            let mut out = Vec::new();
            let mut buf = [0u8; 512];
            loop {
                // SAFETY: `buf` is writable for its whole length.
                let n = unsafe { sys::read(master, buf.as_mut_ptr(), buf.len()) };
                if n > 0 {
                    out.extend_from_slice(&buf[..usize::try_from(n).unwrap()]);
                    continue;
                }
                if n == 0 {
                    break;
                }
                let e = crate::last_errno();
                if e == EIO {
                    break;
                }
                assert!(e == EAGAIN || e == EINTR, "read failed: errno {e}");
                assert!(
                    Instant::now() < deadline,
                    "the child's terminal never closed; got {:?}",
                    String::from_utf8_lossy(&out)
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            // Bytes from a test child, shown to a human if an assertion fails.
            // Lossy is acceptable here and nowhere else: this is a diagnostic,
            // not data being carried anywhere.
            String::from_utf8_lossy(&out).into_owned()
        }

        /// Reap `pid`, waiting up to two seconds for it to finish.
        fn wait_for(pid: i32) -> ChildState {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match try_wait(pid).expect("try_wait") {
                    ChildState::Running => {
                        assert!(Instant::now() < deadline, "the child never exited");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    done => return done,
                }
            }
        }

        fn close(fd: i32) {
            // SAFETY: a descriptor the test owns, closed once.
            unsafe { sys::close(fd) };
        }

        /// The point of the whole module: the program is on a terminal.
        ///
        /// `test -t` is `isatty`, which is exactly the check a shell makes to
        /// decide whether to be interactive -- the check a shell on pipes
        /// fails, and why one on pipes prints no prompt.
        #[test]
        fn the_child_runs_on_a_terminal() {
            let script = c"test -t 0 && test -t 1 && test -t 2 && echo on-a-tty";
            let s = spawn(SH, &[c"sh", c"-c", script], &[], size(24, 80)).expect("spawn");
            let out = read_until_closed(s.master, Duration::from_secs(5));
            close(s.master);
            assert!(out.contains("on-a-tty"), "not on a terminal: {out:?}");
            assert_eq!(wait_for(s.pid), ChildState::Exited(0));
        }

        /// The size given to `spawn` is the size the child sees.
        #[test]
        fn the_child_sees_the_size_it_was_given() {
            let s = spawn(SH, &[c"sh", c"-c", c"stty size"], &[], size(33, 101)).expect("spawn");
            let out = read_until_closed(s.master, Duration::from_secs(5));
            close(s.master);
            assert!(out.contains("33 101"), "stty size said {out:?}");
            wait_for(s.pid);
        }

        /// A resize after the start reaches the child too -- the `TIOCSWINSZ`
        /// half, which a terminal calls every time its window changes.
        #[test]
        fn a_resize_reaches_the_running_child() {
            // `read` holds the child until the resize has happened.
            let s =
                spawn(SH, &[c"sh", c"-c", c"read x; stty size"], &[], size(24, 80)).expect("spawn");
            set_window_size(s.master, size(40, 132)).expect("resize");
            // SAFETY: a one-byte write from a live buffer to our own master.
            let wrote = unsafe { sys::write(s.master, b"\n".as_ptr(), 1) };
            assert_eq!(wrote, 1);
            let out = read_until_closed(s.master, Duration::from_secs(5));
            close(s.master);
            assert!(out.contains("40 132"), "after the resize stty said {out:?}");
            wait_for(s.pid);
        }

        /// The environment is exactly the one passed, not the parent's.
        #[test]
        fn the_environment_is_the_one_given() {
            let s = spawn(
                c"/usr/bin/env",
                &[c"env"],
                &[c"SLATE_A=one", c"SLATE_B=two"],
                size(24, 80),
            )
            .expect("spawn");
            let out = read_until_closed(s.master, Duration::from_secs(5));
            close(s.master);
            assert!(out.contains("SLATE_A=one"), "{out:?}");
            assert!(out.contains("SLATE_B=two"), "{out:?}");
            assert!(
                !out.contains("PATH="),
                "the parent's environment leaked: {out:?}"
            );
            wait_for(s.pid);
        }

        /// A program that does not exist is `ENOENT` from `spawn`, not a child
        /// that exits 127.
        #[test]
        fn a_missing_program_is_an_error_and_leaves_no_child() {
            let missing = c"/nonexistent/slateos-libcall-test";
            assert_eq!(
                spawn(missing, &[c"x"], &[], size(24, 80)),
                Err(posix_enoent())
            );
        }

        fn posix_enoent() -> i32 {
            posix::errno::ENOENT
        }

        /// The exit status comes back, and a signal death is told apart from
        /// an exit.
        #[test]
        fn exit_statuses_and_signal_deaths_are_reported() {
            let s = spawn(SH, &[c"sh", c"-c", c"exit 7"], &[], size(24, 80)).expect("spawn");
            read_until_closed(s.master, Duration::from_secs(5));
            close(s.master);
            assert_eq!(wait_for(s.pid), ChildState::Exited(7));

            let s = spawn(SH, &[c"sh", c"-c", c"kill -9 $$"], &[], size(24, 80)).expect("spawn");
            read_until_closed(s.master, Duration::from_secs(5));
            close(s.master);
            assert_eq!(wait_for(s.pid), ChildState::Signaled(9));
        }

        /// A finished child is reaped once, and asking again is an error.
        #[test]
        fn a_reaped_child_is_not_reported_twice() {
            let s = spawn(SH, &[c"sh", c"-c", c"true"], &[], size(24, 80)).expect("spawn");
            read_until_closed(s.master, Duration::from_secs(5));
            close(s.master);
            assert_eq!(wait_for(s.pid), ChildState::Exited(0));
            assert!(try_wait(s.pid).is_err(), "a second answer for one exit");
        }

        /// The child does not inherit an ignored `SIGPIPE`.
        ///
        /// Rust ignores `SIGPIPE` in every program it starts, this test binary
        /// included, and the disposition survives `execve` unless it is reset.
        /// Bit 12 of `SigIgn` is signal 13.
        #[test]
        fn sigpipe_is_restored_for_the_child() {
            let s = spawn(
                SH,
                &[c"sh", c"-c", c"grep SigIgn /proc/self/status"],
                &[],
                size(24, 80),
            )
            .expect("spawn");
            let out = read_until_closed(s.master, Duration::from_secs(5));
            close(s.master);
            wait_for(s.pid);
            let hex = out
                .lines()
                .find_map(|l| l.trim().strip_prefix("SigIgn:"))
                .expect("no SigIgn line")
                .trim();
            let mask = u64::from_str_radix(hex, 16).expect("hex mask");
            assert_eq!(mask & (1 << 12), 0, "SIGPIPE is still ignored: {hex}");
        }

        /// The master will not leak into the next program this process
        /// starts.
        #[test]
        fn the_master_is_close_on_exec() {
            const F_GETFD: i32 = 1;
            let s = spawn(SH, &[c"sh", c"-c", c"true"], &[], size(24, 80)).expect("spawn");
            // SAFETY: a flag query on a descriptor the test owns.
            let flags = unsafe { sys::fcntl(s.master, F_GETFD) };
            read_until_closed(s.master, Duration::from_secs(5));
            close(s.master);
            wait_for(s.pid);
            assert!(flags >= 0);
            assert_ne!(
                i64::from(flags) & sys::FD_CLOEXEC,
                0,
                "the master is inheritable"
            );
        }

        /// A signal is delivered to the child through the terminal's line
        /// discipline, not by us: `^C` typed at the master interrupts the
        /// foreground job. This is what makes it a terminal and not a pipe.
        #[test]
        fn ctrl_c_at_the_master_interrupts_the_child() {
            let s = spawn(
                SH,
                &[c"sh", c"-c", c"echo ready; sleep 30"],
                &[],
                size(24, 80),
            )
            .expect("spawn");
            // Wait for the child to be running `sleep` before interrupting it.
            std::thread::sleep(Duration::from_millis(300));
            // SAFETY: a one-byte write from a live buffer to our own master.
            let wrote = unsafe { sys::write(s.master, [0x03u8].as_ptr(), 1) };
            assert_eq!(wrote, 1);
            read_until_closed(s.master, Duration::from_secs(5));
            close(s.master);
            match wait_for(s.pid) {
                ChildState::Signaled(2) => {}
                // Some shells exit with 128+SIGINT rather than re-raising.
                ChildState::Exited(130) => {}
                other => panic!("^C did not interrupt the child: {other:?}"),
            }
        }
    }
}

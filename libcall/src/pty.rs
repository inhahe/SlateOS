//! Programs on pseudo-terminals: start one, then talk to it.
//!
//! Requested by lane C for `apps/terminal`
//! (`requests/c-b-a-terminal-needs-a-shell-on-the-other-end-of-its-pty.md`): the
//! emulator had two thousand lines of terminal and no process on the far end of
//! it. [`spawn`] is the one call it asked for; [`PtyChild`] is what an emulator
//! needs afterwards -- the program's output, a way to type into it, a resize,
//! and the news that it exited -- so that no application reaches the C library
//! itself for any of it, which is the reason this crate exists.
//!
//! # Why `forkpty` + `execve`, and not `std::process::Command`
//!
//! `userspace/sshd` starts its login shell with `Command` and a `pre_exec`
//! closure calling `login_tty`, which is the idiomatic Rust shape, and on Linux
//! it is the better one. It is not used here because **on SlateOS today it
//! cannot return while the program runs**. A `pre_exec` closure takes std off
//! `posix_spawn` and onto `fork` + `exec`, and std learns that the `exec`
//! succeeded by reading end-of-file on a close-on-exec pipe. A native SlateOS
//! `exec` leaves a close-on-exec descriptor out of the new image's table but
//! does not release its kernel handle, so the pipe's write end lives on in the
//! child and `spawn()` blocks until the program *exits*. For a terminal that
//! is a window frozen for the life of its shell. The platform half is filed as
//! `requests/b-ad-close-on-exec-does-not-close-on-a-native-exec.md`.
//!
//! This path depends on nothing about close-on-exec, and would still be right
//! once that is fixed:
//!
//! 1. `forkpty` (the C library's) opens the pair and forks; its child becomes a
//!    session leader, takes the slave as its controlling terminal and as fds 0,
//!    1 and 2, and closes the master -- explicitly, not by close-on-exec. A
//!    failure there comes back to the parent through `forkpty`'s own pipe,
//!    which its child closes explicitly too.
//! 2. This module's child then changes directory if asked, puts `SIGPIPE`
//!    back to its default and clears the signal mask, closes every other
//!    descriptor it inherited (`closefrom(3)`), and `execve`s.
//!
//! # What the child may do between the fork and the exec
//!
//! Only what is safe in a forked copy of a multi-threaded process: every
//! other thread vanished mid-flight, possibly holding the allocator's lock.
//! So everything the child touches -- the argument and environment arrays, the
//! failure message -- is built **before** the fork, and the child makes bare
//! calls into the C library and nothing else. It never returns: it becomes the
//! program or it calls `_exit`.
//!
//! The signal reset is not optional. Every Rust program ignores `SIGPIPE` at
//! startup, an ignored disposition survives `exec`, and a shell that inherits
//! it hands it to every pipeline it runs -- `yes | head` then spins forever
//! instead of ending when `head` does. std's own spawn makes the same reset.
//!
//! # A failure to start is reported on the terminal
//!
//! If `execve` (or the `chdir` before it) fails, the child writes one line
//! naming the program and the error number to its own standard error -- which
//! is the terminal -- and exits with 127, the status a shell gives a command it
//! could not run. That is where the emulator's user is already looking, and it
//! is what every terminal emulator does. [`spawn`] cannot return it as an
//! `Err`: learning that an `exec` succeeded needs exactly the close-on-exec
//! pipe this module exists to avoid. It does return every failure before the
//! fork, and `forkpty`'s own.
//!
//! # Linux, the host, and the tests
//!
//! Compiled for `unix`, like [`crate::kill`]: glibc 2.34 and later have
//! `forkpty`, `login_tty` and `closefrom` in `libc.so.6` itself, so the same
//! code runs on a Linux host, and the end-to-end tests below run there (in WSL
//! on this machine) against real terminals. Everywhere else every call answers
//! [`ENOSYS`](crate::ENOSYS), as the rest of this crate does.

use core::ffi::{CStr, c_char};
use std::vec::Vec;

use crate::{EAGAIN, EINTR, EINVAL, EIO, ESRCH};

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------
//
// Private, and asserted against `posix` in the tests below for the same reason
// the crate's public values are: restating a number is how one truth becomes
// two sources. Linux uses the same values, which is what lets the unix arm run
// against glibc unchanged.

/// `ioctl` request: set the terminal's window size.
const TIOCSWINSZ: u64 = 0x5414;
/// `fcntl` command: read the file status flags.
const F_GETFL: i32 = 3;
/// `fcntl` command: write the file status flags.
const F_SETFL: i32 = 4;
/// File status flag: reads and writes return instead of waiting.
const O_NONBLOCK: i64 = 0o4000;
/// `poll`: data may be read without blocking.
const POLLIN: i16 = 0x0001;
/// `poll`: an error condition.
const POLLERR: i16 = 0x0008;
/// `poll`: the other end has gone.
const POLLHUP: i16 = 0x0010;
/// `waitpid`: report nothing rather than wait.
const WNOHANG: i32 = 1;
/// Broken pipe.
const SIGPIPE: i32 = 13;
/// The default disposition, as a handler value.
const SIG_DFL: usize = 0;
/// `sigprocmask`: replace the mask outright.
const SIG_SETMASK: i32 = 2;
/// The status a shell gives a command it could not run.
const CANNOT_RUN: i32 = 127;

/// `struct winsize`, as `openpty`, `forkpty` and `TIOCSWINSZ` read it.
///
/// Rows first. [`WindowSize`] names the two fields rather than ordering them,
/// so the crossing happens once, here, where the C layout is.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

/// `struct pollfd`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Pollfd {
    fd: i32,
    events: i16,
    revents: i16,
}

// ---------------------------------------------------------------------------
// The C library
// ---------------------------------------------------------------------------

/// The linked C library's calls, declared once.
///
/// `ioctl` and `fcntl` are declared variadic because glibc's are; SlateOS's are
/// not, and on x86-64 the two agree, since a variadic call passes its fixed and
/// variable integer arguments in the same registers. The variable argument
/// `fcntl` is given is an `i64`, never an `i32`: a 32-bit value passed to a
/// callee that reads 64 bits leaves the upper half of the register undefined.
#[cfg(unix)]
mod c {
    use super::{Pollfd, Winsize};
    use core::ffi::{c_char, c_void};

    unsafe extern "C" {
        pub fn forkpty(
            amaster: *mut i32,
            name: *mut c_char,
            termp: *const c_void,
            winp: *const Winsize,
        ) -> i32;
        pub fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
        pub fn write(fd: i32, buf: *const u8, count: usize) -> isize;
        pub fn close(fd: i32) -> i32;
        pub fn poll(fds: *mut Pollfd, nfds: u64, timeout: i32) -> i32;
        pub fn ioctl(fd: i32, request: u64, ...) -> i32;
        pub fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        pub fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        pub fn chdir(path: *const c_char) -> i32;
        pub fn execve(
            path: *const c_char,
            argv: *const *const c_char,
            envp: *const *const c_char,
        ) -> i32;
        pub fn signal(signum: i32, handler: usize) -> usize;
        pub fn sigprocmask(how: i32, set: *const u64, oldset: *mut u64) -> i32;
        pub fn closefrom(lowfd: i32);
        pub fn _exit(status: i32) -> !;
    }
}

/// Everywhere that is not `unix`: every call fails, and `errno` says
/// [`ENOSYS`](crate::ENOSYS). Nothing is opened, so `forkpty` failing first
/// means nothing after it ever runs -- but it all has to compile, which is
/// what keeps the logic above the split one copy rather than two.
#[cfg(not(unix))]
#[allow(clippy::missing_safety_doc, clippy::unnecessary_wraps)]
mod c {
    use super::{Pollfd, Winsize};
    use core::ffi::{c_char, c_void};

    pub unsafe fn forkpty(
        _amaster: *mut i32,
        _name: *mut c_char,
        _termp: *const c_void,
        _winp: *const Winsize,
    ) -> i32 {
        -1
    }
    pub unsafe fn read(_fd: i32, _buf: *mut u8, _count: usize) -> isize {
        -1
    }
    pub unsafe fn write(_fd: i32, _buf: *const u8, _count: usize) -> isize {
        -1
    }
    pub unsafe fn close(_fd: i32) -> i32 {
        -1
    }
    pub unsafe fn poll(_fds: *mut Pollfd, _nfds: u64, _timeout: i32) -> i32 {
        -1
    }
    pub unsafe fn ioctl(_fd: i32, _request: u64, _arg: *mut Winsize) -> i32 {
        -1
    }
    pub unsafe fn fcntl(_fd: i32, _cmd: i32, _arg: i64) -> i32 {
        -1
    }
    pub unsafe fn waitpid(_pid: i32, _status: *mut i32, _options: i32) -> i32 {
        -1
    }
    pub unsafe fn chdir(_path: *const c_char) -> i32 {
        -1
    }
    pub unsafe fn execve(
        _path: *const c_char,
        _argv: *const *const c_char,
        _envp: *const *const c_char,
    ) -> i32 {
        -1
    }
    pub unsafe fn signal(_signum: i32, _handler: usize) -> usize {
        0
    }
    pub unsafe fn sigprocmask(_how: i32, _set: *const u64, _oldset: *mut u64) -> i32 {
        -1
    }
    pub unsafe fn closefrom(_lowfd: i32) {}
    pub unsafe fn _exit(_status: i32) -> ! {
        std::process::abort()
    }
}

/// This thread's `errno`, from the library that just failed.
#[cfg(unix)]
fn errno() -> i32 {
    crate::last_errno()
}

/// No C library answered, because none was asked.
#[cfg(not(unix))]
fn errno() -> i32 {
    crate::ENOSYS
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A terminal's size in character cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowSize {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
}

impl From<WindowSize> for Winsize {
    fn from(size: WindowSize) -> Self {
        Self {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }
    }
}

/// How a program on a terminal ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// It exited with this status. 127 is also what [`spawn`]'s child exits
    /// with when the program could not be started, and the terminal then
    /// carries a line saying why.
    Code(i32),
    /// It was ended by this signal.
    Signal(i32),
}

impl Exit {
    /// Decode a `waitpid` status word: the signal in the low seven bits, else
    /// the exit status in the next eight. (Stops are not reported, because
    /// nothing here asks `waitpid` for them.)
    fn from_wait_status(status: i32) -> Self {
        let sig = status & 0x7f;
        if sig == 0 {
            Self::Code((status >> 8) & 0xff)
        } else {
            Self::Signal(sig)
        }
    }
}

/// What a read from the terminal found.
///
/// Three answers rather than a byte count, because the end of a terminal is not
/// a zero-length read. When the last holder of the slave closes it, a read on
/// the master fails with `EIO`; and on SlateOS a zero-length read means
/// "nothing right now" (`design-decisions.md` §259), so a caller that took zero
/// for the end would declare a live session over, and one that took `EIO` for
/// an error would report every normal exit as a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// This many bytes of the program's output are at the front of the buffer.
    Data(usize),
    /// Nothing to read right now; the terminal is still open.
    Nothing,
    /// Everything holding the terminal's far end has closed it. Nothing more
    /// will arrive; what the program's exit status was is [`PtyChild::wait`]'s
    /// to say.
    Closed,
}

/// A program running on a pseudo-terminal, and the terminal's master end.
///
/// Dropping it closes the master, which hangs the terminal up: the kernel sends
/// `SIGHUP` to the terminal's foreground process group, which ends a shell. It
/// does not wait for the program, exactly as dropping a `std::process::Child`
/// does not; call [`wait`](Self::wait) or [`try_wait`](Self::try_wait) to
/// collect its status and free its process-table entry.
#[derive(Debug)]
pub struct PtyChild {
    pid: i32,
    master: i32,
    /// Set once the child has been reaped. After that its pid may belong to
    /// some other process at any moment, so nothing here may name it again.
    exit: Option<Exit>,
}

// ---------------------------------------------------------------------------
// Starting a program
// ---------------------------------------------------------------------------

/// Start `program` on a new pseudo-terminal of `size`.
///
/// The program runs as the leader of a new session with the terminal as its
/// controlling terminal and as its standard input, output and error -- what a
/// login shell expects -- and inherits no other descriptor from the caller.
///
/// * `program` is a **path**: this is `execve`, which does not search `PATH`.
///   A terminal knows its shell's path (from `$SHELL`, or the account).
/// * `argv` is the program's whole argument vector, `argv[0]` included -- a
///   login shell is told it is one by a `-` in front of its own name there.
/// * `envp` is the program's whole environment, as `NAME=value` strings. An
///   emulator sets `TERM` in it; nothing is inherited implicitly.
/// * `cwd`, if given, is where the program starts.
///
/// # Errors
///
/// * [`EINVAL`] -- `program` is empty, or `argv` is. A program with no
///   `argv[0]` breaks every convention that names it, so none is started.
/// * Whatever `forkpty` sets -- no terminal could be opened (`EMFILE` when the
///   table is full), or the process could not be forked.
/// * [`ENOSYS`](crate::ENOSYS) where there is no terminal to open.
///
/// A program that cannot be *run* -- missing, not executable, or `cwd` not
/// enterable -- is not an error here; see the module documentation for why. It
/// exits with 127, and the terminal carries a line saying what failed.
pub fn spawn(
    program: &CStr,
    argv: &[&CStr],
    envp: &[&CStr],
    cwd: Option<&CStr>,
    size: WindowSize,
) -> Result<PtyChild, i32> {
    if program.is_empty() || argv.is_empty() {
        return Err(EINVAL);
    }

    // Everything the child will touch, built now: after the fork it may not
    // allocate. `ptr::null()` ends each array, as `execve` requires.
    let argv_ptrs: Vec<*const c_char> = argv
        .iter()
        .map(|a| a.as_ptr())
        .chain(core::iter::once(core::ptr::null()))
        .collect();
    let envp_ptrs: Vec<*const c_char> = envp
        .iter()
        .map(|e| e.as_ptr())
        .chain(core::iter::once(core::ptr::null()))
        .collect();
    let winsize = Winsize::from(size);

    let mut master: i32 = -1;
    // SAFETY: `master` is a live local the call writes the master fd into on
    // success. `name` is NULL, which `forkpty` documents as "do not report the
    // slave's name"; `termp` is NULL, "leave the slave at the default modes"
    // (cooked and echoing, what a shell expects); `winp` points at a live
    // `Winsize` that outlives the call.
    let pid = unsafe {
        c::forkpty(
            &raw mut master,
            core::ptr::null_mut(),
            core::ptr::null(),
            &raw const winsize,
        )
    };
    if pid < 0 {
        return Err(errno());
    }
    if pid == 0 {
        // SAFETY: this is the forked child, the only place `become_program`
        // may run. Every pointer it is given addresses memory prepared before
        // the fork, which the child's copy of the address space still holds.
        unsafe { become_program(program, &argv_ptrs, &envp_ptrs, cwd) }
    }
    Ok(PtyChild {
        pid,
        master,
        exit: None,
    })
}

/// The forked child's whole life: become `program`, or say why not and exit.
///
/// # Safety
///
/// Call only in the child `forkpty` just created. It is async-signal-safe by
/// construction -- bare C library calls on memory that existed before the
/// fork, no allocation, no lock, no unwinding -- and it never returns.
unsafe fn become_program(
    program: &CStr,
    argv: &[*const c_char],
    envp: &[*const c_char],
    cwd: Option<&CStr>,
) -> ! {
    if let Some(dir) = cwd {
        // SAFETY: `dir` is NUL-terminated by `CStr`'s invariant and lives in
        // memory the fork copied.
        if unsafe { c::chdir(dir.as_ptr()) } != 0 {
            // SAFETY: still the child; see this function's contract.
            unsafe { fail(program, b"cannot enter its start directory", errno()) }
        }
    }

    // SAFETY: plain scalar arguments -- a signal number and a disposition, then
    // a pointer to a zeroed set that outlives the call. Resetting what the
    // parent's runtime changed is what std's own spawn does here too; see the
    // module documentation for what an inherited ignored `SIGPIPE` breaks.
    // Both results are deliberately unread: neither can fail for these
    // arguments, and a child that stopped here would run nothing at all.
    unsafe {
        c::signal(SIGPIPE, SIG_DFL);
        let empty = [0u64; 16];
        c::sigprocmask(SIG_SETMASK, empty.as_ptr(), core::ptr::null_mut());
    }

    // Every descriptor but the terminal. Closed, not merely marked
    // close-on-exec: see the module documentation for why a mark is not
    // enough on SlateOS today.
    //
    // SAFETY: no arguments that address memory; closing descriptors this
    // child inherited and will never use.
    unsafe { c::closefrom(3) };

    // SAFETY: `program` is NUL-terminated by `CStr`'s invariant, and both
    // arrays end in NULL (built that way in `spawn`) with every other element
    // a `CStr` pointer from the caller's borrow, all in memory the fork copied.
    unsafe { c::execve(program.as_ptr(), argv.as_ptr(), envp.as_ptr()) };
    // `execve` returns only on failure.
    // SAFETY: still the child; see this function's contract.
    unsafe { fail(program, b"cannot run", errno()) }
}

/// Write `<program>: <what> (errno <n>)` to the terminal and exit with 127.
///
/// # Safety
///
/// Only in the child, where fd 2 is the terminal. Allocation-free: the number
/// is formatted into a stack buffer.
unsafe fn fail(program: &CStr, what: &[u8], err: i32) -> ! {
    let mut digits = [0u8; 12];
    let n = errno_digits(err, &mut digits);
    for part in [
        program.to_bytes(),
        b": ",
        what,
        b" (errno ",
        digits.get(n..).unwrap_or_default(),
        b")\n",
    ] {
        // SAFETY: `part` is a live slice for the duration of the call. The
        // result is deliberately unread: a child that cannot report is still
        // a child that must exit, and the exit status carries the verdict.
        unsafe { c::write(2, part.as_ptr(), part.len()) };
    }
    // SAFETY: `_exit` takes a scalar and does not return.
    unsafe { c::_exit(CANNOT_RUN) }
}

/// Format `err` in decimal at the END of `buf`, returning where it starts.
///
/// Right-aligned so no reversal is needed. Twelve bytes hold `i32::MIN` with
/// its sign; `errno` is never negative, but a formatter that could overrun on
/// one is a formatter with a bug, and this one runs where nothing can catch it.
fn errno_digits(err: i32, buf: &mut [u8; 12]) -> usize {
    let negative = err < 0;
    let mut value = err.unsigned_abs();
    let mut at = buf.len();
    loop {
        at = at.saturating_sub(1);
        let digit = value.wrapping_rem(10) as u8;
        if let Some(slot) = buf.get_mut(at) {
            *slot = b'0'.wrapping_add(digit);
        }
        value = value.wrapping_div(10);
        if value == 0 || at == 0 {
            break;
        }
    }
    if negative && at > 0 {
        at = at.saturating_sub(1);
        if let Some(slot) = buf.get_mut(at) {
            *slot = b'-';
        }
    }
    at
}

// ---------------------------------------------------------------------------
// Talking to it
// ---------------------------------------------------------------------------

impl PtyChild {
    /// The program's process id.
    #[must_use]
    pub fn pid(&self) -> i32 {
        self.pid
    }

    /// The master descriptor, for an event loop that polls several things at
    /// once. Still owned here: closing it is [`Drop`]'s job, and a caller that
    /// closed it would leave this value holding a number the next `open` may
    /// reuse.
    #[must_use]
    pub fn master_fd(&self) -> i32 {
        self.master
    }

    /// Read the program's output into `buf`.
    ///
    /// # Errors
    ///
    /// Whatever `read` sets other than the three it translates: `EIO` is
    /// [`Output::Closed`], and `EAGAIN` and `EINTR` are [`Output::Nothing`].
    pub fn read(&self, buf: &mut [u8]) -> Result<Output, i32> {
        if buf.is_empty() {
            return Ok(Output::Nothing);
        }
        // SAFETY: `buf` is a live, writable slice of exactly `buf.len()`
        // bytes for the duration of the call.
        let n = unsafe { c::read(self.master, buf.as_mut_ptr(), buf.len()) };
        if n > 0 {
            return Ok(Output::Data(n.unsigned_abs()));
        }
        if n == 0 {
            // §259: on a SlateOS master, zero is "nothing now", never the end.
            return Ok(Output::Nothing);
        }
        match errno() {
            EIO => Ok(Output::Closed),
            EAGAIN | EINTR => Ok(Output::Nothing),
            e => Err(e),
        }
    }

    /// Write `bytes` -- keystrokes, a paste -- to the program. Returns how many
    /// were taken, which may be fewer than offered.
    ///
    /// # Errors
    ///
    /// Whatever `write` sets: `EAGAIN` when the master is non-blocking and the
    /// terminal's input queue is full, `EIO` once the program has gone.
    pub fn write(&self, bytes: &[u8]) -> Result<usize, i32> {
        // SAFETY: `bytes` is a live slice of exactly `bytes.len()` bytes for
        // the duration of the call.
        let n = unsafe { c::write(self.master, bytes.as_ptr(), bytes.len()) };
        if n < 0 {
            Err(errno())
        } else {
            Ok(n.unsigned_abs())
        }
    }

    /// Wait up to `timeout_ms` milliseconds (negative: indefinitely) for there
    /// to be something to [`read`](Self::read). `true` also when the terminal
    /// has closed, because a read is then what says so.
    ///
    /// # Errors
    ///
    /// Whatever `poll` sets.
    pub fn wait_readable(&self, timeout_ms: i32) -> Result<bool, i32> {
        let mut fds = [Pollfd {
            fd: self.master,
            events: POLLIN,
            revents: 0,
        }];
        // SAFETY: `fds` is a live one-element array for the duration of the
        // call, and the count passed is its length.
        let rc = unsafe { c::poll(fds.as_mut_ptr(), 1, timeout_ms) };
        if rc < 0 {
            return Err(errno());
        }
        Ok(fds[0].revents & (POLLIN | POLLHUP | POLLERR) != 0)
    }

    /// Tell the terminal -- and so the program, which receives `SIGWINCH` --
    /// its new size.
    ///
    /// # Errors
    ///
    /// Whatever `ioctl(TIOCSWINSZ)` sets.
    pub fn resize(&self, size: WindowSize) -> Result<(), i32> {
        let mut winsize = Winsize::from(size);
        // SAFETY: `winsize` is a live `struct winsize` for the duration of
        // the call, which is all `TIOCSWINSZ` reads.
        let rc = unsafe { c::ioctl(self.master, TIOCSWINSZ, &raw mut winsize) };
        if rc < 0 { Err(errno()) } else { Ok(()) }
    }

    /// Make reads and writes on the master return at once rather than wait --
    /// what an emulator's single-threaded event loop wants.
    ///
    /// # Errors
    ///
    /// Whatever `fcntl` sets.
    pub fn set_nonblocking(&self, on: bool) -> Result<(), i32> {
        // SAFETY: scalar arguments only; the variable one is an `i64` for the
        // reason `mod c` gives.
        let flags = unsafe { c::fcntl(self.master, F_GETFL, 0i64) };
        if flags < 0 {
            return Err(errno());
        }
        let flags = i64::from(flags);
        let wanted = if on {
            flags | O_NONBLOCK
        } else {
            flags & !O_NONBLOCK
        };
        if wanted == flags {
            return Ok(());
        }
        // SAFETY: as above.
        let rc = unsafe { c::fcntl(self.master, F_SETFL, wanted) };
        if rc < 0 { Err(errno()) } else { Ok(()) }
    }

    /// How the program ended, if it has; `None` if it is still running.
    ///
    /// # Errors
    ///
    /// Whatever `waitpid` sets.
    pub fn try_wait(&mut self) -> Result<Option<Exit>, i32> {
        if let Some(exit) = self.exit {
            return Ok(Some(exit));
        }
        let mut status: i32 = 0;
        // SAFETY: `status` is a live local the call writes into.
        let rc = unsafe { c::waitpid(self.pid, &raw mut status, WNOHANG) };
        if rc < 0 {
            return Err(errno());
        }
        if rc == 0 {
            return Ok(None);
        }
        let exit = Exit::from_wait_status(status);
        self.exit = Some(exit);
        Ok(Some(exit))
    }

    /// Wait for the program to end, and say how it did.
    ///
    /// # Errors
    ///
    /// Whatever `waitpid` sets, other than `EINTR`, which is retried.
    pub fn wait(&mut self) -> Result<Exit, i32> {
        if let Some(exit) = self.exit {
            return Ok(exit);
        }
        let mut status: i32 = 0;
        loop {
            // SAFETY: `status` is a live local the call writes into.
            let rc = unsafe { c::waitpid(self.pid, &raw mut status, 0) };
            if rc >= 0 {
                break;
            }
            let e = errno();
            if e != EINTR {
                return Err(e);
            }
        }
        let exit = Exit::from_wait_status(status);
        self.exit = Some(exit);
        Ok(exit)
    }

    /// Send `sig` to the program.
    ///
    /// # Errors
    ///
    /// [`ESRCH`] once the program has been reaped: from that moment its pid
    /// may name another process, so this refuses rather than signal whoever
    /// has it now. Otherwise whatever [`crate::kill`] returns.
    pub fn signal(&self, sig: i32) -> Result<(), i32> {
        if self.exit.is_some() {
            return Err(ESRCH);
        }
        crate::kill(self.pid, sig)
    }
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        // SAFETY: `self.master` is the descriptor `forkpty` returned, owned by
        // this value alone and closed only here. A failed close is not
        // reportable from `drop` and leaves nothing to retry.
        unsafe { c::close(self.master) };
    }
}

// A failed expectation in a test is a panic by design; the lints that keep
// panics out of production code have nothing to guard here.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use std::string::String;

    /// Every value this module restates equals `posix`'s.
    #[test]
    fn values_agree_with_posix() {
        assert_eq!(TIOCSWINSZ, posix::ioctl::TIOCSWINSZ);
        assert_eq!(F_GETFL, posix::fcntl_ops::F_GETFL);
        assert_eq!(F_SETFL, posix::fcntl_ops::F_SETFL);
        assert_eq!(O_NONBLOCK, i64::from(posix::fcntl::O_NONBLOCK));
        assert_eq!(POLLIN, posix::poll::POLLIN);
        assert_eq!(POLLHUP, posix::poll::POLLHUP);
        assert_eq!(POLLERR, posix::poll::POLLERR);
        assert_eq!(WNOHANG, posix::process::WNOHANG);
        assert_eq!(SIGPIPE, posix::signal::SIGPIPE);
        assert_eq!(SIG_DFL, posix::signal::SIG_DFL);
        assert_eq!(SIG_SETMASK, posix::signal::SIG_SETMASK);
        // The zeroed mask `become_program` passes must be at least as large
        // as the set the library reads, or it reads past the array.
        assert!(
            core::mem::size_of::<[u64; 16]>() >= core::mem::size_of::<posix::signal::SigsetT>()
        );
    }

    /// Rows first in the C layout, whichever order a caller thinks in.
    #[test]
    fn the_window_size_crosses_into_rows_first() {
        let w = Winsize::from(WindowSize {
            cols: 132,
            rows: 43,
        });
        assert_eq!(
            (w.ws_row, w.ws_col, w.ws_xpixel, w.ws_ypixel),
            (43, 132, 0, 0)
        );
    }

    /// The wait-status decoding, against the words Linux and SlateOS produce.
    #[test]
    fn a_wait_status_is_an_exit_code_or_a_signal() {
        assert_eq!(Exit::from_wait_status(0), Exit::Code(0));
        assert_eq!(Exit::from_wait_status(3 << 8), Exit::Code(3));
        assert_eq!(Exit::from_wait_status(127 << 8), Exit::Code(127));
        assert_eq!(Exit::from_wait_status(9), Exit::Signal(9));
        // SIGSEGV with the core-dump bit (0x80): the bit is not the signal.
        assert_eq!(Exit::from_wait_status(0x80 | 11), Exit::Signal(11));
    }

    /// The child's only formatter, which runs where nothing can catch it.
    #[test]
    fn errno_digits_are_right_aligned_and_never_overrun() {
        let show = |e: i32| {
            let mut buf = [0u8; 12];
            let at = errno_digits(e, &mut buf);
            String::from_utf8(buf[at..].to_vec()).unwrap()
        };
        assert_eq!(show(0), "0");
        assert_eq!(show(2), "2");
        assert_eq!(show(38), "38");
        assert_eq!(show(i32::MAX), "2147483647");
        assert_eq!(show(-5), "-5");
        assert_eq!(show(i32::MIN), "-2147483648");
    }

    /// Refused before anything is opened, on every build.
    #[test]
    fn a_program_with_no_name_or_no_argv_is_refused() {
        let size = WindowSize { cols: 80, rows: 24 };
        let sh = c"/bin/sh";
        assert_eq!(
            spawn(c"", &[c"x"], &[], None, size).map(|_| ()),
            Err(EINVAL)
        );
        assert_eq!(spawn(sh, &[], &[], None, size).map(|_| ()), Err(EINVAL));
    }

    /// Where there is no terminal to open, the answer is `ENOSYS`, not a
    /// child that is not there.
    #[cfg(not(unix))]
    #[test]
    fn the_host_arm_declines() {
        let size = WindowSize { cols: 80, rows: 24 };
        assert_eq!(
            spawn(c"/bin/sh", &[c"sh"], &[], None, size).map(|_| ()),
            Err(crate::ENOSYS)
        );
    }

    /// The end-to-end half: a real terminal, a real program. Runs on a `unix`
    /// build -- a Linux host (WSL here), or SlateOS itself.
    #[cfg(unix)]
    mod on_a_real_terminal {
        use super::super::*;
        use std::string::String;
        use std::time::{Duration, Instant};

        const SIZE: WindowSize = WindowSize {
            cols: 100,
            rows: 40,
        };

        /// Everything the program writes, until the terminal closes. Bounded,
        /// so a test that would hang fails instead.
        fn drain(child: &PtyChild) -> String {
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut out = Vec::new();
            let mut buf = [0u8; 4096];
            while Instant::now() < deadline {
                if !child.wait_readable(200).expect("poll") {
                    continue;
                }
                match child.read(&mut buf).expect("read") {
                    Output::Data(n) => out.extend_from_slice(&buf[..n]),
                    Output::Nothing => {}
                    Output::Closed => return String::from_utf8_lossy(&out).into_owned(),
                }
            }
            panic!(
                "the terminal never closed; read so far: {:?}",
                String::from_utf8_lossy(&out)
            );
        }

        fn sh(script: &str) -> PtyChild {
            let script = std::ffi::CString::new(script).unwrap();
            spawn(
                c"/bin/sh",
                &[c"sh", c"-c", script.as_c_str()],
                &[c"PATH=/usr/bin:/bin", c"TERM=xterm"],
                None,
                SIZE,
            )
            .expect("spawn")
        }

        #[test]
        fn the_program_runs_on_a_terminal_of_the_size_asked_for() {
            let mut child = sh("stty size; tty");
            let out = drain(&child);
            assert!(out.contains("40 100"), "{out:?}");
            assert!(!out.contains("not a tty"), "{out:?}");
            assert_eq!(child.wait(), Ok(Exit::Code(0)));
        }

        #[test]
        fn the_program_inherits_no_descriptor_but_the_terminal() {
            // A descriptor the caller left inheritable. Everything std opens is
            // close-on-exec, which Linux's `exec` would close anyway, so a
            // test using only those would pass with no `closefrom` at all --
            // this is the one that proves the step, and the one SlateOS needs,
            // where close-on-exec does not close (see the module docs).
            const F_SETFD: i32 = 2;
            let file = std::fs::File::open("/dev/null").expect("open");
            let fd = std::os::fd::AsRawFd::as_raw_fd(&file);
            // SAFETY: `fd` is open for the life of `file`; clearing its
            // close-on-exec flag touches nothing else.
            assert_eq!(unsafe { c::fcntl(fd, F_SETFD, 0i64) }, 0);

            // `ls` holds one of its own on /proc/self/fd while it reads it,
            // so the honest expectation is 0, 1, 2 and exactly one more.
            let mut child = sh("ls /proc/self/fd");
            let out = drain(&child);
            let fds: Vec<&str> = out.split_whitespace().collect();
            assert_eq!(fds.len(), 4, "{out:?}");
            assert_eq!(&fds[..3], &["0", "1", "2"], "{out:?}");
            assert_eq!(child.wait(), Ok(Exit::Code(0)));
            drop(file);
        }

        #[test]
        fn sigpipe_is_back_at_its_default() {
            // The test harness, like every Rust program, ignores SIGPIPE. The
            // child must not: bit 12 of SigIgn is signal 13.
            let mut child = sh("grep SigIgn /proc/self/status");
            let out = drain(&child);
            let mask = out
                .split_whitespace()
                .nth(1)
                .and_then(|h| u64::from_str_radix(h, 16).ok())
                .expect("a SigIgn mask");
            assert_eq!(mask & (1 << 12), 0, "{out:?}");
            assert_eq!(child.wait(), Ok(Exit::Code(0)));
        }

        #[test]
        fn a_program_that_cannot_run_says_so_on_the_terminal_and_exits_127() {
            let mut child = spawn(c"/nonexistent/program", &[c"program"], &[], None, SIZE)
                .expect("the fork itself succeeds");
            let out = drain(&child);
            assert!(
                out.contains("/nonexistent/program: cannot run (errno 2)"),
                "{out:?}"
            );
            assert_eq!(child.wait(), Ok(Exit::Code(127)));
        }

        #[test]
        fn the_program_starts_where_it_is_asked_to() {
            let script = std::ffi::CString::new("pwd").unwrap();
            let mut child = spawn(
                c"/bin/sh",
                &[c"sh", c"-c", script.as_c_str()],
                &[],
                Some(c"/"),
                SIZE,
            )
            .expect("spawn");
            let out = drain(&child);
            assert_eq!(out.trim(), "/", "{out:?}");
            assert_eq!(child.wait(), Ok(Exit::Code(0)));

            let mut child =
                spawn(c"/bin/sh", &[c"sh"], &[], Some(c"/nonexistent/dir"), SIZE).expect("spawn");
            let out = drain(&child);
            assert!(out.contains("cannot enter its start directory"), "{out:?}");
            assert_eq!(child.wait(), Ok(Exit::Code(127)));
        }

        #[test]
        fn typing_reaches_the_program_and_its_exit_status_comes_back() {
            let mut child = spawn(c"/bin/sh", &[c"sh"], &[c"PS1=$ "], None, SIZE).expect("spawn");
            child.write(b"echo typed-$((6*7))\n").expect("write");
            child.write(b"exit 3\n").expect("write");
            let out = drain(&child);
            assert!(out.contains("typed-42"), "{out:?}");
            assert_eq!(child.wait(), Ok(Exit::Code(3)));
            // Reaped: the pid may be anyone's now, so it is not signalled.
            assert_eq!(child.signal(crate::SIGTERM), Err(ESRCH));
            assert_eq!(child.try_wait(), Ok(Some(Exit::Code(3))));
        }

        #[test]
        fn a_resize_reaches_the_running_program() {
            let mut child = spawn(c"/bin/sh", &[c"sh"], &[c"PS1="], None, SIZE).expect("spawn");
            child
                .resize(WindowSize {
                    cols: 132,
                    rows: 43,
                })
                .expect("resize");
            child.write(b"stty size; exit\n").expect("write");
            let out = drain(&child);
            assert!(out.contains("43 132"), "{out:?}");
            assert_eq!(child.wait(), Ok(Exit::Code(0)));
        }

        #[test]
        fn a_non_blocking_read_of_a_quiet_terminal_is_nothing_not_an_error() {
            let mut child = spawn(c"/bin/sh", &[c"sh"], &[c"PS1="], None, SIZE).expect("spawn");
            child.set_nonblocking(true).expect("set");
            // Idempotent: asking again changes nothing and still succeeds.
            child.set_nonblocking(true).expect("set again");
            let mut buf = [0u8; 64];
            // Whatever the shell has printed so far, a quiet terminal ends in
            // Nothing rather than an error, and never in Closed.
            let mut saw_nothing = false;
            for _ in 0..50 {
                match child.read(&mut buf) {
                    Ok(Output::Nothing) => {
                        saw_nothing = true;
                        break;
                    }
                    Ok(Output::Data(_)) => {}
                    other => panic!("{other:?}"),
                }
            }
            assert!(saw_nothing);
            child.set_nonblocking(false).expect("unset");
            child.write(b"exit 0\n").expect("write");
            drain(&child);
            assert_eq!(child.wait(), Ok(Exit::Code(0)));
        }

        #[test]
        fn dropping_it_hangs_the_terminal_up() {
            // An interactive shell that never exits on its own: only the
            // hangup can end it, and the hangup is the master closing.
            let child = spawn(c"/bin/sh", &[c"sh"], &[], None, SIZE).expect("spawn");
            let pid = child.pid();
            drop(child);
            let mut status = 0;
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                // SAFETY: `status` is a live local; `pid` is our own child.
                let rc = unsafe { c::waitpid(pid, &raw mut status, WNOHANG) };
                if rc == pid {
                    break;
                }
                assert!(Instant::now() < deadline, "the shell outlived its terminal");
                std::thread::sleep(Duration::from_millis(50));
            }
            // Reaching here is the assertion: the shell ended once its
            // terminal was hung up. How it ended is the shell's business --
            // bash and dash take the hangup's failed read as end of input and
            // exit with a status; a shell with no handler dies of the SIGHUP
            // itself -- so the status is not examined.
        }
    }
}

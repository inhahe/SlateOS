//! The three standard descriptors, as the process was actually given them.
//!
//! Every other module in this crate exists because two utilities would
//! otherwise disagree with each other. This one exists because all of them
//! disagree with *GNU*, in the same way, for a reason none of them can see from
//! inside: the Rust runtime tells a program two untruths about descriptors 0, 1
//! and 2, and both of them are told before `main` gets a chance to object.
//!
//! ## The first lie: the descriptor is reopened
//!
//! `std::rt`'s start-up calls `sanitize_standard_fds`, which walks 0, 1 and 2
//! and reopens on `/dev/null` any that is not open. The motive is a real
//! security concern — a set-uid program invoked with descriptor 1 closed will
//! have the *next* file it opens land on 1, and then print its contents — but
//! the effect is that
//!
//! ```text
//! prog >&-
//! ```
//!
//! reaches `main` indistinguishable from `prog >/dev/null`. GNU's utilities
//! see the closed descriptor, get `EBADF` from `write`, and say so:
//!
//! ```text
//! $ nice >&- ; echo $?
//! nice: write error: Bad file descriptor
//! 125
//! ```
//!
//! while ours exited 0 having "written" the number nowhere.
//!
//! ## The second lie: the failure is swallowed
//!
//! Even with the descriptor genuinely closed, `std::io::Stdout` would not
//! report it. `StdoutRaw` and `StderrRaw`'s `Write` impls pass their result
//! through the standard library's `handle_ebadf`, which turns `EBADF` into
//! `Ok(buf.len())` — a write to a closed descriptor reported as a write that
//! wholly succeeded. Measured, before this module existed: `nohup true 2>&-`
//! exited 0 against GNU's 125, with `nohup.out` sitting on disk and no
//! explanation of itself anywhere.
//!
//! The two arrive by different doors and have to be answered separately, which
//! is why this module has two halves.
//!
//! ## Answering the first: [`guard_std_fds!`]
//!
//! By the time any Rust code you wrote runs, the answer is already gone —
//! `fcntl(1, F_GETFD)` succeeds, because 1 is now `/dev/null`. The one window
//! in which the truth is still available is the ELF constructor array:
//! `.init_array` entries run from `__libc_start_main`, before libc calls
//! `main` and therefore before `lang_start` sanitises anything.
//!
//! So a utility that cares writes
//!
//! ```ignore
//! coreutils::guard_std_fds!();
//! ```
//!
//! at module scope, which installs the constructor, and calls
//! [`restore`] as the first statement of its `main`, which re-closes whatever
//! the constructor found closed. Everything afterwards — `isatty` reporting
//! `EBADF`, a diagnostic failing to write, a spawned command inheriting a
//! closed descriptor — then follows from the truth instead of needing a
//! special case at each site.
//!
//! The macro rather than a plain `static` in this crate is deliberate. An
//! `.init_array` entry is kept alive by `#[used]` within its own compilation
//! unit, but a *library*'s object file is pulled out of the rlib archive only
//! if the linker wants a symbol from it, and which CGU the static lands in is
//! not something a caller can rely on. Expanding it in the binary crate makes
//! the entry unconditional, which for a program-lifetime hook is the only
//! acceptable guarantee.
//!
//! ## Answering the second: [`write_all`] and [`Stream`]
//!
//! [`write_all`] is `write(2)` in a loop with `EINTR` retried and nothing else
//! interpreted. [`Stream`] is the buffered writer built on it — the
//! replacement for `println!` in a utility that has to notice when the print
//! did not happen. It buffers the way stdio does (line-buffered to a terminal,
//! block-buffered otherwise, unbuffered on stderr), so the interleaving of a
//! utility's output with its diagnostics matches what the same program does
//! under glibc.
//!
//! ## The same question about descriptor 2: [`diag!`] and [`close_stderr`]
//!
//! `eprintln!` has a third failure of its own, and it is louder than either of
//! the above: it *panics* when the write fails. The panic handler then tries to
//! print the panic message — to the same descriptor 2 — which fails for the
//! same reason, and the runtime aborts. `id --nope 2>/dev/full` was status 134,
//! `Aborted (core dumped)`, where GNU exits 1.
//!
//! GNU's rule comes out of gnulib's `close_stdout`, which closes *stderr* as
//! well as stdout and `_exit (exit_failure)` if that fails. `close_stream` then
//! forgives a close that fails with `EBADF` when nothing was pending, so what
//! actually decides the status is whether a diagnostic was *attempted*:
//!
//! | | GNU |
//! |---|---|
//! | nothing written to stderr | the status the run earned — `id --help` 0, `uname` 0 |
//! | a diagnostic attempted and lost | `exit_failure`, silently — `id --nope` 1, `tty x` **3** |
//!
//! and the second overrides the first in both directions: `pwd x 2>/dev/full`
//! is 1 where the same command with a working stderr is 0, and `tty x` is 3
//! where it is otherwise 2. Measured, for a closed descriptor and a full one
//! alike.
//!
//! So [`diag!`] replaces `eprintln!` — same shape, raw `write(2)`, no panic —
//! and records a lost diagnostic in one process-global flag, which is exactly
//! what `ferror (stderr)` is. [`close_stderr`] turns that flag into upstream's
//! verdict and belongs around `main` itself — one wrapper per binary, standing
//! in for the `atexit` registration, so no exit path can miss it.
//! [`close_stdout`] consults the same flag on its way past.
//!
//! ## What this module deliberately does not do
//!
//! It does not restore `SIGPIPE`. Rust masks it, so `yes | head -1` yields
//! `EPIPE` here where GNU's `yes` dies of the signal and reports 141. That
//! divergence is *forced*: the target kernel has no Unix signals at all (see
//! `design.txt` — "No Unix signals for process control"), so there is no
//! behaviour to restore, only a different one to invent.
//!
//! What it does instead is name the case once, in [`reader_gone`], so that a
//! utility answering it inherits the tree's established convention — say
//! nothing, keep the status the run had already earned — rather than deriving
//! it again and landing somewhere slightly different. Without that, every
//! caller of [`write_error`] would print `prog: write error: Broken pipe`
//! where GNU prints nothing at all.
//!
//! ## Host builds
//!
//! On a non-Linux host the constructor is not installed, [`restore`] is a
//! no-op, and [`write_all`] goes back through `std::io`. That puts the host
//! build back behind `handle_ebadf`, which is correct rather than a
//! compromise: the host is not the target, its console needs std's UTF-16
//! translation to print anything legible, and a closed descriptor 1 is not a
//! configuration a `cargo test` run ever produces.

use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::errmsg::strerror;

// ---------------------------------------------------------------- linux ----

#[cfg(target_os = "linux")]
mod imp {
    use std::io;
    use std::sync::atomic::{AtomicU8, Ordering};

    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        fn close(fd: i32) -> i32;
        fn isatty(fd: i32) -> i32;
        // `*const c_void`, not `*const u8`: rustc's
        // `suspicious_runtime_symbol_definitions` lint compares this
        // declaration against the `write` the standard library itself links and
        // warns on any divergence — the point being that a program which
        // redeclares a runtime symbol differently from the runtime is one
        // mismatch away from a miscompile.
        fn write(fd: i32, buf: *const core::ffi::c_void, count: usize) -> isize;
    }

    const F_GETFD: i32 = 1;

    /// Bit `n` is set if descriptor `n` was **not open** when the process
    /// started. Written once, from an `.init_array` constructor, because by the
    /// time `main` runs the answer is gone.
    static CLOSED_AT_STARTUP: AtomicU8 = AtomicU8::new(0);

    /// The constructor itself. Public only so the macro expansion in a binary
    /// crate can name it; not part of the interface.
    #[doc(hidden)]
    pub extern "C" fn record_closed_std_fds() {
        let mut mask: u8 = 0;
        for fd in 0..3 {
            // SAFETY: `F_GETFD` only reads a descriptor's flags and is defined
            // for any `int` — it reports `EBADF` rather than misbehaving.
            if unsafe { fcntl(fd, F_GETFD) } < 0 {
                mask |= 1 << fd;
            }
        }
        CLOSED_AT_STARTUP.store(mask, Ordering::Relaxed);
    }

    pub fn restore() {
        let mask = CLOSED_AT_STARTUP.load(Ordering::Relaxed);
        for fd in 0..3 {
            if mask & (1 << fd) != 0 {
                // SAFETY: the descriptor at `fd` is the `/dev/null` the runtime
                // opened to stand in for a closed one. Callers are required to
                // run this before touching standard I/O, so no Rust object owns
                // it.
                unsafe { close(fd) };
            }
        }
    }

    pub fn was_closed_at_startup(fd: i32) -> bool {
        let Ok(n) = u32::try_from(fd) else {
            return false;
        };
        if n >= 3 {
            return false;
        }
        CLOSED_AT_STARTUP.load(Ordering::Relaxed) >> n & 1 == 1
    }

    pub fn is_tty(fd: i32) -> bool {
        // SAFETY: `isatty` is a pure query on a descriptor number and has no
        // effect on the process whichever number is passed.
        unsafe { isatty(fd) == 1 }
    }

    pub fn probe(fd: i32) -> io::Result<()> {
        use std::mem::ManuallyDrop;
        use std::os::fd::FromRawFd;

        // SAFETY: `fd` is an integer the caller names, and `File::metadata` on
        // it is `fstat(2)` — which is defined for any `int` and reports `EBADF`
        // rather than misbehaving. `ManuallyDrop` is what makes the borrow a
        // borrow: without it, the `File` would close the descriptor here.
        let f = ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
        f.metadata().map(|_| ())
    }

    pub fn write_all(fd: i32, bytes: &[u8]) -> io::Result<()> {
        let mut written = 0usize;
        while let Some(rest) = bytes.get(written..).filter(|r| !r.is_empty()) {
            // SAFETY: `rest` is a live slice, and `write` reads at most
            // `rest.len()` bytes from its start.
            let n = unsafe { write(fd, rest.as_ptr().cast(), rest.len()) };
            if n < 0 {
                let e = io::Error::last_os_error();
                // A signal that arrived mid-write is not a delivery failure.
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            let Ok(n) = usize::try_from(n) else {
                return Err(io::Error::from(io::ErrorKind::WriteZero));
            };
            if n == 0 {
                return Err(io::Error::from(io::ErrorKind::WriteZero));
            }
            written = written.saturating_add(n);
        }
        Ok(())
    }
}

// ------------------------------------------------------------ elsewhere ----

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::io::{self, Write};

    pub fn restore() {}

    pub fn was_closed_at_startup(_fd: i32) -> bool {
        false
    }

    pub fn is_tty(_fd: i32) -> bool {
        // No `isatty` without libc, and the answer only decides buffering. The
        // conservative choice is the one that shows output soonest.
        false
    }

    pub fn probe(_fd: i32) -> io::Result<()> {
        // Nothing here answers the question `fstat` answers, and a wrong `Err`
        // would stop a utility before it started. The lie this module exists to
        // undo is the target's, so is its undoing.
        Ok(())
    }

    /// The one place `io::stdout()`/`io::stderr()` are still used on purpose.
    ///
    /// Everywhere else in the crate they are a bug, because the runtime maps
    /// `EBADF` on a standard descriptor to `Ok(buf.len())` and a diagnostic
    /// that never arrived then looks delivered. Here there is no alternative:
    /// this arm is the build without libc, so there is no `write(2)` to call
    /// and no descriptor to call it on — only the runtime's own handles. The
    /// lie is therefore still present on this target, which is the Windows host
    /// build used to run the unit tests; the descriptor behaviour those tests
    /// check lives on the Linux and x86_64-slateos arms above.
    pub fn write_all(fd: i32, bytes: &[u8]) -> io::Result<()> {
        match fd {
            1 => io::stdout().write_all(bytes),
            2 => io::stderr().write_all(bytes),
            _ => Err(io::Error::from(io::ErrorKind::Unsupported)),
        }
    }
}

#[cfg(target_os = "linux")]
#[doc(hidden)]
pub use imp::record_closed_std_fds as __record_closed_std_fds;

/// Install the `.init_array` constructor that records which of descriptors 0,
/// 1 and 2 were closed when the process began.
///
/// Write this once at module scope in a binary that calls [`restore`]:
///
/// ```ignore
/// coreutils::guard_std_fds!();
/// ```
///
/// Expands to nothing off Linux. See the module docs for why it is a macro.
#[macro_export]
macro_rules! guard_std_fds {
    () => {
        #[cfg(target_os = "linux")]
        #[used]
        #[unsafe(link_section = ".init_array")]
        static __SLATE_RECORD_CLOSED_STD_FDS: extern "C" fn() =
            $crate::stdfd::__record_closed_std_fds;
    };
}

/// Undo the runtime's substitution, restoring the descriptor table the process
/// was actually invoked with.
///
/// Call this as the first statement of `main`, in a binary that has expanded
/// [`guard_std_fds!`]. Calling it without the macro is harmless and does
/// nothing, which is the failure mode you want if someone forgets: the program
/// keeps the pre-existing behaviour rather than closing a descriptor it should
/// not have.
///
/// It must run before anything touches standard I/O, because it closes the
/// substituted descriptors outright and a live [`std::io::Stdout`] buffer
/// pointed at one would then flush into whatever opened next.
pub fn restore() {
    imp::restore();
}

/// Whether `fd` was closed when the process started — the question
/// [`restore`] answers, kept available for a utility that must act on it
/// rather than merely propagate it.
///
/// Always `false` without [`guard_std_fds!`], and off Linux.
#[must_use]
pub fn was_closed_at_startup(fd: i32) -> bool {
    imp::was_closed_at_startup(fd)
}

/// Ask whether a standard descriptor is usable, without writing to it.
///
/// `fstat(2)`, which is how several GNU utilities find out — `cat` opens with
///
/// ```c
/// if (fstat (STDOUT_FILENO, &stat_buf) < 0)
///   error (EXIT_FAILURE, errno, _("standard output"));
/// ```
///
/// and so answers `cat f >&-` with one diagnostic and status 1 *before* it
/// opens `f`, rather than by failing to write. Reproducing that needs the
/// question asked separately from the writing, because the answers differ:
/// `cat missing f >&-` reports only `standard output`, never `missing`.
///
/// The descriptor is borrowed, not owned — the `File` is wrapped in
/// `ManuallyDrop` so that going out of scope does not close descriptor 1.
///
/// # Errors
///
/// Whatever `fstat(2)` reports, in practice [`io::ErrorKind::NotFound`]'s
/// cousin `EBADF` for a descriptor that is not open. Always `Ok` off Unix,
/// where there is no such thing to ask about and a utility must not start
/// refusing to run.
pub fn probe(fd: i32) -> io::Result<()> {
    imp::probe(fd)
}

/// Write every byte of `bytes` to `fd`, reporting a failure honestly.
///
/// `write(2)` in a loop, retrying only `EINTR` — a signal that arrived
/// mid-write is not a delivery failure. Unlike `std::io::stdout().write_all`
/// this does not pass its result through `handle_ebadf`, so a write to a
/// closed descriptor is an error rather than a silent success.
///
/// # Errors
///
/// Whatever `write(2)` reports, plus [`io::ErrorKind::WriteZero`] for a write
/// that returns zero on a non-empty buffer.
pub fn write_all(fd: i32, bytes: &[u8]) -> io::Result<()> {
    imp::write_all(fd, bytes)
}

/// Whether a diagnostic failed to reach descriptor 2 — `ferror (stderr)`,
/// which is process-global in stdio for the same reason it is here.
///
/// Sticky, and never cleared: the bytes lost to the first failure are still
/// lost, and there is no `clearerr` in any caller's control flow.
static DIAGNOSTIC_LOST: AtomicBool = AtomicBool::new(false);

/// Write one diagnostic to descriptor 2, remembering it if it does not arrive.
///
/// The whole of what [`diag!`] does; call it directly only where the message is
/// already a `String`. Nothing is added but the trailing newline — the program
/// name and its colon belong to the caller, as they do in `error(3)`.
///
/// Unlike `eprintln!` this cannot fail *or* panic. A failure is recorded in
/// the flag [`close_stderr`] reads, which is the only place GNU acts on it
/// either: `error()` does not check its own write, and it is the `fclose` in
/// gnulib's `close_stdout` that turns the lost diagnostic into a status.
pub fn diag_line(line: &str) {
    let mut bytes = Vec::with_capacity(line.len().saturating_add(1));
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    diag_bytes(&bytes);
}

/// [`diag_line`] without the newline, for a diagnostic assembled in pieces or
/// one that is not text — a file name from `argv` need not be UTF-8, and must
/// reach the terminal as the bytes it was given.
pub fn diag_bytes(bytes: &[u8]) {
    diag_to(2, bytes);
}

/// [`diag_bytes`] with the descriptor spelled out, so that the failure path can
/// be exercised on a descriptor that is not the test runner's own stderr.
fn diag_to(fd: i32, bytes: &[u8]) {
    if write_all(fd, bytes).is_err() {
        DIAGNOSTIC_LOST.store(true, Ordering::Relaxed);
    }
}

/// Whether any diagnostic has failed to reach descriptor 2 so far.
///
/// [`close_stderr`] is the usual way to act on this. It is public for the
/// utility that must know mid-run — none does yet — and for the tests.
#[must_use]
pub fn diagnostic_lost() -> bool {
    DIAGNOSTIC_LOST.load(Ordering::Relaxed)
}

/// `eprintln!` that reports a lost diagnostic instead of aborting the process.
///
/// Same shape as `eprintln!`, same arguments, one behavioural difference: a
/// write that fails is *recorded* rather than panicked on. `eprintln!` panics,
/// the panic message fails to print for the same reason, and the process dies
/// of `SIGABRT` — status 134 where GNU exits with its own failure status. See
/// the module docs.
///
/// ```ignore
/// use coreutils::diag;
///
/// diag!("tty: {e}");
/// ```
///
/// The flag it sets is read by [`close_stdout`] and [`close_stderr`], so a
/// utility that swaps `eprintln!` for this and already ends in one of those two
/// is correct with no further change.
#[macro_export]
macro_rules! diag {
    ($($arg:tt)*) => {
        $crate::stdfd::diag_line(&::std::format!($($arg)*))
    };
}

/// Report a failed write the way GNU does, on descriptor 2.
///
/// `error (0, errno, "%s", _("write error"))` from gnulib's `closeout.c`:
///
/// ```text
/// nice: write error: Bad file descriptor
/// ```
///
/// The report's own delivery is not checked, because there is nowhere left to
/// say so.
pub fn write_error(program: &str, err: &io::Error) {
    // Through `diag_bytes`, so that this failing counts as a lost diagnostic
    // like any other. It makes no difference to the status when stdout is what
    // failed — the caller is already returning a failure — but it does when
    // some other stream is, and it costs nothing to be consistent.
    diag_bytes(format!("{program}: write error: {}\n", strerror(err)).as_bytes());
}

/// Whether a failed write failed because the reader went away.
///
/// The one write failure a utility does not report. GNU answers it by dying of
/// `SIGPIPE`: no diagnostic, status 141, and the pipeline ends. SlateOS does
/// not use Unix signals for process control (`design.txt`) and Rust masks the
/// signal anyway, so "die of `SIGPIPE`" has no translation — the faithful one
/// is to stay quiet and keep whatever status the run had already earned, which
/// is also what upstream's own `EPIPE` branches do where it has them
/// (`tee`'s `--output-error` default, `iopoll`'s callers).
///
/// `cut`, `head`, `tail` and `uniq` each derived that convention separately,
/// with the same paragraph of comment copied between them. It lives here now
/// so the next utility inherits it. Guard [`write_error`] with it:
///
/// ```ignore
/// match out.finish() {
///     Ok(()) => code,
///     // Nothing downstream is listening, so there is nobody to tell.
///     Err(e) if stdfd::reader_gone(&e) => code,
///     Err(e) => {
///         stdfd::write_error("seq", &e);
///         ExitCode::FAILURE
///     }
/// }
/// ```
///
/// The status stays the caller's: it is 1 for most of the family but 125 for
/// `env`, 2 for `ls` and `sort`, 3 for `tty`, so folding it in here would get
/// four utilities wrong to save three lines in the rest.
#[must_use]
pub fn reader_gone(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::BrokenPipe
}

/// The last thing a utility does with its standard output: gnulib's
/// `atexit (close_stdout)`, spelled as a return value.
///
/// `earned` is the status the run had otherwise reached. It is returned when
/// the final flush succeeds, and also when it fails with nobody left to read —
/// the case GNU answers by dying of `SIGPIPE`, which see [`reader_gone`].
/// Any other failure prints
///
/// ```text
/// fold: write error: No space left on device
/// ```
///
/// and returns [`ExitCode::FAILURE`], discarding `earned`: upstream's handler
/// runs *after* whatever diagnostic ended the run and overrides its status, so
/// a run that already failed for its own reason still reports the output it
/// could not deliver, and still exits 1 rather than 2.
///
/// # Which utilities cannot use this
///
/// Two kinds. Those whose write failure is not status 1 — `env` (125), `ls`
/// and `sort` (2), `tty` (3) — and those that must know the flush's verdict
/// *before* they finish deciding what to print, such as `comm` and `join`,
/// which report a disordered input only if the output survived. Both spell the
/// tail out with [`reader_gone`] and [`write_error`] instead — and both must
/// still finish with [`close_stderr`], which this does for them.
pub fn close_stdout(program: &str, out: Stream, earned: ExitCode) -> ExitCode {
    close_stdout_with(program, out, earned, 1)
}

/// [`close_stdout`] for a utility whose failure status is not 1.
///
/// `failure` is upstream's `exit_failure`, the value it passes to gnulib's
/// `initialize_exit_failure`: 125 for `env`, 2 for `ls` and `sort`, 3 for
/// `tty`. It is what a lost diagnostic or an undelivered output exits with,
/// and it overrides `earned` in both directions — measured, `tty x` is 2 with
/// a working stderr and 3 without one.
pub fn close_stdout_with(program: &str, out: Stream, earned: ExitCode, failure: u8) -> ExitCode {
    let after_stdout = match out.finish() {
        Ok(()) => earned,
        Err(e) if reader_gone(&e) => earned,
        Err(e) => {
            write_error(program, &e);
            ExitCode::from(failure)
        }
    };
    close_stderr(after_stdout, failure)
}

/// The last word on the exit status: gnulib's `close_stream (stderr)`.
///
/// Returns `failure` if any diagnostic was lost — see [`diag!`] — and `earned`
/// otherwise. `failure` is upstream's `exit_failure`; 1 for most of the family.
///
/// # Wrap `main` with it; do not sprinkle it
///
/// Upstream does not decide this per exit path. It registers `close_stdout`
/// once, with `atexit`, so the verdict is reached on *every* exit — including
/// the early `usage (EXIT_FAILURE)` that never returns from `main`, which is
/// why `pwd x 2>/dev/full` is 1 where `pwd x` is 0. Rust has no `atexit` that
/// can change the status, but it has something better: exactly one value leaves
/// `main`. Funnel it.
///
/// ```ignore
/// fn main() -> ExitCode {
///     stdfd::close_stderr(run_main(), 1)
/// }
///
/// fn run_main() -> ExitCode { … }
/// ```
///
/// `run_main` and not `run`, only because `run` is already the name of a
/// top-level worker in about thirty of these bins and the funnel must not
/// collide with it.
///
/// The alternative — a call at each `return` — is the same rule written N
/// times, and the (N+1)th exit path someone adds later will not have it. The
/// wrapper cannot be forgotten by an edit that does not touch it.
///
/// [`close_stdout`] and [`close_stdout_with`] also call it, so the two compose:
/// the verdict is idempotent, since `failure` is one constant per program.
///
/// # The flag has to be set for this to see anything
///
/// It reads [`diagnostic_lost`], and nothing else. A diagnostic written with
/// `eprintln!` or through `io::stderr()` sets no flag — worse, both *lie* about
/// having arrived, since the runtime maps `EBADF` on a standard descriptor to
/// success and a `let _ =` throws away the `ENOSPC`. So every diagnostic must
/// leave through [`diag!`], [`diag_line`], [`diag_bytes`] or a [`Stream`] on
/// descriptor 2 (whose `record` sets the same flag), or this wrapper will hand
/// back `earned` for a run whose complaint went nowhere. `pwd` is the worked
/// example: `pwd foo` warns and exits 0, so with a lost warning GNU's 1 is the
/// *only* observable difference, and it existed for a day because the warning
/// still went out through `io::stderr()` after the funnel was in place.
///
/// It is deliberately *silent*. There is nowhere left to complain to, which is
/// exactly why the status has to carry the news.
#[must_use]
pub fn close_stderr(earned: ExitCode, failure: u8) -> ExitCode {
    if diagnostic_lost() {
        ExitCode::from(failure)
    } else {
        earned
    }
}

/// How much a [`Stream`] holds before it goes to the descriptor.
///
/// stdio picks this from the destination's `st_blksize`; 4096 is what that is
/// on every filesystem these utilities are run on, and the only thing the size
/// decides is how often `write(2)` is called.
const BUFFER: usize = 4096;

/// Whether output accumulates, and for how long.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Buffering {
    /// Every write goes straight out. What stdio does for stderr, so that a
    /// diagnostic survives whatever happens next.
    None,
    /// Flushed at each newline. What stdio does for a stream on a terminal,
    /// and the reason a prompt appears before the thing it is prompting for.
    Line,
    /// Flushed when full. What stdio does for a stream on a file or a pipe.
    Block,
}

/// A writer to a standard descriptor that says when the write did not happen.
///
/// The replacement for `print!`/`println!` in a utility whose exit status
/// depends on its output arriving. Errors are recorded rather than returned at
/// each call — [`Write::write`] here never fails — so that the ordinary
/// printing path stays free of `?`, and the accumulated verdict is taken once
/// at the end from [`Stream::finish`]. That is stdio's own arrangement
/// (`ferror`), and it is the one the utilities were written against.
pub struct Stream {
    fd: i32,
    buf: Vec<u8>,
    mode: Buffering,
    /// The first delivery failure. Sticky: a later success does not clear it,
    /// because the bytes lost to the first one are still lost.
    error: Option<io::Error>,
}

impl Stream {
    /// Standard output, buffered the way stdio buffers it: by line to a
    /// terminal, by block to anything else.
    #[must_use]
    pub fn stdout() -> Self {
        let mode = if imp::is_tty(1) {
            Buffering::Line
        } else {
            Buffering::Block
        };
        Self::new(1, mode)
    }

    /// Standard error, unbuffered, as stdio has it.
    #[must_use]
    pub fn stderr() -> Self {
        Self::new(2, Buffering::None)
    }

    /// A stream on an arbitrary descriptor, block-buffered.
    ///
    /// For a utility that has redirected its own output and still needs the
    /// honest writer — `nohup`'s saved copy of the original stderr, say.
    #[must_use]
    pub fn on(fd: i32) -> Self {
        Self::new(fd, Buffering::Block)
    }

    fn new(fd: i32, mode: Buffering) -> Self {
        Self {
            fd,
            buf: Vec::with_capacity(if mode == Buffering::None { 0 } else { BUFFER }),
            mode,
            error: None,
        }
    }

    /// The descriptor written to.
    #[must_use]
    pub fn fd(&self) -> i32 {
        self.fd
    }

    /// Whether anything has failed to reach the descriptor so far — stdio's
    /// `ferror`, and like `ferror` it does not account for what is still
    /// buffered.
    #[must_use]
    pub fn errored(&self) -> bool {
        self.error.is_some()
    }

    /// The failure behind [`Stream::errored`], for a caller that must word it
    /// before the end of the run.
    ///
    /// [`Stream::finish`] is the usual way to collect this, and it is the only
    /// way to collect a failure of the *final* flush. This exists for a utility
    /// whose control flow turns on the answer mid-run: GNU's `cat` abandons the
    /// remaining operands the moment a write fails, so `cat a b >/dev/full`
    /// prints one diagnostic and never opens `b`, and reproducing that needs
    /// the question asked between files rather than at the end.
    #[must_use]
    pub fn error(&self) -> Option<&io::Error> {
        self.error.as_ref()
    }

    /// Keep the first failure, and tell the process about it if it was a
    /// diagnostic.
    ///
    /// A `Stream` on descriptor 2 is another way of writing one — `nohup` and
    /// `env` build their messages that way — so a failure here has to reach the
    /// same flag [`diag!`] sets, or the status would depend on which of the two
    /// spellings the utility happened to use.
    fn record(&mut self, e: io::Error) {
        if self.fd == 2 {
            DIAGNOSTIC_LOST.store(true, Ordering::Relaxed);
        }
        if self.error.is_none() {
            self.error = Some(e);
        }
    }

    /// Push whatever is buffered at the descriptor, recording a failure.
    fn drain(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let result = imp::write_all(self.fd, &self.buf);
        self.buf.clear();
        if let Err(e) = result {
            self.record(e);
        }
    }

    /// Flush, and hand back the first failure of the stream's whole life.
    ///
    /// gnulib's `close_stream`, minus the close: a utility calls this instead
    /// of letting the buffer go out with the process, because a buffer flushed
    /// by the runtime has nobody left to report to. Pair it with
    /// [`write_error`].
    ///
    /// # Errors
    ///
    /// The first write that did not arrive, whether during this flush or any
    /// earlier one.
    pub fn finish(mut self) -> io::Result<()> {
        self.drain();
        self.error.take().map_or(Ok(()), Err)
    }
}

impl Write for Stream {
    /// Never fails. A failure is recorded and reported by [`Stream::finish`];
    /// see the type's docs.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self.mode {
            Buffering::None => {
                if let Err(e) = imp::write_all(self.fd, bytes) {
                    self.record(e);
                }
            }
            Buffering::Line => {
                self.buf.extend_from_slice(bytes);
                // Everything up to and including the last newline goes now;
                // a partial line waits for the rest of itself.
                if let Some(end) = self.buf.iter().rposition(|&b| b == b'\n') {
                    let rest = self.buf.split_off(end.saturating_add(1));
                    self.drain();
                    self.buf = rest;
                }
            }
            Buffering::Block => {
                self.buf.extend_from_slice(bytes);
                if self.buf.len() >= BUFFER {
                    self.drain();
                }
            }
        }
        Ok(bytes.len())
    }

    /// Never fails, for the same reason [`Stream::write`] does not.
    fn flush(&mut self) -> io::Result<()> {
        self.drain();
        Ok(())
    }
}

impl Drop for Stream {
    /// A stream dropped without [`Stream::finish`] still delivers what it
    /// holds — losing the verdict is a bug, but losing the *output* on top of
    /// it would be a worse one.
    fn drop(&mut self) {
        self.drain();
    }
}

#[cfg(test)]
mod tests {
    use super::{BUFFER, Buffering, Stream};
    use std::io::Write;

    /// A stream on a descriptor nothing can be written to, so that the error
    /// path is exercised without needing a real closed descriptor 1.
    fn broken(mode: Buffering) -> Stream {
        Stream::new(-1, mode)
    }

    #[test]
    fn write_never_reports_failure_but_finish_does() {
        let mut s = broken(Buffering::Block);
        assert_eq!(s.write(b"hello\n").expect("write is infallible"), 6);
        assert!(s.finish().is_err(), "the failure surfaces at finish");
    }

    #[test]
    fn unbuffered_records_immediately() {
        let mut s = broken(Buffering::None);
        let _ = s.write(b"x");
        assert!(s.errored(), "an unbuffered write is attempted at once");
    }

    #[test]
    fn block_buffered_holds_until_full() {
        let mut s = broken(Buffering::Block);
        let _ = s.write(b"x");
        assert!(!s.errored(), "one byte does not reach the descriptor yet");
        let _ = s.write(&vec![b'y'; BUFFER]);
        assert!(s.errored(), "crossing the buffer size does");
    }

    #[test]
    fn line_buffered_holds_a_partial_line() {
        let mut s = broken(Buffering::Line);
        let _ = s.write(b"no newline yet");
        assert!(!s.errored(), "a partial line waits for the rest of itself");
        let _ = s.write(b"\nand more");
        assert!(s.errored(), "the newline sends everything up to it");
    }

    #[test]
    fn line_buffered_keeps_the_tail_after_the_last_newline() {
        let mut s = broken(Buffering::Line);
        let _ = s.write(b"a\nb");
        assert_eq!(s.buf, b"b", "the tail is held back, not sent or dropped");
    }

    #[test]
    fn the_first_failure_is_the_one_reported() {
        let mut s = broken(Buffering::None);
        let _ = s.write(b"first");
        let first = s.error.as_ref().map(std::io::Error::kind);
        let _ = s.write(b"second");
        assert_eq!(
            s.error.as_ref().map(std::io::Error::kind),
            first,
            "a later failure does not displace the first"
        );
    }

    /// The one test that touches the process-global flag, and it is one test
    /// on purpose: the flag is deliberately sticky and has no `clearerr`, so
    /// two tests asserting on it would depend on which ran first. Nothing else
    /// in this suite reads it, which is what makes setting it here safe.
    ///
    /// Descriptor -1 rather than 2, so that a diagnostic the test *wants* to
    /// fail does not have to be printed at the runner to fail.
    #[test]
    fn a_lost_diagnostic_is_remembered_and_a_delivered_one_is_not() {
        assert!(!super::diagnostic_lost(), "nothing has failed to write yet");
        super::diag_to(2, b"");
        assert!(
            !super::diagnostic_lost(),
            "a write that arrived is not a loss"
        );
        super::diag_to(-1, b"a diagnostic with nowhere to go\n");
        assert!(super::diagnostic_lost(), "one that did not arrive is");
        super::diag_to(2, b"");
        assert!(
            super::diagnostic_lost(),
            "and it stays lost -- a later success does not bring the bytes back"
        );
    }

    #[test]
    fn stdout_and_stderr_pick_the_expected_descriptors() {
        assert_eq!(Stream::stdout().fd(), 1);
        assert_eq!(Stream::stderr().fd(), 2);
        assert_eq!(Stream::on(7).fd(), 7);
    }

    #[test]
    fn stderr_is_unbuffered() {
        assert_eq!(Stream::stderr().mode, Buffering::None);
    }

    #[test]
    fn probing_an_open_descriptor_succeeds_and_leaves_it_open() {
        // Both halves matter. The first is the answer; the second is that
        // asking did not cost the caller the descriptor — `File::from_raw_fd`
        // takes ownership, and a `probe` that let it drop would close standard
        // error and take the rest of this suite's diagnostics with it.
        assert!(super::probe(2).is_ok());
        assert!(super::probe(2).is_ok(), "the first probe closed it");
        assert!(super::write_all(2, b"").is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn probing_a_descriptor_that_is_not_open_reports_it() {
        // 4096 is above any descriptor a test harness has open and below the
        // usual `RLIMIT_NOFILE`, so this is `EBADF` and not a limit error.
        let e = super::probe(4096).expect_err("an unopened descriptor probed ok");
        assert_eq!(e.raw_os_error(), Some(9), "want EBADF, got {e}");
    }

    #[test]
    fn only_a_broken_pipe_is_the_reader_going_away() {
        use std::io::{Error, ErrorKind};

        assert!(super::reader_gone(&Error::from(ErrorKind::BrokenPipe)));
        // The three that the family's `write error:` line is actually for.
        // `EBADF` in particular must not be swallowed: reporting it is the
        // whole reason this module exists.
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::StorageFull,
            ErrorKind::WriteZero,
        ] {
            assert!(
                !super::reader_gone(&Error::from(kind)),
                "{kind:?} must still be reported"
            );
        }
        // `EBADF` has no stable `ErrorKind`, so it is checked as raw errno.
        assert!(!super::reader_gone(&Error::from_raw_os_error(9)));
    }

    #[test]
    fn restore_without_the_macro_does_nothing() {
        // The important half of this is that it does not close descriptor 1 in
        // the test harness, which would take the rest of the suite with it.
        super::restore();
        assert!(!super::was_closed_at_startup(1));
    }
}

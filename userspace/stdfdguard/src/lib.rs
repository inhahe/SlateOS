//! Descriptors 0, 1 and 2 as the process was given them, before the Rust
//! runtime replaced a closed one.
//!
//! `std::rt`'s start-up calls `sanitize_standard_fds`, which walks 0, 1 and 2
//! and reopens on `/dev/null` any that is not open. The motive is a real
//! security concern -- a set-uid program started with descriptor 1 closed
//! would have the *next* file it opens land on 1, and then print into it --
//! but the effect is that
//!
//! ```text
//! prog >&-
//! ```
//!
//! reaches `main` indistinguishable from `prog >/dev/null`. The C programs the
//! utilities here are ports of see the closed descriptor: GNU's get `EBADF`
//! from `write` and say so; util-linux's `lsmem -a >&-` finds its stream's
//! error flag set and prints `lsmem: write error`. A port that lets the
//! runtime's substitution stand exits 0 having written nowhere.
//!
//! By the time any Rust code runs, the truth is gone -- `fcntl(1, F_GETFD)`
//! succeeds, because 1 is `/dev/null` now. The one window in which it is
//! still there is the ELF constructor array: `.init_array` entries run from
//! `__libc_start_main`, before libc calls `main` and so before `lang_start`
//! sanitises anything. So a program that cares writes
//!
//! ```ignore
//! stdfdguard::guard_std_fds!();
//! ```
//!
//! at module scope in its binary crate, which installs the constructor, and
//! calls [`restore`] first thing in `main`, which closes again whatever the
//! constructor found closed. Everything after -- a write failing with
//! `EBADF`, `isatty` saying no, the next file opened taking descriptor 1 as
//! upstream's does -- then follows from the truth.
//!
//! It is a macro rather than a static in this crate because an `.init_array`
//! entry is kept by `#[used]` only within its own compilation unit, and a
//! library's object file is pulled out of the rlib only if the linker wants a
//! symbol from it. Expanded in the binary, the entry is unconditional.
//!
//! # And `SIGPIPE`, for the same reason
//!
//! The runtime also sets `SIGPIPE` to ignored before `main`, so that a write
//! to a closed pipe returns `EPIPE` instead of killing the program -- and,
//! again, without recording what it replaced. A program that runs commands
//! can need the answer: GNU `split --filter` hands its commands `SIGPIPE` at
//! its default only if that is what *it* was started with
//! (`default_SIGPIPE`), and leaves it ignored for them if it was started that
//! way, where Rust's `Command` restores the default in every child. The same
//! constructor records it, and [`sigpipe_ignored_at_startup`] answers.
//!
//! [`restore`] then puts it back, as it puts the descriptors back: a program
//! started with `SIGPIPE` at its default dies of a broken pipe, silently, as
//! every GNU program does, and one started with it ignored gets `EPIPE`.
//!
//! This half of coreutils' `stdfd` moved here on 2026-09-26 so that programs
//! outside coreutils -- the util-linux ports first -- can have it too;
//! `coreutils::guard_std_fds!` and `coreutils::stdfd::restore` are this. The
//! other half, the stdio-shaped writer that does not swallow `EBADF`, stays in
//! coreutils: it is gnulib's `close_stdout`, and util-linux's differs.
//!
//! Linux only: elsewhere the macro expands to nothing, [`restore`] and
//! [`ignore_sigpipe`] do nothing, and [`was_closed_at_startup`],
//! [`sigpipe_ignored_at_startup`] and [`sigpipe_restored`] are always `false`.
//! "Linux" includes SlateOS, whose userspace target is `linux`-`musl` over the
//! POSIX layer, and whose C library raises `SIGPIPE` as Linux's kernel does.

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        fn close(fd: i32) -> i32;
        fn signal(sig: i32, handler: usize) -> usize;
    }

    const F_GETFD: i32 = 1;
    const SIGPIPE: i32 = 13;
    const SIG_DFL: usize = 0;
    const SIG_IGN: usize = 1;
    /// `(void (*)(int)) -1`.
    const SIG_ERR: usize = usize::MAX;

    /// Whether `SIGPIPE` was ignored when the process started. Written once,
    /// from the constructor, for the same reason as [`CLOSED_AT_STARTUP`].
    static SIGPIPE_IGNORED_AT_STARTUP: AtomicBool = AtomicBool::new(false);

    /// Bit `n` is set if descriptor `n` was **not open** when the process
    /// started. Written once, from the `.init_array` constructor, because by
    /// the time `main` runs the answer is gone.
    static CLOSED_AT_STARTUP: AtomicU8 = AtomicU8::new(0);

    /// The constructor itself. Public only so the macro expansion in a binary
    /// crate can name it; not part of the interface.
    pub extern "C" fn record_startup_state() {
        let mut mask: u8 = 0;
        for fd in 0..3 {
            // SAFETY: `F_GETFD` only reads a descriptor's flags and is defined
            // for any `int` -- it reports `EBADF` rather than misbehaving.
            if unsafe { fcntl(fd, F_GETFD) } < 0 {
                mask |= 1 << fd;
            }
        }
        CLOSED_AT_STARTUP.store(mask, Ordering::Relaxed);
        record_sigpipe();
    }

    /// Learn `SIGPIPE`'s disposition by setting it, which is what `signal`
    /// offers: it answers with the handler it replaced.
    ///
    /// At this point that can only be the default or ignored -- a handler does
    /// not survive `exec` -- and the runtime is about to make it ignored
    /// anyway, so the probe changes nothing that lasts. The default is put back
    /// all the same, so that nothing between here and the runtime's own call
    /// runs with a disposition the process was not given.
    fn record_sigpipe() {
        // SAFETY: `signal` with `SIG_IGN` installs no code of ours and touches
        // no memory of ours; it returns the previous handler or `SIG_ERR`.
        let old = unsafe { signal(SIGPIPE, SIG_IGN) };
        if old == SIG_ERR {
            return;
        }
        if old == SIG_DFL {
            // SAFETY: as above -- restoring the default handler.
            unsafe { signal(SIGPIPE, SIG_DFL) };
        }
        SIGPIPE_IGNORED_AT_STARTUP.store(old == SIG_IGN, Ordering::Relaxed);
        SIGPIPE_RECORDED.store(true, Ordering::Relaxed);
    }

    /// Whether the constructor learned `SIGPIPE`'s disposition. Without the
    /// macro it never runs, and [`restore`] then leaves `SIGPIPE` as the
    /// runtime set it, as it leaves the descriptors.
    static SIGPIPE_RECORDED: AtomicBool = AtomicBool::new(false);

    pub fn sigpipe_ignored_at_startup() -> bool {
        SIGPIPE_IGNORED_AT_STARTUP.load(Ordering::Relaxed)
    }

    /// Whether [`restore`] has put `SIGPIPE` back to the disposition the
    /// process inherited. Until then it is the runtime's "ignored".
    static SIGPIPE_RESTORED: AtomicBool = AtomicBool::new(false);

    pub fn sigpipe_restored() -> bool {
        SIGPIPE_RESTORED.load(Ordering::Relaxed)
    }

    pub fn ignore_sigpipe() {
        // SAFETY: `signal` with `SIG_IGN` installs no code of ours and
        // touches no memory of ours.
        unsafe { signal(SIGPIPE, SIG_IGN) };
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
        if !SIGPIPE_RECORDED.load(Ordering::Relaxed) {
            return;
        }
        if !SIGPIPE_IGNORED_AT_STARTUP.load(Ordering::Relaxed) {
            // SAFETY: as in `record_sigpipe` -- restoring the default handler,
            // which the process was started with.
            unsafe { signal(SIGPIPE, SIG_DFL) };
        }
        SIGPIPE_RESTORED.store(true, Ordering::Relaxed);
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
}

#[cfg(not(target_os = "linux"))]
mod imp {
    pub fn restore() {}

    pub fn was_closed_at_startup(_fd: i32) -> bool {
        false
    }

    pub fn sigpipe_ignored_at_startup() -> bool {
        false
    }

    pub fn sigpipe_restored() -> bool {
        false
    }

    pub fn ignore_sigpipe() {}
}

#[cfg(target_os = "linux")]
#[doc(hidden)]
pub use imp::record_startup_state as __record_startup_state;

/// Install the `.init_array` constructor that records which of descriptors 0,
/// 1 and 2 were closed when the process began, and whether `SIGPIPE` was
/// ignored.
///
/// Write this once at module scope in a binary that calls [`restore`]:
///
/// ```ignore
/// stdfdguard::guard_std_fds!();
/// ```
///
/// Expands to nothing off Linux. See the crate docs for why it is a macro.
#[macro_export]
macro_rules! guard_std_fds {
    () => {
        #[cfg(target_os = "linux")]
        #[used]
        #[unsafe(link_section = ".init_array")]
        static __SLATE_RECORD_STARTUP_STATE: extern "C" fn() = $crate::__record_startup_state;
    };
}

/// Undo the runtime's substitutions: close again each standard descriptor the
/// process was started without, and put `SIGPIPE` back to the disposition it
/// inherited.
///
/// Call this as the first statement of `main`, in a binary that has expanded
/// [`guard_std_fds!`]. Without the macro it does nothing, which is the failure
/// mode wanted if someone forgets: the program keeps the runtime's behaviour
/// rather than closing a descriptor it should not.
///
/// It must run before anything touches standard I/O, because it closes the
/// substituted descriptors outright, and a live [`std::io::Stdout`] buffer
/// pointed at one would then flush into whatever opened next.
///
/// **`SIGPIPE`** is usually inherited at its default, and then a write into a
/// pipe whose reader has gone ends the program silently, status 141 -- as it
/// ends every GNU program, which none of them change except where named
/// (`tee`'s `--output-error` modes, `split --filter`, which call
/// [`ignore_sigpipe`]). Started with it ignored (`trap '' PIPE`), a program
/// gets `EPIPE` instead, as GNU's do, and reports it as the write error it is.
/// Until 2026-10-07 the runtime's "ignored" was kept and the error answered
/// quietly, because the target sent no `SIGPIPE`; it does now
/// (design-decisions §1176, and lane B's entry that follows from it).
pub fn restore() {
    imp::restore();
}

/// Whether [`restore`] has put `SIGPIPE` back to the disposition the process
/// inherited. False before it runs, without [`guard_std_fds!`], and off Linux:
/// the runtime's "ignored" then still stands, and an `EPIPE` stands in for the
/// signal the program would have died of.
#[must_use]
pub fn sigpipe_restored() -> bool {
    imp::sigpipe_restored()
}

/// Ignore `SIGPIPE` from now on, for a program whose upstream does --
/// `signal (SIGPIPE, SIG_IGN)` -- so that a write into a broken pipe returns
/// `EPIPE` for it to act on. Does nothing off Linux.
pub fn ignore_sigpipe() {
    imp::ignore_sigpipe();
}

/// Whether `fd` was closed when the process started -- the question
/// [`restore`] answers, kept for a program that must act on it rather than
/// merely propagate it.
///
/// Always `false` without [`guard_std_fds!`], and off Linux.
#[must_use]
pub fn was_closed_at_startup(fd: i32) -> bool {
    imp::was_closed_at_startup(fd)
}

/// Whether `SIGPIPE` was ignored when the process started -- before the
/// runtime ignored it regardless. See the crate docs for who needs to know.
///
/// Always `false` without [`guard_std_fds!`], and off Linux.
#[must_use]
pub fn sigpipe_ignored_at_startup() -> bool {
    imp::sigpipe_ignored_at_startup()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test on purpose: it is the only one that reads the process-wide
    /// mask, so no other test can be reading it at the same moment.
    #[test]
    fn without_the_macro_nothing_is_recorded_and_nothing_closed() {
        // No macro in a test binary, so nothing was recorded; and a number
        // outside 0..3 is never one of them regardless.
        for fd in [-1, 0, 1, 2, 3, 1024] {
            assert!(!was_closed_at_startup(fd), "{fd}");
        }
        assert!(!sigpipe_ignored_at_startup());
        restore();
        // Nothing was recorded, so `SIGPIPE` was left as the runtime set it,
        // which is what lets `reader_gone` keep standing in for the signal in
        // a program without the macro -- this test binary among them.
        assert!(!sigpipe_restored());
        // Standard output is still there to be written to.
        use std::io::Write;
        assert!(std::io::stdout().flush().is_ok());
    }
}

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
//! This half of coreutils' `stdfd` moved here on 2026-09-26 so that programs
//! outside coreutils -- the util-linux ports first -- can have it too;
//! `coreutils::guard_std_fds!` and `coreutils::stdfd::restore` are this. The
//! other half, the stdio-shaped writer that does not swallow `EBADF`, stays in
//! coreutils: it is gnulib's `close_stdout`, and util-linux's differs.
//!
//! Linux only: elsewhere the macro expands to nothing, [`restore`] does
//! nothing and [`was_closed_at_startup`] is always `false`.

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::atomic::{AtomicU8, Ordering};

    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        fn close(fd: i32) -> i32;
    }

    const F_GETFD: i32 = 1;

    /// Bit `n` is set if descriptor `n` was **not open** when the process
    /// started. Written once, from the `.init_array` constructor, because by
    /// the time `main` runs the answer is gone.
    static CLOSED_AT_STARTUP: AtomicU8 = AtomicU8::new(0);

    /// The constructor itself. Public only so the macro expansion in a binary
    /// crate can name it; not part of the interface.
    pub extern "C" fn record_closed_std_fds() {
        let mut mask: u8 = 0;
        for fd in 0..3 {
            // SAFETY: `F_GETFD` only reads a descriptor's flags and is defined
            // for any `int` -- it reports `EBADF` rather than misbehaving.
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
}

#[cfg(not(target_os = "linux"))]
mod imp {
    pub fn restore() {}

    pub fn was_closed_at_startup(_fd: i32) -> bool {
        false
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
        static __SLATE_RECORD_CLOSED_STD_FDS: extern "C" fn() = $crate::__record_closed_std_fds;
    };
}

/// Undo the runtime's substitution: close again each standard descriptor the
/// process was started without.
///
/// Call this as the first statement of `main`, in a binary that has expanded
/// [`guard_std_fds!`]. Without the macro it does nothing, which is the failure
/// mode wanted if someone forgets: the program keeps the runtime's behaviour
/// rather than closing a descriptor it should not.
///
/// It must run before anything touches standard I/O, because it closes the
/// substituted descriptors outright, and a live [`std::io::Stdout`] buffer
/// pointed at one would then flush into whatever opened next.
pub fn restore() {
    imp::restore();
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
        restore();
        // Standard output is still there to be written to.
        use std::io::Write;
        assert!(std::io::stdout().flush().is_ok());
    }
}

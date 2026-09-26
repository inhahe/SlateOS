//! `getopt_long`, in the words glibc actually uses: the shared
//! [`getoptlong`] crate, re-exported under the path the binaries here have
//! always imported it from.
//!
//! It lived in this file until 2026-09-26, when it moved out so that
//! programs outside coreutils -- the util-linux ports, which matched long
//! options whole and so refused `--pri` where upstream accepts it -- could
//! parse the way GNU does too. Everything in it is re-exported unchanged;
//! the one thing that stayed is [`Report`], because it is the one thing that
//! was coreutils': printing through [`crate::stdfd`].

pub use getoptlong::*;

/// Print `name: <error>` to stderr -- the whole of what a utility that stops
/// at the first command-line error does with one.
///
/// It goes out through [`crate::stdfd::diag_line`], not `eprintln!`. A usage
/// error is exactly the case where stderr is most likely to be unavailable --
/// `prog --nope 2>&-` -- and `eprintln!` responds to that by panicking,
/// whereupon the panic message fails to print for the same reason and the
/// runtime aborts: status 134 where GNU exits 1. `diag_line` also flushes this
/// crate's stdout buffer first, as glibc's `error()` does, and records a write
/// that failed for `close_stderr`.
///
/// A trait rather than a method because [`Program`] now belongs to
/// [`getoptlong`], which does no I/O; bring it into scope with the rest of the
/// `getopt` import.
pub trait Report {
    /// Print `e`, prefixed with this program's name.
    fn report(self, e: &Error);
}

impl Report for Program {
    fn report(self, e: &Error) {
        crate::stdfd::diag_line(&format!("{}: {}", self.name(), e.message()));
    }
}

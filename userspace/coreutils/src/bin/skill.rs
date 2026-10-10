//! `skill` -- send a signal to the processes an expression picks.
//!
//! procps-ng 4.0.4's. The whole program is [`coreutils::skill`], which
//! `snice` runs too. As upstream's, it decides what it is from the name it is
//! started under, so this file and `snice.rs` differ only in what they are
//! called: a `skill` run under the name `snice` is `snice`.

use std::process::ExitCode;

coreutils::guard_std_fds!();

#[cfg(unix)]
fn main() -> ExitCode {
    coreutils::skill::main()
}

/// The host build exists for the unit tests; the program needs a `/proc`.
#[cfg(not(unix))]
fn main() -> ExitCode {
    coreutils::diag!("skill: not supported on this host");
    ExitCode::from(1)
}

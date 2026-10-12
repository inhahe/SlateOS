//! `snice` -- set the nice value of the processes an expression picks.
//!
//! procps-ng 4.0.4's. The whole program is [`coreutils::skill`], which
//! `skill` runs too. As upstream's, it decides what it is from the name it is
//! started under, so this file and `skill.rs` differ only in what they are
//! called: a `snice` run under the name `skill` is `skill`.

use std::process::ExitCode;

coreutils::guard_std_fds!();

#[cfg(unix)]
fn main() -> ExitCode {
    coreutils::skill::main()
}

/// The host build exists for the unit tests; the program needs a `/proc`.
#[cfg(not(unix))]
fn main() -> ExitCode {
    coreutils::diag!("snice: not supported on this host");
    ExitCode::from(1)
}

//! `pgrep` — look up processes by name and other attributes.
//!
//! procps-ng 4.0.4's. The whole program is [`coreutils::pgrep`], which
//! `pkill` runs too. As upstream's, it decides what it is from the name it is
//! started under, so this file and `pkill.rs` differ only in what they are
//! called: a `pgrep` run under the name `pkill` is `pkill`.

use std::process::ExitCode;

coreutils::guard_std_fds!();

#[cfg(unix)]
fn main() -> ExitCode {
    coreutils::pgrep::main()
}

/// The host build exists for the unit tests; the program needs a `/proc`.
#[cfg(not(unix))]
fn main() -> ExitCode {
    coreutils::diag!("pgrep: not supported on this host");
    ExitCode::from(3)
}

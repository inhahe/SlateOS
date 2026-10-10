//! `vdir` — list directory contents in the long format.
//!
//! Upstream builds `ls.c` three times, with `ls_mode` set to `LS_LONG_FORMAT`;
//! [`coreutils::ls`] is that file, and [`Mode::Vdir`] is this build. It is
//! `ls -l -b`: the long format and escape quoting whether or not the output is
//! a terminal.

use coreutils::ls::Mode;
use std::process::ExitCode;

// Before `main`, so that the descriptors `vdir` was given are recorded before
// Rust's runtime puts `/dev/null` on a closed one; `coreutils::ls::main` puts
// them back (`stdfd::restore`).
coreutils::guard_std_fds!();

fn main() -> ExitCode {
    coreutils::ls::main(Mode::Vdir)
}

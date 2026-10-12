//! `ls` — list directory contents.
//!
//! Upstream builds `ls.c` three times, with `ls_mode` set to `LS_LS`;
//! [`coreutils::ls`] is that file, and [`Mode::Ls`] is this build. Its
//! defaults follow whether output is a terminal: columns there, one name per
//! line elsewhere.

use coreutils::ls::Mode;
use std::process::ExitCode;

// Before `main`, so that the descriptors `ls` was given are recorded before
// Rust's runtime puts `/dev/null` on a closed one; `coreutils::ls::main` puts
// them back (`stdfd::restore`).
coreutils::guard_std_fds!();

fn main() -> ExitCode {
    coreutils::ls::main(Mode::Ls)
}

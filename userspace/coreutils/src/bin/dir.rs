//! `dir` — list directory contents in columns, whatever the output is.
//!
//! Upstream builds `ls.c` three times, with `ls_mode` set to `LS_MULTI_COL`;
//! [`coreutils::ls`] is that file, and [`Mode::Dir`] is this build. It is
//! `ls -C -b`: columns and escape quoting whether or not the output is a
//! terminal, so a script sees what a person does.

use coreutils::ls::Mode;
use std::process::ExitCode;

// Before `main`, so that the descriptors `dir` was given are recorded before
// Rust's runtime puts `/dev/null` on a closed one; `coreutils::ls::main` puts
// them back (`stdfd::restore`).
coreutils::guard_std_fds!();

fn main() -> ExitCode {
    coreutils::ls::main(Mode::Dir)
}

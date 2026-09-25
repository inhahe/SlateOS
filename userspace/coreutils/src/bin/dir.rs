//! `dir` — list directory contents in columns, whatever the output is.
//!
//! Upstream builds `ls.c` three times, with `ls_mode` set to `LS_MULTI_COL`;
//! [`coreutils::ls`] is that file, and [`Mode::Dir`] is this build. It is
//! `ls -C -b`: columns and escape quoting whether or not the output is a
//! terminal, so a script sees what a person does.

use coreutils::ls::Mode;
use std::process::ExitCode;

fn main() -> ExitCode {
    coreutils::ls::main(Mode::Dir)
}

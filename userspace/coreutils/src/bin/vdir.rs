//! `vdir` — list directory contents in the long format.
//!
//! Upstream builds `ls.c` three times, with `ls_mode` set to `LS_LONG_FORMAT`;
//! [`coreutils::ls`] is that file, and [`Mode::Vdir`] is this build. It is
//! `ls -l -b`: the long format and escape quoting whether or not the output is
//! a terminal.

use coreutils::ls::Mode;
use std::process::ExitCode;

fn main() -> ExitCode {
    coreutils::ls::main(Mode::Vdir)
}

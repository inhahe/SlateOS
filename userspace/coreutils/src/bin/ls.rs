//! `ls` — list directory contents.
//!
//! Upstream builds `ls.c` three times, with `ls_mode` set to `LS_LS`;
//! [`coreutils::ls`] is that file, and [`Mode::Ls`] is this build. Its
//! defaults follow whether output is a terminal: columns there, one name per
//! line elsewhere.

use coreutils::ls::Mode;
use std::process::ExitCode;

fn main() -> ExitCode {
    coreutils::ls::main(Mode::Ls)
}

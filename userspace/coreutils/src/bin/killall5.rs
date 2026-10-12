//! Send a signal to every process but those of its own session: sysvinit
//! 3.08's, ported.
//!
//! The whole program is [`coreutils::killall5`], which `pidof` runs too. As
//! upstream's, it decides what it is from the name it is started under --
//! `pidof` or anything else -- so this file and `pidof.rs` differ only in
//! what they are called.

use std::process::ExitCode;

coreutils::guard_std_fds!();

fn main() -> ExitCode {
    coreutils::killall5::main()
}

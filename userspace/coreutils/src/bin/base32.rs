//! `base32` — encode or decode FILE, or standard input, to standard output.
//!
//! GNU coreutils 9.4's `base32`: `src/basenc.c` built with `BASE_TYPE` 32. All of
//! it is [`coreutils::basenc`]; see that module.

coreutils::guard_std_fds!();

fn main() -> std::process::ExitCode {
    coreutils::basenc::main(coreutils::basenc::Program::Base32)
}

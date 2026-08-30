//! false -- do nothing, unsuccessfully.
//!
//! Usage: false
//!   Always exits with status 1.
//!
//! Wrapped in an `exit_code()` function so the contract is verifiable
//! by a unit test instead of having to spawn the binary.

use coreutils::stdfd;
use std::process::ExitCode;

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    ExitCode::from(exit_code())
}

/// `false` is defined to always fail.  Returning this from `main`
/// (rather than calling `process::exit` inline) makes the contract
/// machine-checkable.
fn exit_code() -> u8 {
    1
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn always_one() {
        assert_eq!(exit_code(), 1);
    }

    #[test]
    fn nonzero_so_shell_treats_as_failure() {
        // Sanity: any non-zero would be a "failure" exit; we specifically
        // promise exit 1 to match POSIX `false`.
        assert_ne!(exit_code(), 0);
    }
}

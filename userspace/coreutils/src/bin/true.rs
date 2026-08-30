//! true -- do nothing, successfully.
//!
//! Usage: true
//!   Always exits with status 0.
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

/// `true` is defined to always succeed.  Returning this from `main`
/// (rather than just falling off the end) makes the contract
/// machine-checkable.
fn exit_code() -> u8 {
    0
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn always_zero() {
        assert_eq!(exit_code(), 0);
    }
}

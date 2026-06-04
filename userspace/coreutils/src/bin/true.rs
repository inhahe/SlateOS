//! true -- do nothing, successfully.
//!
//! Usage: true
//!   Always exits with status 0.
//!
//! Wrapped in an `exit_code()` function so the contract is verifiable
//! by a unit test instead of having to spawn the binary.

use std::process;

fn main() {
    process::exit(exit_code());
}

/// `true` is defined to always succeed.  Returning this from `main`
/// (rather than just falling off the end) makes the contract
/// machine-checkable.
fn exit_code() -> i32 {
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

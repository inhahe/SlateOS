//! `logger`'s command line, run as a program: what a user types, what comes
//! back on stderr, and the exit status.
//!
//! The option loop lives in `main` and ends in `process::exit`, so the only
//! honest way to test what it prints is to run it.

use std::process::Command;

fn logger(args: &[&str]) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_logger"))
        .args(args)
        .output()
        .expect("run logger");
    (
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().expect("an exit status, not a signal"),
    )
}

/// util-linux's wording, byte for byte, for an option it does not have.
#[test]
fn an_unknown_option_is_named_as_util_linux_names_it() {
    let (err, code) = logger(&["-Q"]);
    assert_eq!(
        err,
        "logger: invalid option -- 'Q'\nTry 'logger --help' for more information.\n"
    );
    assert_eq!(code, 1);
}

/// The letter is argv's, so it can be a control character. Printed between
/// hand-written quotes, a newline there would put a line break into stderr
/// that the user never typed as one; quoted properly it arrives escaped, on
/// the one line the message owns.
#[test]
fn an_unknown_option_that_is_a_newline_cannot_break_the_line() {
    let (err, code) = logger(&["-\n"]);
    let first = err.lines().next().unwrap_or_default();
    assert!(first.starts_with("logger: invalid option -- "), "{err:?}");
    assert!(
        !first.ends_with("-- '"),
        "the newline went out raw: {err:?}"
    );
    assert_eq!(
        err.lines().count(),
        2,
        "exactly the message and the hint: {err:?}"
    );
    assert_eq!(code, 1);
}

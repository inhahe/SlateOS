//! `powerctl` run as a program, for the behaviour that lives in `main`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

/// `powerctl reload` refuses, says why, and stops nothing.
///
/// The kernel has no way to restart without the firmware yet (lane C's
/// `requests/c-ab-a-restart-that-keeps-the-computer-on.md`). The refusal must
/// come before anything is asked to stop -- a machine whose services were
/// stopped for a restart that cannot happen is worse off than one that was
/// told no -- so the output must not mention the service manager at all.
#[test]
fn reload_refuses_before_stopping_anything() {
    let out = Command::new(env!("CARGO_BIN_EXE_powerctl"))
        .arg("reload")
        .output()
        .expect("run powerctl");
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "{}", out.stdout.escape_ascii());
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("cannot restart without the firmware"), "{err}");
    assert!(err.contains("Nothing was stopped"), "{err}");
    assert!(!err.contains("org.slateos.ServiceManager"), "{err}");
}

/// The usage lists it, and says it is not available yet.
#[test]
fn reload_is_in_the_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_powerctl"))
        .arg("help")
        .output()
        .expect("run powerctl");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("reload"), "{text}");
    assert!(
        text.contains("cannot restart without the firmware"),
        "{text}"
    );
}

fn powerctl(args: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_powerctl"))
        .args(args)
        .output()
        .expect("run powerctl")
}

/// `--help` after a command shows the usage and does nothing else. It used to
/// be ignored, so `powerctl reboot --help` rebooted.
#[test]
fn help_after_a_command_never_runs_the_command() {
    for cmd in ["reboot", "shutdown", "suspend", "hibernate"] {
        let out = powerctl(&[cmd.as_ref(), "--help".as_ref()]);
        assert_eq!(out.status.code(), Some(0), "{cmd}");
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.starts_with("Slate OS Power Control"), "{cmd}: {text}");
        // Nothing was asked of anyone: no progress note, no fallback.
        assert!(!text.contains("Initiating"), "{cmd}: {text}");
        assert!(
            out.stderr.is_empty(),
            "{cmd}: {}",
            out.stderr.escape_ascii()
        );
    }
}

/// Any other word a command does not take refuses it, and nothing is done.
#[test]
fn an_unexpected_word_refuses_the_command() {
    let out = powerctl(&["reboot".as_ref(), "now".as_ref()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "{}", out.stdout.escape_ascii());
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.starts_with("error: unexpected argument 'now' -- nothing was done\n"),
        "{err}"
    );
}

/// An argument that is not UTF-8 -- any byte but NUL is legal on SlateOS --
/// is refused by name. It used to panic inside `env::args()` before `main`'s
/// first statement.
#[cfg(unix)]
#[test]
fn an_argument_that_is_not_text_is_refused_not_a_crash() {
    use std::os::unix::ffi::OsStrExt;
    let out = powerctl(&[std::ffi::OsStr::from_bytes(b"re\xffboot")]);
    assert_eq!(out.status.code(), Some(1));
    let err = out.stderr;
    assert!(
        err.starts_with(b"unknown command: "),
        "{}",
        err.escape_ascii()
    );
    assert!(
        !err.windows(8).any(|w| w == b"panicked"),
        "{}",
        err.escape_ascii()
    );
}

/// A report whose standard output is gone fails with a message, not a panic.
#[cfg(target_os = "linux")]
#[test]
fn a_report_to_a_full_device_fails_cleanly() {
    let Ok(full) = std::fs::OpenOptions::new().write(true).open("/dev/full") else {
        return; // No /dev/full here: nothing to test against.
    };
    let out = Command::new(env!("CARGO_BIN_EXE_powerctl"))
        .arg("help")
        .stdout(full)
        .output()
        .expect("run powerctl");
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr).unwrap();
    assert_eq!(err, "powerctl: write error: No space left on device\n");
}

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

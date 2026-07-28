//! End-to-end tests for `2>&1` on an **external** command.
//!
//! These have to drive the real binary (via `CARGO_BIN_EXE_osh`) with its
//! stdout and stderr on two *distinct* pipes, because that is the only setup in
//! which the bug they guard against is observable: handing the child an
//! inherited fd 2 instead of a duplicate of fd 1 looks identical whenever both
//! standard descriptors happen to land on one terminal. An in-process unit test
//! cannot express it — the shell's ambient fd 1 is the process's, not a buffer.
//!
//! Inside each script `$BASH` is `osh` itself (the shell publishes its own
//! executable path there), so the child really is a separate process with its
//! own fd table.

use std::process::{Command, Stdio};

/// Run the built `osh` binary with `-c script`, its stdout and stderr on
/// separate pipes, and return `(stdout, stderr)`.
fn run_osh(script: &str) -> (String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_osh"))
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn osh");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// An external child that writes one line to each of its standard descriptors.
const CHILD: &str = r#""$BASH" -c 'echo E >&2; echo O' </dev/null"#;

#[test]
fn external_child_without_redirect_keeps_the_streams_apart() {
    // The control case: no redirect, so each line stays on its own descriptor.
    let (out, err) = run_osh(CHILD);
    assert_eq!(out, "O\n");
    assert_eq!(err, "E\n");
}

#[test]
fn external_child_2to1_duplicates_fd1_not_fd2() {
    // `2>&1` must give the child a duplicate of the *shell's fd 1*. Inheriting
    // fd 2 would leave "E" on stderr, which is what osh used to do.
    let (out, err) = run_osh(&format!("{CHILD} 2>&1"));
    assert_eq!(out, "E\nO\n");
    assert_eq!(err, "");
}

#[test]
fn external_child_2to1_under_a_compound_command() {
    // The same holds when the `2>&1` comes from an enclosing compound command
    // or a function invocation rather than the command's own redirect list.
    let (out, err) = run_osh(&format!("{{ {CHILD}; }} 2>&1"));
    assert_eq!(out, "E\nO\n");
    assert_eq!(err, "");
    let (out, err) = run_osh(&format!("f() {{ {CHILD}; }}; f 2>&1"));
    assert_eq!(out, "E\nO\n");
    assert_eq!(err, "");
}

#[test]
fn compound_2to1_does_not_follow_a_later_exec_redirect() {
    // `2>&1` dups fd 1 as it stands; a runtime `exec > file` inside the
    // redirect's scope moves fd 1 alone, so fd 2 stays on the original stdout.
    let dir = std::env::temp_dir().join(format!("osh_rdup_exec_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("g.txt");
    let p = path.to_string_lossy().replace('\\', "/");

    let (out, err) = run_osh(&format!("( exec >'{p}'; echo X >&2 ) 2>&1"));
    assert_eq!(out, "X\n");
    assert_eq!(err, "");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "");

    // The same for an external child, whose fd 2 the shell hands over explicitly.
    let (out, err) = run_osh(&format!("( exec >'{p}'; {CHILD} ) 2>&1"));
    assert_eq!(out, "E\n");
    assert_eq!(err, "");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "O\n");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compound_2to1_reaches_an_inner_fd_table() {
    // A pipeline stage and a command substitution each get their own fd table,
    // and both must still see the enclosing dup rather than the real stderr.
    let (out, err) = run_osh("{ { echo X >&2; } | sed 's/^/piped: /'; } 2>&1");
    assert_eq!(out, "X\n");
    assert_eq!(err, "");
    let (out, err) = run_osh(r#"{ echo "[$( { echo X >&2; } )]"; } 2>&1"#);
    assert_eq!(out, "X\n[]\n");
    assert_eq!(err, "");
    let (out, err) = run_osh(&format!("{{ {CHILD} | sed 's/^/piped: /'; }} 2>&1"));
    assert_eq!(out, "E\npiped: O\n");
    assert_eq!(err, "");
}

#[test]
fn external_child_2to1_before_a_stdout_redirect() {
    // Redirects apply left to right: `2>&1 >f` sends fd 2 to the *original*
    // stdout and only fd 1 onward to the file.
    let dir = std::env::temp_dir().join(format!("osh_rdup_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("f.txt");
    let p = path.to_string_lossy().replace('\\', "/");

    let (out, err) = run_osh(&format!("{CHILD} 2>&1 >'{p}'"));
    assert_eq!(out, "E\n");
    assert_eq!(err, "");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "O\n");

    // …whereas `>f 2>&1` sends both onward to the file, sharing one handle.
    let (out, err) = run_osh(&format!("{CHILD} >'{p}' 2>&1"));
    assert_eq!(out, "");
    assert_eq!(err, "");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "E\nO\n");

    let _ = std::fs::remove_dir_all(&dir);
}

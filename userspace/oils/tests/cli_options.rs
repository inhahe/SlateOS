//! End-to-end tests for `osh`'s command-line option parsing — in particular
//! the getopt-style bundling of `set` options with the mode letters `-c`/`-s`
//! and `-i` (e.g. `-ec`, `-ic`, `-cx`), which bash accepts as a single cluster.
//!
//! These drive the real binary (via `CARGO_BIN_EXE_osh`) because the option
//! parser lives in `main.rs`'s `run()` entry point, not the library.

use std::io::Write;
use std::process::{Command, Stdio};

/// Run the built `osh` binary with `args`, feeding `stdin_data` to its stdin,
/// and return `(stdout, stderr, exit_code)`.
fn run_osh(args: &[&str], stdin_data: &str) -> (String, String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_osh"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn osh");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin_data.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait osh");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn bare_dash_c_runs_command() {
    let (out, _err, code) = run_osh(&["-c", "echo hi"], "");
    assert_eq!(out, "hi\n");
    assert_eq!(code, 0);
}

#[test]
fn bundled_ec_enables_errexit_with_command() {
    // `-ec`: errexit + command mode. `false` aborts before the second echo.
    let (out, _err, code) = run_osh(&["-ec", "echo one; false; echo two"], "");
    assert_eq!(out, "one\n");
    assert_eq!(code, 1);
}

#[test]
fn bundled_xc_enables_xtrace_with_command() {
    // `-xc`: xtrace + command mode. Command output on stdout, trace on stderr.
    let (out, err, code) = run_osh(&["-xc", "echo hi"], "");
    assert_eq!(out, "hi\n");
    assert!(err.contains("echo hi"), "xtrace trace missing: {err:?}");
    assert_eq!(code, 0);
}

#[test]
fn mode_letter_first_still_applies_later_options() {
    // `-cx`: the mode letter may lead; the trailing `x` still enables xtrace.
    let (out, err, code) = run_osh(&["-cx", "echo hi"], "");
    assert_eq!(out, "hi\n");
    assert!(err.contains("echo hi"), "xtrace trace missing: {err:?}");
    assert_eq!(code, 0);
}

#[test]
fn separate_i_and_c_flags_run_command() {
    // `-i -c`: force-interactive plus command mode as distinct tokens.
    let (out, _err, code) = run_osh(&["-i", "-c", "echo hi"], "");
    assert_eq!(out, "hi\n");
    assert_eq!(code, 0);
}

#[test]
fn dash_s_reads_stdin_with_positional_params() {
    // `-s aa bb`: commands come from stdin; the operands are $1, $2.
    let (out, _err, code) = run_osh(&["-s", "aa", "bb"], "echo \"$1-$2\"\n");
    assert_eq!(out, "aa-bb\n");
    assert_eq!(code, 0);
}

#[test]
fn dash_c_command_name_and_args() {
    // `-c cmd name arg…`: $0 is name, $1… are the following operands.
    let (out, _err, code) = run_osh(&["-c", "echo $0 $1 $2", "myname", "a", "b"], "");
    assert_eq!(out, "myname a b\n");
    assert_eq!(code, 0);
}

#[test]
fn unknown_option_reports_invalid_option_and_exits_2() {
    let (_out, err, code) = run_osh(&["-z"], "");
    assert_eq!(code, 2);
    let first = err.lines().next().unwrap_or("");
    assert_eq!(first, "osh: -z: invalid option");
    assert!(err.contains("Usage:"), "usage summary missing: {err:?}");
}

#[test]
fn invalid_letter_in_cluster_reports_the_offending_letter() {
    // bash applies `x` then aborts on the unknown `z`, naming `-z` (not `-xz`).
    let (_out, err, code) = run_osh(&["-xz"], "");
    assert_eq!(code, 2);
    assert_eq!(err.lines().next().unwrap_or(""), "osh: -z: invalid option");
}

#[test]
fn plus_sign_unknown_option_keeps_its_sign() {
    let (_out, err, code) = run_osh(&["+q"], "");
    assert_eq!(code, 2);
    assert_eq!(err.lines().next().unwrap_or(""), "osh: +q: invalid option");
}

#[test]
fn unknown_long_option_reports_invalid_option() {
    let (_out, err, code) = run_osh(&["--nope"], "");
    assert_eq!(code, 2);
    assert_eq!(err.lines().next().unwrap_or(""), "osh: --nope: invalid option");
}

#[test]
fn bare_dash_reads_commands_from_stdin() {
    // `osh -` (legacy): end options, read stdin, operands become $1….
    let (out, _err, code) = run_osh(&["-", "aa", "bb"], "echo \"$1-$2\"\n");
    assert_eq!(out, "aa-bb\n");
    assert_eq!(code, 0);
}

/// `exit` read from stdin must end the shell, not merely set `$?`: the loop
/// stops reading and the status is the one `exit` named. bash prints only `one`
/// and exits 3 for the same input.
#[test]
fn exit_from_stdin_ends_the_read_loop() {
    let (out, _err, code) = run_osh(&["-s"], "echo one\nexit 3\necho two\n");
    assert_eq!(out, "one\n", "commands after `exit` must not run");
    assert_eq!(code, 3);
}

/// An `exit N` *inside* the EXIT trap replaces the shell's exit status, even
/// when the shell was already exiting with a different one (bash).
#[test]
fn exit_trap_exit_replaces_the_shells_status() {
    let (_out, _err, code) = run_osh(&["-c", "trap 'exit 9' EXIT; exit 2"], "");
    assert_eq!(code, 9);
    // Without an `exit` of its own the handler leaves the status untouched.
    let (_out, _err, code) = run_osh(&["-c", "trap ':' EXIT; exit 2"], "");
    assert_eq!(code, 2);
}

#[test]
fn double_dash_makes_dash_c_a_script_path() {
    // After `--`, `-c` is a *file* name, not the command flag; opening it fails.
    let (_out, err, code) = run_osh(&["--", "-c"], "");
    assert_ne!(code, 0, "opening a nonexistent script must fail");
    assert!(err.contains("-c"), "error should name the file: {err:?}");
}

/// A REPL reading stdin numbers its lines across the *whole* stream, not from 1
/// per command: bash's `$LINENO` and its `line N:` diagnostics both keep
/// counting. Blank and comment-only lines count too, and a function body is
/// numbered where it was *defined*, not where it is called. Every expectation
/// below is bash 5.2.37's actual output for the same input.
#[test]
fn stdin_repl_numbers_lines_across_the_whole_stream() {
    let (out, _err, _code) = run_osh(&["-s"], "echo A$LINENO\necho B$LINENO\n");
    assert_eq!(out, "A1\nB2\n");

    // Blank lines and comments are physical lines and advance the counter.
    let (out, _err, _code) = run_osh(&["-s"], "echo A$LINENO\n\n# c\necho B$LINENO\n");
    assert_eq!(out, "A1\nB4\n");

    // A compound command spans several physical lines; the next command
    // resumes after all of them.
    let (out, _err, _code) =
        run_osh(&["-s"], "echo A$LINENO\nif true\nthen\necho B$LINENO\nfi\necho C$LINENO\n");
    assert_eq!(out, "A1\nB4\nC6\n");

    // A function body reports the line it was defined on, wherever it is run.
    let (out, _err, _code) =
        run_osh(&["-s"], "f() {\necho B$LINENO\n}\necho C$LINENO\nf\n");
    assert_eq!(out, "C4\nB2\n");

    // Runtime diagnostics carry the same number.
    let (_out, err, _code) = run_osh(&["-s"], "echo one\nnosuchcmd_xyz_123\n");
    assert!(
        err.starts_with("osh: line 2: nosuchcmd_xyz_123:"),
        "diagnostic should name line 2: {err:?}"
    );

    // …and so does a syntax error, along with its echoed source line.
    let (_out, err, _code) = run_osh(&["-s"], "echo one\necho two )\n");
    assert_eq!(
        err,
        "osh: line 2: syntax error near unexpected token `)'\nosh: line 2: `echo two )'\n"
    );

    // An unterminated quote is reported on the stream line it opened on, after
    // the complete lines before it have run.
    let (out, err, _code) = run_osh(&["-s"], "echo one\necho two\nv='abc\n");
    assert_eq!(out, "one\ntwo\n");
    assert_eq!(
        err,
        "osh: line 3: unexpected EOF while looking for matching `''\n"
    );
}

//! End-to-end tests for `osh`'s command-line option parsing — in particular
//! the getopt-style bundling of `set` options with the mode letters `-c`/`-s`
//! and `-i` (e.g. `-ec`, `-ic`, `-cx`), which bash accepts as a single cluster —
//! and for the startup files the invocation selects.
//!
//! These drive the real binary (via `CARGO_BIN_EXE_osh`) because the option
//! parser lives in `main.rs`'s `run()` entry point, not the library.
//!
//! Every `osh` here is launched with `HOME` pointed at a throwaway directory and
//! `BASH_ENV`/`ENV` cleared. That is not tidiness: without it a shell started
//! `-i` would source the *developer's* real `~/.bashrc`, so the tests' results
//! would depend on whose machine they run on.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

/// One definition, shared with the in-process tests rather than copied — see
/// the module's own docs.
#[path = "../src/hostpath.rs"]
mod hostpath;

/// A throwaway directory used as `$HOME` (and as the cwd) for one test, removed
/// when the test's binding is dropped.
struct TempHome(PathBuf);

impl TempHome {
    fn new(tag: &str) -> Self {
        // Cargo runs the tests as threads of one process, so the pid alone would
        // let two of them share (and delete) one directory.
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("osh-cli-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp home");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Write `body` to `name` inside the home, as a startup file would be.
    fn write(&self, name: &str, body: &str) {
        std::fs::write(self.0.join(name), body).expect("write startup file");
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What `$0` — and so every diagnostic prefix that is not emitted by the option
/// parser itself — reads as for these runs.
///
/// bash seeds `$0` from `argv[0]`, so a shell with no script and no `-c` *name*
/// operand names itself by the path it was invoked as, not by the string `bash`
/// (`echo 'nosuch' | bash` reports `/usr/bin/bash: line 1: …`). osh does the
/// same, and here `argv[0]` is whatever cargo built the binary as
/// (TD-OILS-DOLLAR-ZERO-ARGV0). Diagnostics printed *before* the shell starts —
/// `osh: -z: invalid option` and friends — are literals in `main.rs` and are not
/// affected.
fn shell_name() -> &'static str {
    env!("CARGO_BIN_EXE_osh")
}

/// Run the built `osh` binary with `args`, feeding `stdin_data` to its stdin,
/// and return `(stdout, stderr, exit_code)`. `$HOME` is an empty throwaway
/// directory, so no startup file exists unless the test makes one.
fn run_osh(args: &[&str], stdin_data: &str) -> (String, String, i32) {
    let home = TempHome::new("plain");
    run_osh_in(&home, args, stdin_data)
}

/// The same, but with `home` as both `$HOME` and the working directory, so a
/// test can plant `.bashrc`/`.bash_profile`/… and see them read.
fn run_osh_in(home: &TempHome, args: &[&str], stdin_data: &str) -> (String, String, i32) {
    run_osh_env(home, &[], args, stdin_data)
}

/// The fullest form: `envs` are set *after* the isolation, so a test can put
/// `BASH_ENV` back deliberately.
///
/// The isolation includes `PATH`: cargo stages ~200 SlateOS coreutils ahead of
/// the host's for the duration of a test run, and a shell that resolved `grep`
/// to one of those would be answering a question about our `grep` rather than
/// about its own option parsing (`hostpath`).
fn run_osh_env(
    home: &TempHome,
    envs: &[(&str, &str)],
    args: &[&str],
    stdin_data: &str,
) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_osh"));
    hostpath::scrub(&mut cmd)
        .args(args)
        .current_dir(home.path())
        .env("HOME", home.path())
        .env_remove("BASH_ENV")
        .env_remove("ENV");
    for &(k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd
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
    assert_eq!(
        err.lines().next().unwrap_or(""),
        "osh: --nope: invalid option"
    );
}

/// A bare `-` is *only* end-of-options — it is not a spelling of `-s`. bash's
/// `parse_shell_options` treats `-` exactly like `--` (both have an empty tail,
/// so the walk stops), and the shell then reads stdin merely because no operand
/// is *left* to run as a script. So the word after `-` is a filename, not a
/// command, and not `$1`.
#[test]
fn bare_dash_is_end_of_options_not_dash_s() {
    // Alone: nothing left to run, so stdin — and no positional parameters.
    let (out, _err, code) = run_osh(&["-"], "echo \"[$1]\"\n");
    assert_eq!(out, "[]\n");
    assert_eq!(code, 0);

    // With operands, the first is a *script path*; the rest are its arguments.
    // bash: `bash - aa bb` fails to open `aa` (127), having set $1=bb.
    let (_out, err, code) = run_osh(&["-", "nosuch_script_xyz", "bb"], "");
    assert_eq!(code, 127, "the operand must be opened as a script: {err:?}");
    assert!(
        err.contains("nosuch_script_xyz"),
        "error should name it: {err:?}"
    );

    // Same word after `--`, same outcome: the two are interchangeable here.
    let (_out, _err, code) = run_osh(&["--", "nosuch_script_xyz"], "");
    assert_eq!(code, 127);
}

/// A named script really does receive the operands, and `-`'s script form is
/// the ordinary one: `$0` is the path and `$@` the words after it.
#[test]
fn bare_dash_script_gets_dollar0_and_args() {
    let home = TempHome::new("dash-script");
    home.write("s.sh", "echo \"[$0] [$*]\"\n");
    let (out, err, code) = run_osh_in(&home, &["-", "s.sh", "aa", "bb"], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "[s.sh] [aa bb]\n");
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
    let (out, _err, _code) = run_osh(
        &["-s"],
        "echo A$LINENO\nif true\nthen\necho B$LINENO\nfi\necho C$LINENO\n",
    );
    assert_eq!(out, "A1\nB4\nC6\n");

    // A function body reports the line it was defined on, wherever it is run.
    let (out, _err, _code) = run_osh(&["-s"], "f() {\necho B$LINENO\n}\necho C$LINENO\nf\n");
    assert_eq!(out, "C4\nB2\n");

    // Runtime diagnostics carry the same number.
    let (_out, err, _code) = run_osh(&["-s"], "echo one\nnosuchcmd_xyz_123\n");
    let sh = shell_name();
    assert!(
        err.starts_with(&format!("{sh}: line 2: nosuchcmd_xyz_123:")),
        "diagnostic should name line 2: {err:?}"
    );

    // …and so does a syntax error, along with its echoed source line.
    let (_out, err, _code) = run_osh(&["-s"], "echo one\necho two )\n");
    assert_eq!(
        err,
        format!(
            "{sh}: line 2: syntax error near unexpected token `)'\n{sh}: line 2: `echo two )'\n"
        )
    );

    // An unterminated quote is reported on the stream line it opened on, after
    // the complete lines before it have run.
    let (out, err, _code) = run_osh(&["-s"], "echo one\necho two\nv='abc\n");
    assert_eq!(out, "one\ntwo\n");
    assert_eq!(
        err,
        format!("{sh}: line 3: unexpected EOF while looking for matching `''\n")
    );
}

/// A `\<newline>` typed at a REPL prompt must reach the *lexer* intact. The
/// read loop only decides whether more input is coming (an odd number of
/// trailing backslashes means the pair is live); it must not splice the pair
/// out itself, because the pair is not always a continuation and because
/// replacing it with a bare newline split one command into two. Expectations
/// are bash 5.2.37's output for the same stdin.
#[test]
fn stdin_repl_leaves_line_continuations_to_the_lexer() {
    // The joined line is ONE command, not `echo a` followed by `b`.
    let (out, _err, _code) = run_osh(&["-s"], "echo a\\\nb\n");
    assert_eq!(out, "ab\n");

    // Inside single quotes the backslash and the newline are both literal.
    let (out, _err, _code) = run_osh(&["-s"], "echo 'x\\\ny'\n");
    assert_eq!(out, "x\\\ny\n");

    // Inside double quotes the pair *is* a continuation and vanishes.
    let (out, _err, _code) = run_osh(&["-s"], "echo \"x\\\ny\"\n");
    assert_eq!(out, "xy\n");

    // An even count is an escaped backslash, not a continuation: the command
    // ends at the newline.
    let (out, _err, _code) = run_osh(&["-s"], "echo a\\\\\necho done\n");
    assert_eq!(out, "a\\\ndone\n");

    // An unquoted here-doc body honours the continuation too.
    let (out, _err, _code) = run_osh(&["-s"], "cat <<E\na\\\nb\nE\n");
    assert_eq!(out, "ab\n");
}

// ---------------------------------------------------------------------------
// The letter pass (bash's `parse_shell_options`)
// ---------------------------------------------------------------------------

/// `case 'c'` and `case 's'` only *set a flag* — they do not stop the argv walk.
/// So options may follow the mode letter in later words, and the command string
/// is whatever word the cursor has reached by the end.
#[test]
fn a_mode_letter_does_not_end_the_option_walk() {
    // `-c -x cmd`: the `-x` is an option, not the command; the command traces.
    let (out, err, code) = run_osh(&["-c", "-x", "echo hi"], "");
    assert_eq!(out, "hi\n");
    assert!(err.contains("echo hi"), "xtrace trace missing: {err:?}");
    assert_eq!(code, 0);

    // `-s -x`: `-x` is applied rather than becoming $1.
    let (out, err, code) = run_osh(&["-s", "-x"], "echo \"[$1]\"\n");
    assert_eq!(out, "[]\n");
    assert!(err.contains("echo"), "xtrace trace missing: {err:?}");
    assert_eq!(code, 0);
}

/// `-c` and `-s` are independent flags, not one setting, and a pending command
/// string is looked for *before* `read_from_stdin` — so `-c` wins either way
/// round and the stdin script is never read.
#[test]
fn dash_c_beats_dash_s_in_either_order() {
    for args in [["-cs", "echo from-c"], ["-sc", "echo from-c"]] {
        let (out, err, code) = run_osh(&args, "echo from-stdin\n");
        assert_eq!(out, "from-c\n", "for {args:?}: {err:?}");
        assert_eq!(code, 0);
    }
}

/// `-o`/`-O` are letters like any other, so they bundle; each takes the *next
/// word* (never the rest of its own cluster) and advances the cursor.
#[test]
fn named_option_letters_bundle_and_take_the_next_word() {
    let (out, err, code) = run_osh(&["-eo", "pipefail", "-c", "shopt -op pipefail"], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "set -o pipefail\n");

    // Two in one cluster consume two following words, in order.
    let (out, err, code) = run_osh(
        &["-oo", "pipefail", "xtrace", "-c", "shopt -op pipefail"],
        "",
    );
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "set -o pipefail\n");

    // `-O`/`+O` do the same for shopt names.
    let (out, err, code) = run_osh(&["-O", "extglob", "-c", "shopt -p extglob"], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "shopt -s extglob\n");
    // `shopt -p` reports a *disabled* option with status 1, so only the text is
    // interesting here — bash does the same.
    let (out, err, _code) = run_osh(
        &["+O", "expand_aliases", "-c", "shopt -p expand_aliases"],
        "",
    );
    assert_eq!(out, "shopt -u expand_aliases\n", "stderr: {err:?}");
}

/// Because each `-o` seen advances the cursor, the cursor is also what decides
/// where a *bundled* `-c`'s command string starts.
#[test]
fn the_cursor_o_advances_is_where_a_bundled_command_starts() {
    let (out, err, code) = run_osh(&["-oc", "pipefail", "echo ok; shopt -op pipefail"], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "ok\nset -o pipefail\n");
}

/// With no word left, `-o`/`-O` *list* the options instead of failing, and the
/// shell carries on to run whatever it was going to run.
#[test]
fn a_trailing_option_letter_lists_instead_of_failing() {
    let (out, err, code) = run_osh(&["-s", "-o"], "echo ran-on\n");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert!(out.contains("braceexpand"), "no listing: {out:?}");
    assert!(out.ends_with("ran-on\n"), "shell did not carry on: {out:?}");

    // `+o` gives the re-inputtable form; `-O`/`+O` the shopt equivalents.
    let (out, _err, code) = run_osh(&["-s", "+o"], "");
    assert_eq!(code, 0);
    assert!(
        out.contains("set -o braceexpand"),
        "not re-inputtable: {out:?}"
    );
    let (out, _err, code) = run_osh(&["-s", "-O"], "");
    assert_eq!(code, 0);
    assert!(out.contains("expand_aliases"), "no shopt listing: {out:?}");
    let (out, _err, code) = run_osh(&["-s", "+O"], "");
    assert_eq!(code, 0);
    assert!(out.contains("shopt -"), "not re-inputtable: {out:?}");
}

/// A bad *name*, on the other hand, is fatal — and before the command runs.
#[test]
fn a_bad_option_name_is_fatal_before_the_command_runs() {
    let (out, err, code) = run_osh(&["-o", "bogus", "-c", "echo not-reached"], "");
    assert_ne!(code, 0);
    assert_eq!(out, "", "the command must not have run");
    assert!(err.contains("bogus"), "error should name it: {err:?}");

    let (out, err, code) = run_osh(&["-O", "bogus", "-c", "echo not-reached"], "");
    assert_ne!(code, 0);
    assert_eq!(out, "");
    assert!(err.contains("bogus"), "error should name it: {err:?}");
}

// ---------------------------------------------------------------------------
// The long-option pass (bash's `parse_long_options`)
// ---------------------------------------------------------------------------

/// The long options are read in an *earlier, separate* pass, so they must come
/// before every single-letter one. Once the letter parser has the cursor, a
/// following `--norc` is just a word starting with `-`, and its empty tail ends
/// the walk — bash reports `--`, not `--norc`.
#[test]
fn long_options_must_precede_every_letter_option() {
    let (_out, err, code) = run_osh(&["--norc", "-c", "echo ok"], "");
    assert_eq!(code, 0, "the accepted order must work: {err:?}");

    let (_out, err, code) = run_osh(&["-x", "--norc", "-c", "true"], "");
    assert_eq!(code, 2);
    assert!(err.contains("--: invalid option"), "expected `--`: {err:?}");
}

/// One dash is as good as two for a long option name.
#[test]
fn a_single_dash_long_option_is_accepted() {
    let (out, err, code) = run_osh(&["-norc", "-c", "echo ok"], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "ok\n");
}

/// A missing argument names the option *without* its dashes and prints no usage
/// summary — the long pass's diagnostics differ from the letter parser's.
#[test]
fn a_long_option_missing_its_argument_names_it_undashed() {
    for name in ["rcfile", "init-file"] {
        let (_out, err, code) = run_osh(&[&format!("--{name}")], "");
        assert_eq!(code, 2, "for --{name}");
        assert_eq!(err, format!("osh: {name}: option requires an argument\n"));
    }
}

/// An unmatched *two*-dash word is fatal with a usage summary, but an unmatched
/// *one*-dash word merely ends the long pass and falls through to the letters.
#[test]
fn an_unmatched_long_option_is_fatal_only_with_two_dashes() {
    let (_out, err, code) = run_osh(&["--bogus"], "");
    assert_eq!(code, 2);
    assert!(err.starts_with("osh: --bogus: invalid option"), "{err:?}");
    assert!(err.contains("Usage:"), "usage summary missing: {err:?}");

    // `-xz`, not `-bogus`: the letter parser would read `bogus`'s `o` as `-o`
    // and eat the next word, which is a different case entirely.
    let (_out, err, code) = run_osh(&["-xz"], "");
    assert_eq!(code, 2);
    assert!(err.starts_with("osh: -z: invalid option"), "{err:?}");
}

/// `--version`/`--help` are *recorded* by the pass and acted on after it ends,
/// so they win over anything after them but not over an error before them.
#[test]
fn version_and_help_win_over_later_words_but_not_earlier_errors() {
    let (out, _err, code) = run_osh(&["--version", "-q"], "");
    assert_eq!(code, 0);
    assert!(!out.is_empty(), "version not printed");

    let (out, _err, code) = run_osh(&["--help", "-q"], "");
    assert_eq!(code, 0);
    assert!(out.contains("Usage:"), "help not printed: {out:?}");

    // The error comes first in argv order, so it wins.
    let (_out, err, code) = run_osh(&["--bogus", "--version"], "");
    assert_eq!(code, 2);
    assert!(err.contains("--bogus"), "{err:?}");
}

// ---------------------------------------------------------------------------
// Startup files
// ---------------------------------------------------------------------------

/// An interactive non-login shell reads `~/.bashrc` — and only it.
#[test]
fn an_interactive_shell_reads_bashrc() {
    let home = TempHome::new("bashrc");
    home.write(".bashrc", "echo from-bashrc\n");
    home.write(".bash_profile", "echo from-profile\n");
    let (out, err, code) = run_osh_in(&home, &["-i", "-c", "echo cmd"], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "from-bashrc\ncmd\n");

    // `--norc` skips it; `--rcfile` replaces it.
    let (out, _err, _code) = run_osh_in(&home, &["--norc", "-i", "-c", "echo cmd"], "");
    assert_eq!(out, "cmd\n");
    home.write("my.rc", "echo from-my-rc\n");
    for opt in ["--rcfile", "--init-file"] {
        let (out, err, code) = run_osh_in(&home, &[opt, "my.rc", "-i", "-c", "echo cmd"], "");
        assert_eq!(code, 0, "{opt}: {err:?}");
        assert_eq!(out, "from-my-rc\ncmd\n", "for {opt}");
    }
}

/// A non-interactive shell reads neither, whatever `--rcfile` says.
#[test]
fn a_non_interactive_shell_reads_no_rc_file() {
    let home = TempHome::new("norc");
    home.write(".bashrc", "echo from-bashrc\n");
    home.write("my.rc", "echo from-my-rc\n");
    let (out, err, code) = run_osh_in(&home, &["--rcfile", "my.rc", "-c", "echo cmd"], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "cmd\n");
}

/// A login shell reads the *first* profile that exists and never `~/.bashrc`.
#[test]
fn a_login_shell_reads_the_first_profile_and_never_bashrc() {
    let home = TempHome::new("login");
    home.write(".bashrc", "echo from-bashrc\n");
    home.write(".bash_login", "echo from-bash_login\n");
    home.write(".profile", "echo from-profile\n");

    // `.bash_profile` absent, so `.bash_login` is next.
    let (out, err, code) = run_osh_in(&home, &["-l", "-c", "echo cmd"], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "from-bash_login\ncmd\n");

    // Adding `.bash_profile` pre-empts it.
    home.write(".bash_profile", "echo from-bash_profile\n");
    let (out, _err, _code) = run_osh_in(&home, &["-l", "-c", "echo cmd"], "");
    assert_eq!(out, "from-bash_profile\ncmd\n");

    // `--noprofile` skips them all, and still no `.bashrc`.
    let (out, _err, _code) = run_osh_in(&home, &["--noprofile", "-l", "-c", "echo cmd"], "");
    assert_eq!(out, "cmd\n");
}

/// `$BASH_ENV` is for *non-interactive* shells, and is read after any profile.
/// Its value is expanded as if double-quoted: substitutions yes, splitting and
/// globbing no.
#[test]
fn bash_env_is_read_by_non_interactive_shells_only() {
    let home = TempHome::new("bashenv");
    home.write("env.sh", "echo from-env\n");
    home.write(".bashrc", "echo from-bashrc\n");

    let (out, err, code) = run_osh_env(&home, &[("BASH_ENV", "./env.sh")], &["-c", "echo cmd"], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "from-env\ncmd\n");

    // Interactive: the rc file instead, never $BASH_ENV.
    let (out, _err, _code) = run_osh_env(
        &home,
        &[("BASH_ENV", "./env.sh")],
        &["-i", "-c", "echo cmd"],
        "",
    );
    assert_eq!(out, "from-bashrc\ncmd\n");

    // A parameter expansion in the value is performed; the word is not split.
    home.write("e nv.sh", "echo from-spaced\n");
    let (out, _err, _code) = run_osh_env(
        &home,
        &[("V", "nv"), ("BASH_ENV", "./e${V}.sh")],
        &["-c", "true"],
        "",
    );
    assert_eq!(out, "from-env\n");
    let (out, _err, _code) = run_osh_env(&home, &[("BASH_ENV", "./e nv.sh")], &["-c", "true"], "");
    assert_eq!(out, "from-spaced\n");

    // A missing file is silent, and so is an empty value.
    let (out, err, code) = run_osh_env(
        &home,
        &[("BASH_ENV", "./nosuch.sh")],
        &["-c", "echo cmd"],
        "",
    );
    assert_eq!((out.as_str(), code), ("cmd\n", 0), "stderr: {err:?}");
    let (out, _err, _code) = run_osh_env(&home, &[("BASH_ENV", "")], &["-c", "echo cmd"], "");
    assert_eq!(out, "cmd\n");
}

/// Startup files see `$0` and the positional parameters already set, and they
/// run before the script file is even opened.
#[test]
fn startup_files_run_before_the_script_is_opened() {
    let home = TempHome::new("beforescript");
    home.write(".bash_profile", "echo \"prof dollar0=$0 args=[$*]\"\n");
    home.write("s.sh", "echo script\n");

    let (out, err, code) = run_osh_in(&home, &["-l", "s.sh", "a", "b"], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "prof dollar0=s.sh args=[a b]\nscript\n");

    // The profile still runs when the script cannot be opened at all.
    let (out, _err, code) = run_osh_in(&home, &["-l", "nosuch_xyz.sh"], "");
    assert_eq!(code, 127);
    assert_eq!(out, "prof dollar0=nosuch_xyz.sh args=[]\n");
}

/// `return` stops a startup file but its operand is discarded (bash reads these
/// files without `FEVAL_BUILTIN`, so `return`'s status never reaches the shell);
/// `exit` in one pre-empts the command entirely and its status *is* kept.
#[test]
fn return_in_a_startup_file_discards_its_operand_but_exit_does_not() {
    let home = TempHome::new("returnexit");
    home.write(".bash_profile", "true\nreturn 5\necho not-reached\n");
    let (out, err, code) = run_osh_in(&home, &["-l", "-c", "echo \"rc=$?\""], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "rc=0\n");

    home.write(".bash_profile", "echo prof\nexit 7\n");
    let (out, _err, code) = run_osh_in(&home, &["-l", "-c", "echo not-reached"], "");
    assert_eq!(out, "prof\n");
    assert_eq!(code, 7);
}

/// `~/.bash_logout` is read from inside the `exit`/`logout` builtin, so only a
/// *login* shell leaving through that builtin reads it — never on falling off
/// the end, never for a subshell, never for a non-login shell. Being inside the
/// builtin also puts it *before* the EXIT trap and leaves `$?` at the status
/// from before the `exit`.
#[test]
fn bash_logout_is_read_only_by_a_login_shells_exit_builtin() {
    let home = TempHome::new("logout");
    home.write(".bash_logout", "echo \"logout rc=$?\"\n");

    let (out, err, code) = run_osh_in(
        &home,
        &[
            "--noprofile",
            "-l",
            "-c",
            "trap 'echo trap' EXIT; false; exit 5",
        ],
        "",
    );
    assert_eq!(code, 5, "stderr: {err:?}");
    assert_eq!(out, "logout rc=1\ntrap\n");

    // The `logout` builtin is the same path.
    let (out, _err, code) = run_osh_in(&home, &["--noprofile", "-l", "-c", "true; logout 6"], "");
    assert_eq!(out, "logout rc=0\n");
    assert_eq!(code, 6);

    // Falling off the end is not an exit; nor is a subshell's.
    let (out, _err, _code) = run_osh_in(&home, &["--noprofile", "-l", "-c", "echo end"], "");
    assert_eq!(out, "end\n");
    let (out, _err, _code) = run_osh_in(
        &home,
        &["--noprofile", "-l", "-c", "(exit 3); echo after"],
        "",
    );
    assert_eq!(out, "after\n");

    // And a non-login shell never reads it.
    let (out, _err, code) = run_osh_in(&home, &["--noprofile", "-c", "exit 4"], "");
    assert_eq!(out, "");
    assert_eq!(code, 4);

    // A failing command in it does not change the status; an `exit` does.
    home.write(".bash_logout", "echo lo\nfalse\n");
    let (_out, _err, code) = run_osh_in(&home, &["--noprofile", "-l", "-c", "exit 4"], "");
    assert_eq!(code, 4);
    home.write(".bash_logout", "echo lo\nexit 9\n");
    let (_out, _err, code) = run_osh_in(&home, &["--noprofile", "-l", "-c", "exit 4"], "");
    assert_eq!(code, 9);
}

/// An rc file that is a directory is reported, and the shell carries on.
#[test]
fn a_directory_rc_file_is_reported_and_survived() {
    let home = TempHome::new("dirrc");
    std::fs::create_dir_all(home.path().join("dir.rc")).expect("mkdir");
    let (out, err, code) = run_osh_in(&home, &["--rcfile", "dir.rc", "-i", "-c", "echo cmd"], "");
    assert_eq!(code, 0, "the shell must carry on: {err:?}");
    assert_eq!(out, "cmd\n");
    assert!(err.contains("is a directory"), "not reported: {err:?}");
}

// ---------------------------------------------------------------------------
// Interactivity (bash's `interactive_shell` vs `interactive`)
// ---------------------------------------------------------------------------

/// `$-`'s `i`, `H` and `s` letters across every way of starting a shell.
///
/// These three are the observable face of the two globals bash keeps apart:
/// `i` is `interactive_shell` (*how the shell was started*), `s` is
/// `read_from_stdin`, and `H` merely *defaults* to `interactive_shell`. Crucially
/// `i` is not a function of the mode — `-i -c cmd` really is an interactive shell
/// running a `-c` string — which is what osh used to get wrong.
///
/// Every expectation below was measured with
/// `bash --norc --noprofile <args>` on this machine; all eleven agree
/// byte-for-byte with osh. Note the absence of `m`: job control cannot be
/// enabled when stdio is a pipe, so even `-i` does not add it.
#[test]
fn dollar_dash_reports_how_the_shell_was_started() {
    let home = TempHome::new("dashi");
    home.write("d.sh", "echo $-\n");
    let cmd = "echo $-";
    for (args, want) in [
        // A `-c` string is not interactive on its own…
        (vec!["-c", cmd], "hBc"),
        // …but `-i` makes it one, `H` following along.
        (vec!["-i", "-c", cmd], "hiBHc"),
        // `-s` alongside `-c` sets `read_from_stdin` too, so both letters show.
        (vec!["-cs", cmd], "hBcs"),
        (vec!["-i", "-cs", cmd], "hiBHcs"),
        // `-H`/`+H` override the interactivity-derived default either way.
        (vec!["-H", "-c", cmd], "hBHc"),
        (vec!["-i", "+H", "-c", cmd], "hiBc"),
        // A script file: not interactive, and no `s` — there is an operand.
        (vec!["d.sh"], "hB"),
        (vec!["-i", "d.sh"], "hiBH"),
    ] {
        let (out, err, code) = run_osh_in(&home, &args, "");
        assert_eq!(code, 0, "{args:?}: {err:?}");
        assert_eq!(out, format!("{want}\n"), "for {args:?}");
    }
    // Reading commands from stdin sets `s` however it was asked for — `-s`, a
    // bare `-`, or simply having no operand. None of the three is interactive
    // here, because the test harness gives the shell a pipe, not a terminal.
    for args in [vec!["-s"], vec!["-"], vec![]] {
        let (out, err, code) = run_osh_in(&home, &args, "echo $-\n");
        assert_eq!(code, 0, "{args:?}: {err:?}");
        assert_eq!(out, "hBs\n", "for {args:?}");
    }
}

/// The two other things `-i` changes, both measured against bash: aliases expand
/// by default, and diagnostics lose bash's `line N:` token.
#[test]
fn interactivity_reaches_aliases_and_diagnostics() {
    let home = TempHome::new("iface");
    let src = "alias g='echo hi'\ng";

    // Non-interactive: `expand_aliases` is off, so `g` is not a command…
    let (out, err, code) = run_osh_in(&home, &["--norc", "-c", src], "");
    assert_eq!(out, "");
    assert_eq!(code, 127);
    // …and the diagnostic carries the line number.
    let sh = shell_name();
    assert_eq!(err, format!("{sh}: line 2: g: command not found\n"));

    // Interactive: the alias expands, so nothing is reported at all.
    let (out, err, code) = run_osh_in(&home, &["--norc", "-i", "-c", src], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "hi\n");
    assert_eq!(err, "");

    // The dropped `line N:` token is a property of interactivity, not of the
    // alias: an interactive shell reports a missing command without it.
    let (_out, err, code) = run_osh_in(&home, &["--norc", "-i", "-c", "nosuchcmd_zz"], "");
    assert_eq!(code, 127);
    assert_eq!(err, format!("{sh}: nosuchcmd_zz: command not found\n"));

    // `shopt`/`$SHELLOPTS` agree with the behaviour: `expand_aliases`,
    // `histexpand` and `history` are all on for an interactive shell. (bash also
    // lists `emacs`; osh has no line editor, so it truthfully does not.)
    let (out, err, code) = run_osh_in(
        &home,
        &[
            "--norc",
            "-i",
            "-c",
            "shopt -p expand_aliases; echo \"$SHELLOPTS\"",
        ],
        "",
    );
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(
        out,
        "shopt -s expand_aliases\n\
         braceexpand:hashall:histexpand:history:interactive-comments\n"
    );
}

/// `\#` and `\s` name the shell and the command it is running, and both depend
/// on *how the shell was invoked* — which is why they are pinned here rather
/// than in the corpus, whose cases are always script files.
///
/// `\#` is bash's `current_command_number`, advanced by the reader loop
/// (`eval.c`'s `reader_loop`) once per top-level command. A `-c` string never
/// goes through that loop — it is handed to `parse_and_execute` — so the counter
/// is never touched and `\#` reads 0 for every command in the string. Reading
/// the same commands from stdin *does* go through the reader loop and counts.
#[test]
fn prompt_command_number_counts_only_the_readers_own_commands() {
    // `-c`: the counter stays at its initial value for the whole string, so the
    // second command reads the same 0 as the first.
    let (out, err, code) = run_osh(&["-c", r#"p='\#'; echo "${p@P}"; echo "${p@P}""#], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "0\n0\n");

    // stdin: the same commands, read one at a time, are counted. The two
    // assignments are commands too, so the first `echo` is the third — and the
    // number is the one being run, not the one being read next.
    let (out, err, code) = run_osh(&[], "p='\\#'\nq=1\necho \"${p@P}\"\necho \"${p@P}\"\n");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "3\n4\n");
}

/// `\s` is bash's `shell_name` — the name the shell was *started* under — and
/// not `$0`, which names whatever it is running. The two coincide for a `-c`
/// *name* operand, which sets both, and diverge when `$0` moves on its own.
/// `\s` shows the base name (`base_pathname(shell_name)`), so an invocation by
/// path still reports a bare name.
#[test]
fn prompt_shell_name_is_the_name_the_shell_started_under() {
    let sh = shell_name();
    let base = Path::new(sh)
        .file_name()
        .expect("binary has a file name")
        .to_str()
        .expect("binary name is UTF-8");

    // No name operand: `$0` is the path cargo invoked, `\s` its base name.
    let (out, err, code) = run_osh(&["-c", r#"s='\s'; echo "${s@P} | $0""#], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, format!("{base} | {sh}\n"));

    // A `-c` name operand is the shell's own name, so both follow it.
    let (out, err, code) = run_osh(&["-c", r#"s='\s'; echo "${s@P} | $0""#, "zzname"], "");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, "zzname | zzname\n");

    // Reading from stdin does not name anything, so `\s` still answers for the
    // shell itself.
    let (out, err, code) = run_osh(&[], "s='\\s'\necho \"${s@P} | $0\"\n");
    assert_eq!(code, 0, "stderr: {err:?}");
    assert_eq!(out, format!("{base} | {sh}\n"));
}

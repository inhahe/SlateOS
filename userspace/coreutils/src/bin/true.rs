//! `true` — do nothing, successfully.
//!
//! A port of GNU coreutils 9.4's `src/true.c`, measured against the real
//! binary. The shipped version returned 0 and nothing else, which is the whole
//! of what `true` *does* but not the whole of what it *is*: `true --help`
//! printed nothing and `true --version` printed nothing, so neither the option
//! that documents the program nor the one that identifies the build existed.
//!
//! This file and `false.rs` are one program with two exit statuses — upstream
//! literally compiles `false.c` as `#define EXIT_STATUS EXIT_FAILURE` followed
//! by `#include "true.c"`. **Any change here belongs in `false.rs` too**; the
//! two are kept as separate files because every binary in this tree is
//! self-contained, and each carries its own copy of the tests so a one-sided
//! edit fails.
//!
//! # The option parsing is not getopt, and that is upstream's decision
//!
//! Every other converted utility here reads its command line through
//! `coreutils::getopt`. This one must not, because `true` is specified to
//! ignore its arguments — a getopt would have to reject `true -x`, and
//! upstream deliberately does not:
//!
//! ```text
//! $ true -x --help --zzz; echo $?
//! 0
//! ```
//!
//! Upstream's rule, from `main`, is a single exact string comparison guarded by
//! `argc == 2`: `--help` and `--version` are recognised **only when one of them
//! is the sole argument**. Everything measured below follows from that one
//! line, and none of it would survive a getopt:
//!
//! | typed | what happens |
//! |---|---|
//! | `true --help` | the help, on stdout |
//! | `true --help extra` | nothing — two arguments, so neither is an option |
//! | `true -- --help` | nothing — `--` is just a first argument here |
//! | `true --hel` | nothing — the comparison is exact; there is no abbreviation |
//! | `true --version --help` | nothing — again two arguments |
//! | `true --zzz` | nothing, status 0 — an unknown option is not an error |
//!
//! # `false --help` prints the help and exits 1
//!
//! This looks like a bug and is not. Upstream's `usage` for these two takes the
//! program's *own* status, so the help goes to **stdout** (not stderr, the way
//! a usage *error* would) and the process still reports what `false` promises.
//! Measured: `false --help >/dev/null; echo $?` is `1`, and `false --version`
//! is `1`. The invariant a script relies on — that `false` never succeeds —
//! outranks the convention that `--help` succeeds, and upstream chose the
//! invariant. So does this.
//!
//! # An undeliverable help *does* change the status, in both directions
//!
//! Upstream registers `atexit (close_stdout)` inside the `argc == 2` branch —
//! only on the path that prints something. So:
//!
//! ```text
//! $ true >&- ; echo $?                 # nothing written, nothing to fail
//! 0
//! $ true --help >&- ; echo $?
//! true: write error: Bad file descriptor
//! 1
//! $ true --help >/dev/full ; echo $?
//! true: write error: No space left on device
//! 1
//! ```
//!
//! All three are measured. That is why [`Stream`] is constructed *inside* the
//! printing branch rather than at the top of [`run_main`]: a stream finished on
//! the silent path would make `true >&-` report a failure GNU does not, and
//! `true` returning non-zero would break every `while true; do` in existence.
//! It is also why `coreutils::guard_std_fds!` is expanded here — without it the
//! runtime substitutes /dev/null for a closed descriptor and the `EBADF` never
//! happens.
//!
//! # One deliberate difference from GNU
//!
//! GNU prints `argv[0]` verbatim in the usage, so `/usr/bin/true --help` says
//! `Usage: /usr/bin/true …` and `./true --help` says `Usage: ./true …`. Ours
//! always says `true`, which is what every other converted utility in this tree
//! does; echoing an attacker-chosen `argv[0]` into output is a habit worth not
//! having, and the canonical name is the one the reader can act on.

use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

// `true --help >&-` must fail with `EBADF`, which it can only do if the
// descriptor is genuinely closed. See `coreutils::stdfd`.
coreutils::guard_std_fds!();

/// The status this program is named for. `false.rs` is this file with a `1`.
const EXIT_STATUS: u8 = 0;

/// The one-line description that differs between the twins.
const PURPOSE: &str = "Exit with a status code indicating success.";

/// What this program is called in its own usage. See the module docs on why
/// `argv[0]` is not used.
const NAME: &str = "true";

/// The funnel. A diagnostic that could not be written turns the earned status
/// into `exit_failure`, which is what upstream's `atexit (close_stdout)` does
/// on every exit path at once. See [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

/// Everything the utility does, so that [`main`] is only the exit path.
fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let earned = ExitCode::from(exit_code());
    let Some(text) = lone_option(&args) else {
        // Nothing is printed, so there is no output to fail to deliver, and
        // upstream registers no handler on this path. `true >&-` is 0.
        return earned;
    };
    let mut out = Stream::stdout();
    // `Stream::write_all` records rather than returns; the verdict is
    // `close_stdout`'s.
    let _ = out.write_all(&text);
    // `earned`, not `SUCCESS`: printing the help does not make `false` succeed.
    stdfd::close_stdout(NAME, out, earned)
}

/// `true` is defined to always succeed. Returning this from `main` (rather than
/// falling off the end) makes the contract machine-checkable.
fn exit_code() -> u8 {
    EXIT_STATUS
}

/// Upstream's `argc == 2` rule: the text to print when the *only* argument is
/// exactly `--help` or exactly `--version`, and `None` otherwise.
///
/// The comparison is against `OsStr`, not a byte conversion, so an argument
/// that is not valid UTF-8 is compared rather than mangled — it simply is not
/// equal to either name, which is the right answer.
fn lone_option(args: &[OsString]) -> Option<Vec<u8>> {
    let [only] = args else { return None };
    if only == "--help" {
        return Some(help_text().into_bytes());
    }
    if only == "--version" {
        return Some(version_text().into_bytes());
    }
    None
}

/// GNU's `--help`, minus the project's `Report bugs to:` block, as every
/// converted utility here omits it.
///
/// The `NOTE:` paragraph is kept, unlike the bug-report block, because it is
/// true on this system too and it is the only thing that explains the most
/// confusing observation a user can make about this program: typing
/// `true --help` at an interactive shell prints nothing at all, because the
/// shell's own builtin answered. `userspace/coreutils/src/bin/sh.rs` has one.
fn help_text() -> String {
    format!(
        "\
Usage: {NAME} [ignored command line arguments]
  or:  {NAME} OPTION
{PURPOSE}

      --help        display this help and exit
      --version     output version information and exit

NOTE: your shell may have its own version of {NAME}, which usually supersedes
the version described here.  Please refer to your shell's documentation
for details about the options it supports.
"
    )
}

/// The `--version` line, in this tree's form rather than GNU's five-line
/// copyright block.
fn version_text() -> String {
    format!("{NAME} (SlateOS coreutils) 0.1.0\n")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn always_zero() {
        assert_eq!(exit_code(), 0);
    }

    #[test]
    fn a_lone_help_is_the_help() {
        let text = lone_option(&argv(&["--help"])).unwrap();
        assert_eq!(text, help_text().into_bytes());
    }

    #[test]
    fn a_lone_version_is_the_version() {
        assert_eq!(
            lone_option(&argv(&["--version"])).unwrap(),
            b"true (SlateOS coreutils) 0.1.0\n"
        );
    }

    /// Upstream's `argc == 2` guard, which is the whole of its option parsing.
    #[test]
    fn an_option_is_only_an_option_when_it_is_alone() {
        for words in [
            vec!["--help", "extra"],
            vec!["extra", "--help"],
            vec!["--version", "--help"],
            vec!["--", "--help"],
        ] {
            assert!(
                lone_option(&argv(&words)).is_none(),
                "{words:?} should print nothing"
            );
        }
    }

    /// No abbreviation and no `=`: the comparison is `STREQ`, not a getopt.
    #[test]
    fn the_comparison_is_exact() {
        for word in ["--hel", "--helpx", "--help=", "-h", "--versio", "--HELP"] {
            assert!(
                lone_option(&argv(&[word])).is_none(),
                "{word} should not be recognised"
            );
        }
    }

    /// `true -x` is 0, not a usage error — the reason this file has no getopt.
    #[test]
    fn an_unknown_option_is_ignored_rather_than_refused() {
        assert!(lone_option(&argv(&["--zzz"])).is_none());
        assert!(lone_option(&argv(&["-x"])).is_none());
        assert!(lone_option(&argv(&[])).is_none());
        assert!(lone_option(&argv(&[""])).is_none());
    }

    /// An argument that is not valid UTF-8 must be compared, not decoded.
    #[test]
    fn an_undecodable_argument_is_simply_not_an_option() {
        #[cfg(unix)]
        let odd = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![0x2d, 0x2d, 0xff, 0xfe])
        };
        #[cfg(not(unix))]
        let odd = OsString::from("--\u{fffd}");
        assert!(lone_option(std::slice::from_ref(&odd)).is_none());
    }

    /// The three things the twins must not accidentally share.
    #[test]
    fn the_help_names_this_program_and_its_own_purpose() {
        let text = help_text();
        assert!(text.starts_with("Usage: true [ignored command line arguments]\n"));
        assert!(text.contains("Exit with a status code indicating success.\n\n"));
        assert!(text.contains("NOTE: your shell may have its own version of true,"));
        assert!(!text.contains("false"));
    }

    /// Both options are documented in the text that documents the program.
    #[test]
    fn the_help_lists_both_options() {
        let text = help_text();
        assert!(text.contains("      --help        display this help and exit\n"));
        assert!(text.contains("      --version     output version information and exit\n"));
    }
}

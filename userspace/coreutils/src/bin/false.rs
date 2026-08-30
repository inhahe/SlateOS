//! `false` — do nothing, unsuccessfully.
//!
//! Upstream is not a separate program: `src/false.c` is three lines,
//! `#define EXIT_STATUS EXIT_FAILURE` followed by `#include "true.c"`. This
//! file is `true.rs` with `EXIT_STATUS`, `PURPOSE` and `NAME` changed and
//! nothing else, and **any change to one belongs in the other**. The long
//! explanation of *why* the parsing looks like this lives in `true.rs`; the
//! summary is that upstream recognises `--help` and `--version` only when one
//! of them is the sole argument, by exact string comparison, and uses no getopt
//! at all — because `false -x` must be status 1, not a usage error.
//!
//! # The one thing that surprises people
//!
//! `false --help` prints the help **on stdout** and exits **1**; so does
//! `false --version`. Both are measured against GNU coreutils 9.4. It is not a
//! bug: upstream's `usage` for these two takes the program's own status, and
//! the invariant a script relies on — that `false` never succeeds — outranks
//! the convention that `--help` succeeds. A caller that does
//! `false --version && …` is asking for the wrong thing, and gets it.
//!
//! Because 1 is already this program's status, the write-failure path is
//! invisible here in a way it is not in `true`: `false --help >/dev/full` says
//! `false: write error: No space left on device` and exits 1, which is also
//! what it exits with when the write succeeds. The diagnostic is the only
//! difference, which is exactly why it has to be emitted.

use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

// `false --version >&-` must fail with `EBADF`, which it can only do if the
// descriptor is genuinely closed. See `coreutils::stdfd`.
coreutils::guard_std_fds!();

/// The status this program is named for. `true.rs` is this file with a `0`.
const EXIT_STATUS: u8 = 1;

/// The one-line description that differs between the twins.
const PURPOSE: &str = "Exit with a status code indicating failure.";

/// What this program is called in its own usage. See `true.rs`'s module docs on
/// why `argv[0]` is not used.
const NAME: &str = "false";

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
        // upstream registers no handler on this path. `false >&-` is 1, with
        // no diagnostic.
        return earned;
    };
    let mut out = Stream::stdout();
    // `Stream::write_all` records rather than returns; the verdict is
    // `close_stdout`'s.
    let _ = out.write_all(&text);
    // `earned`, not `SUCCESS`: printing the help does not make `false` succeed.
    stdfd::close_stdout(NAME, out, earned)
}

/// `false` is defined to always fail. Returning this from `main` (rather than
/// calling `process::exit` inline) makes the contract machine-checkable.
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
/// `false --help` at an interactive shell prints nothing at all, because the
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
    fn always_one() {
        assert_eq!(exit_code(), 1);
    }

    #[test]
    fn nonzero_so_shell_treats_as_failure() {
        // Sanity: any non-zero would be a "failure" exit; we specifically
        // promise exit 1 to match POSIX `false`.
        assert_ne!(exit_code(), 0);
    }

    /// The measured surprise: printing the help does not make this succeed.
    #[test]
    fn the_help_does_not_change_the_status() {
        assert!(lone_option(&argv(&["--help"])).is_some());
        assert!(lone_option(&argv(&["--version"])).is_some());
        assert_eq!(exit_code(), 1);
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
            b"false (SlateOS coreutils) 0.1.0\n"
        );
    }

    /// Upstream's `argc == 2` guard, which is the whole of its option parsing.
    #[test]
    fn an_option_is_only_an_option_when_it_is_alone() {
        for words in [
            vec!["--help", "extra"],
            vec!["extra", "--help"],
            vec!["--version", "--help"],
            vec!["--", "--version"],
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

    /// `false -x` is 1, not a usage error — the reason this file has no getopt.
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
        assert!(text.starts_with("Usage: false [ignored command line arguments]\n"));
        assert!(text.contains("Exit with a status code indicating failure.\n\n"));
        assert!(text.contains("NOTE: your shell may have its own version of false,"));
        assert!(!text.contains("true"));
    }

    /// Both options are documented in the text that documents the program.
    #[test]
    fn the_help_lists_both_options() {
        let text = help_text();
        assert!(text.contains("      --help        display this help and exit\n"));
        assert!(text.contains("      --version     output version information and exit\n"));
    }
}

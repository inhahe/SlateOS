#![deny(clippy::all)]
// The workspace's defensive lints exist to stop a shaped input turning into a
// panic in a caller's process. A test has no caller and no attacker: its
// inputs are literals in the file below it, and a `.unwrap()` that fires is
// the assertion doing its job. Per CLAUDE.md, enabled in production code and
// allowed under `test`.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )
)]
// `needless_doctest_main` is right about what it measures and wrong here.
// It fires because rustdoc already wraps a doctest body in `fn main`, so
// writing one is redundant *to the compiler*. The `fn main` in "How to use
// it" below is not addressed to the compiler: the whole instruction is
// **where the line goes** — first statement, inside the caller's existing
// `main` — and deleting the wrapper deletes the sentence. The alternatives
// were worse: `text` would stop the example being compiled at all, and this
// is the one place `guard`'s real signature is checked against its
// documentation.
#![allow(clippy::needless_doctest_main)]

//! One place for "this command is not implemented on SlateOS".
//!
//! # The defect this exists to close
//!
//! Most of the commands under `userspace/` are props. They print a
//! convincing answer — a file's duration, a disk's size, `Frames: 60 FPS:
//! 60.0`, `Result: PASS` — while containing no filesystem call, no socket, no
//! subprocess and no hardware access of any kind, and then they **exit 0**.
//! `scripts/audit-cli-fabrication.py` counts 2288 of 2756 such crates.
//!
//! Exiting 0 is the whole bug. A stub that refuses is harmless; every caller
//! already knows what to do with a failure. A stub that reports a plausible
//! result and succeeds is indistinguishable from the real tool to a shell
//! script, a `Makefile`, a package build, or a person:
//!
//! ```text
//! $ ffmpeg -i holiday.mov holiday.mp4 && rm holiday.mov
//! frame= 9703 fps=120 q=28.0 size=  45056kB time=00:05:23.43
//! $ ls holiday.*
//! ls: holiday.*: No such file or directory
//! ```
//!
//! The rule, from lane C's audit in
//! `requests/c-b-2288-userspace-tools-report-success-for-work-they-never-did.md`:
//! **a tool that did not do the thing must not exit 0.**
//!
//! # How to use it
//!
//! One line, the first statement of `main`, byte-identical in every crate:
//!
//! ```no_run
//! fn main() {
//!     notimpl::guard(env!("CARGO_PKG_NAME"));
//!     // ...the crate's existing body, now unreachable for real work
//! }
//! ```
//!
//! When the command is genuinely implemented, delete that one line. Nothing
//! else about the crate has to change, in either direction.
//!
//! # Three decisions worth stating, because each has a plausible alternative
//!
//! **The guard runs before the fabricated output, not after it.** So the
//! canned text is never printed for a real invocation — this is lane C's
//! "option A", refusal, rather than "option B", print-it-marked-and-fail. It
//! is achieved without deleting anything, so the sample output survives in
//! the source as a specification for whoever implements the command. That is
//! why the fabricating code is left unreachable rather than removed: it is
//! the most detailed statement anyone has written of what the command is
//! supposed to print, and it is worthless only once it is replaced.
//!
//! **`--help` and `--version` are allowed through.** They are reports about
//! the program itself, which the program does know, so they are honest and
//! stay at exit 0. This is the same carve-out the audit script makes, and it
//! is load-bearing: it is what lets the refusal point at `--help` for the
//! interface the command will have, instead of being a dead end.
//!
//! **An empty argument list is refused, not allowed.** Tempting to let it
//! through so `bzip2` with no arguments still prints its usage — but the pure
//! reporters are exactly the commands that take no arguments, and
//! `vulkaninfo` with an empty argv is the fabrication. Allowing empty argv
//! would exempt the entire class. The refusal names `--help` instead.
//!
//! # What this is not
//!
//! Not a way to make a command *look* finished, and not a substitute for
//! implementing it. A crate carrying a `guard` call is a crate that does
//! nothing, and `scripts/audit-cli-fabrication.py` will still count it —
//! correctly, because the fabricated source is still there. What changes is
//! that it can no longer lie to a caller about having worked.

use std::env;
use std::ffi::OsStr;
use std::path::Path;
use std::process;

/// Exit status for a command that is not implemented.
///
/// 1, and deliberately not 127. 127 is the shell's "command not found", which
/// would be a false statement: the binary is present and did run. 126 ("found
/// but not executable") is false for the same reason. 1 is the one code that
/// means nothing more specific than "this failed", which is exactly the claim
/// being made, and it is what a caller that already handles the real tool
/// failing is prepared for.
///
/// Where a stub is a *port* of a tool with its own documented failure code —
/// `chroot` uses 125, matching GNU — prefer that tool's code and do not use
/// this guard. `userspace/chroot` is the worked example.
pub const EXIT_NOT_IMPLEMENTED: i32 = 1;

/// Flags that ask the program about itself rather than asking it to work.
///
/// Kept small on purpose. Every addition is a way for a real invocation to
/// slip past the guard, and the cost of omitting one is only that
/// `--usage`-style spellings refuse instead of printing help — visible,
/// harmless, and reported the moment anyone tries it.
const SELF_QUERY: [&str; 4] = ["--help", "-h", "--version", "-V"];

/// Does this argument list only ask the program about itself?
///
/// True exactly when the list is non-empty and *every* argument is a
/// self-query flag. Split out from [`guard`] so the classification can be
/// tested without exiting the test process.
///
/// The "every" is not pedantry. `docker run --help` contains a self-query
/// flag but is a request to run a container, and a guard that let it through
/// would print a fabricated help-shaped report for a subcommand that does not
/// exist. Refusing it is the safe direction, and the refusal points at plain
/// `--help`.
#[must_use]
pub fn is_self_query<S: AsRef<OsStr>>(args: &[S]) -> bool {
    !args.is_empty()
        && args
            .iter()
            .all(|a| SELF_QUERY.iter().any(|f| a.as_ref() == OsStr::new(f)))
}

/// The name the user actually typed, for a binary with several personalities.
///
/// Many of these crates dispatch on `argv[0]` — one binary serving `bzip2`,
/// `bunzip2` and `bzcat` — so the crate name is the wrong thing to print. The
/// basename of `argv[0]` is what the user typed and what they will search for.
/// Falls back to the crate name when `argv[0]` is absent or unusable, which is
/// why callers pass `env!("CARGO_PKG_NAME")`.
///
/// Deliberately lossy in one direction only: a name that is not valid UTF-8
/// falls back rather than being rendered with replacement characters, because
/// a mangled name in a diagnostic is worse than a correct generic one.
fn invoked_as(fallback: &str) -> String {
    let Some(arg0) = env::args_os().next() else {
        return fallback.to_string();
    };
    let path = Path::new(&arg0);
    let Some(stem) = path.file_name().and_then(OsStr::to_str) else {
        return fallback.to_string();
    };
    let stem = stem.strip_suffix(".exe").unwrap_or(stem);
    if stem.is_empty() {
        fallback.to_string()
    } else {
        stem.to_string()
    }
}

/// Refuse the invocation unless it only asks the program about itself.
///
/// Returns normally for `--help` / `--version` so the crate's own text still
/// prints at exit 0. For everything else — including an empty argument list —
/// prints a refusal to **stderr** and exits [`EXIT_NOT_IMPLEMENTED`].
///
/// stderr, never stdout: stdout is where a command that filters or compresses
/// puts its payload, and a caller redirecting it must get an empty file rather
/// than prose that would corrupt the output.
///
/// `fallback` is the crate name, used only when `argv[0]` cannot supply a
/// better one. Pass `env!("CARGO_PKG_NAME")`.
pub fn guard(fallback: &str) {
    let args: Vec<std::ffi::OsString> = env::args_os().skip(1).collect();
    if is_self_query(&args) {
        return;
    }
    let name = invoked_as(fallback);
    // Two lines, and the second is the one that matters. The first says the
    // command failed; the second says what a caller must not assume happened
    // anyway, because the failure mode this whole effort exists to close is a
    // script that proceeds as though the work was done.
    eprintln!("{name}: not implemented on SlateOS");
    eprintln!("{name}: nothing was read, created, modified, or deleted");
    eprintln!("{name}: `{name} --help` describes the interface it will have");
    process::exit(EXIT_NOT_IMPLEMENTED);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn v(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn bare_self_query_flags_are_allowed() {
        assert!(is_self_query(&v(&["--help"])));
        assert!(is_self_query(&v(&["-h"])));
        assert!(is_self_query(&v(&["--version"])));
        assert!(is_self_query(&v(&["-V"])));
        assert!(is_self_query(&v(&["--help", "--version"])));
    }

    /// The case the whole guard exists for: an argument list that asks for
    /// work must never be mistaken for a self-query.
    #[test]
    fn anything_that_asks_for_work_is_refused() {
        assert!(!is_self_query(&v(&["holiday.mov"])));
        assert!(!is_self_query(&v(&["-i", "in.mov", "out.mp4"])));
        assert!(!is_self_query(&v(&["-9", "big.log"])));
        assert!(!is_self_query(&v(&["--decrypt", "-i", "key", "f.age"])));
    }

    /// A self-query flag mixed with a real request is a real request.
    /// `docker run --help` is not a question about docker.
    #[test]
    fn a_help_flag_among_real_arguments_does_not_excuse_the_rest() {
        assert!(!is_self_query(&v(&["run", "--help"])));
        assert!(!is_self_query(&v(&["--help", "big.log"])));
        assert!(!is_self_query(&v(&["-i", "in.mov", "-h"])));
    }

    /// Empty argv is refused on purpose: the pure reporters take no
    /// arguments, so `vulkaninfo` with an empty list *is* the fabrication.
    #[test]
    fn an_empty_argument_list_is_refused() {
        let empty: [&str; 0] = [];
        assert!(!is_self_query(&empty));
    }

    /// Near-misses must not be waved through. A guard that matched prefixes
    /// would let `--helpful-flag` and `-hq` past.
    #[test]
    fn lookalike_flags_are_not_self_queries() {
        assert!(!is_self_query(&v(&["--help-all"])));
        assert!(!is_self_query(&v(&["-hq"])));
        assert!(!is_self_query(&v(&["--Version"])));
        assert!(!is_self_query(&v(&["-v"])));
        assert!(!is_self_query(&v(&["help"])));
    }

    /// Arguments are not required to be UTF-8 — a path on this system may be
    /// any bytes except `/` and NUL — so the classifier compares `OsStr` and
    /// must not panic or accidentally match on undecodable input.
    #[test]
    fn non_utf8_arguments_are_refused_without_panicking() {
        #[cfg(unix)]
        let bad = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![0xff, 0xfe])
        };
        #[cfg(windows)]
        let bad = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0xd800])
        };
        assert!(!is_self_query(&[bad]));
    }

    #[test]
    fn the_exit_status_is_not_success() {
        assert_ne!(EXIT_NOT_IMPLEMENTED, 0);
    }
}

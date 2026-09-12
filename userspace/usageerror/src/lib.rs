//! `getopt`'s usage diagnostics, spelled once.
//!
//! Every program in this tree that parses options has to refuse the ones it
//! does not have, and every one of them has to say so in the same words,
//! because the words are not ours: they come out of `getopt_long`, and both
//! GNU coreutils and util-linux print them verbatim. A behavioural sweep
//! (`scripts/unknown-option-sweep.py`) found **70** programs here that
//! accepted an unrecognised option and exited 0 instead. Fixing them one at
//! a time means writing this wording out several dozen times, and a wording
//! written several dozen times is a wording that drifts -- which is exactly
//! the history behind `modechange`, `userspec` and `quoting`.
//!
//! The wordings below were measured in the C locale, not remembered:
//!
//! | Command line | What GNU prints |
//! |---|---|
//! | `nproc --zzq`   | `nproc: unrecognized option '--zzq'` |
//! | `nproc -q`      | `nproc: invalid option -- 'q'` |
//! | `nproc extra`   | `nproc: extra operand 'extra'` |
//!
//! each followed by `Try 'nproc --help' for more information.`
//!
//! # What this crate deliberately does not decide
//!
//! **The exit status**, because the two families disagree and both are
//! right: coreutils exits **1** for a usage error, util-linux exits **64**
//! (`EX_USAGE`). `flock` and `nproc` in this tree are measured against
//! different references and must keep their own answers, so the caller
//! picks. [`EX_USAGE`] is offered for the util-linux side rather than
//! guessed at each call site.
//!
//! **Whether the program name prefixes the message.** Most callers already
//! have an error path that prints `"{prog}: {e}"`, so these functions return
//! the body without it. [`with_help_pointer`] is the exception: the pointer
//! line names the program itself, because it is a *second* line and the
//! caller's prefix only reaches the first.

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;

/// The exit status util-linux uses for a usage error (`sysexits.h`).
///
/// coreutils uses 1 instead; this crate does not choose between them, see
/// the module docs.
pub const EX_USAGE: i32 = 64;

/// `unrecognized option '--zzq'` — a long option the program does not have.
#[must_use]
pub fn unrecognized_option(arg: &[u8]) -> String {
    format!("unrecognized option {}", quoting::quoteaf(arg))
}

/// `invalid option -- 'q'` — a short option the program does not have.
///
/// Takes the offending byte, not the cluster: given `-xq`, getopt reports
/// whichever letter it choked on, one at a time.
#[must_use]
pub fn invalid_option(opt: u8) -> String {
    format!("invalid option -- {}", quoting::quoteaf(&[opt]))
}

/// `extra operand 'x'` — a word where the program takes no more.
#[must_use]
pub fn extra_operand(arg: &[u8]) -> String {
    format!("extra operand {}", quoting::quoteaf(arg))
}

/// The refusal for an argument that is an option this program does not have,
/// dispatched on shape the way `getopt_long` dispatches it.
///
/// A `--` prefix is a long option and is named in full; anything else is a
/// short cluster and only its **first** letter is named, because that is the
/// one getopt stopped on. Callers should not have to remember that rule, and
/// two of them in this tree had each re-derived it.
///
/// A bare `-` and the end-of-options `--` are operands, not options, and are
/// not this function's business; the caller must exclude them first.
#[must_use]
pub fn unknown_option(arg: &[u8]) -> String {
    if arg.starts_with(b"--") {
        unrecognized_option(arg)
    } else {
        invalid_option(arg.get(1).copied().unwrap_or(b'-'))
    }
}

/// `body`, then GNU's pointer line on a second line.
///
/// The pointer names the program because it is the second line of the
/// message and a caller's `"{prog}: "` prefix only reaches the first. Pass
/// the name exactly as the caller's prefix spells it, so the two agree.
#[must_use]
pub fn with_help_pointer(name: &str, body: &str) -> String {
    format!("{body}\nTry '{name} --help' for more information.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_option_reads_as_gnu_prints_it() {
        assert_eq!(
            unrecognized_option(b"--zzq-not-an-option"),
            "unrecognized option '--zzq-not-an-option'"
        );
    }

    #[test]
    fn short_option_names_the_letter() {
        assert_eq!(invalid_option(b'q'), "invalid option -- 'q'");
    }

    #[test]
    fn extra_operand_reads_as_gnu_prints_it() {
        assert_eq!(extra_operand(b"extra"), "extra operand 'extra'");
    }

    /// The rule a caller should not have to remember: getopt names the
    /// letter it stopped on, not the cluster it was in.
    #[test]
    fn a_short_cluster_is_named_one_letter_at_a_time() {
        assert_eq!(unknown_option(b"-xq"), "invalid option -- 'x'");
    }

    #[test]
    fn shape_selects_the_wording() {
        assert_eq!(unknown_option(b"--long"), "unrecognized option '--long'");
        assert_eq!(unknown_option(b"-s"), "invalid option -- 's'");
    }

    #[test]
    fn the_pointer_is_a_second_line_naming_the_program() {
        assert_eq!(
            with_help_pointer("nproc", "extra operand 'x'"),
            "extra operand 'x'\nTry 'nproc --help' for more information."
        );
    }

    /// The option text is argv, and an argv word may hold a newline. Pasted
    /// raw it would let whoever wrote the command line append an invented
    /// line to the program's stderr, indistinguishable from a real one.
    #[test]
    fn an_option_name_cannot_add_a_line() {
        let msg = unknown_option(b"--a\nnproc: /etc/shadow: Permission denied");
        assert!(
            !msg.contains('\n'),
            "escaped option text must not introduce a line: {msg}"
        );
    }

    /// A byte that is not valid UTF-8 still has to render, because argv on
    /// this OS is bytes and any byte but `/` and NUL can appear in one.
    #[test]
    fn a_non_utf8_option_byte_still_renders() {
        let msg = unknown_option(&[b'-', 0xff]);
        assert!(!msg.is_empty());
        assert!(msg.starts_with("invalid option -- "), "{msg}");
    }
}

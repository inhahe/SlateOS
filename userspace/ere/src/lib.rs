#![deny(clippy::all)]
// The workspace's defensive lints (`unwrap_used`, `indexing_slicing`,
// `arithmetic_side_effects`, …) exist to stop a shaped input turning into a
// panic in a *caller's* process — a pattern from `grep -f`, a subject from a
// pipe. A test has no caller and no attacker: its inputs are literals in the
// file below it, and a `.unwrap()` that fires is the assertion doing its job.
// Per CLAUDE.md, they are enabled in production code and allowed under `test`.
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

//! POSIX regular expressions over byte strings — one engine for the shell and
//! for the utilities that have to agree with it.
//!
//! ## Why this is a crate
//!
//! Five programs in this tree match regular expressions, and until this crate
//! existed four of them did it by calling `str::contains`:
//!
//! | caller | wanted | had |
//! |---|---|---|
//! | `osh`'s `[[ =~ ]]` | ERE | a real ERE engine (this one, in-tree) |
//! | `grep` | BRE, and ERE under `-E` | substring search |
//! | `sed` | BRE | substring search, plus a hand-rolled `.`/`*` matcher |
//! | `awk`'s `/re/` and `~` | ERE | substring search |
//! | `expr`'s `:` | BRE anchored at the start | substring search |
//!
//! That is not four small gaps; it is one missing component, absent four times.
//! It also fails *quietly* — `grep '^posix'` finds nothing rather than
//! complaining, `sed 's/^/E:/'` copies its input through unchanged, and a
//! bracket expression matches only a line that literally contains `[ax]`. Each
//! one looks like the program working, which is how the whole family survived
//! its own test suites: every test asserted the substring behaviour it had.
//!
//! It is the same move `tzrules`, `textfmt`, `textfind` and `byteread` already
//! made in this tree, and for the strongest version of the same reason — the
//! shell and `grep` disagreeing about what `[a-z]` means is not a cosmetic
//! difference, it is `[[ $f =~ ^a ]]` and `grep '^a'` giving different answers
//! about the same file.
//!
//! `posix`'s `regcomp`/`regexec` (`posix/src/regex.rs`) deliberately stays a
//! separate implementation: it is `no_std` with fixed-size buffers and a C ABI,
//! and it answers to a different specification — POSIX's, byte-for-byte,
//! including the error codes. This crate is for the Rust programs.
//!
//! ## What is here
//!
//! * [`ch`] — the character model everything is defined over: a decoded scalar
//!   *or* one undecodable byte. `osh`'s `bytes` module re-exports it, so there
//!   is exactly one definition and the shell cannot drift from the utilities.
//! * [`engine`]'s [`Regex`] — a Pike VM (no catastrophic backtracking) for
//!   POSIX **Extended** regular expressions, with captures.
//! * [`bre`] — POSIX **Basic** regular expressions, translated to Extended and
//!   handed to the same engine. BRE is what `grep`, `sed` and `expr` use when
//!   no `-E` is given, and it is *not* a subset of ERE: `a+b` is three literal
//!   characters in BRE and a repetition in ERE, so the difference has to be
//!   handled somewhere, and one translator is better than three parsers.

pub mod bre;
pub mod ch;
pub mod engine;

pub use ch::{BStr, Ch, Str, chars, from_chars};
pub use engine::{EreError, Regex};

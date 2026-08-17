#![deny(clippy::all)]

//! The pieces more than one coreutil needs.
//!
//! Almost everything in this crate is a `src/bin/*.rs` — one file per utility,
//! standalone, because a utility that pulls in the others' machinery is harder
//! to read and no faster to build. This library is for the exceptions: the
//! things where two utilities disagreeing would itself be the bug.
//!
//! There are four so far. Three are about the interface these programs share
//! whether or not anyone designed it that way: a script that reads `grep`'s
//! diagnostic and a script that reads `cp`'s are the same script, and a person
//! who learned to type `ls --col` expects `cat --squeeze` to work too.
//!
//! - [`errmsg`] — how an I/O failure is worded.
//! - [`quote`] — how a file name, or any other untrusted text, is rendered
//!   inside a message. Not a nicety: a path may contain a newline, so a
//!   utility that prints one raw lets whoever chose the name write extra
//!   lines into its error stream.
//! - [`getopt`] — how options are *parsed*, which is the same question one
//!   layer earlier. GNU's utilities all call one `getopt_long`, so they all
//!   abbreviate long options and all word a bad one identically; ours parsed
//!   argv by hand, 85 times, so they did neither.
//!
//! The fourth is here for a different reason — not because the utilities must
//! agree with each other, but because they must all disbelieve the same lie:
//!
//! - [`filekind`] — whether an open file is a *regular* file. One line on the
//!   target and a trap on the host, where a pipe claims to be an ordinary file,
//!   reports a length, and accepts seeks it then ignores. Every utility that
//!   takes a shortcut for regular files needs this, and two have already been
//!   caught getting it wrong by hand.
//!
//! The regex engine, which is the other thing they must not disagree about,
//! lives in `userspace/ere` rather than here — the shell needs it too, and it
//! cannot depend on the coreutils. See `design-decisions.md` §322.

pub mod errmsg;
pub mod filekind;
pub mod getopt;
pub mod quote;

#![deny(clippy::all)]

//! The pieces more than one coreutil needs.
//!
//! Almost everything in this crate is a `src/bin/*.rs` — one file per utility,
//! standalone, because a utility that pulls in the others' machinery is harder
//! to read and no faster to build. This library is for the exceptions: the
//! things where two utilities disagreeing would itself be the bug.
//!
//! There are two so far, and both are about what a diagnostic *says*, because
//! that is the interface these programs share whether or not anyone designed
//! it that way: a script that reads `grep`'s diagnostic and a script that
//! reads `cp`'s are the same script.
//!
//! - [`errmsg`] — how an I/O failure is worded.
//! - [`quote`] — how a file name, or any other untrusted text, is rendered
//!   inside a message. Not a nicety: a path may contain a newline, so a
//!   utility that prints one raw lets whoever chose the name write extra
//!   lines into its error stream.
//!
//! The regex engine, which is the other thing they must not disagree about,
//! lives in `userspace/ere` rather than here — the shell needs it too, and it
//! cannot depend on the coreutils. See `design-decisions.md` §322.

pub mod errmsg;
pub mod quote;

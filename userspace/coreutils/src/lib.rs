#![deny(clippy::all)]

//! The pieces more than one coreutil needs.
//!
//! Almost everything in this crate is a `src/bin/*.rs` — one file per utility,
//! standalone, because a utility that pulls in the others' machinery is harder
//! to read and no faster to build. This library is for the exceptions: the
//! things where two utilities disagreeing would itself be the bug.
//!
//! There is exactly one such thing so far ([`errmsg`], how an I/O failure is
//! worded), and it earned its place: a script that reads `grep`'s diagnostic
//! and a script that reads `cp`'s are the same script.
//!
//! The regex engine, which is the other thing they must not disagree about,
//! lives in `userspace/ere` rather than here — the shell needs it too, and it
//! cannot depend on the coreutils. See `design-decisions.md` §322.

pub mod errmsg;

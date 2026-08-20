#![deny(clippy::all)]

//! The pieces more than one coreutil needs.
//!
//! Almost everything in this crate is a `src/bin/*.rs` — one file per utility,
//! standalone, because a utility that pulls in the others' machinery is harder
//! to read and no faster to build. This library is for the exceptions: the
//! things where two utilities disagreeing would itself be the bug.
//!
//! There are eight so far. Three are about the interface these programs share
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
//! The fifth is the shared-interface argument again, but between two utilities
//! rather than all of them:
//!
//! - [`tabstops`] — the list `expand -t` and `unexpand -t` both take. It is one
//!   file upstream (`expand-common.c`) for the reason it is one module here:
//!   the two are documented as taking the *same* option, and the grammar is
//!   intricate enough — a `/` or `+` prefix, a rule for when a prefix is no
//!   prefix at all, and a distinction between the errors that abandon the rest
//!   of the argument and the errors that don't — that two hand-written parsers
//!   would certainly disagree somewhere.
//!
//! The sixth is that argument again one layer down — not an option's grammar
//! but its *argument's*:
//!
//! - [`xnum`] — the number. `fold -w`, `head -c`, `nl -v`, `split -b` and the
//!   rest all read one through gnulib's `xstrtoumax`/`xdectoint` pair, and that
//!   grammar is much larger than "a decimal integer": multiplier suffixes with
//!   two possible bases, a bare suffix meaning one of it, and a choice between
//!   two different `strerror` sentences for out-of-range decided by a heuristic
//!   on the *value* rather than on the bound that was violated. `nl` and `head`
//!   had each already written a partial copy, disagreeing in exactly the places
//!   two partial copies would.
//!
//! The seventh is the same argument one layer further down still — past the
//! option, past its argument's grammar, to the *arithmetic* the argument names:
//!
//! - [`extfloat`] — the real number. `seq`, and after it `printf %f` and the
//!   numeric side of `sort` and `factor`, read and write reals through libc's
//!   `strtold` and `printf("%Lf")`, which on x86-64 glibc are 80-bit extended
//!   precision — 64 significand bits, not 53. Rust has no `long double`, so
//!   every utility that needed one would otherwise reach for `f64` and each be
//!   wrong in its own way. It is not a small wrongness: over 4000 random
//!   `seq FIRST INCREMENT LAST` ranges printed to 10–20 decimal places, `f64`
//!   disagreed with GNU on 1355 of them, and the disagreement was already
//!   present in the *first* line — before any arithmetic, in the round trip
//!   through the decimal literal alone. This module is that type, in software:
//!   parse, arithmetic, compare, and `printf` conversions, exact because the
//!   decimal↔binary question is answered over integers ([`bignat`]) rather
//!   than in floating point.
//!
//! The eighth is [`extfloat`]'s other half. `printf` has to answer `%d` and
//! `%s` as well as `%f`, and those conversions are not hard arithmetic — they
//! are a dozen small interacting rules (a precision is a *minimum* on an
//! integer and a *maximum* on a string; `%.0d` of zero prints nothing; `#` on
//! `%o` raises the precision instead of prepending; the `0` flag loses to `-`
//! and loses again to a precision) which nobody recalls correctly and which
//! `awk`'s `printf` and the shell's builtin will need to get right the same
//! way:
//!
//! - [`cfmt`] — the C conversions that are not floating point: `%d %i %o %u
//!   %x %X %c %s`, with the flag, width and precision handling that surrounds
//!   them. It delegates `%a %e %f %g` to [`extfloat`], so one call site covers
//!   the whole of `printf`'s directive vocabulary.
//!
//! The ninth is small enough to look as though it does not need sharing, and
//! is here because the one decision inside it is invisible until it is wrong:
//!
//! - [`shell`] — handing a command line to `sh -c`. `awk`'s `system()` and its
//!   two pipe forms, and `split --filter`, all take a shell *command* rather
//!   than an argv, so none of them may tokenise it themselves; and each has to
//!   choose what to run on a host with no `/bin/sh`, where the obvious answer —
//!   `cmd /c` — silently changes the quoting rules the script was written
//!   against rather than failing.
//!
//! The regex engine, which is the other thing they must not disagree about,
//! lives in `userspace/ere` rather than here — the shell needs it too, and it
//! cannot depend on the coreutils. See `design-decisions.md` §322.

mod bignat;
pub mod cfmt;
pub mod errmsg;
pub mod extfloat;
pub mod filekind;
pub mod getopt;
pub mod quote;
pub mod shell;
pub mod tabstops;
pub mod xnum;

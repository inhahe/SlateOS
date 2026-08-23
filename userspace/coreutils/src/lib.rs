#![deny(clippy::all)]

//! The pieces more than one coreutil needs.
//!
//! Almost everything in this crate is a `src/bin/*.rs` — one file per utility,
//! standalone, because a utility that pulls in the others' machinery is harder
//! to read and no faster to build. This library is for the exceptions: the
//! things where two utilities disagreeing would itself be the bug.
//!
//! There are thirteen so far. Three are about the interface these programs share
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
//! The tenth is the shared-interface argument applied to *output* rather than
//! input — the same number, printed the same way by everything that prints it:
//!
//! - [`human`] — `1.5G`, `1.5 GB`, `1.4 GiB`. gnulib has one
//!   `human_readable` and every size-printing utility calls it; we had three
//!   hand-rolled `human_size` functions (`df`, `du`, `ls`) that agreed with
//!   each other and with GNU on nothing but the letters. All three now call
//!   this module and the copies are gone. The rules that a `{:.1}` format
//!   string cannot express are the ones that matter: the decimal disappears at
//!   ten rather than at a byte count, rounding is done on the exact ratio and
//!   can carry up into the next prefix, and the value is a *ratio* of two
//!   block sizes — which is what lets one function serve both `df`'s
//!   512-byte-blocks-as-kibibytes and `dd`'s bytes-per-nanosecond.
//!
//!   Those are not abstract concerns. Measured against GNU, the three copies
//!   were between them wrong in four independent ways — a spurious `B` on
//!   unscaled counts, round-to-nearest where GNU rounds up, a decimal that
//!   never disappeared at ten, and no prefix above `G`, so a 5 TB file listed
//!   as `4768.4G`. `ls` additionally printed `1024.0K` for one byte under a
//!   mebibyte, because it chose the prefix before rounding and so could not
//!   carry. Each of those had a unit test asserting the wrong answer.
//!   `tests/human_gnu.rs` checks this module against ~36k renderings measured
//!   from GNU itself; see `design-decisions.md` §338 for why that fixture is
//!   committed rather than diffed live.
//!
//! The eleventh is the one where "two hand-written copies would disagree" is
//! not a prediction but a description of the code that was already here:
//!
//! - [`canon`] — turning a name into *the* name of the file it denotes:
//!   following every symlink, resolving every `..` physically, and deciding
//!   what to do when something along the way does not exist. `readlink -f`,
//!   `readlink -e`, `readlink -m` and all three of `realpath`'s corresponding
//!   modes are, upstream, a single gnulib function
//!   (`canonicalize_filename_mode`) with a three-valued parameter — and the
//!   three values differ in exactly one `if`. Our two utilities had instead
//!   each called `std::fs::canonicalize`, which is only the *first* of the
//!   three modes: it requires the whole path to exist, so `-f` and `-m` were
//!   both silently answering `-e`'s question.
//!
//!   The parts that make it worth a module rather than a function are the two
//!   rules nobody would guess and neither copy had. A name ending in `/`, in
//!   `/.`, or containing `/..` obliges the component before it to be a
//!   *directory* — and the check is not free, because the main walk never
//!   looks at those suffixes: a trailing slash yields a zero-length component,
//!   `.` is skipped, and `..` is arithmetic on the accumulated path rather
//!   than a lookup. That single rule is why GNU's `readlink -f d/sub/real/`
//!   fails with `Not a directory` while `readlink -f d/sub/missing/` succeeds.
//!   The second is that there is no depth limit at all, only cycle detection:
//!   a 300-link chain resolves here even though `cat` on the same name reports
//!   `Too many levels of symbolic links`, because a canonicaliser never hands
//!   the kernel a name with more than one link left in it. Where the loop is
//!   declared is *observable* — gnulib expands the first 20 links with no
//!   bookkeeping before it starts recording, and that constant decides which
//!   of a three-link cycle's members `-m` prints.
//!
//! The twelfth is [`canon`]'s question asked without touching the disk, and it
//! earns its place the same way [`human`] did — by being duplicated already,
//! wrongly, in code nobody suspected:
//!
//! - [`pathname`] — cutting a name into a directory part and a last component,
//!   which is gnulib's `dirname.h` family. Sixteen binaries here do this, and
//!   nearly all of them called [`std::path::Path`]'s `parent` and `file_name`.
//!   Those answer a *host* question: on the build host `\` is a separator and
//!   `C:` is a drive, so a name containing a backslash — which our filesystem
//!   permits, forbidding only `/` and NUL — was cut where the target would
//!   never cut it. The rules are also not the ones a reasonable person would
//!   write: trailing slashes are ignored rather than stripped, so `dirname a/`
//!   is `.` and not `a`; a slash run collapses only where it separates, so
//!   `dirname ////a////b////` keeps all four leading slashes; and `dirname /`
//!   is `/`, which the previous implementation's own unit test asserted was
//!   `.`.
//!
//! The thirteenth is the [`human`] story again, but with eight copies instead
//! of three and with the disagreements reaching further than the output column:
//!
//! - [`fnmatch`] — the shell glob. `find -name`, `du --exclude`,
//!   `tar --exclude` and `cpio`'s pattern list are all POSIX `fnmatch(3)`, and
//!   eight binaries here had each written their own. They agreed on `*` and `?`
//!   and on nothing else. `find`'s copy is representative: it matched `char`s,
//!   which on a filesystem whose names are arbitrary bytes means it could not
//!   be handed the name at all; its `*` recursed once per possible split, so
//!   `find . -name '*x*x*x*x*x*y'` is a hang reachable from a command line; its
//!   `[…]` was a set of literal characters, so `[a-z]` matched exactly `a`, `-`
//!   and `z` and `[!o]` matched `!` and `o` — wrong files, silently, rather
//!   than an error; and `\` was not an escape, so no pattern could name a file
//!   containing a `*`.
//!
//!   The flags are glibc's, and the two that decide what a `/` means are the
//!   reason this is a parameterised matcher rather than one function.
//!   [`Flags::PATHNAME`] and [`Flags::PERIOD`] are what a *shell's* own globbing
//!   wants and what the utilities here want **off**: `find -name` is given a
//!   single component, and `du --exclude` deliberately lets `*` cross a
//!   separator — measured, `du --exclude='*/keep'` prunes `ex/aa/keep`.
//!
//! [`Flags::PATHNAME`]: fnmatch::Flags::PATHNAME
//! [`Flags::PERIOD`]: fnmatch::Flags::PERIOD
//!
//! The regex engine, which is the other thing they must not disagree about,
//! lives in `userspace/ere` rather than here — the shell needs it too, and it
//! cannot depend on the coreutils. See `design-decisions.md` §322.

mod bignat;
pub mod canon;
pub mod cfmt;
pub mod errmsg;
pub mod extfloat;
pub mod filekind;
pub mod fnmatch;
pub mod getopt;
pub mod human;
pub mod pathname;
pub mod quote;
pub mod shell;
pub mod tabstops;
pub mod xnum;

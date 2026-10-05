## D-Q6 — [D] Some of the C library is translated from glibc, whose licence binds every program the library is built into. Keep it, or rewrite those parts? — Status: OPEN (raised 2026-09-28)

**In short:** to make the C library behave exactly as Linux's (glibc)
does, several parts of it were written by translating glibc's own source
code into Rust, line by line -- most recently the Tamil character set,
the new C23 maths functions and `clog10` -- and `<obstack.h>`'s macros
follow glibc's header's, macro for macro, and argp, the timezone code
and the system logger glibc's source, function for function. glibc's
licence (the LGPL) allows that, on a condition: anyone who receives a
program containing it
must be able to rebuild that program with their own copy of the library.
The C library is built into *every* program on SlateOS, so the condition
reaches every program, ours and anyone else's. An earlier decision
(design-decisions.md §1133) assumed the library should stay free of that
condition and chose other sources for the complex-number functions; the
translations since have not followed it. Which should hold?

**Terms used below.** *LGPL*: the licence glibc is under -- free to use and
change, but code derived from it stays under it, and a program containing
it must let the user swap in their own build of that code. *Statically
linked*: the library's code is copied into each program, as all programs
here are today. *Clean-room rewrite*: writing the code again from the
standards and from glibc's observable behaviour, without its source open --
the tests that compare us with glibc (glibc as the *oracle*) stay exactly as
they are, since running a program is not copying it.

What is translated, as far as lane D knows:

| Where | From glibc's | Since |
|---|---|---|
| `posix/src/iconv.rs`: the CP1255, CP1258 and TCVN converters' loops | `iconvdata/cp1255.c`, `cp1258.c`, `tcvn5712-1.c` | 2026-09-27, on `main` |
| `posix/src/iconv.rs`: the T.61 / ISO 6937 / ANSI X3.110 decoder | `iconvdata/t.61.c` and kin | 2026-09-28, on `main` |
| `posix/src/iconv.rs`: TSCII | `iconvdata/tscii.c` | 2026-09-28, on `main` |
| `posix/src/c23math.rs`: `nextup` ... `fminimum_mag_num`, `scalbl` | `math/`, `sysdeps/ieee754/*`, `e_scalbl.S` | 2026-09-28, on `main` |
| `posix/src/narrow.rs`: the narrowing functions' checks | `math/math-narrow.h` | 2026-09-28, on `main` |
| `posix/src/complex*.rs`: `clog10` | `math/s_clog10_template.c`, `x2y2m1` | 2026-09-28, on `main` |
| `posix/include/obstack.h`: the macros, macro for macro -- C, in a header a program compiles into itself | the installed `<obstack.h>` (`malloc/obstack.h`) | 2026-09-30 |
| `posix/src/argp/`: the parse, the help's order and layout, the line filler -- function for function | `argp/argp-parse.c`, `argp-help.c`, `argp-fmtstream.c` | 2026-10-01 |
| `posix/src/tz.rs`: `tzset`, the POSIX rule engine, the zoneinfo reader and `mktime`'s search -- function for function, read from the source | `time/tzset.c`, `time/tzfile.c`, `time/mktime.c` | 2026-10-01 |
| `posix/src/syslog.rs`: the logger -- connecting, building the record, sending it and trying again -- function for function, read from the source | `misc/syslog.c`, BSD's in origin (the University of California's licence) with glibc's changes under the LGPL | 2026-10-01 |

One more part, since this was raised, was written with glibc's source
open, though not translated from it: `posix/src/regex/parse.rs`
(2026-09-30) takes the order of glibc's `regcomp.c` checks -- which
character is special where, which error a malformed interval gets, and
what each of the GNU interface's syntax bits changes -- from reading that
file, and the oracles' cases then pin each rule: 11,250 pairs of tokens in
POSIX's two syntaxes, some 41,000 patterns in the GNU ones. Its code is not
glibc's: an explicit stack where glibc recurses, its own types, none of
glibc's lines. Under **B** it would be derived again from the oracles'
answers alone, which already fix every rule it has. (The matcher behind
it, the rest of `posix/src/regex/`, follows the standard and owes glibc's
code nothing; its fastmap, `fastmap.rs`, reaches glibc's answer by its
own reasoning over the tree, where glibc reads its automaton's states.)

The obstack functions behind `<obstack.h>`'s macros, `posix/src/obstack.rs`,
are not translated: they are held to the oracle's answers, which show every
chunk size the program's allocation function is asked for. The header is the
interface itself -- a program expands its macros, and must get what glibc's
give -- so under **B** it would be written again from the glibc manual's
description of each macro and held to the same check
(`posix/tools/oracle/obstack_harness.py --header`: both of the header's forms,
over glibc's own functions). (The LGPL, in 2.1's §5, lifts its conditions
from a program that uses only a header's data structure layouts and small
macros, ten lines or fewer -- as each of these is; whether that settles it
for this header is part of this question.)

argp, `posix/src/argp/`, was written from what glibc's argp source does, as
known rather than read -- but for argp-fmtstream.c's line-breaking scan,
read to settle a case -- and every rule held to the oracle: 175 scenarios
of glibc 2.39's. Its code is its own, a parse over raw pointers and the
crate's lists rather than glibc's structures, but its algorithms are
glibc's on purpose: help text that breaks where glibc's does needs the
same buffering and the same scan. Under **B** it would be written again
from the manual and the oracle alone, which fix the order of the calls
and the layout, though not every effect of the buffering.

The timezone code, `posix/src/tz.rs`, is glibc's `tzset.c`, `tzfile.c` and
`mktime.c` translated function by function, with glibc 2.39's source open
(2026-10-01): which zone a `TZ` value names, and what each call leaves for
the next -- `tzname`, the `posixrules` offset, `mktime`'s remembered guess
-- live in those files' details, and a program written against glibc
sees them. Under **B** it would be written again from the oracle's answers
alone (`posix/tools/oracle/tz_harness.py`, 57 scenarios), which fix every
behaviour they exercise, though not glibc's state after sequences of
calls they do not.

The system logger, `posix/src/syslog.rs`, is glibc's `misc/syslog.c`
translated function by function with its source open (2026-10-01):
what a record looks like, when the connection is made again, which
copies stop at a NUL -- a program logging through glibc sees all of it.
Under **B** it would be written again from the oracle's answers alone
(`posix/tools/oracle/syslog_harness.py`, 44 scenarios, with daemons
that restart, vanish and change kind), which fix every record, copy and
retry they exercise. That file began as BSD's, and under **A** its
notice -- the University of California's licence -- travels with the
translation as well; glibc's changes since are the LGPL's. The journal
that stands in for the daemon on SlateOS (design-decisions §1166) owes
glibc nothing.

The wide classes and case mappings, `posix/src/wctype_tables.rs`
(2026-10-01), are generated by rules of the shape of glibc's own
generator (`localedata/unicode-gen`), as known rather than read:
`Alphabetic` and the other scripts' digits make `alpha`, and the no-break
spaces are kept out of `space`. Each rule was then held to the oracle
until no code point differed (`posix/tools/wctype_gen.py --oracle`), and
two were found only that way. The tables are Unicode's data, not glibc's
code, so under **B** nothing changes: the rules are what the oracle fixes.

(The character tables themselves -- which byte means which letter -- are
facts read from glibc's data files and from running its converters, not
code; they are not in question. And not everything follows glibc's
source: the `long double` Bessel functions, `posix/src/besl.rs`, were
written from the mathematics, with glibc only run to see its answers.)

| Option | *What changes:* |
|---|---|
| **A.** Keep the translations; honour the LGPL | The files above say they are LGPL. Every program built on the C library must be re-linkable by its user -- which means shipping the library's object files with the system, or making the C library a shared library (`libc.so`) as design.txt plans for later. Nothing is rewritten. |
| **B.** Rewrite those parts clean-room; glibc stays the oracle, never the source | The library stays under whatever licence SlateOS chooses, with no condition on programs. The six parts are written again from the standards (C23, IEEE 754, the TSCII and ISO 6937 specifications) and must pass the same glibc-comparison tests they pass now; a rule is written down: glibc may be tested against, not read and copied. |
| **C.** Decide before the first public release, not now | Work continues as it is; the table above is kept current; before anything is distributed as a binary, A or B is applied. |

**If never answered:** nothing breaks and nothing is distributed yet; the
cost of B grows with every further translation, and lane D will keep
translating where glibc is the clearest description of the behaviour
wanted.

**Claude's recommendation:** **B.** The C library is the one piece of code
every program contains; keeping it free of conditions is worth a few hours
of rewriting, and the glibc comparison tests -- the part that actually
guarantees glibc's behaviour -- stay unchanged, so the rewrites cannot drift
from what the translations do today. Until you answer, lane D writes from
the standards with glibc as the oracle, and where it has read glibc's
source after all -- the timezone code and the logger, both 2026-10-01 --
the table above says so.

**Where it bites:** the six places above; `design-decisions.md` §1133 (the
earlier assumption); and every future port where glibc's behaviour is the
target.

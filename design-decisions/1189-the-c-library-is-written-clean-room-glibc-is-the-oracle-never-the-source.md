## 1189. The C library is written clean-room: glibc is the oracle, never the source

**Date:** 2026-10-11 (answered 2026-10-09)
**Decided by:** Operator (Claude's recommendation)
**Lane:** D

**In short:** the C library is built into every program on SlateOS, and some
parts of it were translated from glibc's source, whose licence (the LGPL)
would then bind every one of those programs: whoever receives one must be
able to rebuild it with their own copy of the library. Those parts are
written again from the standards and manual pages, and glibc is used only by
running it -- the tests that compare this library's answers with glibc's stay
exactly as they are. From now on glibc's (and libxcrypt's) source is not read
to write this library.

**The operator's answer** (`operator-answers/2026-10-09-open-questions-answers.txt`,
D-Q6): "B".

**The options as asked** (D-Q6):

| Option | *What changes* |
|---|---|
| A. Keep the translations and honour the LGPL | every program must be re-linkable by its user (ship the library's objects, or make it a shared library) |
| **B. Rewrite those parts clean-room; glibc stays the oracle** (chosen) | the library carries no condition on programs; the parts are rewritten from the standards and must pass the same comparison tests |
| C. Decide before the first public release | work continues as it is |

**The rule.** Running glibc to see what it answers -- the oracles under
`posix/tools/oracle/` -- is how this library learns glibc's behaviour, and is
not copying. Reading glibc's or libxcrypt's source to write this library is
not done. Character tables (which byte is which letter) are facts, not code,
and stay. Where a behaviour is reachable only from source -- glibc's state
after a sequence of calls no oracle exercises -- the oracle is extended to
exercise it.

**What is rewritten** -- the list D-Q6 kept, each from the named standard or
manual page and held to its existing oracle: the CP1255, CP1258, TCVN, T.61
and TSCII converters (`posix/src/iconv.rs`); the C23 maths functions
(`posix/src/c23math.rs`) and the narrowing checks (`posix/src/narrow.rs`);
`clog10` (`posix/src/complex*.rs`); `<obstack.h>`'s macros (from the glibc
manual's descriptions); argp (`posix/src/argp/`); the timezone code
(`posix/src/tz.rs`); the logger (`posix/src/syslog.rs`); the crypt API's
order of checks (`posix/src/crypt.rs`, `gensalt.rs`) and `<crypt.h>`; and
the regex parser's order of checks (`posix/src/regex/parse.rs`), which was
written with `regcomp.c` open though not translated from it. Tracked as a
roadmap item; until each is rewritten, `known-issues/` says which files
still derive from LGPL source. This confirms §1133's assumption, which the
translations since had not followed.

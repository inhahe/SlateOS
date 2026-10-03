### TD-B-GREP-PRINTS-POSIX-EXTENDED-DIAGNOSTICS-FOR-EGREP-SYNTAX -- FIXED (2026-08-23)

**Where:** `userspace/ere/src/engine.rs` (`Syntax`, `Regex::new_syntax`),
`userspace/coreutils/src/bin/grep.rs`.

**The original report was right about the problem and half right about the
cause.** `grep -E` is indeed not the syntax `find -regextype posix-extended`
gets; it is glibc's `RE_SYNTAX_EGREP`. But the entry guessed at which flags
differ from the header rather than measuring, and the guess was off — so the
fix started by running both real binaries over the same patterns:

| pattern | `find -regextype posix-extended` | `grep -E` |
|---|---|---|
| `*a` | `REG_BADRPT` | matches `a` |
| `{b}a` | `REG_BADRPT` | matches `a` |
| `{}a` / `{1,2,3}a` | `REG_BADRPT` | exit 1, **no diagnostic** |
| `a{b}` | invalid interval | matches `a{b}` |
| `a{}` / `a{1,2,3}` / `a{1,0}` / `a{99999999}` | error | **error** |
| `a^*b` | `REG_BADRPT` | matches `ab` |

Two consequences the header alone would not have told you. First, the
difference reduces to exactly **two** bits — `RE_CONTEXT_INDEP_OPS` without
`RE_CONTEXT_INVALID_OPS` (a quantifier with nothing to quantify repeats the
*empty* expression instead of erroring) and `RE_INVALID_INTERVAL_ORD` (a `{`
that does not open a well-formed interval is a literal brace) — so `Syntax`
is a two-field struct with two constants, and the parser is parameterised
rather than forked. Second, only the "this is not a count" forms roll back;
a *well-formed-looking but wrong* interval stays an error in both dialects,
which is why `repeat()` folds `{0,0}` to `Node::Empty` but `parse_brace`
rolls back only on `BraceNum::Invalid`.

**The posix-extended side needed no change at all.** The original entry
implied our `find` answer might also be wrong for malformed braces; measured,
every *leading* quantifier form — `{b}a`, `{}a`, `{a`, `{1,2,3}a` included —
is `REG_BADRPT` "Invalid preceding regular expression", never a brace error,
and that is already what this parser did. (A first measurement said otherwise
because the pattern was passed as `"\./$p"`, which put a `/` in front of the
"leading" quantifier. Worth remembering: `find`'s regex is whole-path
anchored, so a probe pattern must be used verbatim.)

**The two `grep -E` cases that exit 1 with no diagnostic are GNU being two
engines**, not a syntax bit: glibc `regcomp` decides accept/reject and
`dfa.c` decides what matches, and at the start of an expression glibc skips
the offending token while dfa.c makes it a literal. We do not reproduce the
silent-nonmatch behaviour — we accept those patterns and match the empty
expression, which is what `dfa.c` alone would do. Nothing in the tree depends
on the difference and reproducing it would mean shipping two disagreeing
parsers, which is the thing this crate exists to stop.

**`-G` was the follow-up question, and it is now measured too.** `bre::compile`
translates to extended and hands it to the same engine, so it inherits
`POSIX_EXTENDED`; GNU asks for `RE_SYNTAX_GREP`, which has its own bits. Rather
than read that header — the mistake this entry started with — `scripts/grep-diff.sh`
now runs 40 BRE patterns through both binaries, including every form the two
`-E` bits are about (`*a`, `a{b}`, `a{`, `{b}`, `a\{2,1\}`, `\(a`, `a\)`,
`a[`). All agree. So BRE needs no dialect parameter: in BRE a bare `{` is
already a literal and a leading `*` is already an ordinary character, which is
what makes both bits moot there.

Covered by 59 `ere` tests including `the_dialects_differ_in_exactly_two_places`,
and by `scripts/grep-diff.sh` — 155 passed, 0 differed, 11 deliberate, with the
`OURS=/usr/bin/grep` control turning all 11 into XPASS, which is the shape that
makes the markings mean something.

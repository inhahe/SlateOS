## TD-B-ERE-QUANTIFIED-ANCHOR -- a `*` after `$` or a word assertion compiles here and is refused by glibc (lane B, 2026-10-01) — **FIXED** 2026-10-01

**Status:** FIXED 2026-10-01

**Resolution (2026-10-01, the same day).** Measured again tool by tool --
grep 3.11 and grep -E, sed 4.9 and sed -E, ed 1.20, bash 5.2 `=~`, gawk 5.2.1
`--posix` -- because GNU grep turned out to be two engines that do not agree
with each other:

| dialect | after `^ $` and the buffer anchors | after `\b \B \< \>` |
|---|---|---|
| POSIX extended, awk (glibc only) | `REG_BADRPT` | `REG_BADRPT` |
| egrep (`grep -E`) | repeated; zero repetitions is the empty string | `*`, `+`, `?` leave the assertion as it was |
| basic (glibc: sed, ed, expr, find) | `*`, `\+`, `\?` literal; `\{` refused | the same |

So: the engine refuses any quantifier after any assertion unless the syntax
has `context_indep_ops`; under egrep a word assertion is left as it was by `*`,
`+` and `?`; and `bre::to_ere` treats every assertion as ending what can be
repeated, with a second flag (`at_start`) so that a `^` after one stays a
literal. On the way: a leading `\+` or `\?` in a basic expression was refused
here ("nothing to repeat") where grep, sed and ed all read the character --
`grep '\+a'` matches `+a` -- and now does too.

**Where GNU grep is followed only halfway, deliberately:** its dfa matcher
repeats a buffer anchor in a basic expression (``grep 'a\`*'`` matches every
line with an `a`) where glibc -- and so GNU sed, ed and expr, which share this
translation -- reads a literal `*`; and an interval on a word assertion under
`-E` is self-contradicting there (`\b{1}` alone matches the line `a{1}`, while
`a\b{1}` matches nothing at all). Ours follows glibc for the first and plain
repetition for the second; both are xfail rows in `scripts/grep-diff.sh`.

**In short:** in a regular expression, `$` means "end of line" and `\b` means
"word edge" -- they match a position, not a character, so there is nothing for
`*` ("repeat") to repeat. glibc refuses `a$*` and `a\b*` outright ("Invalid
preceding regular expression"); ours accepts them and quietly repeats the
position. So `find -regex 'a$*'`, `[[ $x =~ a$* ]]`, `sed -E` and awk run a
pattern GNU's tools reject. Only `^` and `` \` `` are refused today.

**Measured** (bash 5.2 `=~` and find 4.9 `-regextype posix-extended`, both
glibc 2.39; gawk 5.2.1 `--posix` for awk): every one of `a$*`, `$*`, `a$+`,
`a$*b`, `a\b*`, `a\<*`, `a\>*`, `a\B*`, ``a\`*``, `a\'*`, `\b*` is a compile
error. glibc's `parse_expression` returns from *every* anchor token before its
repetition loop, so the quantifier meets the start of a fresh expression and
`RE_CONTEXT_INVALID_OPS` refuses it. GNU grep is two engines and differs again,
so the egrep and basic dialects need their own rows: `grep -E 'a\b*'` prints
`a` and `a*` but not `ab` (the quantifier is dropped, not applied), while
``grep -E 'a\`*'`` and `grep -E 'a$*'` print every line with an `a` (zero
repetitions of a line anchor); `grep 'a\b*'` (basic) reads the `*` as a
literal after a word assertion and as a repetition after `` \` `` and `\'`.

**Where:** `userspace/ere/src/engine.rs`, `EParser::stack_quantifiers` (the
check covers `Node::Start | Node::BufStart` only), and `rejects_what_glibc_rejects`
(which no longer asserts the wrong answer for `a$*`, but does not yet pin the
right one). `bre::to_ere` passes `\b*` etc. through as a quantified assertion,
which is wrong for basic grep in the other direction.

**The proper fix:** per dialect, from the measurements above. POSIX-extended
and awk: a quantifier after any assertion is `REG_BADRPT`. Egrep: after `^ $
\` \'` it repeats the anchor (zero repetitions allowed, as today); after a word
assertion it is dropped. Basic: `to_ere` emits a literal `*` after a word
assertion and keeps the repetition after the line and buffer anchors -- which
the extended engine must then accept, so `to_ere` should rewrite it (zero or
more of a zero-width assertion is the empty string; one or more is the
assertion). Harness rows in `grep-diff.sh`, `find-diff.sh` and `awk-diff.sh`.

### TD-OILS-A-BACKQUOTE-WHOSE-BODY-SWALLOWS-A-BRACE-IS-A-LEX-ERROR-RATHER-THAN-A-FAILED-READ — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/lexer.rs`, `read_backtick` and whatever raises
`bad substitution: no closing "`"`.

**What is wrong.** A `` ` … ` `` whose body runs over a `${ … }` and then fails to
parse is, to bash, a *command substitution whose read failed* — reported and
carried on from, exactly as
TD-OILS-A-BRACE-SCAN-CARRIES-ON-PAST-A-FAILED-EXTENT-READ describes for `$( … )`.
osh raises a lex error instead and abandons the whole script.

**Reproduce** (two lines; the bytes matter, so they are spelled out):

```text
line 1:  echo "a\\`x";  echo ${z:-p"\`"r}
line 2:  echo "b\\`y"

  bash: command substitution: line 2: unexpected EOF while looking for matching `` ` ``'
        a\y
        rc=0
  osh:  line 2: bad substitution: no closing "`" in `x";  echo ${z:-p"\`
        rc=1
```

Line 1's `\\` is an escaped backslash, so the `` ` `` after it opens a
substitution; the `` ` `` on line 2 closes it, and the body in between holds an
unterminated `"`. bash reports that and goes on to run both lines. Two controls
that already agree: the same unterminated `"` in a backquote body with no `${`
in it (`echo "a`echo "u`b"`) and a plain syntax error in one (`` echo "a`for`b" ``)
— both give bash's wording and `rc=0` in osh too. So it is the `${` inside the
swallowed body that turns osh's failed read into a lex error.

**What the proper fix looks like.** The backquote reader must report and yield a
failed read the way the `$( … )` path does, rather than refusing the word.

#### What the fix turned out to be — not the reader at all

The reading above was wrong on both the location and the mechanism, and the
controls it cited are what should have given it away: an unterminated `"` in a
backquote body already agreed, so the backquote *reader* was never the problem.
Nothing about this case reaches `lexer.rs`. The parser reads the word exactly as
bash does — the whole thing, both lines, up to the backquote that closes the
substitution — and hands it on. What fails is the **expansion-time walk** in
`wordscan.rs`, entered from `word_fault` because the word contains a `${`. That
walk closes the backquote early and, from there, is reading a different word
than the parser read.

It closes it early because of one clause order. POSIX says a backquote runs to
``the next backquote that is not preceded by a backslash'', and bash's scanners
implement that by testing the backslash **before** the backquote flag — the same
way round in all three of them:

```c
      if (c == '\\')                        /* subst.c:925, 1044, 2093 */
	{ pass_next++; i++; continue; }
      if (backquote)
	{ if (c == '`') backquote = 0; … }
```

osh had written `if !backquote && c == b'\\'` in each of the three analogues —
`dquote_run` (`string_extract_double_quoted`), `skip_dq` (`skip_double_quoted`)
and `skip_matched` (`skip_matched_pair`) — which is the test the other way
round: once inside a backquote the backslash was ignored, so the `` ` `` of a
`` \` `` closed the substitution. In the reproducer that happens at the `` \` ``
on line 1, the `"` just after it is then taken for the double-quoted run's own
close, and the walk resumes mid-word, meets the *opening* backquote again out of
context, and reports it as one nothing closes.

A fourth instance was found by measurement rather than by reading: the backquote
arm of `edbs` (`extract_dollar_brace_string`) scanned to the next backtick with a
plain byte search, where bash calls `string_extract`, which "understand[s] about
backslashes in the string" (subst.c:788, 812). So a `` \` `` inside a backquote
inside a `${ … }` operand — `"${z:-A`echo m\`echo n\``B}"`, bash `AmnB` — was
still a spurious `` no closing `}' `` after the first three were fixed.

**The fix.** Drop the `!backquote` guard in `dquote_run`, `skip_dq` and
`skip_matched`; use `backquote_extent` (which was already correct) for `edbs`'s
backquote arm instead of `skip_to`.

**Regression cover.** Corpus case
`an-escaped-backquote-inside-a-backquote-does-not-close-it.sh` (the reproducer
plus one shape per scanner), and the unit test
`wordscan::tests::a_backslash_is_honoured_inside_a_backquote_by_every_scanner`.

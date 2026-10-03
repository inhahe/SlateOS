### TD-OILS-A-COMMENT-INSIDE-ARITHMETIC-CAN-EAT-ITS-OWN-CLOSER. `$(( #5 ))` is not an arithmetic error at all — bash's second scan of the word honours `#`, so the comment swallows the `))` — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `arith_comment_hides_closer`,
`arith_unclosed_by_comment`, `arith_sub`'s new `bracket` parameter, and
`enter_quoted_run`/`leave_quoted_run`.

**What:** bash reads a `$(( … ))` word **twice**, and only the second read
knows what a comment is. The parser matches the parentheses with no such
notion and hands the expander a body; the expander then re-scans the *word
source* with `extract_delimited_string`, which does honour `#`. When the
comment runs to the end of the line it takes the closing `))` with it, and what
comes out is not an arithmetic diagnostic but

```text
bad substitution: no closing `)' in "A$(( #5 ))B"
```

naming the **whole word**, quotes and neighbours included. osh evaluated the
body the parser had already extracted and reported an ordinary arithmetic
error.

**Measured rule** (bash 5.2.37):

* A comment opens at a `#` whose **preceding byte is a space, a tab or a
  newline**. Body index 0 is preceded by the `(`, so `$((#5))` is ordinary
  arithmetic — and so are `1 +#2`, `x#2`, `1#5` and `\#`.
* Quoting protects: `$(( "a # b" ))` and `$(( 'a # b' ))` both evaluate, and a
  backslash still escapes inside the double quotes. A `${ … }` does **not**
  protect: `$(( ${x # b} ))` is the bad substitution.
* A newline closes the comment, so `$(( 1 #x⏎ + 2 ))` finds its `))` after all
  and the comment text reaches the evaluator inside the echoed expression.
* Only the `$(( … ))` spelling. `$[ … ]`, `${a[…]}`, `(( … ))` and `let` are
  read by extractors with no comment rule and give ordinary arithmetic errors.
* The word it names is the one the **scanner** saw, not the one the expander
  is inside: it keeps the enclosing quotes (`"A$(( #5 ))B"`) where an ordinary
  `bad substitution` drops them (`echo "A${x!}B"` names `A${x!}B`). The two
  diagnostics come from different levels, so `enter_quoted_run` now puts the
  outer spelling aside instead of replacing it.
* The class is DISCARD and, unusually, **errexit-only**: bash raises this from
  the word scanner rather than from `parameter_brace_expand`, so posix mode's
  "an expansion error ends the shell" hook never sees it. `set -e` exits;
  `set -o posix` complains and runs the next line — the mirror image of an
  ordinary arithmetic error, which posix mode makes fatal and errexit lets
  through.

**Fixed** by deciding the question from the parser's already-extracted body,
which is exact because the two scans differ in exactly that one rule. Pinned by
`a_comment_that_eats_the_closer_is_not_an_arithmetic_error`,
`only_a_hash_after_whitespace_opens_a_comment_in_arithmetic` and the corpus
case `an-arith-comment-can-eat-its-own-closer.sh`.

**Standing lesson:** when a construct is *scanned* by one pass and *parsed* by
another, the two grammars are not the same grammar, and the difference is
visible wherever the scan stops early. Reading the closing delimiter is part of
the syntax, not a formality — which is why the diagnostic here belongs to the
word and not to the expression.

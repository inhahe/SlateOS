### TD-OILS-A-DOUBLE-QUOTED-DOLLAR-BRACKET-IS-NOT-SKIPPED-BY-THE-QUOTE-EXTRACTOR. `echo "$[ ${ ]"` names the wrong string — 2026-08-07 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/interp.rs` / the double-quote scanner — osh
recognises `$[ … ]` inside double quotes and hands the arithmetic text to the
arithmetic expander, where bash never gets that far.

**What:** bash's `string_extract_double_quoted` knows `${`, `$(` and `` ` `` but
**not** `$[`. So in a double-quoted word it walks straight past the `$[`, meets
the `${`, and calls `extract_dollar_brace_string` on the *whole word* — which
runs off the end and reports ``no closing `}'`` naming the word, quotes and all,
before any arithmetic is involved. `$(( … ))` is skipped properly (it starts
`$(`), so only the `$[` spelling is affected.

**Repro** (bash 5.2.37 left, osh right):

| input | bash | osh |
|---|---|---|
| `echo "$[ ${ ]"` | ``no closing `}' in "$[ ${ ]"`` | `` ${ : bad substitution`` |
| `echo "x$[ ${x:- ]y"` | ``no closing `}' in "x$[ ${x:- ]y"`` | ``no closing `}' in  ${x:- `` |
| `echo "$[ ${x:- ]"` | ``no closing `}' in "$[ ${x:- ]"`` | ``no closing `}' in  ${x:- `` |
| `echo "$(( ${x:- ))"` | ``no closing `}' in  ${x:- `` | same ✓ |

Only an *unterminated* `${` diverges: with the brace closed
(`echo "$[ ${x!} ]"`) the quote extractor steps over it and both shells report
the arithmetic string.

**Fixed** as one half of
TD-OILS-AN-UNCLOSED-SUBSCRIPT-IN-A-QUOTED-BRACE-BODY-IS-NOT-A-RUNAWAY-SCAN,
which is the same fact from the other side. `userspace/oils/src/wordscan.rs`
re-runs bash's *extent* pass over the word's source, and the two double-quote
scanners it mirrors know exactly `${`, `$(` and a backtick — so a `${` written
after a `$[` inside double quotes is met there, before the arithmetic text is
ever handed to the evaluator. `scan` (the `expand_word_internal` walk) does
know `$[`, and skips it; `dquote_run` and `skip_dq` deliberately do not.

**Pinned by:** `a_dollar_bracket_is_not_a_construct_the_quote_scanner_knows` in
`wordscan.rs`, and the group of the same name in
`tests/corpus/an-unclosed-subscript-in-a-quoted-brace-body-is-a-runaway-scan.sh`.

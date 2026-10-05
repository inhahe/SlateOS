### TD-OILS-AN-EXPANSION-TIME-ARITHMETIC-SCAN-IGNORES-THE-COMMENT-RULE. `a='A$(( 1 # )) B'; "${a@P}"` reports and keeps the text where bash is silent and drops it — 2026-08-09 — FIXED 2026-08-09

**Where (as first written, and wrong):** this entry named
`Shell::scan_arith_sub` (~31434) and its `command: false` argument to
`Shell::skip_opaque`. That was a guess from reading, not a measurement, and it
is **not** the site: no input was ever found in which that flag's value changes
an answer. The real site was `Shell::arith_unclosed_by_comment`, which had only
the reporting half of subst.c:1493-1506 and no `no_longjmp_on_fatal_error`
branch — so a prompt got the report and kept its text where bash is silent and
drops the rest. Recorded here rather than deleted because the mis-attribution is
the lesson: the flag was inferred, the ending was measured, and the measurement
won.

**Reproduce:**

```text
f='A$(( 1 # )) B';          printf '[%s]\n' "${f@P}"
  bash: [A]                                     (silent)
  osh:  bad substitution: no closing `)' in "${f@P}"
        [A$(( 1 # )) B]

e='A$(( 1 + 2 B';           printf '[%s]\n' "${e@P}"   # both [A], silent
```

`e` is the same *ending* reached without a comment, and the two shells agree
there — so this is the comment rule alone, not the unclosed-extent ending.

**Why.** The doc comment on `scan_arith_sub` justifies `command: false` by
citing subst.c:1303, but that line is `extract_arithmetic_subst` — the **`$[`**
spelling, which really does pass flags `0`. A `$((` does not go through it.
`param_expand`'s `case LPAREN` calls `extract_command_subst (string, &t_index,
…)` (subst.c:10575), and because `string[*sindex] == LPAREN` that is

```c
return (extract_delimited_string (string, sindex, "$(", "(", ")", xflags|SX_COMMAND));
                                                                         /* subst.c:1284-1286 */
```

— `SX_COMMAND` **is** set. So `#` at a word boundary starts a comment
(subst.c:1415) that eats the rest of the line, `))` included, and the extent
then runs the string out: `no_longjmp_on_fatal_error` takes the silent `*sindex
= i; return NULL;` branch (subst.c:1493-1506), which is why bash prints no
diagnostic and the `$((` contributes nothing.

That the two spellings genuinely differ here is measured, not assumed —
`k='A$[ 1 # ] B'` gives the *same* arithmetic error in both shells (`invalid
arithmetic operator (error token is "# ")`), i.e. no comment, confirming `$[`
is the flags-`0` case `command: false` was written for.

**Fix (commits `a1a0518ee`, `993acbe74`).** Not a flag on the old scan at all.
`Shell::arith_extent_scan` was replaced by a real port of the paren count —
`Shell::arith_extent_frame`, with `Shell::skip_single_quoted`,
`Shell::skip_double_quoted`, `Shell::arith_nested_read` and `Shell::chk_arithsub`
alongside it — run at expansion time by `Shell::arith_extent_expand` over the
arithmetic *plus the word's tail*, which is the string bash hands
`extract_command_subst`. Carrying the tail needed a new
`ast::WordPart::ArithSub::tail`, filled by `unparse::attach_tails_by` (the
sentinel-swap loop lifted out of `attach_comsub_tails_in` and run a second time).
The count's answer then drives `param_expand`'s `temp[len-1] != ')'` /
`chk_arithsub` routing, so an extent that does not close as arithmetic falls
back to a command substitution over the same text.

`arith_unclosed_by_comment` gained the prompt branch. The `#` rule's `i == 0`
half is the *word's* first byte — the `$` — and not the frame's, so a recursive
frame gets no exemption and `$((#5))` stays arithmetic; getting that wrong broke
`only_a_hash_after_whitespace_opens_a_comment_in_arithmetic`.

A second site fell out of the same measurement: `extract_dollar_brace_string`'s
`$(` row reads a `$((` through the very same count (subst.c:1894-1903), and a
count that overruns the scan's index leaves the *brace* with nothing to close
(`CHECK_STRING_OVERRUN`, subst.c:135-141). `Shell::brace_scanned_subs_in` now
hands `extent_read_of` the whole arithmetic instead of descending into its parts,
and `extent_read_of` runs the count there — `A${x:-$(( #5 ))}B` under `${…@P}`
was `A5B`, now the `bad substitution` and undecoded word bash gives. Outside a
prompt the count reports and longjmps first, so the brace gets no ending of its
own.

Corpus `an-arith-comment-can-eat-its-own-closer.sh` grew eleven probes (the
prompt ending, a closer found on a later line, `PS4`, and the five brace shapes);
`an-arithmetic-extent-read-parses-a-command-substitution-inside-it.sh` grew the
multi-line ones.

One shape was left over and narrowed into
TD-OILS-AN-ARITHMETIC-EXTENT-CARRIES-ON-COUNTING-AFTER-A-FAILED-READ — not a
scan-flag problem either, and fixed there since by giving the same count to a
`$((` osh's lexer had routed to a command substitution.

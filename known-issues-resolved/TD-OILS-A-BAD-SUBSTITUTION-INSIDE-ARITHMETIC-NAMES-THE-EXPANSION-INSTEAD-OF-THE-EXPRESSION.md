### TD-OILS-A-BAD-SUBSTITUTION-INSIDE-ARITHMETIC-NAMES-THE-EXPANSION-INSTEAD-OF-THE-EXPRESSION. `echo $(( 1 + ${x!} + 2 ))` named `${x!}` — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/interp.rs` — `Shell::expand_arith_params` and
`Shell::expand_to_arith_string` (the two implementations of bash's
`expand_arith_string`), plus `Shell::enter_inner_source`.

**What.** bash reports a "bad substitution" against `string` — the whole text
the `expand_word_internal` call in flight was handed (subst.c:10041). For
arithmetic that text is the *arithmetic string*, because `expand_arith_string`
is a word expansion in its own right that runs before `evalexp`. osh instead
wrapped each `${ … }` it met in a synthetic one-part `Word` and expanded it
through `expand_word`, whose `begin_word` renamed the source to that part:

```text
                              bash 5.2.37            osh
echo $(( 1 + ${x!} + 2 ))      1 + ${x!} + 2 : …     ${x!}: …
echo $(( ${x!} ))              ${x!} : …             ${x!}: …
for ((i=${x!};i<0;i++))       i=${x!}: …             ${x!}: …
a=(1); echo ${a[${x!}]}       ${x!}: …               ${a[${x!}]}: …
v=abc; echo ${v:${x!}:1}      ${x!}: …               ${v:${x!}:1}: …
```

The surrounding spaces in rows 1–2 and their absence in rows 4–5 are the same
rule, not two: each context names its own arithmetic string exactly as its
scanner carved it out.

A second half came with it. bash's `expand_arith_string` longjmps out of such a
failure, so the expression is never evaluated — osh went on to hand the
evaluator the gap the failed expansion left and complained about that too
(`${a[1+${x!}]}` printed the bad substitution *and* `1+: syntax error`).

**Fixed.** `enter_quoted_run`'s save/restore was generalised to
`enter_inner_source(named)` — "bash re-enters `expand_word_internal` on this
stretch of text" — with the double-quoted-run entry now one caller of it and the
two arithmetic-string expanders the others. The outer spelling is still put
aside in `quoted_outer_word`, so the *scanner's* "no closing `)'" complaint goes
on naming the whole word (`p"A$(( #5 ))B"q`) as before.

Inside `expand_arith_params` the `${ … }` arm now calls `expand_word_inner`
rather than `expand_word`: it is not a word bash scanned, it is the middle of
the pass already running, so it must not open a word context of its own. And
`arith_sub` / `eval_arith_expr_checked` return early when
`Shell::expansion_failed` is set, which is the longjmp.

A failure in a *subshell* still reaches the evaluator, because it does not
unwind this shell — `echo $(( 1 + $(echo ${x!}) ))` really is two diagnostics in
bash too. Corpus:
`a-bad-substitution-in-an-arithmetic-string-names-the-whole-string.sh`.

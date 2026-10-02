### TD-OILS-ARITH-ESCAPED-DOLLAR-LEAK. An escaped `\$` inside `(( ))` or a subscript is expanded anyway, and leaks osh's internal quoting markers into the diagnostic — 2026-07-30 — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — `Shell::expand_arith_params` (the
`(( … ))` / `for (( … ))` section path) and the subscript expansion that feeds
`arith::Sub`.

**Symptom.** `(( … ))` sections are raw parser text, so the shell expands
`$params` in them itself (see BUG-OILS-ARITH-WHO-EXPANDS above). A **backslash-
escaped** `\$` must survive that pass unexpanded and then lose its backslash to
quote removal, leaving a literal `$` for the arithmetic evaluator to reject. osh
instead expands it, and what it hands the evaluator still carries osh's internal
quoting markers:

```
n=5
(( x=\$n+1 ))      # bash: ((: x=$n+1 : syntax error … (error token is "$n+1 ")
                   # osh:  ((: x=\5+1 : syntax error … (error token is "\5+1 ")
echo $(( \$n+1 ))  # bash: $n+1 : …    osh: \5+1 : …
a=(zero one two)
echo "${a[\$n]}"   # bash: $n: …       osh: '$'n: …
```

So there are two faults. In the section path the escape is ignored outright and
`$n` expands to `5`, with the backslash left in place. In the subscript path the
escape *is* honoured — the `$` is not expanded — but the `'$'` marker osh uses
internally to carry a quoted `$` reaches the diagnostic verbatim, where bash
shows a bare `$`.

**Repro.** The three lines above, under `osh` and MSYS bash. Status and fatality
already agree in every case (rc=1 for the `((`, and the script continues); only
the text differs, so this is a diagnostic-fidelity bug rather than a behaviour
one.

**Proper fix.** `expand_arith_params` must treat a backslash as an escape while
scanning for expansions (skipping the escaped character rather than expanding
it), and the de-quoting that produces the text handed to `arith::parse` must
reduce osh's internal `'$'`-style markers back to the plain character — the same
reduction the `error token is "…"` text already needs. Both callers want the same
helper, so add one rather than patching each site.

**✅ RESOLVED 2026-07-30.** Measuring the fix turned this from a
diagnostic-fidelity bug into a **layering** bug, and closed six divergences at
once. The root cause was that osh **removed double quotes inside the arithmetic
evaluator** — `arith::parse` built its character stream with
`expr.chars().filter(|&c| c != '"')` — and did no backslash processing anywhere.
bash does the opposite: `expand_arith_string`, the *expansion* pass in front of
the evaluator, removes double quotes and applies **double-quoting's backslash
rules**, and the evaluator itself removes nothing. Which pass owns the dequoting
is exactly what decides the observable behaviour, because the two passes do not
see the same text (the same split as BUG-OILS-ARITH-WHO-EXPANDS): a `(( … ))`
section and a `for (( … ))` header are raw parser text the shell expands itself,
whereas a `let` argument, an integer-assignment value and a `[[ … -eq … ]]`
operand are ordinary words that were already word-expanded and are never
dequoted a second time. So quotes reaching the evaluator are ordinary — and
invalid — characters:

| Case | bash | osh before |
|---|---|---|
| `$(( "3" + "4" ))` | `7` (expansion handed over `3 + 4`) | `7` ✓ |
| `x='"3"'; $(( x+1 ))` | `"3": syntax error: operand expected` | `4` |
| `let 'y="3"+4'` | `"3"+4: syntax error: operand expected` | rc=0 |
| `declare -i k='"3"+4'` | `"3"+4: syntax error: operand expected` | `k=7` |
| `[[ '"3"' -eq 3 ]]` | `"3": syntax error: operand expected` | rc=0 |
| `(( \$n ))` | `$n : …` (escape survives expansion) | `\5 : …` |
| `(( "\$n" ))` | `$n : …` (quotes cannot hide the escape) | `\5 : …` |
| `(( \"q\" ))` | `"q" : …` (a literal quote is rejected) | rc=0 |
| ``(( \`echo 1\` ))`` | `` `echo 1` : … `` | ran the command |
| `(( 1 + \<newline>2 ))` | `3` (line continuation) | syntax error |
| `${a[\$n]}` | `$n: …` | `'$'n: …` |

Single quotes are removed by neither pass, so `(( '3' ))` and `${a['1']}` are
errors in both shells; `'` is *not* in the escape set either, so `\'` keeps its
backslash. Three changes, one per layer:

* `arith.rs` — deleted the quote filter in `parse()`, with a comment recording
  that quotes arriving here are upstream's leftovers and therefore invalid.
* `interp.rs` `expand_arith_params` — handle `\` before scanning for expansions:
  `\<newline>` vanishes (line continuation), `\` before `$`, `` ` ``, `"` or `\`
  drops the backslash and makes the next character *literal* (which is what
  stops `\$n` from expanding), and anywhere else both characters survive (`\q`
  stays `\q`). Then drop `"` outright — no open/closed state is needed, since an
  arithmetic string's rules are the same inside and outside double quotes.
* `interp.rs` `expand_to_arith_string` (the subscript path) — the lexer stores a
  backslash escape as `WordPart::SingleQuoted { escaped: true }`, which the old
  code re-emitted with `'` quotes; it now re-emits the escape's own spelling,
  which is what removes the `'$'n` marker leak while leaving a *genuine*
  single-quoted run quoted.

**Tests.** `interp::tests::an_arithmetic_string_is_dequoted_by_whoever_expands_it`
(the expansion-side removals plus all four evaluator-side rejections),
`interp::tests::a_backslash_in_an_arithmetic_string_follows_double_quoting_rules`
(the whole escape matrix and the three subscript spellings),
`arith::tests::quotes_are_not_the_evaluators_to_remove` (replacing the old
`double_quotes_are_stripped`, whose premise this inverted), and
`tests/corpus/arith-quoting.sh`.

**Note measured along the way.** bash does *not* trim the section's surrounding
blanks before blaming it, so `(( \$n ))` reports the subject and error token as
`$n ` — with the trailing space. osh already agreed; two of the new tests
initially asserted the trimmed text and were wrong.

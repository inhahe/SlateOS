### TD-OILS-A-NESTED-BRACKET-IN-AN-ASSIGNMENT-TARGET-IS-NOT-AN-ASSIGNMENT. `c[b['1']]=R` runs as a command — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/parser.rs`,
`Parser::spanning_subscript_assignment` — the branch that recognises an
assignment whose subscript holds an expansion and so arrives as several lexer
segments rather than as one literal. It took the first `]` in a later literal
segment, so a word whose subscript held a nested bracket was not recognised as
an assignment at all and was run as a command. (The all-literal branch,
`balanced_subscript_end`, already counted brackets and was always right — which
is why `c[b[1]]=R` worked and `c[b[$i]]=R` did not.)

**Reproduce.**

```sh
declare -a c b
c[b['1']]=R; echo "rc=$?"
```

| bash 5.2.37 | osh |
|---|---|
| `'1': syntax error: operand expected (error token is "'1'")`, `rc=1` | `c[b[1]]=R: command not found`, `rc=127` |

**The fix — done.** bash measures the subscript with `skipsubscript`
(`newi = skipsubscript (string, indx, (flags & 2) ? 1 : 0);`, general.c:469),
which is `skip_matched_pair`: `[`/`]` nest, and a quoted run or a substitution
is stepped over whole. In a segmented word the second half is free — a `'…'`,
`"…"`, `$(…)` or `${…}` *is* one segment, so it contributes no brackets — and
the first half is a bracket count that has to survive from one literal run to
the next. That is the new `subscript_close_in_lit(s, &mut depth)`, a resumable
`balanced_subscript_end`; `spanning_subscript_assignment` now carries `depth`
across its segments instead of taking the first `]`.

Fixing the recognition was the whole of it: once the word is an assignment,
osh's existing subscript reading already reproduced bash's answers for every
shape probed, including the ones where the two array kinds diverge. An indexed
subscript takes `array_expand_index` → `expand_arith_string (exp,
Q_DOUBLE_QUOTES|Q_ARITH|Q_ARRAYSUB)` (arrayfunc.c:1358), whose gate is closed
for a subscript with no `$`/`` ` ``/CTLESC/`~` — so it goes to
`string_quote_removal (string, quoted)` (subst.c:11892), where under
`Q_DOUBLE_QUOTES` a `'` is an ordinary character that *stays* while a `"` is a
quote that goes. Hence `c[b['1']]=R` is `'1': syntax error` but
`c[b["1"]]=R` assigns. An associative subscript takes `expand_subscript_string
(t, 0)` (arrayfunc.c:1593) instead, quoting 0, where the `'` is a real quote —
so `m[b['x']]=R` is simply the key `b[x]`.

Corpus: `a-nested-bracket-in-an-assignment-target-is-still-an-assignment.sh`.
Unit test: `parser::tests::a_nested_bracket_in_a_spanning_subscript_is_still_an_assignment`.

Found while measuring TD-OILS-A-SUBSCRIPT-IN-AN-ARITHMETIC-WORD-IS-NOT-EXPANDED-
IN-PLACE-FIRST; it was the one row of that probe still diverging.

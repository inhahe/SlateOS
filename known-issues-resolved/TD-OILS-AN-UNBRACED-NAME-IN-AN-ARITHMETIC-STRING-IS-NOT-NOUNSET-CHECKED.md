### TD-OILS-AN-UNBRACED-NAME-IN-AN-ARITHMETIC-STRING-IS-NOT-NOUNSET-CHECKED. `set -u; echo $(( $y + 1 ))` printed `1` — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/interp.rs` — the `$name` arm of
`Shell::expand_arith_params` (~line 28412) and `Shell::eval_arith_raw`
(~line 10047).

**What was wrong.** An arithmetic string is a word expansion in its own right:
bash's `expand_arith_string` hands the text to `expand_word_internal` before
`evalexp` ever sees it, so a `$name` in it is read by the same code that reads
one in a word and `err_unboundvar` fires there under `set -u`. osh's
`expand_arith_params` did `self.param_value(&n).unwrap_or_default()` with no
nounset check at all, so the reference silently became the empty string.

| probe (`set -u`) | bash 5.2.37 | osh (before) |
|---|---|---|
| `echo $(( $y + 1 ))` | `y: unbound variable`, rc=1 | `1`, rc=0 |
| `echo $[ $y + 1 ]` | `y: unbound variable`, rc=1 | `1`, rc=0 |
| `echo $(( 1 + $y ))` | `y: unbound variable`, rc=1 | `1 + : syntax error`, rc=1 |
| `(( $y )); echo after=$?` | `y: unbound variable`, rc=1 | `after=1`, rc=0 |
| `for (( $y; 0; ))` | `y: unbound variable`, rc=1 | silent, rc=0 |

The `${y}` spelling, a bare `y`, `${#y}`, and every subscript/substring context
(`${a[$y]}`, `${v:$y:1}`, `a[$y]=9`) were already right — those route through
`expand_to_arith_string` → `expand_dynamic`, which checks.

**Fix.** Three parts:

* The `$name` arm now calls `note_unbound_spelled(&n, false)` when
  `param_value` comes back `None`, exactly as `expand_dynamic_with`'s
  `WordPart::Param` arm does.
* The scan loop breaks on `expansion_failed()`, so one failure ends the string
  where it stood and the text after it is never expanded —
  `$(( $y + $z ))` names `y` alone, as bash's longjmp out of
  `expand_arith_string` does.
* `eval_arith_raw` bails on `expansion_failed()` rather than only on
  `discard_error`. Without this the `for (( $y; 0; ))` case evaluated the empty
  hole the failure left and reported a silent success.

**Covered by:**
`tests/corpus/an-unbraced-name-in-an-arithmetic-string-is-nounset-checked.sh`
(24 cases).

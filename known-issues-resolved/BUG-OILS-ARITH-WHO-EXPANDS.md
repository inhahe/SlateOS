### BUG-OILS-ARITH-WHO-EXPANDS. `let` and integer assignments expanded their own `$params`, and a failed expansion inside `(( ))` was ignored instead of abandoning the section — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — `Shell::eval_arith_raw`,
`Shell::eval_arith_expanded` (new), `Shell::eval_int_assign`,
`Shell::eval_referent_subscript` (new), `Shell::apply_value_attrs`,
`Shell::exec_arith`, and the `-i` branches of `Shell::apply_assignment` /
`Shell::builtin_declare`.

**What.** Arithmetic text reaches the evaluator by two routes that disagree about
who has already expanded it, and osh ran everything down the first route:

* A `(( … ))` command and the three sections of a `for (( … ))` are text the
  *parser* set aside raw. Nothing has expanded them, so the shell expands
  `$params` in them on the way to the evaluator (bash's `expand_arith_string`).
* A `let` argument, and the value of an integer-attributed assignment, are
  ordinary *words* that word expansion has already finished with. bash expands
  them no further — so a `$` that survived quoting reaches the evaluator, which
  has no rule for one.

osh expanded both, which turned three measured behaviours into wrong ones:

1. `n=5; let 'x=$n+1'` set `x=6`, where bash refuses:
   `let: x=$n+1: syntax error: operand expected (error token is "$n+1")`,
   status 1, `x` never set. Same for `${n}` and even a nested `$((n))` — none of
   them are expansions here, just unparseable text. `declare -i k='$n+1'` and a
   plain `k='$n+1'` under the `-i` attribute answer the same way (the first
   tagged `declare:`, the second untagged), and both are *fatal*.
2. A **failed nested expansion** inside an arithmetic section was ignored.
   `arith_sub` reports the error, arms `discard_error` and returns the string
   `"0"`; `eval_arith_raw` then went on to evaluate that. bash's
   `expand_arith_string` longjmps out instead, so the section is abandoned where
   it stands. The visible damage was a *side effect bash never performs* —
   `x=keep; (( x=$((1/0)) ))` left `x` as `0` instead of `keep` — plus a
   swallowed following command whenever the fabricated `0` happened to evaluate
   cleanly: `for ((i=0; i<$((1/0)); i++)); do …; done` broke out of the loop
   normally, returned `Flow::Next`, and left the abort armed for the *next*
   line's command to consume.
3. A **failed integer assignment stored a zero.** `eval_int_assign` returned `0`
   as a placeholder on error and every caller stored it, so
   `declare -i p=7; p=1/0` left `p` at `0`. bash leaves the variable exactly as
   it was (`7`), and one that was never set stays unset rather than becoming 0.

**Fixed (2026-07-30).**

* Split the one evaluator entry point in two. `eval_arith_raw` is the *section*
  path: it lifts the builtin tag around `expand_arith_params` (so a nested
  `$(( … ))` failure is reported untagged, as the expansion error it is), and now
  **returns `None` the moment the expansion arms `discard_error`**, leaving the
  flag armed for `arith_discard_flow` and evaluating nothing. `eval_arith_expanded`
  is the *word* path — no expansion at all — and `builtin_let` and
  `eval_int_assign` call it directly.
* `exec_arith` no longer duplicates the tag/emit/subscript logic inline; it goes
  through `eval_arith_cmd`, so it picks both behaviours up automatically.
* `eval_int_assign` returns `Option<i64>` and `apply_value_attrs` returns
  `Option<String>`; `None` means *store nothing*. All eleven assignment sites
  (scalar, `+=`, indexed element, associative element, the array-literal
  elements, and `declare`/`local`'s own operand) bail out instead of writing a
  fabricated zero.
* One caller of `eval_int_assign` was **not** an already-expanded word: the
  subscript of an indirect reference's referent (`ref='a[$n]'; ${!ref}`). That
  text came out of a variable and has never been expanded, so bash expands it
  here — `ref='a[$(echo 2)]'` reads `a[2]`. It moved to a new
  `eval_referent_subscript`, which keeps the raw path plus the fatality.

**Also fixed on the way (a latent bug the split exposed).** When `${!ref<op>}`
resolves to nothing, `exec`'s indirect path uses the *reference text* as a
stand-in target name — `!a[$n]` — since no variable can be called that, so the
modifier sees an unset parameter. But it then **read** that fabricated name, and
`indirect_target_value` re-parsed it as a subscripted reference, evaluating the
subscript text a second time. It was silent only because the second evaluation
also expanded; with expansion gone it printed a spurious
`$n: syntax error: operand expected`. The lookup is now skipped entirely for a
reference that points nowhere (`points_nowhere`), which is what "unset by
construction" was supposed to mean.

**Tests.** `interp::tests::let_and_integer_assignment_do_no_expansion_of_their_own`,
`interp::tests::a_nested_expansion_failure_in_an_arith_section_is_untagged_and_fatal`
(including the two "the variable keeps its old value" probes),
`interp::tests::a_failed_integer_assignment_stores_nothing`,
`interp::tests::an_indirect_referents_subscript_is_expanded_and_fatal_when_bad`,
and `tests/corpus/arith-no-expansion.sh`.

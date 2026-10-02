### TD-OILS-A-DISCARD-STOPS-AT-THE-EVAL-OR-FUNCTION-IT-WAS-RAISED-IN. A bad array subscript unwinds one frame, not to the top — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, `Shell::arm_discard` and whatever
consumes it. osh's discard abandons the *current parse unit*; bash's
`jump_to_top_level (DISCARD)` abandons everything up to the top level of the
shell, and `top_level_cleanup` explicitly tears the intervening frames down
first:

```c
  while (parse_and_execute_level)
    parse_and_execute_cleanup (-1);
  ...
  run_unwind_protects ();
```

`parse_and_execute_level` is `eval`/`source`/`-c`/trap nesting, and
`run_unwind_protects` pops the function-call frames — so neither an `eval` nor
a shell function contains it. Only a *subshell* does, the longjmp being
per-process.

**Reproduce.**

```sh
echo "--- 1"
eval 'f1[1x]=R'; echo "rc=$? reached"
echo "--- 3"
( eval 'f3[1x]=R' ); echo "rc=$? reached"
echo "--- 4"
f() { eval 'f4[1x]=R'; echo "in-func"; }
f; echo "rc=$? reached"
```

| row | bash 5.2.37 | osh (before) |
|---|---|---|
| 1 | error alone; `; echo …` never runs | error, then `rc=1 reached` |
| 3 | error, then `rc=1 reached` (the subshell stops it) | same |
| 4 | error alone; neither `in-func` nor the outer `echo` runs | error, then `in-func`, then `rc=0 reached` |

The error text and the exit status already match at the point of failure; only
how far the abandonment reaches differs. Without `eval`/a function — a bare
`f[1x]=R; echo hi` at script top level — osh already discards correctly, which
is why this went unnoticed.

**Fixed**, but the entry's framing above was wrong on two counts and the
measurement corrected both.

*Wrong count 1: the scope.* Row 4's `f() { eval …; }` is not evidence that a
*function* contains the discard — it is the `eval` inside it. Measured on a
function with no `eval` (`g() { f5[1x]=R; echo in-func; }; g`), osh already
matched bash before the fix. The only two frames that were wrongly containing
it were `eval` and `.`/`source`, which is exactly what
`parse_and_execute_level` counts — so the flag did *not* need to survive a
function return, and nothing about function calls changed.

*Wrong count 2: the trigger.* This is not "a bad array subscript". A
*non*-arithmetic bad subscript (`eval 'a[-9]=x'`) is an ordinary expansion
error and bash's `eval` **does** catch it; so does `unset 'a[1x]'`, and so does
a `${v:1x}` substring bound. The trigger is narrower: an **arithmetic** error
while evaluating a subscript. bash reaches every subscript through
`array_expand_index`, and that function does not ask what depth it is standing
at before cleaning up —

```c
  val = evalexp (t, eflag, &expok);
  …
  if (expok == 0)
    {
      set_exit_status (EXECUTION_FAILURE);
      if (no_longjmp_on_fatal_error)
        return 0;
      top_level_cleanup ();
      jump_to_top_level (DISCARD);            /* arrayfunc.c:1363-1375 */
    }
```

— where an ordinary expansion error reaches `exp_jump_to_top_level`, which runs
`top_level_cleanup ()` **only** `if (parse_and_execute_level == 0)`
(subst.c:12151-12153). That one test is the whole difference. So the two
"depths" osh already modelled — `Flow::Discard` and `Flow::Abort` — were
exactly the right pair of behaviours; the subscript case was simply arming the
wrong one.

**What changed.** `DiscardAbort` is now built in one place, `Shell::arm_with
(status, past_eval)`, and the three arming paths differ only in what they pass
it:

| path | status | `past_eval` | bash |
|---|---|---|---|
| `arm_discard(s)` | caller's | false | `exp_jump_to_top_level` |
| `arm_int_bind_discard()` | `last_status`, or 1 | true | `bind_int_variable` |
| `arm_subscript_discard()` | fixed 1 | true | `array_expand_index` |

The status is a *fixed* 1 for the new one because `set_exit_status
(EXECUTION_FAILURE)` stands on the line above the jump — measured, `f() { eval
'a[$(exit 3)1x]=v'; }; f; echo $?` is 1, not the 3 the integer-binding path
would have carried through.

Three sites arm it: `eval_arith_expanded` and `eval_arith_cond_operand` (both
already tested `ArithError::in_subscript`, for a subscript met inside a `let` /
`(( … ))` / `[[ … ]]` expression), and the subscript evaluation itself.
`eval_arith_expr_checked` was serving *both* a subscript and a `${v:off:len}`
bound and could not tell them apart, so it grew an inner
`eval_arith_expr_at (s, subscript)` — `true` from
`eval_arith_index_text_checked`, `false` from `eval_arith_substr_bound`. A
subscript *nested inside* a bound is still the fatal one, which the existing
`in_subscript` flag catches: measured, `v=abcdef; f() { eval 'echo
${v:t[1x]}'; echo yes; }; f` prints no `yes`.

Corpus: `an-arithmetic-error-in-a-subscript-unwinds-past-the-eval.sh`, 24 rows
— the eval/source/nested-eval frames, every context that reaches a subscript
(`(( ))`, `[[ -eq ]]`, `let`, a compound literal, `declare`, `printf -v`), the
three that are *not* this depth (`unset`, both substring bounds, `a[-9]`), a
subscript inside a bound, the `||`/`if`/`while`/`for` that do not stop it, and
the three forks that do.

Found while probing TD-OILS-AN-UNCLOSED-SUBSCRIPT-IN-AN-ASSIGNMENT-POSITION-IS-NOT-A-READER-ERROR
(row 6 of its probe put the failing assignment inside an `eval` and the
difference showed up as an extra `rc=` line).

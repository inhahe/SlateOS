### TD-OILS-ARITH-SUBSCRIPT-LVALUE-ERR. An arithmetic error inside an array *subscript* is tagged and survivable in osh, bare and fatal in bash — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — `Shell::emit_arith_error` (the `((`
tag and the echoed expression) and the `(( … ))` command path that decides
whether an arithmetic failure discards the rest of the list.

**What.** bash evaluates the subscript of an array-assignment lvalue through a
*separate* entry point from the expression around it, and a failure there is
reported without the surrounding command's name and without the expression it
came from — only the subscript's own text — and is fatal to the command list,
the way a `$(( ))` expansion error is:

```
$ bash -c '((a[1/0]=9)); echo after'
bash: line 1: 1/0: division by 0 (error token is "0")      # `after` never runs
$ osh -c '((a[1/0]=9)); echo after'
osh: line 1: ((: a[1/0]=9: division by 0 (error token is "0")
after
```

**Proper fix.** Evaluate an lvalue's subscript through a path that carries its
own error context — a subscript-scoped `ArithError` whose `expr_override` is the
subscript text and whose `subject`-like flag suppresses the builtin tag — and
have the `(( … ))` command treat a subscript failure as a discarding error
rather than a status-1 one. The `subject` field added for the readonly refusal
is the right shape to generalise.

**Impact.** Narrow: only an lvalue whose *subscript* is itself malformed. Kept
out of `tests/corpus/arith-subscript-quoting.sh`.

**Fixed (2026-07-30).** The entry above had the shape of the rule right and its
*scope* wrong: it is not about lvalues. Measured against bash 5.2, **every**
subscript evaluated during arithmetic behaves this way — a read (`(( a[1/0] ))`),
a right-hand side (`(( b=a[1/0] ))`), an increment, a compound assignment, a
`[[ a[1/0] -eq 1 ]]` operand, a `for ((i=a[1/0];…))` section and a
`declare -i n=a[1/0]` value all report the bare subscript and abandon the parse
unit, exactly as the lvalue form does. bash reaches all of them through the same
separate entry point.

The subscript's parsed AST is now bundled with its raw source text as
`arith::Sub`, whose `parse`/`eval` tag any error with `in_subscript` and record
the subscript text in `expr_override`. `Shell::emit_arith_error` drops the
builtin tag for such an error, and the evaluation sites arm `discard_error`,
which is what makes it fatal. Four details fell out of measuring rather than
guessing:

* **The blamed text is the raw subscript, blanks and all.** `((  a[  1/0  ]  =9))`
  reports `1/0  : division by 0 (error token is "0  ")` — leading blanks are
  skipped by the existing `trim_start` in the diagnostic, trailing ones are not.
  A rejected number literal truncates *within the subscript* (`a[2#9]` reports
  `2#9`, where osh had reported `a[2#9`).
* **The innermost failing thing wins.** `a[b[1/0]]` blames `1/0`, and
  `x=1/0; ((a[x]=9))` blames the *value* `1/0` that `str_to_val` recorded, not
  the subscript `x` — so `expr_override` is only filled in if still empty.
* **A parse error in the subscript counts** (`((a[1+]=9))` → `1+: syntax error:
  operand expected`), so `Sub::parse` tags too, not just `Sub::eval`.
* **Even a refusal becomes fatal there.** `readonly ro=1; ((a[ro=5]=9))` keeps the
  untagged `ro: readonly variable` wording but abandons the line, where the same
  refusal outside a subscript (`((ro=5))`) only fails the command.

**A pre-existing flag leak came out with it.** `discard_error` was armed by these
evaluations but consumed by whatever *next* simple command ran, so a compound
command alone on its line swallowed the line after it: `[[ $((1/0)) -eq 1 ]]`
followed by `echo rc=$?` printed nothing at all. It only looked correct because
the probes had always been written as `…; echo rc=$?` on one line, where eating
"the next command" and "abandoning the rest of the line" are indistinguishable.
`Shell::arith_discard_flow` now converts a pending flag into `Flow::Discard` at
the tail of `exec_arith`, `exec_cond` and each `exec_for_arith` section.

**Tests.** `arith::tests::a_subscript_failure_blames_the_subscript` (the flag and
the blamed text, including the nesting and value-recursion precedence),
`interp::tests::arith_subscript_error_is_untagged_and_fatal` (the diagnostics and
their fatality in every context, each with its non-subscript control) and
`tests/corpus/arith-subscript-error.sh`, whose probes are deliberately one per
line for the reason above.

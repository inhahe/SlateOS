### TD-OILS-A-DEFERRED-COMMAND-SUBSTITUTION-BODY-REPORTS-THE-ENCLOSING-INPUTS-NAME. `` eval 'echo `echo 2)3`' `` said `eval:` where bash says `command substitution:` — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/interp.rs` — `Shell::syntax_error_prefix`, and the
`self.comsub_read_eval && self.at_outermost_read_eval()` test it and
`Shell::parse_error_flow` share.

**What.** A syntax error in a body bash reads at expansion time (a backtick, or
a `$((` that fell back — see
TD-OILS-AN-UNTERMINATED-ARITHMETIC-EXPANSION-IS-REPARSED-AS-A-SUBSTITUTION-BODY)
is tagged `command substitution` — but only when the substitution is not itself
inside an `eval` or a `.`/`source`:

```text
echo `for`                     bash: p.sh: command substitution: line 1: … | osh: same ✅
eval 'echo `for`'              bash: p.sh: command substitution: line 1: … | osh: p.sh: eval: line 1: …
. ./inner.sh   (inner: echo `for`)
                               bash: inner.sh: command substitution: line 1: … | osh: inner.sh: line 1: …
```

Only the token is wrong; `$?` is 2 in both, and the enclosing command carries on
in both. A nested `eval` *inside* the body correctly says `eval` in both.

**Why.** bash's `command_substitute` pushes a **new input source** for the body
(it calls `parse_and_execute`, subst.c), and `report_syntax_error` names the
*innermost* one — so whatever `eval`/`source` frames the caller sat in are no
longer in scope. osh models the innermost source as
`comsub_read_eval && at_outermost_read_eval()`, and `at_outermost_read_eval()`
is `eval_depth == 0 && source_stack.is_empty()` — both of which the
substitution's subshell *inherits* from the caller (`clone_for_subshell` copies
`eval_depth`; `source_stack` has to be kept for `BASH_SOURCE`). So the body's
own loop is not recognised as outermost and the caller's token wins.

**Fix (2026-08-07).** Record where the body's loop sits rather than asking
whether the shell is globally outermost. `comsub_read_eval: bool` became
`Option<(u32, usize)>` — the `(eval_depth, source_stack.len())` the body started
at — and `at_comsub_read_eval()` compares the current pair against it. A nested
`eval` moves off the base and reports as itself, which is what bash does.
`syntax_error_prefix`'s token cascade now tests the body *before*
`eval_depth > 0`, since the body is the innermost of the two; `parse_error_flow`
shares the predicate, so the status rule follows the same base.

**Tests.** `tests/corpus/a-substitution-body-is-an-input-source-of-its-own.sh`
walks the matrix — a bare body, one inside an `eval`, one inside a `.`, an
`eval` inside a body, a plain `eval`, and the `$((` fallback in each position —
plus assertions in `interp.rs::a_parse_error_reads_as_bashs_does` that sweep
`eval_depth` 0..2 with the base at each and one past it.

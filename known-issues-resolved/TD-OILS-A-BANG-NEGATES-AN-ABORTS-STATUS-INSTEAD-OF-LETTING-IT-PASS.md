### TD-OILS-A-BANG-NEGATES-AN-ABORTS-STATUS-INSTEAD-OF-LETTING-IT-PASS. `! f` turns a discarded frame's 1 into a 0 — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, `exec_pipeline`:

```rust
if pipe.negated {
    self.last_status = i32::from(self.last_status == 0);
}
```

This runs whatever the pipeline's `Flow` turned out to be. In bash a
`jump_to_top_level (DISCARD)` leaves `execute_pipeline` by `longjmp`, so the
negation at the bottom of it is never reached and the status the abort carried
survives untouched.

**Reproduce.**

```sh
g() { eval 'q[1x]=v'; }
! g
echo "rc=$?"
h() { eval 'declare -i n=2+'; }
! h
echo "rc=$?"
```

bash prints `rc=1` for both — the diagnostic, then the abort's own 1. osh
prints `rc=0` for both: the `!` negated it on the way out.

| shape | bash 5.2.37 | osh |
|---|---|---|
| `f() { eval 'q[1x]=v'; }; ! f` | `rc=1` | `rc=0` |
| `f() { eval 'declare -i n=2+'; }; ! f` | `rc=1` | `rc=0` |
| `! q[-9]=x` (an ordinary discard raised right here) | `rc=1` | `rc=0` |
| `! eval 'q[1x]=v'` | `rc=1` | `rc=0` |
| `if ! f; then …` | takes the `else` at `rc=1` | takes the `then` |
| `! eval 'for i in 1; do break 1 2; done'` | `rc=1` | `rc=0` |
| `x=$( ! q[1x]=v )` | `rc=1` | `rc=0` |
| `! f` where the `eval` **caught** the discard (`q[-9]=x`) | `rc=0` | same |
| `! exit 3` in a subshell | `rc=3` | same |

**Careful — a `( ! cmd )` is not this case, and already matches.**
`execute_in_subshell` *hoists* the `!` off the subshell's single top-level
command and applies it **after** its own `setjmp`:

```c
  invert = (tcom->flags & CMD_INVERT_RETURN) != 0;
  tcom->flags &= ~CMD_INVERT_RETURN;

  result = setjmp_nosigs (top_level);
  …
  else if (result)
    return_code = (last_command_exit_value == EXECUTION_SUCCESS) ? EXECUTION_FAILURE : last_command_exit_value;
  …
  if (invert)
    return_code = (return_code == EXECUTION_SUCCESS) ? EXECUTION_FAILURE
						     : EXECUTION_SUCCESS;
                                        /* execute_cmd.c:1701-1726 */
```

so the negation *does* run for `( ! q[1x]=v )` (`rc=0`) and does *not* for
`( ! q[1x]=v; echo after )` (`rc=1`, the `!` sitting on an inner pipeline of a
`cm_connection` that the longjmp flies past). `user_subshell` is the gate, so a
`$( … )` gets no hoist — hence the `x=$( ! q[1x]=v )` row above. osh currently
gets the first two of those three right by accident (it negates
unconditionally) and the third wrong.

**The fix.** Skip the negation in `exec_pipeline` when the pipeline's flow is a
longjmp (`Flow::is_jump`, i.e. `Discard | Abort`) — `Flow::Exit` already
behaves, `( ! exit 3 )` being 3 in both. Then give the *user-subshell* executor
bash's hoist explicitly: when its body is a single negated pipeline, take the
`!` off it and apply it to the subshell's own result, including the one a
longjmp produced. Doing only the first half would regress `( ! q[1x]=v )`.
Both arming paths are affected, so this is one fix, not two.

**Fixed** as described, and the hoist needed no AST surgery. `exec_pipeline`'s
negation is now guarded by `!flow.is_jump()`, and the `Command::Subshell` arm
applies the negation to the subshell's own status when — and only when — the
body unwound *and* `Shell::subshell_hoists_invert` says the `!` was on the
whole of it (one item, no `&&`/`||` rest, the pipeline negated). Asking only on
an unwind is exactly equivalent to bash's strip-then-reapply: for an ordinary
return the pipeline already negated its own status on the way out, which is the
same answer.

Corpus: `a-bang-does-not-negate-a-status-an-unwind-carried.sh`, 25 rows.
Unit test: `interp::tests::a_bang_does_not_negate_a_status_an_unwind_carried`.

Found while measuring TD-OILS-A-DISCARD-STOPS-AT-THE-EVAL-OR-FUNCTION-IT-WAS-RAISED-IN;
it was the one row of that probe still diverging after the fix, and it diverged
for the pre-existing `declare -i` path too.

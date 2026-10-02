### TD-OILS-EVAL-SWALLOWS-RETURN. `return` inside `eval` does not return from the enclosing function — 2026-08-01 — ✅ **RESOLVED 2026-08-01**

**Where:** `userspace/oils/src/interp.rs` — the `eval` builtin, which runs its
argument through the same read-eval path a sourced file uses and then reports
only a status, so a `Flow::Return` raised inside the eval'd text stops there.

**What:** bash's `eval` is not a return boundary. The text runs in the calling
context, so a `return` in it returns from whatever function or sourced script
encloses the `eval`, taking the rest of that body with it.

```
$ cat z.sh
g() { eval "echo a; return 3; echo b"; echo "after eval"; }
g
echo "g rc=$?"
$ bash z.sh          $ osh z.sh
a                    a
g rc=3               after eval
                     g rc=0
```

The same swallowing happens for a subshell that encloses the eval:
`f() { ( eval "return 2"; echo sub ); }` prints `sub` under osh and nothing
under bash.

**Proper fix.** `eval` already has a side channel for the two flows a builtin
cannot express directly — `pending_builtin_exit` for `Flow::Exit` and
`pending_abort` for `Flow::Abort` (see the `Flow::Abort` docs). `Flow::Return`
needs the same treatment: a `pending_return` set by `eval` (and re-raised by
its caller in `exec_simple`) so the return unwinds past the builtin into the
enclosing function/source frame, where `return`'s own "can only return from a
function or sourced script" check has already decided it is legal. `break` and
`continue` inside an `eval` want the identical treatment and should be checked
in the same pass.

**Impact.** Found while measuring the extdebug DEBUG-trap verdict (a DEBUG trap
returning 2 simulates a `return`), where the probe harness ran its cases through
`eval` inside a function and so could not observe the unwind. Any script using
`eval` to build a guard clause — `eval "$check" || return 1` is unaffected, but
`eval 'return 1'` is not — keeps running past the return.

**✅ RESOLVED 2026-08-01.** A `pending_unwind: Option<Flow>` field now carries
the flow across the builtin boundary, converted back in `run_builtin_body`'s
teardown beside `pending_builtin_exit` and `pending_abort`. `eval_string` puts a
`Flow::Return` there; the shared `read_eval_builtin_status` puts a
`Flow::Break`/`Flow::Continue` there for `eval` **and** `.`/`source` alike,
which was the second half of the bug — a `break` in a sourced file did not end
the loop the `.` stood in either. A `return` in a sourced file is still caught
where it was, since ending the file is what it means. Measured against bash and
locked down by `tests/corpus/eval-and-source-are-not-a-boundary-for-return.sh`.

### TD-OILS-UNBOUNDED-SHELL-RECURSION-OVERFLOWS-THE-RUST-STACK. `f() { f; }; f` aborts the process instead of unwinding — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Fixed by a stack-headroom guard rather than a depth count.** The binary
already runs the shell on a thread whose stack *it* sizes (`INTERP_STACK_SIZE`,
64 MiB), so it can tell the shell how much of it to use:
`Shell::set_stack_budget` takes three quarters of that, `Shell::new` records the
stack origin, and `Shell::exec_command` — the one point every nested construct
descends through, so function bodies, `eval`, sourced files, loop bodies and
subshells alike — refuses to go deeper once the origin is that far above the
current frame. The command fails with `maximum nesting level exceeded (out of
stack)` and status 1, and the recursion unwinds normally, so the `EXIT` trap
still runs and output is still flushed.

A *measured* budget rather than a frame count because the two are not
proportional: a shell level costs a different number of Rust frames through a
function call than through an `eval` or a nested compound command, so any single
depth number would be both too low for one path and too high for another. The
measured ceiling lands at ~4400 nested function calls in a debug build (the
overflow was at ~5900), and rises on its own in a release build and on a bigger
stack. `FUNCNEST` is unchanged and still the lower, explicit, bash-compatible
ceiling. The budget defaults to `None` — unguarded — so an embedder that never
says how big its stack is gets exactly the old behaviour.

Covered by the `a_runaway_recursion_stops_instead_of_overflowing_the_stack` lib
test, which sets a small budget so the ceiling is reached in a few frames. No
corpus case is possible: bash segfaults on the same input, so there is nothing
to diff. The original text follows.

---



**Where:** `userspace/oils/src/interp.rs` — `Shell::call_function` and the
compound-command evaluators it re-enters (`exec_command`, `exec_pipeline`, …).
`FUNCNEST` is honoured (`Shell::funcnest_limit`, verified byte-for-byte against
the reference bash), but it is only consulted when the variable is *set*.

**What:** with `FUNCNEST` unset — the default — a runaway recursion has no depth
guard at all. The evaluator is a recursive tree-walker, so each shell-level call
costs several Rust frames, and `f() { f; }; f` runs until the thread's stack is
exhausted:

```
$ osh -c 'f() { f; }; f'

thread '<unknown>' (66612) has overflowed its stack
$ echo $?
127
```

The reference bash is no better behaved here — it segfaults, status 139 — so
this is not a *differential* failure and the corpus cannot cover it (neither
shell produces usable output). It is a robustness problem: a Rust stack
overflow is an immediate `abort()`, so no `trap`, no `EXIT` handler and no
partial output survives it, and the shell cannot be embedded in a process that
must stay up.

**Proper fix:** carry an evaluator-depth counter on `Shell`, incremented in
`call_function` and in every compound-command evaluator that re-enters, and fail
the *command* — not the process — past a ceiling chosen to sit comfortably below
the real stack (bash's own `-DEVALNEST` guard reports `maximum eval nesting
level exceeded` and unwinds to the top level; the same shape works here, and the
existing `FUNCNEST` message is the model for the wording). The counter should be
the one `FUNCNEST` already consults, so the explicit limit becomes a *lower*
ceiling on the same guard rather than a second mechanism. Deferred only because
it wants the whole re-entrant surface enumerated first — missing one evaluator
leaves the hole open on that path.

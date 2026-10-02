### BUG-OILS-SOURCE-FRAME-SEMANTICS. `.`/`source` (and `eval`) were not a trap frame, swallowed `exit`, and sent their output to the real fd 1 instead of the caller's sink — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** Four independent divergences from bash, all rooted in
`builtin_source`/the `eval` arm treating the nested script as an unrelated
top-level run. Every expectation below was captured from bash 5.x running the
identical script (`C:/Program Files/Git/usr/bin/bash.exe`), not inferred:

| Case | bash | osh (before) |
|---|---|---|
| `trap 'S+=" ret"' RETURN; . p.sh` | RETURN fires when the script finishes | never fired |
| `trap 'D+="\|$BASH_COMMAND"' DEBUG; . p.sh` | DEBUG masked *inside* the script unless `set -T` | fired for every command inside |
| `x=$(. p.sh)` / `x=$(eval echo hi)` | captures the output | empty — output escaped to the real fd 1 |
| `. quitter.sh` / `eval 'exit 3'` where the script runs `exit N` | shell exits `N` | `N` became the builtin's status; the shell continued |

**The exact bash rules** (probed, since the manual is ambiguous here):

* A sourced script is a trap frame **like a function** for `DEBUG` (masked
  without `functrace`), but **not** for `RETURN` — a nested `source` fires its
  own RETURN trap even without `set -T` — and **not** for `ERR`, which fires
  inside a source regardless of `errtrace`.
* The source's *own* RETURN firing is not gated on functrace, but **is** gated
  on the **caller's** mask: `h() { . p.sh; }; h` with no `set -T` fires neither
  the source's nor the function's RETURN; with `set -T` both fire.
* Under `functrace` one extra DEBUG fires immediately before the source's
  RETURN action, with `$BASH_COMMAND` still the `. script` word — mirroring the
  function case.
* `return N` from the script still fires RETURN and its status survives the
  handler.

**Fix** (`userspace/oils/src/interp.rs`).

1. `builtin_source` pushes `TrapSuppress { debug: !functrace, ret: false,
   err: false }` around the script and **pops it before** firing the source's
   RETURN trap, so the firing is checked against the caller's frame.
2. Generalised the builtin unwind side channel: `pending_signal_exit` →
   **`pending_builtin_exit`**, and moved its `.take()` from inside the `kill`
   arm to the tail of `run_builtin` — after the stderr-stack / scoped-fd /
   temporary-assignment teardown, and ahead of the `arith_error` discard. Any
   builtin can now unwind the shell; `source` and `eval` are the new producers.
   (A builtin handler returns `i32`, so this channel is the only way.)
3. Factored `run_source_flow_out(src, out) -> Flow` as the common core of
   `run_source_out` and `run_source_flow`, and routed both `source` and `eval`
   through it with the **caller's** `out`, fixing the capture bug and giving
   both access to the uncollapsed `Flow::Exit`.

**Tests.** `return_trap_fires_for_sourced_script` (6 cases: natural end,
`return N`, nested source, source-in-untraced-function, ditto with `set -T`,
function-inside-source), `sourced_script_is_a_debug_trap_frame` (DEBUG masking
both ways + ERR unaffected by `errtrace`),
`eval_and_source_write_to_the_callers_sink`, `exit_unwinds_out_of_eval`, and
the `exit`-in-RETURN-trap case in `exit_unwinds_out_of_a_sourced_script`. A new
`run_marker(src, var)` helper asserts trap-firing *order* through an
accumulator variable, because trap handler stdout still bypasses the capture
sink (TD-OILS-TRAP-CAPTURE, still open). 718 + 13 tests pass, clippy clean.

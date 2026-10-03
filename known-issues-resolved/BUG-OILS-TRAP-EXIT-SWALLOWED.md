### BUG-OILS-TRAP-EXIT-SWALLOWED. `exit N` inside an `ERR`/`DEBUG`/`RETURN`/`EXIT` trap handler did not terminate the shell — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** In bash, an `exit N` executed inside a trap handler terminates the
**shell** with status `N`; the triggering command's continuation and everything
after it is unwound. `osh` ran the handler, discarded its control flow, and
carried on:

```sh
trap 'exit 7' ERR;   false; echo after   # bash: (nothing), status 7 | osh: "after", status 0
trap 'exit 5' DEBUG; echo a; echo b      # bash: (nothing), status 5 | osh: "a\nb"
set -T; trap 'exit 4' RETURN; f() { echo in; }; f; echo after
                                         # bash: "in", status 4    | osh: "in\nafter"
trap 'exit 9' EXIT                       # bash: shell exits 9     | osh: kept the pre-trap status
```

**Root cause.** `Shell::fire_trap_flow` already returned the handler body's
`Flow`, but the only caller was the thin wrapper `fire_trap`, which threw it
away (`let _ = …`). All five synchronous firing sites went through that wrapper.
`run_exit_trap_out` had the same hole independently — it ran the EXIT handler
with `let _ = self.exec_program(…)` and then unconditionally restored the
pre-trap status. (`kill -SIG $$` was unaffected: the self-signal path already
propagated the trap's `Flow::Exit`, which is why `self_kill_trap_exit_unwinds`
passed while the synchronous traps were broken.)

**Fix** (`userspace/oils/src/interp.rs`). Deleted the flow-discarding
`fire_trap` wrapper entirely, so the type system now forces every site to look
at the result, and documented on `fire_trap_flow` that a returned `Flow::Exit`
must be honoured. The five sites:

* `exec_and_or` (ERR) and `exec_pipeline` / `exec_simple_inner` (DEBUG) return
  `Flow::Exit(code)` directly — nothing frame-scoped is live there.
* `call_function` (entry-DEBUG under tracing, and the DEBUG+RETURN pair on
  return) records the outcome in a local `trap_exit: Option<i32>` and returns it
  **after** the frame teardown, never via an early `return`. Returning early
  would have leaked the trap mask, the locals frame, the `FUNCNAME` /
  `BASH_ARGC`/`BASH_ARGV` stacks and the saved positional parameters. An
  entry-DEBUG that exits also skips the function body, and a pending exit
  suppresses the later RETURN firing.
* `run_exit_trap_out` now keeps the handler's `Flow` and sets `last_status` to
  the handler's exit code when it is `Flow::Exit`, otherwise restores the
  pre-trap status.

**Tests.** `exit_inside_synchronous_trap_unwinds_shell` (ERR/DEBUG/RETURN, plus
a non-exiting handler to pin the "trap does not alter `$?`" rule) and
`exit_inside_exit_trap_overrides_status`. Both failed before the change. Full
suite: 713 + 13 pass, `cargo clippy -p oils --all-targets` clean.

**Still open** in TD-OILS11: a `RETURN` trap is not fired for a returning
*sourced script* (only function returns), and async signal delivery.

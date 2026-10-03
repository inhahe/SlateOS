### TD-OILS-DEBUG-TRAP-VERDICT-IN-A-PIPELINE. The extdebug DEBUG-trap verdict is not applied to a pipeline stage — ✅ **RESOLVED 2026-08-01**

**Where:** `userspace/oils/src/interp.rs` — `exec_pipeline`'s parent-side DEBUG
loop used `fire_trap_flow`, so it honoured only an `exit` in the handler; the
`DebugVerdict` a non-zero status carries (see
`tests/corpus/extdebug-debug-trap-status-skips-a-command.sh`) was read only by
`exec_simple`.

**What the rule actually is.** The description this entry carried until now was
wrong, and wrong in an instructive way — it was written from measurements taken
through two separate flaws, both since found. It claimed that refusing *any*
bare stage takes the **whole pipeline** away, on the evidence of
`echo one > f1 | cat` leaving no `f1` when the second stage was refused. Both
halves of that were measurement error:

- the probe used `(exit N)` and `{ …; }` stages, which are **compound** and so
  are not announced by the owning shell at all — each is announced inside its
  own child, off that child's own counter — so the `K`th firing was not the
  stage the probe thought it was;
- the missing `f1` was a **race**, not a skip. bash does not wait for the
  stages of an abandoned pipeline, so the file genuinely is not there the
  instant the pipeline returns. Given a `sleep 1` to settle, it appears.

The rule, re-measured cleanly, is per-stage and has one exception:

- every **simple-command** stage is announced in the shell that owns the
  pipeline, left to right, before any stage starts;
- a refused **non-last** stage is simply never started. Its element vanishes
  from `${PIPESTATUS[@]}` and its neighbours are *not* joined to each other:
  the stage after it reads an immediate EOF and the stage before it writes into
  a pipe nobody holds. `$?`, `pipefail` and `!` then read only the stages that
  ran;
- a refused **last** stage abandons the pipeline's *result*, which is not
  really a rule about pipelines at all — the last stage is the one the owning
  shell runs itself, so refusing it returns out before anything is published.
  The pipeline answers 0 and leaves `${PIPESTATUS[@]}` exactly as the previous
  command left it, identical to refusing a lone command. The earlier stages
  have already been started and still run;
- the enclosing command carries on regardless (`if`, `&&`, `||`, `{ … }`), and
  `!` negates the abandoned 0 to 1;
- a **group** stage is announced inside its own child, so refusing that
  announcement costs a command *inside* a stage that runs either way — the
  stage stays in `${PIPESTATUS[@]}`, merely empty.

**Fixed** by giving `exec_pipeline` a `skipped: &[bool]`, one flag per stage,
filled in by the announce loop now that it reads a `DebugVerdict` rather than a
`Flow`. Both pipeline runners (`exec_threaded_pipeline`,
`exec_concurrent_pipeline`) take it and leave a refused stage unstarted,
dropping its endpoints so the hole propagates the way bash's does; the caller
then folds only the started stages into `${PIPESTATUS[@]}`. Refusing the last
stage sets `abandoned` and publishes nothing at all, for which a lone refused
simple command needed the same treatment — hence `Shell::debug_skipped`, set by
`exec_simple_inner`'s `Skip` arm and consumed by `exec_pipeline`.

**Known remaining divergence.** osh waits for the already-started stages of an
abandoned pipeline; bash does not. This is visible only in when side effects
land, not whether they do, so the corpus case settles with a `sleep` where it
reads them. Making osh not wait would mean orphaning stage threads, which is a
worse trade than the wait.

**Impact (before the fix).** A debugger could not step over a pipeline under
osh; the pipeline ran.

Covered by `tests/corpus/debug-trap-verdict-takes-a-pipeline-stage.sh`.

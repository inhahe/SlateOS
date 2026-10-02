### TD-OILS-DEBUG-TRAP-NOT-FIRED-FOR-A-BACKGROUND-COMMAND. `cmd &` is never announced — RESOLVED 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — the `&` path (the async-job branch
of the pipeline/list driver) reaches the child without passing through
`exec_simple`'s DEBUG firing in the *parent*.

**Reproduce:**

```sh
b() { echo "B:[$BASH_COMMAND]"; }
trap b DEBUG
echo one &
wait
```

bash prints `B:[echo one]` (from the parent, before the fork) and then
`B:[wait]`; osh prints only `B:[wait]`.

**Impact.** A step-debugger cannot see background commands.

**Fixed** by `Shell::announce_async`, called from the `&` branch of
`exec_items` before `exec_background`. It announces what a foreground pipeline
announces — each stage that is a simple command, left to right — and nothing at
all for an `&&`/`||` list, a compound command or a `time`d pipeline, which bash
hands to the child whole. The extdebug verdict is honoured: a refusal takes a
one-stage job away entirely (leaving 0), a `return 2` leaves the function the
job was written in, and an `exit` in the handler unwinds before the job starts.
Refusing one stage of a *longer* pipeline costs that stage and nothing else —
including the last stage, unlike a foreground pipeline, where refusing the last
stage abandons the pipeline's answer (see
`TD-OILS-DEBUG-TRAP-VERDICT-IN-A-PIPELINE`). The difference is not about
pipelines: a foreground pipeline's last stage is the one the announcing shell
runs itself, whereas here the announcing shell forks the job and runs none of
it. The refusals travel to the job's clone as `Shell::debug_stage_skips`, which
`exec_pipeline` honours without letting them abandon anything. The clone is
also marked `debug_announced` so it does not repeat the announcement (see
`TD-OILS-DEBUG-TRAP-PIPELINE-DOUBLE-FIRE`).

Covered by `tests/corpus/debug-trap-announces-a-background-command.sh`.

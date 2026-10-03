### TD-OILS-DEBUG-TRAP-PIPELINE-DOUBLE-FIRE. The DEBUG trap fires twice per pipeline stage under functrace — RESOLVED 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — `exec_pipeline` fires DEBUG in the
parent once per simple-command stage before the pipeline runs, and
`clone_for_subshell` hands the DEBUG trap on to the stage's own shell whenever
`functrace` is set (which `shopt -s extdebug` now also sets), so the stage fires
it a second time from `exec_simple`.

```
$ b() { echo "B:[$BASH_COMMAND]" >&2; }
$ ( set -T; trap b DEBUG; echo one | cat )
bash: B:[echo one] B:[cat]                  osh: B:[echo one] B:[cat] B:[echo one] B:[cat]
```

Without functrace both agree (the stage's shell resets the trap, so only the
parent-side firing is observable), which is why the existing corpus cases pass.

**Impact.** Any script that counts DEBUG firings (a step-debugger, a coverage
counter) sees double for pipeline stages under `set -T` or `shopt -s extdebug`.

**Fixed** by `Shell::debug_announced`: a one-shot flag, set on the clone a
pipeline stage (or a `&` job) is run in, that suppresses the *outermost*
DEBUG announcement there because the forking shell has already made it. It is
taken rather than tested, so it covers only the stage's own command — a
function stage still makes bash's extra entry-time firing from inside, and the
function's body commands still announce normally under functrace.
`exec_pipeline` decides whether the stages were announced (by itself, or by a
parent that forked it for this very pipeline) and passes that down to
`exec_threaded_pipeline`, which marks each simple-command stage's clone; the
`lastpipe` stage, which is not forked at all, is marked the same way.

Covered by `tests/corpus/debug-trap-announces-a-background-command.sh`, whose
last section runs the same pipelines in the foreground under `set -T` for the
comparison.

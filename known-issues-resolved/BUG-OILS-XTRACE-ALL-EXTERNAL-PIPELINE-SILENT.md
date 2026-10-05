### BUG-OILS-XTRACE-ALL-EXTERNAL-PIPELINE-SILENT. `set -x` emitted *no* trace at all for an all-external pipeline — 2026-07-31 — ✅ **RESOLVED 2026-07-31**

**Where:** `userspace/oils/src/interp.rs` — `exec_concurrent_pipeline`, the
per-stage expansion loop.

**Reproduce (found while measuring TD-OILS-XTRACE-PIPE-ORDER).**

```
$ bash -c 'set -x; cat /dev/null | cat'      $ osh -c 'set -x; cat /dev/null | cat'
+ cat /dev/null                              (nothing whatsoever)
+ cat
```

A single external command (`set -x; cat /dev/null`) traced fine, and a pipeline
with a builtin stage (`set -x; echo hi | cat`) traced both stages — just in the
wrong order (that is TD-OILS-XTRACE-PIPE-ORDER). Only the **all-external** shape
was silent, which is the shape most real scripts use, so this was much worse than
the ordering bug it was found under.

**Why it happened.** `exec_pipeline` routes a pipeline whose every stage passes
`stage_is_plain_external` to `exec_concurrent_pipeline`, which spawns the stages
with real OS pipes and `std::process::Command`. That function expands each
stage's words and assignment prefixes itself and never calls `exec_simple_inner`
— and the `set -x` trace lived *inside* `exec_simple_inner` as an inline block,
so this path had no way to emit it. The trace was not lost, it was never written.

**Fix.** The inline block became `Shell::xtrace_simple(&assigns, &argv,
with_command)` next to `xtrace_prefix`/`bxtrace_emit`, and
`exec_concurrent_pipeline` now calls it for each stage right after it expands
that stage. Because that loop expands the stages **in pipeline order on the
current thread, before any child is spawned**, this also gives bash's
left-to-right trace order for this path for free — TD-OILS-XTRACE-PIPE-ORDER is
now confined to `exec_threaded_pipeline`.

`with_command` is false when the command word expanded to nothing, matching
bash: `set -x; A=1 $e | cat /dev/null` prints `+ A=1` and then `+ cat /dev/null`,
never a bare prefix line.

**Verification.** New corpus case `tests/corpus/xtrace-pipeline.sh` — stage
order, temporary assignments, argument quoting, `PS4` per stage, and a pipeline
inside a command substitution (which nests the prefix one level). Every stage in
it is arranged to write nothing, so the merged stream holds only trace lines and
their order is not a race. Matches bash byte-for-byte.

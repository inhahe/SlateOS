### TD-OILS-CORPUS-XTRACE-PIPELINE-FLAKE. `tests/corpus/xtrace-pipeline.sh` failed once in a full run and has not reproduced — 2026-07-31 — ✅ RESOLVED 2026-07-31 (test bug, not a shell bug)

**Symptom.** One full `scripts/osh-bash-diff.py` run reported
`172 matched, 0 waived, 1 failed` with `X xtrace-pipeline`. Re-running that
case alone gave `1 matched, 0 failed`, and the very next full run gave
`173 matched, 0 failed`. Not a timeout — both shells produced complete
output.

**Why it is plausible.** osh runs a pipeline whose stages include a
builtin on a *threaded* executor rather than by forking, so two stages'
`set -x` lines are written by two threads to one stderr. bash forks, so
its ordering is fixed by process. If the interleaving is what differed,
it is a real (if benign) nondeterminism in osh and not just a flaky test.

**What was ruled out.** The section's five pipelines
(`true | cat`, `cat /dev/null | true`, `true | true`, `true | cat | true`,
`A=1 true | B=2 true`) were run 40× serially and 150× across six
concurrent processes: byte-identical every time. So either the flake is
elsewhere in the case, or it needs load the harness produces and this did
not. The failing run's diff was not captured — the next occurrence should
be, with both shells' output side by side, before any fix is attempted.

**2026-07-31 — the same nondeterminism reproduced in-process, which is a
much better handle on it.** A full `cargo test -p oils --lib` run failed
`interp::tests::a_pipelines_stages_begin_in_pipeline_order`; the test
passed on its own immediately afterwards and the next full run passed
too. That test asserts the `set -x` trace order of all-builtin pipelines
(`{ set -x; true | :; } 2>&1` → `+ true\n+ :\n`), so it exercises exactly
the stage-start handshake (`Shell::signal_stage_started`) suspected here —
no bash, no harness, no subprocess. Reproducing it needs the *parallel*
test load, which is consistent with "needs load the corpus harness
produces". Next step: run that one test under `--test-threads` pressure
in a loop (e.g. `cargo test … a_pipelines_stages_begin_in_pipeline_order
-- --test-threads=16` repeatedly, or a loop harness inside the test) to
get a capture of the wrong order, then audit the handshake for the case
where a stage signals started before its trace has been written.

**2026-07-31 — root-caused and fixed (a test bug, not a shell bug).** The
in-process test failed again in a full run, this time with its output
captured: `stages wrote out of pipeline order: "B\nA\n"` from the
assertion `{ { echo A >&2; } | { echo B >&2; }; }`.

The handshake orders each stage's **start**, and that is all it can order.
`Shell::signal_stage_started` fires once, from `exec_simple_inner`, after
the stage's first command has been *traced* and before it *runs* — so
everything the stage writes afterwards is concurrent with the stages
downstream of it. It could not be otherwise: a stage made to finish before
its successor began would deadlock the moment it filled its pipe (which is
why the wait is a bounded 100 ms preference, not a lock). bash promises no
more either — its stages are concurrent processes, confirmed by putting a
`sleep 0.3` in the first stage's function body, where both shells print
the second stage's trace *between* the first stage's two lines:

```
$ bash -c '( set -x; f() { sleep 0.3; true; }; f | cat ) 2>&1'
+ f / + sleep 0.3 / + cat / + true          # osh: identical
```

So two tests were asserting an ordering neither shell provides:

* the unit test's last assertion, which compared ordinary stderr writes
  rather than the stage starts — replaced with one that checks what is
  actually guaranteed (`f | :` starts with `+ f`), and the test's doc
  comment now says the start is the whole of the promise;
* `tests/corpus/xtrace-pipeline.sh`'s `f() { true; }; f | cat`, whose
  `+ true` (from inside the body, after the signal) and `+ cat` were a
  coin flip. Every other pipeline in that case is already immune — its
  stages trace one line each, or the concurrent lines are spelled the same
  — so this one became `f | true`, and the case's header now explains the
  arrangement instead of claiming the order "is not a race".

The corpus flake this entry opened with is the same race, from the same
line: `f | cat` was the only case in the file that could interleave
observably.

### TD-OILS-XTRACE-PIPE-ORDER. `set -x` traces multi-stage pipeline commands in reverse (last-stage-first) order rather than bash's left-to-right — 2026-07-20 — ✅ **RESOLVED 2026-07-31**

**Where:** `userspace/oils/src/interp.rs` — `exec_threaded_pipeline`. Each
pipeline stage emits its own xtrace line from inside `exec_simple_inner` when it
runs. **Narrowed 2026-07-31:** the all-external `exec_concurrent_pipeline` is no
longer affected — it expands its stages in pipeline order on the current thread
and now traces there (BUG-OILS-XTRACE-ALL-EXTERNAL-PIPELINE-SILENT), so it emits
bash's order. What remains is any pipeline with a builtin, function or compound
stage, e.g. `echo A | cat`.

**What:** `set -x; echo A | cat` prints, in bash:
```
+ echo A
+ cat
```
but in osh:
```
+ cat
+ echo A
```
The trace *content* (fully-expanded, one `+ ` line per stage) is identical; only
the line ordering within the pipeline differs.

**Determinism, re-measured 2026-07-30.** bash is stable left-to-right (5/5 runs
of a three-stage pipeline). osh is stable only for **two** stages, where the
reversal is total; with three or more the spawned workers race each other and the
order is luck — `echo p | cat | cat > /dev/null` happened to come out in bash's
order in 5/5 runs. So this is not merely "a different deterministic order": for
n ≥ 3 osh has no order at all. (bash's own order is a race in principle too — its
children trace after the fork — but it is observably stable, so a corpus case may
depend on it and osh should reproduce it.)

**Not confined to `xtrace`, also re-measured 2026-07-30.** What is wrong is the
*stage start order*, so anything a stage writes to stderr shows the same
reversal (`{ echo A >&2; } | { echo B >&2; }`). The `set -x` case is just the
easiest one to see, which makes this less cosmetic than the original write-up
assumed.

**Why it happens:** osh runs the pipeline's **last** stage synchronously on the
current thread (required so `shopt -s lastpipe` can keep its mutations/flow, and
to avoid an extra thread) while stages `0..n-1` run on worker threads that the OS
has not necessarily scheduled yet. The current thread therefore reaches the last
stage's `exec_simple` — and emits its trace — before the workers emit theirs.

**Proper fix (refined 2026-07-30).** Make the stages *begin* in pipeline order,
which keeps the "last stage on the current thread" design intact. A handshake at
thread entry (thread `i` waits for thread `i-1` to signal, signals, then runs —
and the caller waits for the last signal before running stage `n-1`) is the shape
of it, but for the order to be *guaranteed* rather than merely likely the signal
must be sent after the stage has emitted the trace for its leading command, not
merely at thread entry. So the "stage has started" point has to sit inside the
execution path: a one-shot `Option<Sender<()>>` on the stage's `Shell`, fired
from `xtrace_emit`/`exec_simple` on the stage's first command *and*
unconditionally on stage completion, so a stage that traces nothing cannot
deadlock the chain.

Two approaches to *avoid*: (a) hoisting the trace into the parent and expanding
each stage's words there would run each stage's command substitutions **twice**
(verified: bash expands each stage exactly once, in the child subshell); (b)
pre-tracing only the simple-command stages in the parent, the way the `DEBUG`
trap loop does, leaves a compound stage (`{ …; …; } | cat`) with no first command
for the parent to print — the bug would survive for exactly the stages where it
is hardest to see.

**Resolved 2026-07-31**, as the refined sketch above. `Shell::stage_started` is
an `Option<mpsc::Sender<()>>`; `exec_threaded_pipeline` builds n−1 channels, hands
stage *i* the sender of channel *i* and makes it await channel *i−1* before it
begins, and the current thread awaits channel *n−2* before running the last
stage. `Shell::signal_stage_started` fires the sender (taking it, so it is
one-shot per shell) from `exec_simple_inner` **immediately after** the trace is
emitted — which is what makes the trace order guaranteed rather than likely —
and again unconditionally when a worker's stage finishes, so `x=1 | true`, an
empty group, or a stage that dies cannot stall the chain.

Two properties worth keeping in mind if this is ever touched again:

- **The sender is *cloned* into subshells** (`clone_for_subshell`), not reset.
  A stage whose body is a subshell or a command substitution — `(yes) | head` —
  would otherwise never signal until it ended, which for an unbounded producer
  means never.
- **The wait is bounded** (100 ms, `start_wait`) and a lapsed wait simply runs
  the stage. The ordering is a strong preference, not a lock, so no arrangement
  of stages can deadlock on it — bash's own order is a race in principle too.

Measured after the fix: `set -x; echo A | cat` and the three-stage version give
bash's order in 5/5 runs each, and `{ echo A >&2; } | { echo B >&2; }` prints
`A` then `B` (the non-xtrace half of the bug). No pipeline shape tried took the
100 ms path: `(yes) | head -2`, `x=1 | cat`, `seq 1 5 | while read …`, and the
`lastpipe` variant all complete in <90 ms wall clock.

**Pinned by** `interp::tests::a_pipelines_stages_begin_in_pipeline_order` (which
also asserts the non-xtrace half — that a stage's ordinary stderr output follows
the same order) and by `tests/corpus/xtrace-pipeline.sh`, which now covers both
executors: external stages, builtin stages, function and compound (`{ … }`,
`( … )`, `for`) stages, assignment-only stages, `PS4` across stages, and a
pipeline inside a command substitution. Every stage in it writes nothing to
stdout, so the merged stream holds only trace lines and the case cannot be flaky
for a reason unrelated to the ordering (verified: 6/6 clean runs).

### TD-OILS-KILL-PIPELINE-STATUS. Killing a background *pipeline* gives it the signal's status, where bash gives it the last stage's own exit — OPEN 2026-07-27 — ⚠️ **NOT REPRODUCIBLE on the reference shell, re-measured 2026-07-31**

**Where:** `userspace/oils/src/interp.rs` — the `JobBody::Thread` arm of
`Shell::kill_one`.

**What.** `sleep 1 | cat & sleep 0.1; kill -TERM %1; sleep 0.2; jobs` lists
`Done` in bash and `Terminated` in osh.

**Why.** A pipeline in bash is several processes, and `kill %1` signals the
leading stage. `sleep` dies of SIGTERM, `cat` then reads EOF and exits 0 — and a
pipeline's status is its *last* stage's, so the job is a clean `Done`. osh runs
the whole pipeline as one thread-backed job, so there is only one thing to
signal and its status is the signal's.

**Proper fix.** Needs per-stage job membership: record each stage of a pipeline
job separately, signal the leader, and take the job's status from the last
stage. That is the same structural gap as TD-OILS-KILL-PGRP and the
thread-backed-job limitations in TD-OILS13 §1 — a job is currently one entry,
not a set of processes.

**Impact.** Narrow, and only observable through the `jobs` state word: `wait`
already reports the last stage's status. Kept out of
`tests/corpus/kill-dispositions.sh`, which signals only single-process jobs.

**Re-measured 2026-07-31: the reference shell does not show this.** The exact
script from the write-up, five runs each, gives the *same* answer in both:

```
$ bash -c 'sleep 1 | cat & sleep 0.1; kill -TERM %1; sleep 0.2; jobs'
[1]+  Terminated              sleep 1 | cat            (5/5)
$ osh  -c '…same…'
[1]+  Terminated              sleep 1 | cat            (5/5)
```

and `wait $!` after the kill reports 143 in both. The same holds when a stage is
a builtin or a group (`sleep 1 | { cat; }`, `{ sleep 1; } | cat`), i.e. for the
threaded executor as well as the concurrent one.

The likely explanation is that MSYS bash's `kill %1` reaches *every* member of
the pipeline rather than only the leader, so `cat` is terminated too and the
pipeline's status is a signalled one — exactly what osh reports for its own
reason. On Linux, where the leader alone is signalled and `cat` then exits 0 on
EOF, the original `Done` is presumably still what bash prints.

So this stays **open as a structural gap**, not as a measured divergence: a job
is still one entry rather than a set of processes, which is what
TD-OILS-KILL-PGRP and TD-OILS13 §1 also need. But it must not be "fixed" by
copying the write-up's expectation — there is no way to verify the target
behaviour from this host, and changing osh to report `Done` here would *create*
a divergence from the shell the corpus actually compares against. Revisit when a
Linux reference bash is reachable.

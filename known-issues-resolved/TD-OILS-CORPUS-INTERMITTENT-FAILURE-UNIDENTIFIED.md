### TD-OILS-CORPUS-INTERMITTENT-FAILURE-UNIDENTIFIED. some corpus case — not always the same one — fails about 1 run in 4 and has never been caught in the act — 2026-08-03 — ✅ **RESOLVED 2026-08-03** (osh's stack guard, anchored on the wrong thread)

**Symptom.** A full `scripts/osh-bash-diff.py` sweep occasionally reports
`299 matched, 0 waived, 1 failed`. Both times it has been seen, the very next
sweep was clean. The failing case's *name* was captured once —
`nounset-last-bg-pid` — and lost the second time, because the run's stdout was
piped through `tail -6` and the `X <case>` line with its diff had already
scrolled past.

**What has been ruled out for `nounset-last-bg-pid`:**

* 8 runs of that case alone through the harness: all matched.
* 40 direct invocations of the script under *each* shell in a scratch cwd,
  comparing stdout hashes: one hash, both shells, byte-identical.
* 60 more osh invocations comparing status, stdout hash and stderr length: all
  identical.
* 2 concurrent full sweeps (600 case measurements under real load): both clean.

So either the failure needs a form of load these did not reproduce, or the
second occurrence was a *different* case and the one identification is a
coincidence. The `true & wait` lines (49–52, 56) are the obvious suspect in
that case — they are the only ones that start a child — but nothing has been
shown to misbehave there.

**What was done instead of guessing:** the harness now writes every failing
case's full measurement — both shells' stdout, stderr and status — to
`target/dvscratch/corpus-failures/<timestamp>/<case>.txt` (commits "scripts:
keep a failing corpus case's full measurement on disk" and "scripts: give each
corpus run its own failure-report directory"). The next occurrence therefore
captures itself whether or not anyone is watching and whether or not the
summary was truncated.

**Third occurrence, 2026-08-03, and a harness bug it exposed.** A `-k array`
run (10 cases) reported `9 matched, 0 waived, 1 failed`; the immediate re-run
was clean, as always. The report file *had* been written — but the first
version of the mechanism emptied the report directory at the start of every
run, so the re-run deleted it. The reflex on seeing `1 failed` is to run it
again, which was precisely the command that destroyed the evidence. Each run
now gets its own timestamped directory and a clean run creates none.

The occurrence still says something. The failing case was one of the first
seven of `array-literal-partial`, `arrays`, `assoc-arrays`,
`declare-array-kind-conflict`, `declare-nameref-array-refusal`,
`dynamic-var-array`, `nounset-array-element` — so it was **not**
`nounset-last-bg-pid`, and that one identification really was a coincidence.
Whatever this is, it is not tied to a single case, which fits the "needs load,
takes whichever case is running" reading and retires the `true & wait` theory
above. It also happened in a *ten*-case run, so it does not need a full sweep's
load either.

**Fourth occurrence, 2026-08-03 — caught, and it was osh.** The next full sweep
reported `301 matched, 0 waived, 1 failed` and this time the report survived:
`target/dvscratch/corpus-failures/20260803-184518/bang-parameter.txt`. osh had
written seventeen copies of

```
case.sh: line 34: maximum nesting level exceeded (out of stack)
```

to stderr and silently dropped the output of every line that produced one. Line
34 is `( eval "echo \"[$b]\"" ) 2>&1 | sed …` — a subshell in a *pipeline*.

**Root cause.** The depth guard measures how far execution has descended from an
origin (`stack_base`) recorded by `Shell::new` on the thread it was built on.
Every pipeline stage but the last, every `&` job and every coproc body runs a
*subshell clone* on a freshly spawned thread — and the clone copied the parent
thread's `stack_base`. Two unrelated stack allocations were then being
subtracted from one another, so `stack_base - stack_mark()` was not a depth at
all. Which way it read depended only on where the OS happened to place the new
thread's stack: above the parent's origin (the common case) and the difference
saturated to zero, leaving the stage *unguarded*; more than the 48 MiB budget
below it and every single command in the stage was refused. Thread stacks are
placed by ASLR, so the same script failed roughly one run in four — and always
a case that pipes a subshell or a shell function, which is why it moved from
case to case.

The unguarded direction was the worse of the two: a deep recursion inside a
pipeline stage had no ceiling at all *and* only the platform-default ~1 MiB
stack to overflow, which aborts the process outright.

**Fix** (commit "oils: anchor the stack guard on the thread that is running"):
`Shell::rebase_stack` re-records the origin on the running thread, and the three
spawn sites (pipeline stage, `&` job, coproc body) now go through
`spawn_shell_thread`, which reserves the same 64 MiB `SHELL_STACK_SIZE` the
binary's interpreter thread gets and calls `rebase_stack` with it before
evaluating anything. `INTERP_STACK_SIZE`/`stack_budget` moved out of `main.rs`
into the library so there is one figure. A spawn failure is now reported and the
stage/job/coproc is skipped, where the old `thread::spawn` would have panicked.
Regression test: `a_spawned_shell_thread_anchors_the_guard_on_its_own_stack`.
`bang-parameter` ran 12/12 clean afterwards.

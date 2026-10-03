### [A] ctest-pty exit 45 is a scheduling question, not necessarily a budget one -- 2026-09-21
**Status:** RESOLVED 2026-09-24 — neither scheduling nor budget. The `^C` was never turned into a `SIGINT` at all, because the kernel only looked for it when the child read the terminal, and the child never reads. See `A-PTY-CTRL-C-IS-ONLY-SEEN-BY-A-READER` at the end of this file.

**In short:** a test waits for its child to finish by asking repeatedly, up
to a fixed number of tries, yielding the processor between asks. It runs out
of tries. The tempting fix is more tries. That is only right if the child was
going to run eventually.

**What exit 45 actually is.** `services/ctest-pty/main.c:489` -- the
`waitpid(kid, &status, WNOHANG)` loop completed `SPIN` iterations without the
child being reaped. The fixture's own comment states the dependency it is
resting on: *WNOHANG means the child must run to exit, and it cannot while
this loop owns the quantum.*

So the rung passes only if `sched_yield()` hands the CPU to the child.

**`sched_yield` is not a no-op, which was the first thing worth ruling out.**
`sys_sched_yield` -> `sched::yield_now()` -> `schedule_inner(true,
SwitchKind::Voluntary)`, which reports an RCU quiescent state and
**re-enqueues** the caller (`PER_CPU_SCHED.enqueue(current_id, prio, cpu)`).
It is a real voluntary reschedule.

**What is therefore still open, stated as a question rather than a theory:**
whether the child is *picked* after the parent re-enqueues. That depends on
the per-priority queue discipline and on CPU placement -- the queues are
per-CPU, so a child enqueued on a CPU that is not scheduling would starve
regardless of how many times the parent yields. I have not established
either, and will not guess: the last two diagnoses of this fixture family
were guesses from an error message and both were wrong.

**Why this matters more than the rung.** If the child can starve, raising
`SPIN` makes the test pass by spinning longer against a fairness bug --
converting a reproducible failure into an intermittent one, which is strictly
worse. If the child cannot starve, `SPIN` is simply too small and raising it
is correct and boring. Those need different work and the log cannot tell them
apart.

**Next step:** instrument the pick, not the budget. A one-line count of how
many times the parent yielded while the child stayed un-picked separates the
two cases in a single boot.

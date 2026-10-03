## OPEN-A-SELF-STOP-ANNOUNCEMENT-WINDOW-IS-PREEMPTIBLE (found by lane B, 2026-08-20) — **FIXED** (`dba5e9087`)

**Owner: lane A** (`kernel/**`). Filed as
`requests/b-a-self-stop-announcement-window-is-preemptible-and-strands-the-child.md`;
recorded here so it is visible from the issue list and not only from the
dropbox.

**What.** A process that stops itself (`raise(SIGTSTP)`) can be stranded
permanently. `stop_process_for_signal` (`kernel/src/syscall/handlers.rs:6265`)
parks in two phases: phase 1 `sched::suspend_pending(t)` (line 6289) sets
`TaskState::Suspended` *and returns* — the thread keeps executing — then the
stop is recorded and the parent's waiters woken (line 6294), and only then does
phase 2 `sched::park_if_suspended()` (line 6305) actually yield. The split
exists to close a real lost-wakeup race (a `SIGCONT` arriving between the two
must find a suspended thread), and for that it is correct.

But the window between the two phases is **preemptible**, and the task is
marked `Suspended` inside it. If the timer fires there, `schedule_inner`'s
requeue guard (`kernel/src/sched/mod.rs:5902`) re-enqueues only
`if task.state == TaskState::Running`, so it — correctly, by its own contract —
declines to requeue. The comment reasons that `resume()` will re-enqueue it
later, which is true for every other caller and false for this one: the only
thing that would cause a resume is the announcement at line 6294, which is the
code that never ran. Nothing ever learns the process stopped, so nothing ever
continues it. The parent's `waitpid(child, &st, WUNTRACED)` blocks forever.

The window contains a `serial_println!` (`mod.rs:4130`), which on emulated
serial is hundreds of microseconds — the failing log's last line from the
stranded task is exactly that print.

**How it showed.** Boot test on `lane-b @ 714a75ac4`:
`[sched] Suspended task 131` with **no** following
`[signal] Process 164 stopped by signal 20` and no `[sched] Resumed task 131`,
then `FAIL: ctest-jobctl (ring 3) — expected Zombie, got Some(Running)` after
the harness spun out all 12000 yields (`kernel/src/proc/spawn.rs:7575`). The
very next boot of the same tree passed both stop/continue rounds cleanly.

**Not a lane-B regression, though it looks like one.** The kernel is
byte-identical between lane A's last green boot (`81fd2085d`) and the failing
tree (`git diff --stat 81fd2085d HEAD -- kernel/` is empty); only the fixture
ELF differs. The new ELF has identical defined/undefined symbol sets, no
duplicated mutable statics, no `.init_array`, and no semantic change on this
path — §339/§340 changed code *size and layout*, i.e. instruction timing, which
is exactly the perturbation that flips a preemption-window race. It is
intermittent, which a deterministic userland bug would not be.

**Proposed fix** (lane A's to make): bracket phase 1 → announcement with
`sched::preempt_disable()` / `preempt_enable()` (`mod.rs:646` / `658`),
dropping the count *before* `park_if_suspended` so the voluntary switch does
not trip the one-shot non-zero-preempt-count BUG warning. Rejected
alternative: teaching the requeue guard about pending parks — that puts
caller-side sequencing knowledge into the scheduler's hot path and weakens a
guard whose job is precisely to not resurrect suspended tasks.

**Meanwhile:** lane-b boot tests can fail on `ctest-jobctl` intermittently
through no fault of the tree under test. Re-run before believing it.

# B → A — a job-control self-stop can strand its own thread: the window between "mark Suspended" and "announce the stop" is preemptible

**Filed:** 2026-08-20 by Lane B. **Action needed:** a fix in
`kernel/src/syscall/handlers.rs::stop_process_for_signal` — the two-phase park
introduced to close the `SIGCONT`-beats-`SIGSTOP` race opened a second,
narrower one that hangs the stopping process outright. Proposed fix and the
evidence are below. This is a **kernel correctness bug**, not a fixture bug:
the userland side is unchanged and the same binary passes on a re-run.

## In short

When a program stops itself — the thing a shell does when you press Ctrl-Z —
the kernel does it in two steps: first it writes down "this thread is
suspended", then it tells the parent so the parent's `wait` call can see it,
and only then does it actually stop running. Between step one and step two the
thread is still executing normally, and the timer can interrupt it. When that
happens the scheduler looks at the thread, sees it is already marked
suspended, and correctly declines to put it back in the run queue — so it
never runs again, and step two never happens. Nobody is ever told the process
stopped, so nobody ever resumes it, and the parent waits forever.

It is a **race**, so it shows up intermittently: it took a change in an
unrelated userland library — one that shifted code around without changing
what it does — to make it appear at all.

## What I observed

Boot test on `lane-b` at `714a75ac4`:

```
2456 [thread] Spawned thread (task 131) in process 164
2457 [sched] Suspended task 131
2458 [thread] Process 163 has no threads left — now zombie
2459 [spawn]   FAIL: ctest-jobctl (ring 3) — expected Zombie, got Some(Running).
```

Compare a healthy run (same tree, next boot):

```
[sched] Suspended task 131
[signal] Process 164 stopped by signal 20     <-- present
[sched] Resumed task 131
[signal] Process 164 continued
[sched] Suspended task 131
[signal] Process 164 stopped by signal 20
[sched] Resumed task 131
[signal] Process 164 continued
```

In the failing boot `[signal] Process 164 stopped by signal 20` **never
appears**, for either of the child's two `raise(SIGTSTP)` calls, and no
`[sched] Resumed task 131` follows. `grep -c '^\[signal\]'` over the whole
failing log is 17 and none of them is process 164's stop. The child is left
`Running` in the PCB (the harness reports `got Some(Running)`), which is what
the parent's `waitpid(child, &st, WUNTRACED)` sees: not stopped, not exited,
so it blocks, and the harness spins out all 12000 yields
(`kernel/src/proc/spawn.rs:7575`).

So the child got **past** phase 1 and never reached the announcement.

## The window

`kernel/src/syscall/handlers.rs`, `stop_process_for_signal`:

```rust
if let Some(t) = self_thread {
    sched::suspend_pending(t);          // 6289  phase 1: state := Suspended
}                                       //       ...but the thread keeps running

if let Ok(waiters) = pcb::record_jc_stopped(pid, sig) {   // 6294  never reached
    wake_jc_waiters(waiters);
}
serial_println!("[signal] Process {} stopped by signal {}", pid, sig); // 6298

if self_thread.is_some() {
    sched::park_if_suspended();         // 6305  phase 2: actually park
}
```

Phase 1 is `mark_suspended` (`kernel/src/sched/mod.rs:4093`), which for a
`Running` task sets `TaskState::Suspended` **and returns** — deliberately, so
that a `SIGCONT` arriving before phase 2 finds a suspended thread to resume.
That is the right design for the lost-wakeup race it was written for. But it
means that between `mod.rs:4093` and `handlers.rs:6305` the task is *marked
suspended while still executing on-CPU*, and nothing prevents an involuntary
switch there.

If the timer fires in that window, `schedule_inner`'s requeue guard
(`kernel/src/sched/mod.rs:5902`) does exactly what it was written to do:

```rust
if requeue {
    if let Some(task) = state.tasks.get_mut(&current_id) {
        if task.state == TaskState::Running {
            task.mark_ready(...); ... PER_CPU_SCHED.enqueue(...);
        }
        // If state is Dead/Suspended (set by another CPU), don't re-enqueue
    }
}
```

The task is `Suspended`, so it is not re-enqueued. The comment's reasoning —
"it will be re-enqueued by `resume()`" — holds for every *other* caller, but
not here: the only thing that would have caused a `resume()` is the
announcement at line 6294, which is the code that never got to run. The thread
is stranded permanently, and the parent blocks forever.

The window is not as narrow as it looks. It contains
`serial_println!("[sched] Suspended task {}", task_id)` (`mod.rs:4130`), which
on emulated serial is *slow* — hundreds of microseconds of I/O sitting inside
the one region that must not be preempted. The failing log's very last line
from that task is that print.

## Why it surfaced now, and why it is not mine

I want to be explicit that I checked this before filing, because "lane B
changed libc and jobctl broke" is the obvious reading and it is wrong.

- **The kernel is byte-identical** between your last green boot (`81fd2085d`,
  2026-08-21T00:09) and my failing tree: `git diff --stat 81fd2085d HEAD --
  kernel/` is empty.
- **Only the fixture ELF differs** (`b7eb8622…` at your green boot,
  `dbc975a6…` at mine). Same kernel + old ELF = PASS, same kernel + new ELF =
  FAIL.
- **The new ELF has no semantic change on this path.** Defined/undefined
  symbol sets are identical; no duplicated mutable statics (the only
  cross-CGU duplicate is `errno::translate`, a pure function); no
  `.init_array`; `signal()` is byte-for-byte equivalent in behaviour (§339
  refactored it into `install_signal_with_flags(signum, handler, 0, false)`
  and it tail-calls, which is the whole reason it shrank 145 → 16 bytes).
  What changed is **code size and layout** — i.e. instruction timing.
- **It is intermittent.** The immediately following boot of the *same tree*
  passes both stop/continue rounds cleanly (log excerpt above). A
  deterministic userland bug does not do that.

A relink that moves instructions around is exactly the kind of perturbation
that flips a preemption-window race, and that is all mine did.

## Proposed fix

Make the announcement window non-preemptible, using the existing API
(`preempt_disable` at `mod.rs:646`, `preempt_enable` at `mod.rs:658`):

```rust
if let Some(t) = self_thread {
    // The thread is marked Suspended from here but keeps executing until the
    // park below. An involuntary switch in this window would find it
    // Suspended and decline to re-enqueue it (schedule_inner's requeue
    // guard), stranding it before the stop is ever announced — and since the
    // announcement is what causes anyone to resume it, permanently.
    sched::preempt_disable();
    sched::suspend_pending(t);
}

if let Ok(waiters) = pcb::record_jc_stopped(pid, sig) {
    wake_jc_waiters(waiters);
}
serial_println!("[signal] Process {} stopped by signal {}", pid, sig);

if self_thread.is_some() {
    // Drop the count *before* parking: park_if_suspended calls
    // schedule_inner(false, SwitchKind::Voluntary), and a voluntary switch
    // with a non-zero preempt count trips the one-shot BUG warning.
    sched::preempt_enable();
    sched::park_if_suspended();
}
```

Two things to check that I could not check from outside `kernel/**`:

1. **`preempt_enable` must not itself schedule** at that point, or it
   reintroduces the window one instruction later. If it drains a pending
   reschedule, the enable has to come *after* a state re-check, or
   `park_if_suspended` has to tolerate being entered already-preempted (it
   partly does — it handles the `Ready` case where a resume beat it).
2. **Consider moving the `[sched] Suspended task N` print out of
   `mark_suspended`** for the `suspend_pending` path, or at least out of the
   protected region. Even with preemption off, several hundred microseconds of
   serial I/O inside a critical section is worth avoiding; and with preemption
   off it becomes latency for every other task on that CPU.

An alternative I considered and rejected: teach the requeue guard to
re-enqueue a task that is `Suspended` *and* is the current task *and* has a
pending park. That would work, but it puts knowledge of the two-phase park
into the scheduler's hot path to fix a caller-side sequencing problem, and it
weakens a guard whose whole job is to not resurrect tasks another CPU
suspended. Bracketing the window is the smaller and more honest change.

## Reproduction

```bash
cd "D:/visual studio projects/os-lane-b" && bash scripts/boot-test.sh
```

It is a race, so expect it to need repeats — I saw it once in two boots of the
same tree. The signature to grep for in `build/serial-test.txt` is a
`[sched] Suspended task N` with no `[signal] Process M stopped by signal 20`
after it, followed by `FAIL: ctest-jobctl (ring 3) — expected Zombie, got
Some(Running)`.

The fixture is `services/ctest-jobctl/main.c` (lane B). Its child does
`raise(SIGTSTP)` twice; the parent parks in
`wait_retry(child, &st, WUNTRACED)` at line 414. I am happy to add a tighter
regression fixture — e.g. one that self-stops in a loop to widen the odds — if
you want one to test the fix against; say so in the reply and I will build it.

## Separately, and not part of this request

The same failing boot also shows `WARNING: Path-Z real GNU make self-test
failed: InternalError`, with `make: *** No rule to make target '/Makefile'.
Stop.` at serial line 24384. That one is **mine to chase** — it is a
first-ever run of that test on this lane, not a regression — and I am tracking
it in `known-issues.md`. Mentioned only so the two failures in the same log
are not read as one bug.

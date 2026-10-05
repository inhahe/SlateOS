## `C-TASKSCHEDULER-HAS-NO-EXECUTOR` (lane C, 2026-08-27) — **open**, missing backend

**In short:** The Task Scheduler now opens as a real window and everything in
it works — the task list, the add/edit form, the delete confirmation, the
history tab, keyboard and wheel — and it correctly computes *when* every task
is next due, including full cron expressions. What it cannot do is **run** one.
Nothing in SlateOS can yet start a process on another program's behalf, so a
task's command is a string the scheduler stores, displays and schedules, but
never executes. A user can build a perfectly correct schedule and nothing will
ever happen at the appointed time.

**What is missing, precisely.** A way to spawn a process and wait for its exit
status. That is a kernel/userland facility (lane A/B), not a graphics one: the
`posix` layer has the syscalls, but nothing exposes a "run this command line
and tell me how it went" service to an application in `apps/`.

**How the gap is held open rather than papered over.** A function-pointer seam
on `TaskScheduler`, following the precedent set by defrag's `ScanFn` (see
`C-DEFRAG-HAS-NO-WAY-TO-SCAN-A-DRIVE`) and pdfviewer's `OpenFn`/`PrintFn`:

| Seam | Type | Gate |
|---|---|---|
| `runner` | `RunFn = fn(&str) -> Result<u64, TaskRunError>` | `can_run()` |

`Ok(duration_ms)` records a success in the history and reschedules; `Err` goes
through the existing retry policy. The seam is `None` in the shipping binary.

**The policy that matters: with no runner, a due task stays overdue.**
`run_due_tasks` returns early when `runner` is `None` and — this is the part
worth reading twice — does *not* advance any task's `next_run_timestamp`. A
scheduler that could not execute a command but rescheduled the task anyway
would have moved a nightly backup's "next run" to tomorrow without ever having
run it, and the history would show nothing amiss. Leaving the task overdue is
the honest state, and it is the state the window shows.

**Alternatives rejected.** Fabricating a success so the history tab has
something in it would put a lie on screen and, worse, a lie about a backup
having run. A `runner` that always returns `Err` would fill the history with
failures the user cannot act on and would burn through each task's retry
budget. Both were rejected for the same reason as their equivalents in defrag
and pdfviewer.

**Proper fix.** When a process-spawning service exists, write the real `RunFn`
against it and call `scheduler.set_runner(...)` in `main`. Nothing in the UI
needs to change: the tick handler already calls `run_due_tasks` once a second,
so the window becomes live on the day the backend arrives. At that point
`can_run()` should also gate a user-visible hint — a scheduler that cannot run
anything ought to say so in its status bar — which is deliberately *not* done
now, because there is no second state to contrast it with.

**Where it bites:** `apps/taskscheduler/src/main.rs` — `RunFn`, `TaskRunError`,
`TaskScheduler::runner`, `TaskScheduler::run_due_tasks`,
`TaskScheduler::set_runner`, `TaskScheduler::can_run`, and
`SchedulerUI::refresh_clock`, which is the only caller.

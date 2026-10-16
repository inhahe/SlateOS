## A-KILL-ENDS-A-THREAD-WHEREVER-IT-IS-IN-THE-KERNEL (lane A, 2026-10-16) — OPEN

**Status:** OPEN (lane A). Raised by the Opus review of lane A's batch
0713e88ab..a53272118, about `ipc::semaphore::signal`; the hazard is
kernel-wide. The sibling of
`known-issues/A-JOB-CONTROL-STOP-SUSPENDS-A-THREAD-WHEREVER-IT-IS.md`.

**In short:** when one program kills another, the kernel ends each of its
threads wherever it is at that moment -- including in the middle of a system
call, between two steps that belong together. Whatever the call had done by
then stays done and the rest never happens. Linux instead marks the thread
and lets it finish its call (or leave a wait) and end itself when it is
about to return to its program, holding nothing and owing nothing.

### Where

`kernel/src/sched/mod.rs`, `kill_task_from`: a thread in any state but
exiting is marked `Dead`, and a running one is switched away at once (on
another CPU, at its next switch). A `Dead` thread never runs again, so no
code after the switch point runs: not the rest of the call, not its cleanup.

### What it breaks (instances found so far)

- **`ipc::semaphore::signal`** grants units to waiters a batch at a time,
  letting `SEM_TABLE` go between batches (a leaf lock: the wakes happen
  outside it). Killed between batches, the units it holds for the rest of the
  queue (`Semaphore::granting`) are neither granted nor counted: those
  waiters stay parked, and `max_count` stays reduced by them. Killed after a
  batch's grants and before its wakes, the granted waiters are off the queue
  and never woken.
- The same shape anywhere a call hands work across a lock release and
  finishes it after: a channel send's wake of the receiver, a close's wakes
  of its waiters, a sleeper's slot (`BUG-SLEEPSLOT-LEAKED-BY-KILL`, patched
  where it was seen).

### The fix

Make a kill a fatal pending signal acted on at checkpoints, as Linux's
`SIGKILL` is (`fatal_signal_pending`): set it, wake the thread from an
interruptible wait, and let the thread end itself at its next return to user
mode, or let an interruptible wait return `Interrupted` so the call unwinds
-- the shape the freezer's `wait_ends` / `park_if_frozen` already has, and
the one the job-control issue above proposes for stops. Both want one
change, with the job-control and pty fixtures run several times over: the
two are the kernel's most race-prone paths. A thread that never reaches a
checkpoint (a kernel loop that never waits) would need one added.

Until then a local repair of one instance only moves the gap: a signal that
grants everything under one hold still wakes outside it.

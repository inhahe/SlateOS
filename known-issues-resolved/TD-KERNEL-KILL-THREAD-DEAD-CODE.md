### TD-KERNEL-KILL-THREAD-DEAD-CODE. `proc::thread::kill_thread` has no callers — ✅ RESOLVED 2026-08-14 (and it was not dead code: it was an unwired bug fix) 2026-08-13

`kernel/src/proc/thread.rs:kill_thread` is a `pub fn` with zero call sites
anywhere in the tree, so the binary build emits a `dead_code` warning for it.
Noticed while building the two fixes above; deliberately left alone rather than
churned, because it is plausibly intended as part of the thread-teardown API
surface. Proper resolution: either wire it into the process-teardown path that
should be using it, or delete it and let `sched::kill_task` remain the single
entry point. Pre-existing — it is not a regression from those fixes (verified:
the same single occurrence, definition only, exists at commit `315a7e0ca`).

**Resolution 2026-08-14 — this was filed under the wrong heading. It was a
live bug, not tech debt.** The "either wire it up or delete it" framing above
treats the two options as comparable. They were not, and the answer was
written inside the function's own doc comment the whole time:

> Calling `sched::kill_task` on its own — which is what the shell's `kill`
> command **used to** do — skips every one of those and leaves the thread
> registered forever with its joiner parked.

The shell's `kill` command did not "used to" do that. It still did:
`kshell.rs:cmd_kill` called `crate::sched::kill_task(task_id)` directly. So
`kill_thread` was not a speculative API-surface addition — it was the fix for
that exact defect, written but **never wired to its one caller**. Every
`kill <tid>` typed at the kernel prompt therefore:

1. leaked a `THREAD_OWNERS` entry (the thread→process mapping is never
   removed, so the process can never reach its zombie transition through that
   thread);
2. left any task parked in `join()` on the victim parked forever; and
3. recorded no `ThreadOutcome::Killed`, so a joiner that *was* woken by some
   other path would read a fabricated normal return instead of
   `KernelError::Cancelled` — the precise wrong-answer failure mode that
   `B-PTHREAD-CHILD-JUMPS-TO-GARBAGE` defect 2 was fixed to prevent.

**Fix:** `cmd_kill` now calls `proc::thread::kill_thread`, with a doc comment
recording *why* it must not use the scheduler call directly (so the next
person to "simplify" it has the reason in front of them).

**Why nothing caught it.** There was no test of `kill_thread` at all — the
existing `test_killed_thread_does_not_join_normally` exercises `record_killed`
and `join` directly with synthetic task IDs, which covers the outcome map but
deliberately never calls `kill_thread`. So the only evidence the function had
no callers was a `dead_code` warning, and a warning that says "unused" reads
as "harmless" rather than "your fix is not connected".

**Test added:** `thread::test_kill_thread_cleans_up` (test 9). It spawns a
victim and kills it **before it is ever scheduled**, which (a) makes the test
unable to leave a runaway task behind if the kill is refused, and (b) lets the
entry function's untouched counter prove the task never ran, so the stack
`&counter` it was handed cannot outlive the frame. It asserts the accepted
kill deregisters the thread and joins as `Cancelled`, and that a *second*,
refused kill withdraws the speculative `Killed` marker (the `else` branch that
exists only in `kill_thread`) so a later join reports `NoSuchProcess` rather
than a death that never happened.

**Deliberately left alone:** `syscall/handlers.rs:5877` also calls
`sched::kill_task` directly, on the sibling threads of a process being torn
down by a fatal signal. That site pairs it with an unconditional
`thread::on_thread_exit(t)`, which `kill_thread` would skip for a thread that
is already dead — and for a dying process, deregistering an already-dead
sibling is the wanted behaviour. Its missing `record_killed` is immaterial
there because the joiners are inside the same process and are being killed in
the same loop.

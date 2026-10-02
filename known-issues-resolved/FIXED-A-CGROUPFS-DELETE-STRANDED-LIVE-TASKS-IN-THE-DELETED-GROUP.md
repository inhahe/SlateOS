### FIXED-A-CGROUPFS-DELETE-STRANDED-LIVE-TASKS-IN-THE-DELETED-GROUP (lane A)

**Status:** FIXED 2026-08-23 (commit `660e86a61`) — awaiting a boot test on `main`.

**In short:** If you put a program into a resource group (a "cgroup" — a
container that caps how much memory and CPU the programs inside it may use)
and then deleted that group, the program did not come back out. It stayed
inside the deleted group forever: its memory kept being counted against a
group you could no longer see, and the group's caps kept throttling it, until
the machine rebooted. Deleting the group looked like it worked. Fixed by
moving the group's members back to the root group before freeing it.

**Where:** `kernel/src/fs/cgroupfs.rs` → `delete_group`.

**What was wrong.** `delete_group` removed its own record of the group and
then called `crate::cgroup::delete` on the backing kernel group, discarding
the result with `let _ =`. The discard carried a comment justifying itself:

> A failure here only leaks one of the 256 kernel cgroup slots until reboot,
> never a correctness hazard, so we don't fail the frontend delete on it.

That was wrong on the central point. `cgroup::delete` refuses a group whose
`nr_tasks` is nonzero (`KernelError::NotEmpty`), and `add_pid` puts *real*
tasks into that count via `sched::set_task_cgroup`. So the failure was not the
rare case — it was the ordinary case of deleting a group that still had
members. The leaked slot was the least of it; the stranded task was the bug.

**How it was found.** Not directly. `cgroup::self_test` asserted
`active_count() == 1, "only root at startup"`, which panicked boot batch 27
with `left: 2`. The leaked slot was the second entry.

**Why the self-test missed it.** The cgroupfs suite *did* delete a non-empty
group — it used PIDs `100` and `200` and treated them as synthetic. By the
time that suite runs, those are real, live task IDs (batch 27's log shows task
100 spawned at line 1524 and task 200 at line 6441). So the suite was in fact
reproducing the bug, on two unrelated tasks, and asserting nothing about it.
This is the "synthetic constant that is not synthetic" trap: a literal chosen
to mean "cannot exist" that quietly starts existing as the system grows.

**The fix.** Evict the members first, then delete:

- `delete_group` returns every PID in `g.processes` to `ROOT_CGROUP` before
  calling `cgroup::delete`, so the delete actually succeeds.
- If it still fails, something attached behind the frontend's back — an
  anomaly the frontend cannot repair, so it now warns on serial instead of
  passing silently.
- Tests 5 and 6 use `u32::MAX` and `u32::MAX - 1`, which no task can have.
- Test 7 makes *this* task a member immediately before the delete and asserts
  both that the kernel cgroup was released and that the task came back to
  root. A group holding only synthetic PIDs has `nr_tasks == 0` and deletes
  cleanly with or without the fix, so it would prove nothing.

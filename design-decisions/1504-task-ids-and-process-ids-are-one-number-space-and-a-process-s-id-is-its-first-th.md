## 1504. Task ids and process ids are one number space, and a process's id is its first thread's

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** every running program has a process id, and each of its
threads has a thread ("task") id. They used to come from two separate
counters that both started at 1, so the same number could mean a process in
one place and an unrelated thread in another. `/proc`, which `ps`, `top` and
the task managers read, names its directories by *task* id. The result:
- `ps` listed numbers `kill` could not use;
- `/proc/self` was not the calling process;
- a program's `/proc` entry showed some other process's parent, group and
  memory.

Now both come from one counter, and a process's first thread is given the
process's own number, as Linux does. `/proc/<pid>` is then the process, and
`gettid() == getpid()` in its main thread.

**What changed:**
- `sched::task::alloc_id` is the one counter, and `pcb::alloc_pid` draws
  from it.
- `pcb::claim_leader_id` lets the first thread `proc::thread` creates for
  a process take the process's id, through `sched::spawn_suspended_with_id`.
  The scheduler checks the id is free under its lock and falls back to a
  fresh one, so no request can replace a task.
- procfs:
  - `/proc/self` is the calling process;
  - the root lists processes (their main threads) and kernel tasks, not
    every thread;
  - a stat line's process-wide fields are the owner's.

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. One space, leader id = pid (chosen)** | process and main-thread ids coincide, as on Linux | every `/proc` generator, which looks the process up by its directory number, becomes right at once; `gettid() == getpid()` holds in the main thread, which ported programs use to ask "am I the main thread?" | touches the allocators and thread creation; pid values grow faster, since tasks share the counter |
| **B. Key `/proc` by pid, two spaces** | `/proc` maps pid to a representative thread | local to procfs | about 30 generators to re-key, and kernel tasks collide with pids numerically, so one of the two must be hidden |
| **C. Separate spaces, disjoint ranges** | e.g. tasks from 2^32 up | no collisions | `gettid() != getpid()` everywhere; `/proc` still needs B's re-keying |

**Consequences:**
- No process is numbered 1 any more: boot tasks take the low numbers first.
  pid 1 stays what `initproc::INIT_PID` makes it, the kernel's reaper of
  orphans and the ppid they report.
- Left open, in known-issues A-PROC-DIRECTORIES-ARE-TASK-IDS-AND-EVERYTHING-ELSE-IS-PIDS:
  - A process whose main thread has exited while others run is not listed
    and has no `/proc/<pid>`; Linux keeps the leader as a zombie.
  - A non-leader thread's `/proc/<tid>` files other than `stat` look its
    process up by the thread's id.

**Revisit** if exec by a non-leader thread is modelled (Linux's `de_thread`
hands it the leader's id), or if the leader-exit gap above matters to a
program.

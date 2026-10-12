## 1515. Native CPU affinity: a process or a thread, the signalling rule, and the mask kept as given

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a program can now ask the kernel to keep a process -- or one of
its threads -- on particular CPUs, and ask which CPUs another process may use.
Before this, only the kernel's own debugging shell could pin anything. The
Linux calls a program would use (`sched_setaffinity`) reported success and
changed nothing, and `sched_getaffinity` always answered "every CPU". Lane E's
process explorer asked for the native pair
(`requests/e-adf-what-the-process-explorer-still-cannot-ask.md`, part 2).
Lane D routes libc's calls to it. Writing the pair turned up three scheduler
gaps, all fixed here (`A-CPU-AFFINITY-DID-NOT-MOVE-A-RUNNING-THREAD`).

**What changed:**
- `SYS_SCHED_SET_AFFINITY` (1100) / `SYS_SCHED_GET_AFFINITY` (1101):
  `arg0` a process id, or with `SCHED_AFFINITY_THREAD` a thread id, and 0 for
  the caller; `arg1` the mask, or a pointer for the get; `arg2` the flag.
- The Linux `sched_setaffinity` / `sched_getaffinity` now apply and report
  the real mask, to the thread `pid` names.
- `sched::set_affinity` moves the task at once. A queued task changes queue.
  A task running elsewhere has its CPU asked to reschedule. A caller that
  moved itself switches before returning.

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. Who may set: the signalling rule -- the process itself, its parent, or a `Process` capability holder (chosen)** | the same people who may stop or kill a process may pin it | the request asked for it; it is how this kernel already decides who may act on another process, with no ambient authority; one rule for both ABIs | a Linux program running as the same user as its target, but neither its parent nor holding a capability, gets `EPERM` where Linux would let it |
| B. Linux's rule: same user id, or `CAP_SYS_NICE` | a user can pin any of their processes | what Linux programs expect | ambient authority, which the design rules out; every process here is uid 0 today, so it would let anything pin anything |

**Smaller decisions:**

| decision | alternative | why this one |
|---|---|---|
| The native call takes a process (every thread) or, with a flag, a thread | a thread only, as Linux's `pid` is | the explorer pins processes; a process's threads would otherwise need pinning one by one, racing the threads it starts |
| Reading is open to every process | the setting rule | Linux checks nothing here, and `/proc/<pid>/status`'s `Cpus_allowed` already publishes it |
| The mask is kept as given; it must name a CPU online now; reads report it less the offline CPUs | keep only its online part | as Linux: a CPU that comes online later is used by a mask that named it |
| New threads, forked children and spawned processes take their creator's mask | every CPU | as Linux's `clone`, `fork` and `posix_spawn` |
| The flags are checked first, then the target (`NoSuchProcess`), then permission, then the mask | the mask first | Linux's order (`ESRCH`, `EPERM`, `EINVAL`); a refused mask leaves a process unchanged, never half moved |
| No process may move a kernel task | allow it to whoever may signal the kernel | kernel tasks belong to no process, so the rule has nobody to ask; Linux refuses its per-CPU threads too |

**Revisit** when user identities exist: whether a same-user rule should
join A's three (as `A-PROC-PID-FILES-CHECK-NO-READER` will decide for
`/proc`).

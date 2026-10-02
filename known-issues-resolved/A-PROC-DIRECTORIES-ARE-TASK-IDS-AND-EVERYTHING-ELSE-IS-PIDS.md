### A-PROC-DIRECTORIES-ARE-TASK-IDS-AND-EVERYTHING-ELSE-IS-PIDS -- 2026-10-01 -- FIXED the same day by option A (design-decisions §1504); two gaps left, at the end

**In short:** the numbers `/proc` shows as process ids are not process ids.
`/proc/<n>` is keyed by scheduler *task* id, while `getpid()`, `kill`,
`waitpid` and every process-wide field use *process* ids, which a separate
counter numbers. So `ps` lists numbers that `kill` cannot use,
`/proc/<getpid()>` is usually missing or someone else's, and a process's
`/proc/<n>/stat` shows the parent, group and memory of whichever process
happens to have the number `n`.

**Where.** `kernel/src/fs/procfs.rs`:
- the root `readdir` lists `sched::task_list()` ids;
- `/proc/self` resolves to `sched::current_task_id()`;
- `gen_pid_stat(task_id)` calls `build_pid_stat(task, task_id)`, so every
  process-wide field (ppid, pgrp, session, nice, num_threads, vsize, rss,
  rsslim) is looked up with the task id as if it were a pid.

`gen_thread_stat(proc_id, tid)` is the one place that keeps the two apart.
`proc::pcb::alloc_pid` (`NEXT_PID`) and `sched::task::alloc_task_id`
(`NEXT_TASK_ID`) both count from 1 and are independent.

**Who it bites:**
- `ps`, `top`, `htop` and the task managers: their PIDs are task ids.
- Anything that opens `/proc/self/...` or `/proc/<getpid()>/...` and
  expects its own process: fd lists, maps, status.
- libc's old `tgkill` check, `/proc/<tgid>/task/<tid>`. It is superseded
  by `SYS_SIGNAL_TGKILL` (1087).

**The proper fix, and the choice in it.** Linux numbers tasks and
processes from one space: a process's id is its first thread's id. The
options here:
- **A. One id space.** Give a process's main thread the task id equal to
  its pid, or allocate both from one counter. `/proc/<pid>` and
  `/proc/<tid>` then agree, as on Linux, and kernel tasks take ids from the
  same space, so they can be listed without colliding. This is the
  allocator change.
- **B. Key `/proc` by pid.** List processes at the root and threads under
  `task/`. Kernel tasks with no process are not listed, or are listed
  somewhere of their own.

A is Linux's model and fixes every consumer at once. B is local to procfs
but leaves two id spaces that collide numerically.

**Reproduce.** In a native process, compare `getpid()` with
`readlink("/proc/self")`. They differ whenever the process's first thread
was not given the same number.

**Fixed (§1504):**
- One counter now numbers tasks and processes.
- A process's first thread takes the process's id
  (`pcb::claim_leader_id`).
- `/proc/self` is the calling process.
- The root lists processes and kernel tasks.
- A stat line's process-wide fields are the owner's.

**Still open:**
- **A process whose main thread exited while other threads run** has no
  `/proc/<pid>` and is not listed. Linux keeps the leader as a zombie
  thread so the directory stays; here the leader task is gone. Fix:
  resolve `/proc/<pid>` to the process's first live thread when no task
  has the pid's number.
- **A non-leader thread's `/proc/<tid>`** (not listed, but it resolves):
  every file but `stat` looks its process up by the thread's id and finds
  nothing. Fix: one `owner_process` resolution in the generators' common
  path.

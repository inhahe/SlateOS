### [D] B-D-SYSV-SEM-SEMCTL-TIMEOUT-AND-LIMITS — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/sysv_sem.rs`; the plumbing it shares with
`sysv_msg.rs` is the new `posix/src/sysv_ipc.rs`.

**What it was.** `semctl` was declared with three arguments, so the fourth a
C program passes -- `SETVAL`'s value, `GETALL`'s and `SETALL`'s array,
`IPC_STAT`'s buffer -- never arrived: `semctl(id, 0, SETVAL, 1)`, the way
every program sets a semaphore up, did not set it, and `IPC_STAT`,
`IPC_SET`, `GETALL` and `SETALL` were reachable only through extra functions
no C program calls. `semtimedop` read its timeout as a `CLOCK_REALTIME`
deadline, so "wait five seconds" had passed in 1970 and every timed wait
failed at once with `EAGAIN`. One `IPC_NOWAIT` anywhere in a batch made
every operation in it non-blocking. The sets were a static pool of 16 sets
of 32 semaphores; `GETPID`, `GETNCNT` and `GETZCNT` were always 0; a blocked
call spun without yielding; permissions were not checked; `IPC_INFO`,
`SEM_INFO` and `SEM_STAT` did not exist.

**Fix.** Linux 6.6's ipc/sem.c behind glibc's `semctl`, inside one process:
`semctl` takes `union semun`; `semtimedop`'s timeout is relative;
operations apply in order, all or none, the one that cannot proceed
deciding between `EAGAIN` and a sleep; Linux's limits (`SEMMSL` 32000,
`SEMMNI` 32000, `SEMOPM` 500) and error orders; `SEM_UNDO`'s range;
`GETPID`, `GETNCNT` and `GETZCNT` counted as Linux counts them; `IPC_INFO`,
`SEM_INFO`, `SEM_STAT` and `SEM_STAT_ANY`; futex waits and `EIDRM`. Ids,
permissions and the table of slots moved into `sysv_ipc.rs`, which
`sysv_msg.rs` now uses too.

**What remains.** The sets are one process's (D-Q3), so `SEM_UNDO`'s
adjustment is kept but never applied: nothing outlives the process to undo.
The ring-3 check is `services/ctest-sysvipc`, its rung requested of lane A.

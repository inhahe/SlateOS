### A-FLOCK-OWNERS-COLLIDED-AND-THE-NATIVE-DOORS-TOOK-ANY-OWNER -- 2026-10-01 -- FIXED (lane A)

**In short:** `flock`, the whole-file lock `flock(1)`, package managers and
lock files use, had four defects in the kernel. Any process could release
anyone's lock. Two unrelated owners could be treated as one. A request that
had to wait failed at once instead. Two holders upgrading their shared locks
could wait on each other forever. All four are fixed (design-decisions
§1506).

| defect | where | what a user saw |
|---|---|---|
| The native doors took the owner from the caller | `SYS_FS_FLOCK` (`arg3`), `SYS_FS_FUNLOCK` (`arg2`), the latter with no capability check | any process could take a lock in another's name, or release anyone's lock: mutual exclusion gone |
| Two owner spaces in one table | `vfs::LOCK_TABLE`: native locks keyed by pid, Linux `flock(2)` by handle; exit released by pid | pid 57's lock and handle 57's did not conflict, and process 57's exit dropped handle 57's locks |
| `flock` never waited | Linux `sys_flock`: `EWOULDBLOCK` with or without `LOCK_NB`; native libc polled | blocking callers failed or spun; libc's poll could not be ended by a signal (lane B's `TD-B-FLOCK-WAIT-POLLS`) |
| An upgrade was atomic | `Vfs::flock_resolved`: shared to exclusive only with no other holder, keeping the shared lock while refused | two sharers both upgrading, once they can wait, wait on each other forever |

**The fix:**
- **Owner tags.** `vfs::flock_process_owner(pid)` and
  `vfs::flock_description_owner(handle)` are disjoint owner spaces. The
  native path doors take the caller's own; exit releases it.
- **Waiting.** `Vfs::flock_wait_resolved` parks until the lock is free.
  Every release, conversion and unmount wakes its file's waiters. A signal
  ends the wait with a restart, as Linux's `flock` restarts. The Linux
  `flock(2)` waits without `LOCK_NB`.
- **Linux's conversion.** A conversion first gives up the lock held
  (`flock_lock_inode`).
- **A door with BSD semantics for native programs:** `SYS_FS_FLOCK_HANDLE`
  (1094). The lock belongs to the open file description, shared by `dup`
  and `fork`, and ends at the final close. It is the Linux `flock(2)`'s
  lock.
- **`/proc/locks`** speaks Linux's format (`lslocks` parses it), and lists
  record locks as well as `flock` ones.

**Tests:** `vfs::self_test`'s new rungs (owner tags, a refused upgrade),
`vfs::self_test_flock_wait` (a waiter woken into the lock; two upgrading
sharers), `procfs::self_test_locks`, and `syscall::dispatch`'s
`test_dispatch_flock` (1094 across two processes; 609/640 ignore a
caller-named owner).

**Left for lane D:** libc's `flock` can move to 1094 and drop its polling
loop. That also closes lane B's `TD-B-FLOCK-WAIT-POLLS`: a blocking
`flock` a signal interrupts.

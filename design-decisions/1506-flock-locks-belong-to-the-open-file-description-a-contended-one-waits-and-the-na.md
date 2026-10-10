## 1506. `flock` locks belong to the open file description, a contended one waits, and the native path-based door no longer takes an owner from the caller

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** `flock` is the whole-file lock that `flock(1)`, package
managers and lock files use. Through the native calls any program could
release any other program's lock, because the caller named the owner. A
request that had to wait failed instead. Locks now belong to whoever holds
them, never to a name the caller supplies. A request without "don't wait"
waits until the lock is free. Native programs get a call with the same
behaviour Linux and BSD programs have.

**What changed:**
- **Owners.** A lock belongs to a process (`vfs::flock_process_owner`, the
  path-based `SYS_FS_FLOCK`) or to an open file description
  (`vfs::flock_description_owner`: the Linux `flock(2)` and the new
  `SYS_FS_FLOCK_HANDLE`). The two are tagged apart; they had collided
  numerically. The path doors ignore their owner argument now.
- **Waiting.** `Vfs::flock_wait_resolved`, woken by releases, conversions
  and unmounts; a signal ends it with a restart.
- **Conversion.** As Linux's `flock_lock_inode`: the lock held is given up
  first.
- **`SYS_FS_FLOCK_HANDLE` (1094):** `(handle, op)` with Linux's `LOCK_*`
  values, per description, waiting unless `LOCK_NB`.

**Alternatives for the path-based door's owner argument:**

| | What changes | For | Against |
|---|---|---|---|
| **A. Ignore it; the owner is the caller (chosen)** | libc's calls are unchanged, since it passed its own pid | closes the hole with no caller to update | a caller that meant another owner silently gets its own |
| B. Refuse an owner that is not the caller's pid | the same, plus `InvalidArgument` for a stranger's | a misuse is reported, not absorbed | a pid is not the only sensible value a caller passed (a tid), and nothing passes another |
| C. Retire the path doors | callers must move to 1094 | one door | breaks the libc in the field until lane D moves |

**Alternatives for conversion:**

| | What changes | For | Against |
|---|---|---|---|
| **A. Linux's: give up the held lock first (chosen)** | a refused upgrade leaves no lock, as on Linux | two sharers that both upgrade cannot wait on each other forever; ported programs see Linux's behaviour | a refused `LOCK_NB` upgrade loses the shared lock |
| B. Atomic: keep the held lock until the new one is granted | a refused upgrade keeps the shared lock | nothing lost on refusal | with waiting, two upgrading sharers deadlock; differs from Linux |

**Revisit** if a native program is found that passed a meaningful owner other
than its own pid to `SYS_FS_FLOCK`.

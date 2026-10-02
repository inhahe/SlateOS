## 1505. Record locks: native programs use the Linux `fcntl`'s own code, and a lock that must wait does wait, refusing only a wait that could never end

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a program can lock part of a file so that no other program
writes it at the same time; databases such as SQLite rely on this. Programs
built for SlateOS itself could not reach the kernel's lock table at all, so
their `fcntl` said yes to every lock and two of them could hold the same
exclusive lock. They now reach it through a new system call that runs the same
code as the Linux one, so the two kinds of program see each other's locks. A
request that has to wait (`F_SETLKW`) now waits; it used to fail at once with
"try again". A wait that could never end is refused with "would deadlock":
two programs, each waiting for a range the other holds.

**What changed:**
- **`SYS_FS_RECORD_LOCK` (1093):** `(handle, op, flock_ptr)`. The ops are
  `F_GETLK`, `F_SETLK` and `F_SETLKW` (0, 1, 2), with the `struct flock` in
  the Linux layout, so libc passes its own structure through
  (`requests/d-a-native-programs-cannot-reach-the-record-lock-table.md`).
- **`syscall::record_lock`** is the one implementation both ABIs call. It
  reads the `struct flock`, resolves the range against the handle, applies
  the access-mode rule, and fills in `F_GETLK`'s answer.
- **`fs::reclock`** gained the wait (`set_wait`), the deadlock search, the
  merging of touching locks, and a key type (`LockKey`). The key names a file
  without resolving its path again: the old path-taking API applied a jailed
  process's namespace a second time. It also names a memfd, whose locks had
  been looked up in the file-handle table under the memfd's number.
- **POSIX's close rule:** closing *any* descriptor for a file drops the
  process's locks on it, on every path that removes one (close, `dup2`,
  `close_range`, close-on-exec, the native close).
- **Linux fidelity fixes:**
  - `F_GETLK` leaves the fields alone when nothing is in the way; it zeroed
    `l_pid`.
  - It reports a holder's range from byte 0 (`l_whence = SEEK_SET`).
  - `F_GETLK` with `F_UNLCK` is `EINVAL`, and with `F_OFD_GETLK` it reports
    the description's own lock.
  - A lock the open mode does not allow is `EBADF`.
  - A full table is `ENOLCK`.
  - A lock taken while another thread closed the descriptor is dropped
    (`EBADF`), as Linux's `fcntl_setlk` does.

**The deadlock search -- alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. Search the whole wait-for graph (chosen)** | any cycle of waiting processes is refused with `EDEADLK` | no cycle goes undetected, however long; the work is bounded by the waiters present | answers differ from Linux's for cycles longer than ten processes, where Linux lets them hang |
| B. Linux's search: one chain, ten steps | identical answers to Linux | bit-for-bit Linux | misses longer cycles, and where a process has several waiting threads it follows one arbitrarily |
| C. No search | `F_SETLKW` just waits | simplest | two deadlocked programs hang forever; POSIX asks for `EDEADLK` where the system can tell |

Both A and B share one false alarm, as Linux documents for itself. The owner
of a lock is a process, so a cycle through one waiting thread of a
multi-threaded process is reported even when another of its threads could
still release the lock. OFD locks take no part, as on Linux: their owner is
an open file description, not something that waits.

**The native ABI -- what was left out, and why:**
- **OFD ops.** Lane D asked for none; nothing native uses them. They fit as
  op values 3 to 5 when something does.
- **Closes the kernel never sees.** A native process's descriptor table is
  libc's, so closing one of two descriptors that share a handle does not
  reach the kernel. libc applies the close rule itself, with a whole-file
  `F_UNLCK` through the same call. The kernel applies it on `SYS_FS_CLOSE`
  and on close-on-exec.

**Consequences:**
- A file renamed or unlinked since it was opened keeps being locked under
  its open-time path when the filesystem cannot give its identity, as
  before. The path is what the VFS hands a handle; keying by the file
  itself waits on handles that carry their file's identity (the VFS
  unlinked-open-file redesign).

**Revisit** when handles carry their file's identity (key by it), or if a
program is found that relies on Linux's ten-step limit.

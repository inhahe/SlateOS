## 1121. The C library's streams are musl's design with glibc's behaviour

**Date:** 2026-09-27
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** a C program reads and writes files through *streams*
(`FILE *`: `fopen`, `fgets`, `printf`).  This library's were written from
scratch as a pool of sixteen with no locking, and answered a dozen calls
differently from glibc (`known-issues.md` →
`D-POSIX-STDIO-WAS-SIXTEEN-UNLOCKED-SLOTS`).  They are now a port of musl's
design -- a buffer and four operations that say what is at the far end, a
file descriptor, a program's own callbacks or memory -- changed, wherever a
program could tell musl from glibc, to do what glibc does.  This entry
records the choices that were not obvious, and the places this library still
differs from glibc on purpose.

| Choice | Alternatives | Why this one |
|---|---|---|
| **Port musl's `FILE` model**: read/write windows over one buffer, and per-stream `read`, `write`, `seek`, `close` | port glibc's libio; write a third design | libio is vtables, a separate wide buffer and thirty years of old-ABI layers; musl's is small, and it is what makes `fopencookie`, `fmemopen` and `open_memstream` one design rather than three -- and lets host tests drive real buffering through an in-memory far end.  Writing our own is how the old one came to have sixteen slots and no locks. |
| **glibc's behaviour where a program can see it** | musl's | §1119: the numbers are musl's, the behaviour glibc's.  Mode letters (`x`, `e`, six letters, `,` ends them), `fputs` answering 1 and `puts` the length plus one, sticky end of file, `fflush` on an input stream giving back its read-ahead, line buffering decided by whether the descriptor is a terminal, `a` starting at the end, `exit` syncing input streams, `fdopen` checking the descriptor. |
| **4096-byte buffers, allocated with the stream** | musl's 1024; glibc's size from `st_blksize`, allocated at first use | 4096 is glibc's actual size for a file or a pipe; one `malloc` for the stream and its buffer, as musl does.  So `__fbufsize` answers the size from the start, where glibc answers 0 until first use -- the one visible difference, recorded on `__fbufsize`. |
| **A recursive lock per stream**, glibc's `_IO_lock_t` over `lowlevellock`'s futex lock | musl's owner-in-the-word lock | the futex lock already exists; the owner's cached thread id makes the uncontended case a compare-and-swap and a load. |
| **Every call locks, from the first thread** | musl and glibc skip locking until a second thread exists | the skip needs a process-wide "threaded" flag that every entry point reads and every way of making a thread sets; an uncontended lock is cheap enough not to have one more thing to keep right. |
| **In a `fork` child, the forking thread keeps the stream locks it held**; other threads' are released | glibc releases them all | glibc's reset leaves the forking thread holding nothing it believes it holds, so its next `funlockfile` releases a lock it no longer has. |
| **`freopen(NULL, mode, f)` reopens `/proc/self/fd/N`**, as glibc does, and falls back to changing the descriptor's flags in place where there is no file to reopen by name | musl's in-place change always | glibc's is the behaviour -- a new open, so the position starts again, `w` truncates and the access mode can change.  But this system's `/proc/<pid>/fd` links name files by path, so a pipe or socket has nothing to reopen; the fallback keeps `freopen(NULL, "wb", stdout)` working there. |
| **`fdopen(1, "w")` is a new stream**, not `stdout` | the old answer: fds 0-2 returned the standard streams | glibc makes a second stream with its own buffer; returning `stdout` made `fclose` of the new stream close `stdout`'s `FILE`. |
| **`fclose(stdout)` closes descriptor 1** | the old answer: flush only | glibc's; gnulib's `close_stdout` calls it to learn whether the output was written, and a close that never happened cannot say. |

**Still different from glibc, on purpose:**

- A NULL `FILE *` is `EBADF` (§1120), and a NULL `path`, `mode`, buffer or
  position pointer is `EFAULT` where glibc would fault (§1115, §303).
- `funlockfile` from a thread that does not hold the stream does nothing;
  in glibc it releases another thread's lock.
- `fgetln` exists (musl's; glibc has none).

**How to reverse a choice.**  Each is local: the mode parsers are three
small functions, the buffer size one constant, `freopen`'s fallback one
branch, the fork rule one function (`reset_lock_after_fork`).

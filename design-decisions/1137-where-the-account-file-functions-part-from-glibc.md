## 1137. Where the account-file functions part from glibc

**Date:** 2026-09-28
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** the old functions for reading and writing `/etc/passwd`-style
files -- `fgetpwent`, `putpwent`, `sgetspent`, `lckpwdf`, `cuserid`,
`getusershell` and their relatives -- now exist, and answer as glibc 2.39
does in everything its tests could ask (`posix/tools/oracle/accounts_harness.py`
replays 272 of its answers). In five places glibc's answer is an accident
that can hurt a caller, and this library answers differently, on purpose.

| where | glibc | here | why |
|---|---|---|---|
| `sgetspent_r` on a string that is no entry | returns whatever `errno` held -- often 0, "success", with a NULL result | `EINVAL`, and `errno` too | a caller that checks the return value then reads through NULL |
| `cuserid` with a name too long for the buffer | cuts it to fit | an empty string, as musl | a cut name is someone else's, or no one's; `L_cuserid` is musl's 20, the header callers here size by |
| `fgetpwent`, `fgetgrent`, `fgetspent` on a pipe | NULL at once: it must re-read a line after growing its buffer, so it refuses a stream `fgetpos` cannot place | reads it -- the line is read once, then the buffer grown | nothing is lost, and a pipe is a natural thing to read a password file from |
| `lckpwdf` waiting for another process | `F_SETLKW` under `alarm(15)`, which cancels the caller's own alarm | `F_SETLK` tried against the clock for 15 s | the caller's alarm survives; the answer (-1, `EINTR`) is the same |
| `getusershell` on a file of very short lines | overruns the array it sized as the file's length over three | counts the shells | a heap overrun |

**And one bug of glibc's not copied:** its `__nss_readline` moves a line past
its leading white space without the NUL, so the last line of a file, if it
has both leading white space and no newline, reads with its tail doubled --
a shell of `/sh` becomes `/shsh`. The readers here read what is written.

**What was copied, though it is an artifact:** after a successful read,
`errno` is `EINVAL` if a malformed line was skipped on the way, and
`ERANGE` if the non-reentrant form had to grow its buffer -- both left by
glibc's internal retries. Harmless (`errno` means nothing after a success)
and cheap, and it keeps the oracle's lines comparable without exceptions.

**Alternatives:** copying glibc exactly in all five. Rejected for each for
the reason in its row: every one is either a crash waiting for a caller, a
wrong answer that looks right, or a limitation with no purpose. None is
something a program could be relying on.

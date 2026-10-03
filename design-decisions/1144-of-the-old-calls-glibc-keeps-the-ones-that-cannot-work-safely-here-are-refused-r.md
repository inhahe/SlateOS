## 1144. Of the old calls glibc keeps, the ones that cannot work safely here are refused rather than pretended: `sigstack` and a starting `profil` are `ENOSYS`

**Date:** 2026-09-29
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** glibc still carries calls from 1980s Unix. Most can be given
their old meaning over the modern calls (`posix/src/legacy.rs`); two cannot,
and for those this library says "not supported" where glibc says "done".
`sigstack` sets a stack for signal handlers but gives no size; glibc guesses
one and places it past the end of the caller's memory. `profil` starts
profiling that needs a timer this system does not have; answering "done"
would leave a program waiting for data that never comes.

| Call | Here | glibc 2.39 | Why |
|---|---|---|---|
| `sigstack(ss, oss)` | -1, `ENOSYS` | 0: `sigaltstack` with `ss_sp` as the base and `MINSIGSTKSZ` bytes up from it | By the old convention `ss_sp` is the stack's *top*, so the kernel is handed memory above the caller's buffer, and a signal frame written there corrupts whatever follows it. `sigaltstack` is the call that works. |
| `profil(buf, ...)` starting | -1, `ENOSYS` | profiling by `SIGPROF` | There is no `ITIMER_PROF` to drive it; `setitimer` refuses it the same way (§1004). Stopping (a NULL buffer or a scale of 0) is 0, as there. |
| `isfdtype` on a bad descriptor | -1, `errno` `EBADF` | -1, `errno` put back as it was | The manual says `errno` is set; a -1 with `errno` unchanged cannot be told from a stale one. |
| `sigblock`, `sigsetmask`, `siggetmask` | signal 32 is bit 31 | bit 31 always clear | Signal 32 is `SIGRTMIN` here; glibc keeps 32 and 33 for its threads. The same difference as `strsignal`'s real-time numbers (commit f7a524b0d). |

Refusing leaves a program a fallback it can act on, which "done" does not
-- §1004's argument, applied again.

**Where:** `posix/src/legacy.rs`.

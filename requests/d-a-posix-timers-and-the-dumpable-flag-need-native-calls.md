# Lane D -> lane A: POSIX timers that fire, and the dumpable flag, need native calls

**Filed:** 2026-10-05 by lane D. **For:** lane A (`kernel/src/syscall/`, `kernel/src/proc/`).
**Status:** OPEN.

**In short:** a C program can ask for a timer that sends it a signal when
it expires (`timer_create`, `timer_settime`), and can mark itself "not
dumpable" (`prctl(PR_SET_DUMPABLE, 0)`). The library cannot do either
honestly today: it has no native call that arms such a timer, so it says
yes and nothing ever fires; and it has no route to the kernel's dumpable
flag, so it refuses. Two small native interfaces would close both. They come
from lane B's `requests/b-d-two-calls-gnu-timeout-makes-are-not-native-yet.md`:
GNU `timeout`, ported, makes both calls.

## 1. Per-process POSIX timers

**What exists:**

- `SYS_TIMER_CREATE` (12) / `SYS_TIMER_CANCEL` (13): kernel timers that
  notify a completion port, not a signal.
- `SYS_ITIMER_SET` (1069): one `ITIMER_REAL` per process, delivering
  `SIGALRM` -- what `alarm`, `ualarm` and `setitimer` use since 2026-09-12.
- The Linux ABI's `timer_create` (`kernel/src/syscall/linux.rs`) checks its
  arguments in Linux's order and then answers `ENOSYS`; `timer_delete` says
  "no timers exist in our kernel". (Lane B's request read it as
  implemented; it is the checks only.)

**What is asked:** native calls with Linux's semantics for the per-process
timer family -- create, set, get, get-overrun, delete -- or one call with an
operation argument, as lane A prefers:

- clocks `CLOCK_REALTIME`, `CLOCK_MONOTONIC`, `CLOCK_BOOTTIME` at least;
  `EOPNOTSUPP` for the reader-only clocks, as the Linux ABI's checks already
  answer;
- `SIGEV_SIGNAL` (signal number and `sigval`, delivered with `si_code =
  SI_TIMER`, the timer id and the overrun count in the `siginfo`), and
  `SIGEV_NONE` (armed and readable, delivering nothing);
- `SIGEV_THREAD` stays the library's, as in glibc, which builds it on a
  signal delivered to a helper thread (`SIGEV_THREAD_ID` -- Linux's
  `SIGEV_SIGNAL | 4` -- if lane A offers it, otherwise the library will use a
  process-directed signal and route it);
- `TIMER_ABSTIME`, interval re-arming, and the overrun count;
- a process's timers are not inherited by `fork` and are deleted by `exec`,
  as POSIX says.

**Meanwhile:** since 2026-10-05 the library's `timer_create` answers
`ENOSYS` after its argument checks, as the Linux ABI does, instead of
succeeding with a timer that never fires (design-decisions 1170). Callers fall back on that answer:
GNU `timeout` falls back to `alarm` on exactly it. Nothing on the image
calls `timer_create` today; the interpreter's binary carries the symbol only
because it shares an archive member with time functions it does call.

## 2. The dumpable flag

**What exists:** `pcb.linux_dumpable`, with an accessor pair
(`kernel/src/proc/pcb.rs`, around line 3786), set by the Linux ABI's
`prctl`, inherited by `fork` and reset by `exec`. Nothing in the kernel acts
on it yet.

**What is asked:** a native call to set and read it -- `PR_SET_DUMPABLE`
takes 0 or 1 and refuses anything else `EINVAL`, as Linux's `prctl` does --
so that the native `prctl` and the Linux ABI's agree on one flag, and
`/proc` and `ptrace` can consult it the day they do.

**Meanwhile:** since 2026-10-05 the library keeps the flag itself, as it
keeps `no_new_privs` (`posix/src/unistd.rs`): `PR_SET_DUMPABLE` 0 or 1 accepted,
`PR_GET_DUMPABLE` answered from it, 2 and above `EINVAL`. That is exact for
the one thing the flag does on SlateOS today -- be read back -- and the
native call replaces it when it lands.

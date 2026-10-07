# Lane D -> lane A: POSIX timers that fire, and the dumpable flag, need native calls

**Filed:** 2026-10-05 by lane D. **For:** lane A (`kernel/src/syscall/`, `kernel/src/proc/`).
**Status:** DONE on lane A 2026-10-07 (reaching `main` with lane A's next
publish): `SYS_POSIX_TIMER` (1141) and `SYS_PROCESS_DUMPABLE` (1142) -- see
"Lane A's answer" at the end.

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

---

## Lane A's answer (2026-10-07)

**Both calls exist, with the Linux ABI's own bodies behind them, answering
Linux's errnos as `-errno`** (the device door's convention, as
`SYS_MEMORY_ADVISE`), so the library can pass the user's arguments and the
answers straight through. Design: §1540; numbers and docs in
`kernel/src/syscall/number.rs`.

### `SYS_POSIX_TIMER(op, a, b, c, d)` -- 1141

| `op` | is | arguments | answers |
|---|---|---|---|
| 0 | `timer_create` | `a` clock id, `b` `struct sigevent *` or 0 | the new id (not written through a pointer) |
| 1 | `timer_settime` | `a` id, `b` flags, `c` new `struct itimerspec *`, `d` old one or 0 | 0 |
| 2 | `timer_gettime` | `a` id, `b` `struct itimerspec *` | 0 |
| 3 | `timer_getoverrun` | `a` id | the count |
| 4 | `timer_delete` | `a` id | 0 |

Linux's x86-64 structures: `struct sigevent` is 64 bytes (`sigev_value` 0,
`sigev_signo` 8, `sigev_notify` 12, the thread id 16), `struct itimerspec` 32
(`it_interval`, then `it_value`). What you asked for, and how it came out:

- **Clocks:** `CLOCK_REALTIME`, `CLOCK_MONOTONIC`, `CLOCK_BOOTTIME`,
  `CLOCK_TAI`. `TIMER_ABSTIME` on a wall clock follows `clock_settime`; a
  relative wall-clock timer does not move, as on Linux. The reader-only
  clocks are `EOPNOTSUPP`; so are the **CPU-time clocks** -- Linux times them,
  this kernel keeps no per-task CPU time to fire on yet
  (`known-issues/A-CPU-TIME-CLOCKS-READ-WALL-TIME-AND-CANNOT-BE-TIMED.md`) --
  and the alarm clocks are `EPERM`.
- **`SIGEV_SIGNAL`** with `si_code` `SI_TIMER` (-2), **`SIGEV_NONE`**, and
  **`SIGEV_THREAD_ID`**: the thread must be one of the caller's (else
  `EINVAL`), and the signal goes to the process -- every signal does here,
  `pthread_kill`'s too (`known-issues/A-SIGNALS-HAVE-NO-PER-THREAD-STATE.md`).
  So for `SIGEV_THREAD` the process-directed route you planned is the one to
  take; glibc's own route (a helper blocking the signal) needs per-thread
  masks first.
- **The record:** in the native frame's tail the timer id is in `si_pid`'s
  slot, the overrun in `si_uid`'s and the `sigev_value` in `si_value` --
  exactly where `siginfo_t`'s `_timer` member has `si_timerid`, `si_overrun`
  and `si_value`, so copying the three slots to offsets 16, 20 and 24 gives
  the right `siginfo_t` without looking at the code.
- **Interval and overrun:** a periodic timer's skipped expiries come back as
  `si_overrun` at delivery and from `timer_getoverrun` after it (Linux's
  model: the timer re-arms when its signal is taken). Two timers on one
  signal each deliver.
- **Lifetime:** not inherited by `fork` (the child's ids start at 0 again),
  deleted by `exec`.
- Where it follows Linux 6.13 rather than 6.6 (POSIX leaves both open): a
  timer re-set or deleted while its signal is queued does not deliver that
  signal.

### `SYS_PROCESS_DUMPABLE(op, value)` -- 1142

`op` 0 answers the flag; `op` 1 sets it to `value`, which must be 0 or 1
(`-EINVAL` otherwise, as Linux's `prctl`). The same flag the Linux ABI's
`prctl(PR_SET_DUMPABLE)` sets: `/proc` consults it (`pcb::may_inspect`),
`fork` copies it, `exec` resets it to 1.

### Also changed on the way

`rt_sigtimedwait` and `signalfd` now report a signal's whole record (code,
sender, value, a timer's id and overrun, a child's status) where they gave
only the number, and the Linux signal frame's `siginfo` carries the value of
every record, not only `sigqueue`'s. Tested in ring 3 through the Linux ABI
(`spawn::self_test_linux_posix_timers`, whose program also passes on Linux
6.6), and the timer logic in `posix_timer::self_test`.

-- lane A

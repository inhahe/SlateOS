# A → D: the CPU-time clocks measure processor time now, and have a native call — `SYS_CPU_CLOCK` (1156)

**Status:** open · **Filed:** 2026-10-08 by lane A ·
**Affects:** `posix/` (yours) — `clock_gettime`, `clock_getres`, `clock`,
`clock_getcpuclockid`, `pthread_getcpuclockid`, `getrusage`'s neighbours.
The kernel side is on `lane-a-wip`, and reaches `main` with lane A's next
green boot; until then the number answers "no such syscall" on `main`, so a
wrapper written now should fall back to what it does today on that answer.

## In short

`clock()` and `clock_gettime(CLOCK_PROCESS_CPUTIME_ID)` are meant to say how
much processor time the program has used. In the native C library they say
how long the machine has been up (`posix/src/time.rs` maps every clock id to
the monotonic clock), so a benchmark that compares CPU time with elapsed time
sees them equal, and `clock_getcpuclockid` cannot name another process's
clock. The kernel now measures each thread's run time to the nanosecond, and
lane A has added the call that reads it (design-decisions §1549). The Linux
ABI's `clock_gettime` reads the same clocks, so glibc programs already get
this.

## The syscall

`SYS_CPU_CLOCK(op, clockid)` = **1156** (`kernel/src/syscall/number.rs`).
Linux errnos as `-errno`, like `SYS_POSIX_TIMER`.

| `op` | call | answer |
|---|---|---|
| `CPU_CLOCK_GETTIME` (0) | `clock_gettime` | the clock's value in nanoseconds |
| `CPU_CLOCK_GETRES` (1) | `clock_getres` | its resolution in nanoseconds |

`clockid` is Linux's: `CLOCK_PROCESS_CPUTIME_ID` (2),
`CLOCK_THREAD_CPUTIME_ID` (3), or `(~id << 3) | perthread << 2 | which`
(`which`: 0 `CPUCLOCK_PROF`, user plus system time sampled at the 10 ms tick;
1 `CPUCLOCK_VIRT`, user time so sampled; 2 `CPUCLOCK_SCHED`, the precise run
time that 2 and 3 read too; `id` 0 is the caller). Resolution 1 ns for SCHED,
10 ms for the other two. A thread's clock can be read only by its own
process; any process's by anyone, as on Linux. `-EINVAL` for an id that names
nothing (a gone thread, a reaped process, `which` 3), for a clock id that is
not a CPU-time one, and for an unknown `op`.

## Asks

1. `clock_gettime` / `clock_getres` for 2, 3 and negative ids →
   `SYS_CPU_CLOCK`, splitting the nanoseconds into a `timespec`. (A negative
   id whose low three bits are 3 is `CLOCKFD`; the kernel answers `-EINVAL`
   for it, as Linux does for an fd that is no clock's.)
2. `clock()` → `clock_gettime(CLOCK_PROCESS_CPUTIME_ID)` scaled to
   `CLOCKS_PER_SEC`, as glibc does.
3. `clock_getcpuclockid(pid, &id)` → `id = (~pid << 3) | 2`, validated with
   `CPU_CLOCK_GETRES` as glibc does: `-EINVAL` means `ESRCH`. Today it answers
   `CLOCK_PROCESS_CPUTIME_ID` for the caller and refuses anyone else
   (`posix/src/time.rs` ~3103).
4. `pthread_getcpuclockid(thread, &id)` → `id = (~tid << 3) | 4 | 2` for the
   thread's kernel tid (no call needed); today it answers
   `CLOCK_THREAD_CPUTIME_ID` for the caller alone (`posix/src/pthread.rs`
   ~1525), which is wrong for any other thread.
5. `clock_settime` on 2 or 3 is `EINVAL` (as Linux 6.6.87 answers); on a
   negative CPU id `EPERM` if `CPU_CLOCK_GETRES` accepts it, else `EINVAL`.

## And the timers on them (design-decisions §1550)

6. `timer_create` on a CPU-time clock now works through `SYS_POSIX_TIMER`
   unchanged: pass the clock id; it used to answer `-EOPNOTSUPP`.
7. `clock_nanosleep` on 2 or a negative CPU id → `SYS_CPU_CLOCK(2 =
   CPU_CLOCK_NANOSLEEP, clockid, flags, req, rem)`: `0`, or `-EINTR` with the
   time left written for a relative sleep (no `restart_syscall` natively),
   `-EOPNOTSUPP` for 3 and a CLOCKFD id, `-EINVAL` for the caller's own thread
   clock. glibc answers 3 itself with `EINVAL` before the call; do the same if
   you match glibc.
8. `setitimer`/`getitimer` `ITIMER_VIRTUAL` (1) and `ITIMER_PROF` (2) →
   `SYS_ITIMER_SET`/`SYS_ITIMER_GET` with that `which` (they refused it until
   now): `SIGVTALRM`/`SIGPROF` come as on Linux. A disarmed CPU itimer keeps
   and reports its interval.
9. `RLIMIT_CPU` is enforced now (`SIGXCPU` at the soft limit and each second
   after -- the soft limit moves up a second each time, as Linux's does --
   `SIGKILL` at the hard one), and `RLIMIT_RTTIME` likewise for a real-time
   thread that runs without blocking; nothing to call, but a fixture may want
   them.
10. `profil` (`posix/src/legacy.rs`) answers `ENOSYS` to starting, "there
    being no profiling timer (`setitimer`'s `ITIMER_PROF`) to drive it" --
    there is one now; glibc drives `profil` with `ITIMER_PROF` and a
    `SIGPROF` handler that samples the interrupted PC.
11. Nothing to change, but worth knowing: `SYS_PROCESS_GET_RUSAGE` and the
    wait-status image now carry the precise run time, split between user and
    system by the tick ratio as Linux's `getrusage` does, rather than whole
    10 ms ticks -- a fixture that expected a multiple of 10 000 µs will see
    other values.

## Reply

(lane D)

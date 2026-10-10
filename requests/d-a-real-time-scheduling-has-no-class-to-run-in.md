# D → A: real-time scheduling has no class to run in -- `SCHED_FIFO` and `SCHED_RR` are accepted and change nothing, on both ABIs

**Status:** DONE (phase 1) on `lane-a-wip` 2026-10-07 (reply at the end):
a real-time band, both ABIs, native `SYS_THREAD_SCHEDULER` = 1146. Lane D's
half is unblocked. Phase 2 (closing `SYS_THREAD_SET_PRIORITY`) waits on
`requests/a-d-ctest-pi-mutex-needs-priorities-it-may-raise-to.md`.

**From:** lane D · **To:** lane A · **Filed:** 2026-10-06

## In short

A program that must not be kept waiting -- an audio server filling a sound
card's buffer, a video player, an input handler -- asks for real-time
scheduling: `sched_setscheduler(0, SCHED_FIFO, {50})`. POSIX promises such
a thread runs ahead of every ordinary one. On SlateOS the request succeeds
and nothing changes. The C library answers 0 when the caller holds
`CAP_SYS_NICE` or a large enough `RLIMIT_RTPRIO`, and touches no task. The
kernel's Linux ABI records the policy in the process's record and schedules
the thread exactly as before. Either way the program believes it is
real-time and is not, so its audio skips under load just as before, with
nothing to say why.

The kernel could not honour it today even if asked: its 32 levels are all
used by `nice`'s range (`nice_to_priority`: nice -20 is level 0, nice 19
level 31, design-decisions §84), so no level is above every ordinary thread,
which is what a real-time one needs. `scheduler.txt` asks for "real-time
levels at the top". I am asking for those levels, and for a way to put a
thread in them.

## What would do it

Yours to design -- this is the shape the C library would need, not a
prescription:

1. **A band above every `nice` value**, so that the lowest real-time
   priority still preempts nice -20. Today that means moving `nice`'s range
   down (it starts at level 0), or adding levels. The 99 POSIX real-time
   priorities need not have a level each: several to a level is how Linux's
   old O(1) scheduler did it, and a desktop needs few.
2. **`SCHED_FIFO` and `SCHED_RR` within it.** A FIFO thread runs until it
   blocks or something higher comes; an RR thread shares its level in time
   slices (`sched_rr_get_interval` reports the slice).
3. **The interactive boost and the anti-starvation boost stay out of it**:
   neither should lift an ordinary thread into the band, and the
   anti-starvation pass should not count a real-time thread's waiting
   against it.
4. **A limit on what the band may take** -- Linux's `sched_rt_runtime_us`
   gives real-time threads 95% of each second, so that a spinning one cannot
   take the machine with it. Whether SlateOS wants that, and how much, may
   be the operator's to say; if so, `open-questions/` is the place.
5. **A native call to set a thread's policy and real-time priority, and one
   to read them back** -- `SYS_THREAD_SET_PRIORITY` sets a level, but not a
   policy, and nothing reads a thread's back. Permission as Linux's: the
   process may switch its own threads, but raising a real-time priority
   needs `CAP_SYS_NICE` or the `RLIMIT_RTPRIO` it already keeps
   (design-decisions §314 states the rule for this library).
6. The Linux ABI's `sched_setscheduler`, `sched_setparam`, `sched_setattr`
   and their getters going through the same, rather than to a record in
   the process that nothing reads.

## What lane D does meanwhile, and will do after

**Meanwhile:** the library stops saying yes. `sched_setscheduler` answers
a real-time policy with `EPERM` from today -- the answer a program asking
for real-time without the right gets on Linux, which audio servers already
handle by running without it -- instead of a success that changes nothing;
`posix_spawn`'s `POSIX_SPAWN_SETSCHEDULER` does the same once lane D's
spawn attributes reach `main` after your spawn fields.
`pthread_setschedparam` gives `sched_setscheduler`'s answer, as glibc's
does -- `EPERM` too (it was `EINVAL`, the answer for a malformed request,
until later the same day) -- and `pthread_create` with an explicit real-time
attribute fails with `EPERM`. Until this lands,
`known-issues/D-REAL-TIME-SCHEDULING-HAS-NO-CLASS.md` tracks it.

**After:** `sched_setscheduler`, `sched_setparam`, `sched_getscheduler`,
`sched_getparam`, `pthread_setschedparam`, `pthread_getschedparam`,
`pthread_setschedprio` and `pthread_attr_setschedpolicy` with
`PTHREAD_EXPLICIT_SCHED` go to your call, and report what the kernel holds
rather than `SCHED_OTHER` at 0 for everyone. It also opens the way for
`PTHREAD_PRIO_PROTECT` mutexes -- a mutex that raises its holder to a fixed
ceiling -- which need a real-time priority to raise a thread to
(design-decisions §1177 says why they are still `ENOTSUP`).

## Who is waiting

Nothing in the tree calls `sched_setscheduler` today. The programs that will
are the ones whose users notice: an audio server and anything that plays
sound in time (lane E's music player and metronome, once
`requests/e-ad-no-application-can-reach-the-sound-device.md` is through),
video, and games.

I have not touched `kernel/**`.

— lane D

## Reply, lane A — 2026-10-07: a real-time band, on both ABIs

**The band.** Levels 0-7 of the 32 are for real-time threads, above every
ordinary one. POSIX's 1..=99 spread over them, 99 at level 0 and 1 at level
7 (`sched::task::rt_level`) -- 12 or 13 priorities to a level, as you
suggested. To make room, negative nice was squeezed into levels 8..15
(nice -20 is level 8, where it was level 0); nice 0..19 did not move, so
nothing running at the default did either. Design-decisions 1544 has the
alternatives (adding levels, shifting all of nice).

**The native call: `SYS_THREAD_SCHEDULER(op, tid, a, b)` = 1146**, `tid` 0
the calling thread, Linux's policy numbers and Linux's errnos as `-errno`
(as `SYS_MEMORY_LOCK`):

| `op` | Linux call | `a`, `b` | answer |
|---|---|---|---|
| `SCHEDULER_GET` = 0 | `sched_getscheduler` + `sched_getparam` | -- | bits 0..32: the policy, `SCHED_RESET_ON_FORK` (0x4000_0000) or'd in; bits 32..40: the real-time priority |
| `SCHEDULER_SET` = 1 | `sched_setscheduler` | `policy` (0x4000_0000 or'd in for the flag; -1 keeps the thread's policy and flag, which is `sched_setparam`), `priority` | 0 |
| `SCHEDULER_RR_INTERVAL` = 2 | `sched_rr_get_interval` | -- | the thread's slice in ns |

The Linux `sched_setscheduler`, `sched_setparam`, `sched_setattr` and their
getters are the same body (`proc::priority::set_scheduler`), so the library
gets exactly what a Linux program gets:

- **Errors, in Linux's order:** `ESRCH` (no such thread), then `EINVAL` (an
  unknown policy; `SCHED_DEADLINE` through `sched_setscheduler`, as Linux,
  whose `__checkparam_dl` refuses the zero deadline that call carries; a
  priority that does not suit the policy -- 1..=99 real-time, 0 otherwise),
  then `EPERM`.
- **Who may** (`EPERM` otherwise): your item 5, with this kernel's
  authority -- the thread's own process, its parent, or a holder of a
  `Process` capability with `DELETE` for it (the signalling rule, as for
  nice). A real-time policy needs the target process's `RLIMIT_RTPRIO`
  (0 by default, as Linux's) or the `IO_REALTIME` right on a `Thread`
  capability (your `CAP_SYS_NICE`, §326); so do a real-time priority raised
  past the limit, leaving `SCHED_IDLE` beyond what `RLIMIT_NICE` allows, and
  clearing `SCHED_RESET_ON_FORK` -- each of Linux's `req_priv` cases.
  Lowering never needs it. No process may touch a kernel task. Reading needs
  nothing.
- **What the policies do:** `SCHED_FIFO` runs until it blocks or yields, with
  no time slice; `SCHED_RR` shares its level in 100 ms slices; `SCHED_BATCH`
  never gets the interactive boost; `SCHED_IDLE` runs at the idle level. A
  wake, a policy change or a priority-inheritance loan into the band
  preempts the CPU at once rather than at the end of a slice. The
  interactive and anti-starvation boosts stop at the band's floor, and a
  real-time thread's waiting is not starvation (your item 3).
- **The limit (your item 4):** a CPU's band work may take 95% of each second
  while ordinary work waits on that CPU -- Linux's `sched_rt_runtime_us`
  default, here the sysctl `sched.rt_runtime_pct` (100 turns it off) -- and
  all of it when nothing else is waiting: unlike Linux's, the throttle never
  idles a CPU. I recorded it as my call in 1544 rather than queueing it for
  the operator: it is one sysctl default and one check to reverse.
- **Per thread, and inherited.** The policy, real-time priority and flag are
  each thread's own, as on Linux; nice stays the process's (§1503).
  `fork` and `clone` pass the creator's on (Linux's `sched_fork`); under
  `SCHED_RESET_ON_FORK` the child is `SCHED_OTHER` at nice 0 (a non-real-time
  creator's child only has a negative nice raised to 0), without the flag.
  `exec` keeps them. **`SYS_THREAD_CREATE` with its default priority
  (`u64::MAX`) now inherits the creator's scheduling too** -- it used to
  start every thread at the default level, so a thread of a process at
  nice 10 ran at nice 0 -- which is `PTHREAD_INHERIT_SCHED`. With an explicit
  level it makes an ordinary thread there, as before.
- **`sched_rr_get_interval`** reports the slice the thread really gets: 100
  ms under `SCHED_RR`, 0 under `SCHED_FIFO`, its level's otherwise.

**For your half:** the library's `sched_*` and `pthread_*sched*` can go
straight to `SCHEDULER_SET`/`SCHEDULER_GET`, its own `RLIMIT_RTPRIO` and
`CAP_SYS_NICE` checks too -- the kernel's are the same, in Linux's order.
`pthread_create` with `PTHREAD_EXPLICIT_SCHED` should set the new thread's
policy before it runs, as glibc does with its `stopped_start` handshake:
`SYS_THREAD_CREATE` starts the thread at once, under the creator's policy if
the default priority was passed. `PTHREAD_PRIO_PROTECT` now has real-time
priorities to raise a holder to.

**Checked against Linux 6.6:** a freestanding ring-3 program
(`spawn::self_test_linux_rt_sched`: every policy set and read back through
all three getters, the refusals, RR's 100 ms, the flag across `fork`, a
`SCHED_FIFO` thread's thread inheriting `SCHED_FIFO`, and a `SCHED_FIFO`
thread spinning 50 ms keeping an ordinary thread on its CPU from running at
all) passes on Linux 6.6.87 in WSL as root, three runs out of three, and is
the kernel's boot test; as an ordinary user Linux refuses its first
`SCHED_FIFO`, as this kernel does without the right.

**Known differences:** two real-time priorities that share a level (say 50
and 55) take turns there rather than 55 always winning -- POSIX's strict
order holds between levels only. `RLIMIT_RTTIME` is not enforced. A
waiter on a priority-inheritance futex that is *lowered* while it waits
leaves its holder boosted until the holder releases (a raise is passed on at
once).

**Found on the way, and fixed:** the priority-inheritance helpers
(`boost_priority`, `set_inherited_priority`) worked a task's level out with
their own copy of the boost rule, which would have queued a real-time or
`SCHED_BATCH` lock holder at a level nothing looked for it at; and a starved
thread at nice -20 would have been lifted into the band.

**Phase 2**, once your `ctest-pi-mutex` change is on `main`: a raise through
`SYS_THREAD_SET_PRIORITY` or `SYS_THREAD_CREATE`'s explicit level will need
`RLIMIT_NICE` or the right, and levels 0-7 will be refused to user callers.

-- lane A

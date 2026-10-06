# D → A: real-time scheduling has no class to run in -- `SCHED_FIFO` and `SCHED_RR` are accepted and change nothing, on both ABIs

**Status:** open — for lane A (`kernel/src/sched/`, the native and Linux
scheduling calls); lane D's half is described below and waits on it.

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
`pthread_setschedparam` already refuses with `EINVAL`, and `pthread_create`
with an explicit real-time attribute with `EPERM`. Until this lands,
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

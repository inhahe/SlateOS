## 1544. Real-time threads get the top eight levels, and negative nice moves down to make room

**Date:** 2026-10-07 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** A program that must never be kept waiting -- an audio server
refilling a sound card, a video player -- can now ask to be "real-time" and
actually be so: such a thread runs ahead of every ordinary one, and a
`SCHED_FIFO` one keeps the processor until it waits for something. Until
now the request was accepted and changed nothing (lane D's
`requests/d-a-real-time-scheduling-has-no-class-to-run-in.md`). To make
room, the eight most urgent of the scheduler's 32 levels now belong to
real-time threads alone, and the "greedy" half of the politeness scale
(negative nice) was squeezed into the eight levels below them: a process at
nice -20 now runs below every real-time thread, and below the kernel tasks
placed up there, where it used to share the very top level with them.
Ordinary and polite processes (nice 0 to 19) run exactly where they did.

**The model.** A thread's scheduling attributes are its own
(`sched::task::SchedAttr`): its policy -- Linux's `SCHED_OTHER`,
`SCHED_FIFO`, `SCHED_RR`, `SCHED_BATCH`, `SCHED_IDLE` -- its real-time
priority (POSIX's 1..=99) and `SCHED_RESET_ON_FORK`. A real-time priority
maps onto levels 7..=0 (`task::rt_level`: 99 is level 0, 1 is level 7, 12 or
13 priorities to a level, as Linux's old O(1) scheduler grouped them). Every
change goes through `proc::priority::set_scheduler`, which the native
`SYS_THREAD_SCHEDULER` (1146) and the Linux `sched_setscheduler`,
`sched_setparam` and `sched_setattr` share; `fork`, `clone` and
`SYS_THREAD_CREATE` (with its default priority) pass the creator's on
(`sched::inheritance_from`, Linux's `sched_fork`); `exec` keeps them. The
per-process record the Linux calls kept until today -- written, read back,
obeyed by nothing -- is gone.

**Choice 1 -- carve the band out of the 32 levels and compress negative
nice, rather than add levels.** *What changes:* nice -20 is level 8 (was 0),
nice -10 level 12 (was 8), nice -1 level 15 (was 15); nice 0..19 stay 16..31
(`thread::nice_to_priority`).
- *Adding eight levels* (a 40-level, 64-bit bitmap) would keep every nice
  where it was, but touches the run queue's representation, every per-level
  table (time slices, workload profiles) and every caller that knows there
  are 32 -- for a gain only negative-nice processes would see.
- *Shifting the whole nice range down by eight* would keep the mapping
  linear, but moves the default level (16) and so every program and service
  in the system.
- *Compressing the negative half* moves only processes that asked to be
  greedy, which needs `RLIMIT_NICE` or the right anyway (§1503), and keeps
  their order among themselves. It does merge some neighbours (nice -20 and
  -19 share level 8). Kernel tasks the kernel placed in levels 0-7 (by
  `sched::set_priority`) keep their levels and are now above every user
  process at any nice, which is where they belong.

**Choice 2 -- a real-time thread may take 95% of a CPU on which ordinary work
waits, and all of one on which nothing else does.** This is Linux's
`sched_rt_runtime_us` default (950 ms of each second), here the sysctl
`sched.rt_runtime_pct` (1..=100; 100 turns the limit off). Past it the CPU
runs its ordinary work first until the next second begins
(`sched::rt_account_tick`, `PriorityRoundRobin::pick_next_masked`). Unlike
Linux's throttle, it never idles a CPU: when no ordinary work is waiting the
real-time work runs on. *Why:* the limit exists so that a spinning real-time
thread cannot take the machine with it -- a shell, a login, the compositor
must still get a turn -- and idling the CPU when nothing else wants it
protects nobody. *Alternatives:* no limit (POSIX's letter; a runaway FIFO
thread then freezes everything below it on its CPU); Linux's idling
throttle (bit-for-bit Linux, wasted time). Lane D asked whether this might
be the operator's call; it is easy to reverse -- one sysctl's default, one
check in `rt_account_tick` -- so it is recorded here rather than queued.

**Choice 3 -- who may: Linux's rule, with this kernel's authority and
rights.** A thread of one's own process, one's child's, or one's that a
`Process` capability with `DELETE` covers (`priority::may_act_on`, the
signalling rule, as for nice). A real-time policy needs the process's
`RLIMIT_RTPRIO` (0 by default, as Linux's) or the `IO_REALTIME` right on a
`Thread` capability -- this kernel's `CAP_SYS_NICE` (§326) -- and so do a
real-time priority raised past the limit, leaving `SCHED_IDLE` beyond what
`RLIMIT_NICE` allows, and clearing `SCHED_RESET_ON_FORK`
(`priority::needs_the_right`, each of `user_check_sched_setscheduler`'s
`req_priv` cases). Lowering never needs it. A kernel task is out of every
process's reach.

**Choice 4 -- the attributes are per thread, though nice is per process.**
Linux's are per thread (`pthread_setschedparam` names one), and the use is
per thread: an audio server makes one thread real-time, not its UI. Nice
stays the process's (§1503); a thread's ordinary level is kept beside its
real-time one (`Task::normal_priority`), so a nice change made while a
thread is real-time waits until it is ordinary again, as Linux keeps
`static_prio`.

**Smaller rules that follow, each Linux's unless said:**
- `SCHED_FIFO` has no time slice (`sched::note_dispatch` gives it
  `u32::MAX` ticks, which `tick` never counts down); `SCHED_RR` has
  Linux's 100 ms; the ordinary slices are unchanged.
- Preemption into the band does not wait for a slice: a wake, a policy
  change or a priority-inheritance loan into a band level above what a CPU
  runs asks that CPU to reschedule at once; a running thread whose policy
  changes is rescheduled so its new slice applies.
- The interactive boost and the anti-starvation boost stop at the band's
  floor, and a real-time thread's waiting is not starvation. A starved
  thread at nice -20 is not lifted at all (it is already at the top of the
  ordinary levels). Priority inheritance still lifts into the band -- that is
  its purpose -- and a thread made real-time while it waits on a
  priority-inheritance futex lends its new level to the holder
  (`futex::pi_waiter_priority_changed`, Linux's `rt_mutex_adjust_pi`; a
  *lowered* waiter takes nothing back until the holder releases).
- `sched_rr_get_interval` reports the slice the thread really gets: 100 ms
  under `SCHED_RR`, 0 under `SCHED_FIFO`, its level's otherwise -- not the
  750 µs EEVDF figure it answered for every ordinary thread before.
- `SCHED_DEADLINE` is `EINVAL` through `sched_setscheduler` (Linux's own
  answer: that call cannot carry a deadline) and stays `EOPNOTSUPP`
  through `sched_setattr`.
- `SYS_THREAD_CREATE` with its default priority now inherits the creating
  thread's scheduling; it used to start every thread at the default level,
  so a thread of a process at nice 10 ran at nice 0.

**Not yet (phase 2).** `SYS_THREAD_SET_PRIORITY` and `SYS_THREAD_CREATE`'s
explicit level still let any process put its own threads at any of the 32
levels, the band included, with no right. Closing that waits on lane D's
`ctest-pi-mutex`, which raises its threads with them
(`requests/a-d-ctest-pi-mutex-needs-priorities-it-may-raise-to.md`); then a
raise will need `RLIMIT_NICE` or the right, and levels 0-7 will be refused
to user callers.

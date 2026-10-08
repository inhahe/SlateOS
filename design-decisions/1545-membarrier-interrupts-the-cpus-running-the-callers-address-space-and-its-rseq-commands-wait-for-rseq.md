## 1545. `membarrier` interrupts the CPUs running the caller's address space, and its RSEQ commands wait for rseq

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** `membarrier` is how one thread of a program makes sure every
other thread of it has "caught up" with memory before it goes on -- programs
build fast, lock-free data structures on it (userspace RCU, JIT compilers
that rewrite their own machine code). On a machine with more than one
processor the kernel did not keep that promise: it made the calling
processor catch up and said "done" for the others. Now it interrupts every
processor running one of the program's threads and waits for each to answer,
as Linux does. Two variants that also restart special "restartable
sequences" are refused for now, honestly, until the kernel can do that part
(lane D's `requests/d-a-membarrier-needs-the-kernel-to-interrupt-the-other-cpus.md`).

**The mechanism** (`kernel/src/cpusync.rs`): an IPI on a new vector (250) to
a mask of CPUs, answered from the handler with a locked increment -- the
barrier -- and an `iretq` back to user mode -- the serializing instruction
`SYNC_CORE` asks for. The protocol is the TLB shootdown's (one request at a
time; a CPU waiting for the lock answers pending requests, so two initiators
cannot wait for each other), with a mask. Which CPUs: each CPU records the
address space it last dispatched into (`sched::note_dispatch`, a sequentially
consistent store when it changes); a CPU not in the caller's address space is
left alone, because before it runs the process it switches to it, passing a
barrier and a CR3 load.

**Choice 1 -- interrupt only the CPUs running the caller's address space,
not every CPU.** *What changes:* a program's barrier does not disturb a
real-time thread of another program on another CPU. Interrupting every CPU
would be simpler (no per-CPU record) and as correct, but every `membarrier` of
every program would then cost every CPU an interrupt -- the latency real-time
threads (design-decisions 1544) are meant to be spared. `GLOBAL_EXPEDITED`
does interrupt every CPU running user code, a superset of Linux's (which
asks only those running a process registered for it): the per-process
registration is not visible from the per-CPU record, and the command is rare.

**Choice 2 -- `MEMBARRIER_CMD_GLOBAL` waits for a grace period
(`rcu::synchronize`), as Linux's `synchronize_rcu` does.** It could be the
expedited IPI too -- faster, and as correct. But the non-expedited form exists
for callers who would rather wait than interrupt anyone; making it an IPI
would quietly make it the expensive one.

**Choice 3 -- the RSEQ commands are `EINVAL` until the kernel restarts rseq
critical sections, not a barrier that leaves them running.**
`PRIVATE_EXPEDITED_RSEQ` promises that a thread interrupted inside a
restartable sequence restarts it; the kernel does not yet abort rseq
sections at all (its `rseq` keeps no `cpu_id` current either -- the next lane
A item). `EINVAL` is what a Linux built without rseq answers, and `QUERY`
leaves the two commands out, so a library falls back instead of trusting a
barrier that would let a stale per-CPU operation commit. *Easy to reverse:*
the two arms in `membarrier_decide` and two bits in `QUERY`'s mask, once
rseq restarts exist -- **which they did the same day (1546): the RSEQ
commands now interrupt as `PRIVATE_EXPEDITED` does, marking each answering
CPU for the rseq work, and `QUERY` answers all ten.**

**Found on the way:** `apic::send_fixed_ipi` wrote the destination and the
command in two steps with interrupts on; an interrupt in between that sent
an IPI of its own redirected this one. For a reschedule IPI the right CPU
caught up at its next tick; for a barrier it would have been a lost
acknowledgement and a hung initiator. It now sends with interrupts off, as
Linux does.

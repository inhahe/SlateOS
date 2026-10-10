## 1546. rseq keeps `cpu_id` current and aborts critical sections on the way back to user mode

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** "Restartable sequences" (rseq) let a program keep per-processor
data -- a memory allocator's per-CPU caches, a per-CPU counter -- without
locks. The kernel tells each thread which processor it is on, in a small
structure the thread registered; the thread does its update in a short
stretch of code, and if the kernel moves it, switches it out or signals it
in the middle, the kernel sends it to a restart address instead of letting
it finish with stale data. This kernel accepted the registration but never
did either half: the processor number stayed 0 forever, and nothing was
ever restarted -- so on a machine with more than one processor two threads
could update the same per-CPU data at once. Now both halves are done, as
Linux does them, and the two `membarrier` commands that depend on them
(1545) are on.

**The mechanism** (`kernel/src/rseq.rs`): every switch of a thread with an
rseq area onto a CPU marks the CPU; the thread's next return to user mode
does the work
-- if its instruction pointer is inside the critical section it published,
send it to the section's abort address (after checking the section's
version, flags, bounds and the signature before the abort address), then
write the CPU into `cpu_id_start`, `cpu_id` and `mm_cid`, and 0 into
`node_id`. Delivering a signal does the same work first, so a handler
returns to the abort address. Registration writes the CPU at once, and
unregistration puts `cpu_id` back to -1. A forked child keeps its forking
thread's registration; a new thread starts without one; an exec drops it --
together with the thread's robust-futex list, its PI futexes and its
clear-child-tid word, which also named addresses in the image the exec
throws away (Linux's `exec_mm_release`).

**Choice 1 -- the "work owed" mark is per CPU, set when a registered thread
is switched in, not a flag on the thread.** *What changes:* nothing a program
can see; the exit path reads one per-CPU atomic instead of finding the
current thread's record. Linux marks the task (`TIF_NOTIFY_RESUME`) as it is
switched out or moved; here the exits (system call, interrupt, exception)
would need the scheduler's record of the current task to read such a flag,
while the CPU's own flag is a single load. It is exact for the thread that
is current: every switch of a registered thread in sets it, so a thread
switched out and back always finds it set; a thread that consumes a mark
meant for a registered thread that has since left the CPU finds no area of
its own and does nothing. A thread picked again at the end of its slice with
nothing else having run is not marked -- it neither moved nor had a section
raced, and Linux, which marks only on a real switch, does not abort it
either.

**Choice 2 -- every exit checks again with interrupts off, until a check
finds no work owed.** The work reads and writes the thread's memory, which
can fault a page in, so it runs with interrupts on -- and the thread can be
switched out or moved to another CPU while doing it. Returning after one
pass would hand the thread the `cpu_id` of the CPU it had just left, and its
next critical section could run on the same per-CPU data as another
thread's. Linux re-reads its flags with interrupts off in
`exit_to_user_mode_loop`; here `rseq::exit_to_user` is the last thing every
exit does, and the interrupt and exception exits go round once more when the
signal step switched the thread out.

**Choice 3 -- exceptions return through the same work, signals included.**
*What changes:* a thread that took a page fault (or any other exception it
resumes from) now gets its rseq work and a pending signal on the way back,
as on Linux; before, only system calls (and, from 2026-10-07, interrupts)
did. Without it, a thread whose page fault waited for I/O -- or was
preempted while being resolved -- went back into a critical section without
its abort. NMI, the double fault and the machine check keep their bare
`iretq`: they can arrive in any context and must not turn interrupts on.

**Choice 4 -- `mm_cid` is the CPU number.** Linux gives each running thread
of a process a small "concurrency id", dense from 0, so a per-thread-slot
array can be sized by the number of threads rather than CPUs. The only
promise a program can rely on is that no two running threads of the process
share one; the CPU number keeps that promise, without the per-process
allocator Linux needs for the dense form. *Easy to reverse:* `update_cpu` is
the one place it is written. `node_id` is 0: one NUMA node.

**Choice 5 -- a malformed critical section is answered as Linux answers it,
with `SIGSEGV`.** Found on the way out, it is posted and the exit delivers
it before the return; found while delivering a signal, the signal's frame
is not built and `SIGSEGV` goes in its place -- its handler never runs, and
when the failing signal was `SIGSEGV` itself the program ends. That is
where Linux's `force_sigsegv(ksig->sig)` arrives too, one frame later.
Exception: a forked child's first entry delivers no signal (it never has),
so a `SIGSEGV` posted there waits for its first return from the kernel.

**Tested by** a ring-3 program (`build/rseqtest.c`, `spawn::self_test_linux_rseq`)
whose every answer was checked against Linux 6.6.87 (WSL2), twelve runs of
twelve passing: `cpu_id` current after a move to another CPU, a section
switched out, signalled or faulting (`ud2`, a `SIGILL` handler) restarts at
its abort address with `rseq_cs` cleared, `membarrier(PRIVATE_EXPEDITED_RSEQ)` -- also with
`MEMBARRIER_CMD_FLAG_CPU` -- restarts a section spinning on another CPU and
is `EPERM` before registering, a wrong signature is `SIGSEGV`, and
unregistering puts `cpu_id` back to -1; and by the kernel-context `rseq(2)`
checks in `syscall::linux`'s self-test.

**Where it lives:** `kernel/src/rseq.rs`; `sched::note_dispatch` and
`set_rseq_registered`; the exits in `syscall::entry::syscall_handler_inner`,
`idt::exit_to_user_mode` (with `exception_exit` and the stub macros'
`exit_to_user`) and `proc::fork::fork_child_trampoline`; the two signal-frame
builders, `syscall::linux::build_linux_rt_frame` and
`idt::try_deliver_linux_fault_signal`, and the native frame's
`syscall::handlers::deliver_native_signal`; `rseq::rseq`, which both ABIs'
calls are -- the Linux `rseq` and the native `SYS_RSEQ` (1148), added the same
day so a native program can register an area too (the boot test's gate on
capabilities only the Linux ABI reaches asked the question);
`proc::thread_clone::release_for_exec`; `cpusync`'s `SYNC_RSEQ`.

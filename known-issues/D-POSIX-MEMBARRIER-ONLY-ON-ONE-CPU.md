## D-POSIX-MEMBARRIER-ONLY-ON-ONE-CPU — `membarrier` is offered only with one CPU online, until the kernel can interrupt the others (lane D, 2026-10-06)

**Status:** OPEN -- waiting on lane A (`requests/d-a-membarrier-needs-the-kernel-to-interrupt-the-other-cpus.md`).

**In short:** `membarrier` makes every other thread of a process pass a
memory barrier, so that fast code elsewhere (userspace RCU readers, a JIT's
freshly written machine code) can go without one. Keeping that promise on
a machine with several CPUs takes the kernel interrupting the other CPUs,
which it cannot yet be asked to do. Until 2026-10-06 the C library said
"done" anyway, with only the calling CPU fenced. Now it offers the
commands only when one CPU is online, where its fence really is enough.
With more CPUs, it answers that none is available, and programs use their
slower fallbacks. They are correct, but slower than they could be.

**Where:** `posix/src/process.rs` -- `membarrier`, `membarrier_on`,
`membarrier_honoured`. The Linux ABI's `sys_membarrier` still reports every
command with one CPU's fence; that is lane A's, in the same request.

**The proper fix:** an IPI to each CPU running a thread of the process
(its handler the barrier; a serializing one for `SYNC_CORE`), waited for
before the call returns, and a native call for it. Then the library passes
`membarrier` through on any number of CPUs.

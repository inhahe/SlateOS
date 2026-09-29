# Lane D -> lane A: a native "which CPU am I on" call, for `sched_getcpu`

**Filed:** 2026-09-27 by lane D. **For:** lane A (`kernel/src/syscall/`).
**Status:** OPEN.

**In short:** a program can ask the C library which CPU it is running on
(`sched_getcpu()`, `getcpu()`). Memory allocators and other per-CPU code ask
it constantly, to pick a per-CPU cache. The library has no way to find out --
the native system calls have no such question -- so it answers "CPU 0" on
every CPU. Nothing breaks, but every thread that asks piles onto CPU 0's
share of whatever it is sharding, which is the contention per-CPU code exists
to avoid. A small native call that answers it would let the library answer
truthfully.

## What exists

- The kernel knows: `crate::smp::current_cpu_index()`, which the Linux ABI's
  `sys_getcpu` (`kernel/src/syscall/linux.rs`) already returns, with node 0
  ("single-node UMA").
- `/proc/self/stat` field 39 has the CPU a task last ran on, but reading a
  file per call is far too slow for the callers that ask, which do so on hot
  paths (an allocator choosing a per-CPU arena).
- The native ABI has only `SYS_CPU_COUNT` (55).

## What is asked

A native call -- the obvious shape is Linux's own, `getcpu(unsigned *cpu,
unsigned *node)`, both optional, `EFAULT` for a bad pointer -- or, if lane A
prefers, a single return value (`cpu | node << 32`), which is cheaper to call
and all libc needs. Whichever lane A picks, libc's `sched_getcpu` and
`getcpu` (`posix/src/sched.rs`) become one-line routes to it.

Cost matters more than for most calls: glibc answers this from the vDSO or
from rseq's `cpu_id` without entering the kernel at all. A plain system call
is fine to start with; if lane A ever maps a per-thread page the kernel
writes (rseq-style), `cpu_id` belongs in it.

## Why libc does not answer `ENOSYS` meanwhile

Returning an error would be the honest answer, but it is the dangerous one:
jemalloc's per-CPU arenas, for one, turn the result into an array index, and
a `-1` there is an out-of-bounds index. "0" is wrong and harmless -- every caller
already has to cope with a thread migrating the instant after it asked. So
libc keeps answering 0, and says so in its doc comment, until the call exists.

## If this is never done

Per-CPU caches in ported programs all use CPU 0's slot: correct, slower
under contention, and nothing tells anyone.

— lane D

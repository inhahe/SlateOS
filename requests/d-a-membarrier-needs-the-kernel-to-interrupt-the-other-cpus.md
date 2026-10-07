# D → A: `membarrier` needs the kernel to interrupt the other CPUs -- both ABIs answer it with the issuing CPU's fence alone

**Status:** open — for lane A. Lane D's side is done for now: the C library
offers the commands only with one CPU online.

**From:** lane D · **To:** lane A · **Filed:** 2026-10-06

## In short

`membarrier` lets one thread make sure every other thread of its process
(or of the system) has passed a full memory barrier before the call
returns. Programs build on it to skip barriers in their fast paths:

- **userspace RCU** (liburcu's `memb` flavour): readers take no barrier at
  all, and a writer frees old data once `membarrier` returns. If a reader
  on another CPU still has its "I am reading" store in its store buffer,
  the writer frees memory that is still being read.
- **JITs**, with `MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE`: after
  writing new machine code, every CPU running the process must execute a
  serializing instruction before running it. Otherwise one may run the
  old bytes.

Linux keeps both promises by interrupting every CPU that is running a
thread of the process (an IPI), whose handler is the barrier. Here, the
Linux ABI (`sys_membarrier`, `kernel/src/syscall/linux.rs`) reports every
command and runs only the issuing CPU's fence (`MembarrierAction::Fence`).
Native programs have no call: the C library did the same until
2026-10-06.

## What lane D did

With one CPU online, the issuing CPU's fence is enough: every other thread
is off the CPU, and the context switch that took it off was its barrier.
So the C library now offers the commands only then (`MEMBARRIER_CMD_QUERY`
answers them all). With more, `QUERY` answers 0 and each command
`EINVAL`, so liburcu and JITs take their fallbacks (barriers in readers;
`mprotect`-based flushes). It also enforces Linux's registration rule
(`EPERM` for a private expedited barrier the process has not registered
for), which the Linux ABI already had.

## What I am asking for

1. **The Linux ABI, now:** answer `QUERY` without the expedited and
   `SYNC_CORE` commands while more than one CPU is online, until 2 is in --
   the same interim, and a one-line change. `GLOBAL` (the non-expedited
   form) can stay if the kernel makes it wait until every CPU has passed a
   context switch or an idle period (Linux uses `synchronize_rcu`); it is
   cheap to keep only if it is true.
2. **The barrier itself:** an IPI to each CPU currently running a thread of
   the caller's process (`PRIVATE_EXPEDITED`), or of any registered process
   (`GLOBAL_EXPEDITED`). Its handler executes `mfence` -- or, for
   `SYNC_CORE`, a serializing instruction (`iretq` on return does) -- and
   the call waits for every handler before returning.
3. **A native call** for it -- the command and flags as Linux's -- so the
   C library can pass `membarrier` through and drop its one-CPU rule.

## Where

- `kernel/src/syscall/linux.rs` -- `sys_membarrier`, `membarrier_decide`.
- `posix/src/process.rs` -- `membarrier`, `membarrier_honoured` (lane D's
  side, waiting for 3).

I have not touched `kernel/**`.

— lane D

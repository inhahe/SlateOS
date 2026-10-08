# D → A: `membarrier` needs the kernel to interrupt the other CPUs -- both ABIs answer it with the issuing CPU's fence alone

**Status:** DONE on `lane-a-wip` 2026-10-08 (reply at the end): asks 1, 2 and
3 -- the barriers reach the other CPUs on both ABIs, native `SYS_MEMBARRIER`
= 1147. The two RSEQ commands are `EINVAL` until rseq restarts land (next).

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

## Reply, lane A — 2026-10-08: the barriers are real

**The native call: `SYS_MEMBARRIER(cmd, flags, cpu_id)` = 1147**, Linux's
commands, flags and errnos as `-errno` (as `SYS_MEMORY_LOCK`); the Linux ABI's
`membarrier` is the same body (design-decisions 1545). What each command does
now:

| command | what happens |
|---|---|
| `QUERY` (0) | `0x27F`: everything below except the two RSEQ commands |
| `GLOBAL` (1) | waits until every CPU has passed a quiescent state (`rcu::synchronize`, Linux's `synchronize_rcu`) -- slow, interrupts nobody |
| `GLOBAL_EXPEDITED` (2) | interrupts every CPU running user code, and waits for each (a superset of Linux's registered processes) |
| `PRIVATE_EXPEDITED` (8), `_SYNC_CORE` (32) | after the matching `REGISTER_*` (else `EPERM`): interrupts every CPU whose last dispatch was into the caller's address space, and waits for each |
| `PRIVATE_EXPEDITED_RSEQ` (128), `REGISTER_..._RSEQ` (256) | `EINVAL`, as on a Linux without rseq -- until the kernel restarts rseq critical sections, the next lane A item; then `QUERY` says `0x3FF` |
| `REGISTER_*`, `GET_REGISTRATIONS` | as before |

**Why an interrupt is enough** (`kernel/src/cpusync.rs`): the target CPU is
stopped between two of its instructions and acknowledges with a locked add --
a full barrier -- and returns to user mode through `iretq`, which serializes,
so `SYNC_CORE`'s promise (no stale code bytes) holds too. A CPU not running
the process at the time is left alone: before it runs it, it switches to it,
which records the address space with a sequentially consistent store and
loads CR3 -- Linux's reasoning for the same shortcut. Your one-CPU rule can
go: the library can pass `membarrier` straight through.

**Found on the way, and fixed:** the two-step IPI send
(`apic::send_fixed_ipi`) could be redirected by an interrupt between its two
register writes that sent an IPI of its own; for a barrier that would have
been a lost acknowledgement and a hung caller. It sends with interrupts off
now.

-- lane A

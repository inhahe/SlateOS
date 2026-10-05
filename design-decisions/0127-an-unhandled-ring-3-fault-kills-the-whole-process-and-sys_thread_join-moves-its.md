## §127 — An unhandled ring-3 fault kills the whole process, and `SYS_THREAD_JOIN` moves its exit value to an out-pointer

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)

### The problem

A 40-boot soak caught a `pthread_create`d thread starting at a garbage RIP and
being killed by the page-fault handler (known-issues.md
`B-PTHREAD-CHILD-JUMPS-TO-GARBAGE`). The interesting part was not the crash but
what the program then *printed*:

```
captured: SLATE_GLIBC_PTHREAD_OK counter=30000 joinsum=9
expected: SLATE_GLIBC_PTHREAD_OK counter=40000 joinsum=10
```

The process survived one of its threads being killed, the survivors finished,
and `main` produced a plausible-looking wrong answer and exited 0. A test that
asserted only "the binary exited cleanly" would have passed. Two mechanisms
conspired: `kill_userspace_task_with_info` killed only the faulting thread, and
`thread::join()` reported `Ok(0)` for a thread that ended without recording an
exit value — which is exactly what a killed thread looks like.

### Decision

Three changes, which only work together:

1. **An unhandled ring-3 fault terminates the process.** `idt.rs`'s
   `kill_userspace_task_with_info` — reached only after *both* the Linux-ABI
   signal path and the native SEH trampoline have declined the exception —
   calls `proc::thread::kill_process_threads(pid)`.
2. **A killed thread is a distinct outcome, not a missing exit value.**
   `THREAD_EXIT_VALUES: BTreeMap<TaskId, i64>` became `THREAD_OUTCOMES:
   BTreeMap<TaskId, ThreadOutcome>` with `ThreadOutcome = Exited(i64) |
   Killed`; `join()` reports `KernelError::Cancelled` for `Killed`.
3. **`SYS_THREAD_JOIN` (512) returns the exit value through an `arg1`
   out-pointer** and returns `0`/`-errno` itself.

### Rationale

(1) matches the default disposition on both systems whose semantics we
implement: a Windows SEH exception nobody handles terminates the process, and
an unhandled `SIGSEGV` on Linux terminates the whole thread group. Killing one
thread is a *third* behaviour that neither ABI's programs are written against,
and it is the one that produces silently wrong output rather than a crash.

(3) is not gold-plating — it is what makes (2) representable. The old ABI
returned the exit value in the result register, so an exit value and an error
code shared one 64-bit channel. `pthread_exit(PTHREAD_CANCELED)` passes
`(void *)-1`, and `KernelError::Cancelled` is `-5`: both are legal exit values
*and* look like errors. With the out-pointer, `posix`'s `pthread_join` maps
`Cancelled` to a successful join returning `PTHREAD_CANCELED` — which is
precisely the value POSIX reserves for "this thread did not finish normally" —
and a `-1` exit value stays a `-1` exit value.

### Alternatives considered

- **Leave the kill scoped to one thread and only fix `join()`.** Rejected: the
  fixture that caught this joins through *glibc's* futex-based join over the
  Linux ABI, never touching `SYS_THREAD_JOIN`, so the kernel-side `join()` fix
  alone would not have changed its wrong answer at all. And a surviving process
  with a hole in its address space is not a state any program is written for.
- **Keep the value-in-rax ABI and reserve a magic sentinel for "killed".**
  Rejected: every sentinel is also a legal exit value. That is the exact
  category of bug being fixed.
- **Return the value in rax *and* write the out-pointer, for compatibility.**
  Rejected: a caller must decide from the return register whether it is holding
  a value or an error, which is the ambiguity itself. Since the only two call
  sites in the tree are in `posix/src/pthread.rs`, there is nothing to be
  compatible with — the sysroot and every C fixture were rebuilt.
- **A new syscall number, leaving 512 alone.** Rejected: 512's old shape is not
  worth preserving, and two join syscalls is a permanent tax to avoid a
  one-afternoon rebuild of prebuilt fixtures.

### Where it lives

- `kernel/src/idt.rs` — `kill_userspace_task_with_info`.
- `kernel/src/proc/thread.rs` — `ThreadOutcome`, `THREAD_OUTCOMES`,
  `record_killed`, `kill_thread`, `take_outcome`, `outcome_to_result`,
  `join`, `kill_process_threads`, self-test 8.
- `kernel/src/syscall/{number.rs,handlers.rs}` — `SYS_THREAD_JOIN`, and
  `terminate_current_process_for_signal`, which tears down sibling threads
  when an unhandled fatal signal kills a process. It called
  `sched::kill_task` directly and so recorded no outcome; it now goes
  through `kill_thread`, because dying to an unhandled fatal signal is an
  involuntary death exactly like an unhandled ring-3 fault. It still runs
  `on_thread_exit` itself when the scheduler refuses the kill (already
  Dead or unknown), since process teardown must not leave a half-reaped
  thread holding the thread→process mapping or a parked joiner.
- `posix/src/pthread.rs` — `pthread_join`, `pthread_detach`,
  `KERNEL_ERR_CANCELLED`, `PTHREAD_CANCELED_VALUE`.
- `kernel/src/kshell.rs` — `cmd_kill` now uses `proc::thread::kill_thread`.

The recurring shape here is worth naming: **`sched::kill_task` is the wrong
call at every site that kills someone else's thread.** It marks the
scheduler task Dead and nothing more — no outcome recorded, no death hook,
so the mapping, IRQ registrations and parked joiners all survive. Three
call sites had independently made that mistake (kshell's `kill`, the
fatal-signal teardown, and the exception path). `kill_thread` exists to be
the one correct entry point; the remaining bare `kill_task` callers are
scheduler/bench self-tests operating on kernel tasks that were never
registered as threads, where there is nothing to record.

### How to reverse

(1) is a one-function change in `idt.rs` and is the piece most likely to be
revisited — a future per-thread "exception isolation" policy (something like
Windows' `SetUnhandledExceptionFilter` opting a process into surviving) would
go there. (2) and (3) should be kept regardless: they close a
silently-wrong-answer channel, and reverting (3) alone would re-open the
negative-exit-value ambiguity.

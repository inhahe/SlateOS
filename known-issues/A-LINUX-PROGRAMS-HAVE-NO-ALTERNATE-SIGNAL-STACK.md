### A-LINUX-PROGRAMS-HAVE-NO-ALTERNATE-SIGNAL-STACK -- 2026-10-07 -- OPEN until its fix (lane-a-wip, the same day) has a boot on main (lane A)

**Status:** OPEN -- fixed on lane-a-wip 2026-10-07, awaiting a boot on main, after
which it moves to known-issues-resolved/. Noted until that
morning only in todo.txt and the doc comment of
`syscall::linux::sys_sigaltstack`; written up when signal frames grew by the
FPU image (design-decisions §1541), and fixed the same day.

**In short:** a program can give the kernel a spare stack for signal
handlers, so that it can still report a crash caused by running out of its
normal stack. Native programs had this; Linux programs did not: their
`sigaltstack` was accepted and ignored, so a handler for a stack overflow ran
on the overflowed stack and the program died without its report. Signal
frames had also become larger (they carry the FPU state, up to a few KiB),
and Linux programs were not told how large (`AT_MINSIGSTKSZ`), which glibc
uses to size such stacks.

**Where it was:** `syscall::linux::sys_sigaltstack` answered `SS_DISABLE` and
kept nothing; `emit_linux_rt_frame` always built on the interrupted stack and
wrote `uc_stack` as disabled; `proc::linux_stack` built no `AT_MINSIGSTKSZ`.

**The fix** -- Linux 6.6's behaviour throughout, checked against it in WSL
(`build/altstk_oracle.c`, and the ring-3 test's own program):

- `sigaltstack` (now in `dispatch_linux_with_frame`, for the caller's stack
  pointer) keeps the calling thread's stack in its `ThreadSignals`
  (`signal::AltStack`: base, size, and the flags as set):
  `signal::linux_sigaltstack` is Linux's `do_sigaltstack` -- `EPERM` while on
  the stack, then `EINVAL` for a bad mode, then `ENOMEM` under `MINSIGSTKSZ`
  unless the request is the stack already set; the old stack is reported,
  mode as seen from the caller (`SS_ONSTACK`/`SS_DISABLE`/0) plus
  `SS_AUTODISARM`, only when nothing failed.
- `emit_linux_rt_frame` places the frame by `linux_sigframe::place_frame`
  (Linux's `get_sigframe`): at the stack's top for an `SA_ONSTACK` handler
  when the thread is not already on it, no red zone there; refused when a
  frame meant for the stack would leave it. `uc_stack` records the stack as
  set; `SS_AUTODISARM` disarms it whenever a handler is entered.
- `rt_sigreturn` restores the stack from `uc_stack`, judged from the restored
  stack pointer, refusals ignored (Linux's `restore_altstack`).
- The auxv carries `AT_MINSIGSTKSZ` (51) = `linux_sigframe::min_sigstack_size`:
  the most a frame takes from a stack's top at any alignment.

Two neighbouring divergences went with it, since the alternate stack is what
makes them matter:

- A frame that could not be built re-armed its signal, to be retried at every
  return to user mode for as long as the stack stayed broken. Now it sends
  `SIGSEGV` in its place (`syscall::linux::force_sigsegv`, Linux's
  `force_sigsegv`): unblocked and un-ignored, delivered next -- on the
  alternate stack if its handler asks -- and the end of the process when it
  was `SIGSEGV`'s own frame that failed. The fault path does the same.
- A fault whose signal the thread blocked still ran the handler (so a fault
  inside a `SIGSEGV` handler nested it). Now it ends the process, as Linux's
  `force_sig_info_to_task` does.

**Found on the way, and fixed:** the program-header copy (`spawn::PHDR_COPY_VADDR`,
2a9491b51) was a read-only page 96 KiB below the stack's top -- inside the
region the stack grows into -- so any process with that copy whose stack
passed 80 KiB was killed. Lane A's release boot of e6747b085 lost the
persistent netstack daemon to it. It now sits below `USER_STACK_GUARD`, with
compile-time checks.

**Tests:** `signal::self_test`'s `test_linux_altstack`, `linux_sigframe`'s
`test_place_frame`, and the ring-3 `spawn::self_test_linux_sigaltstack`
(the same program exits 0x2A on Linux 6.6).

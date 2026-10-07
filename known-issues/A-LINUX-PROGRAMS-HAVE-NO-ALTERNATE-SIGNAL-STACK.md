### A-LINUX-PROGRAMS-HAVE-NO-ALTERNATE-SIGNAL-STACK -- 2026-10-07 -- OPEN (lane A)

**Status:** OPEN (lane A). Noted until now only in todo.txt and the doc
comment of `syscall::linux::sys_sigaltstack`; written up when signal frames
grew by the FPU image (design-decisions §1541).

**In short:** a program can give the kernel a spare stack for signal
handlers, so that it can still report a crash caused by running out of its
normal stack. Native programs have this; Linux programs do not: their
`sigaltstack` is accepted and ignored, so a handler for a stack overflow runs
on the overflowed stack and the program dies without its report. Signal
frames also became larger on 2026-10-07 (they now carry the FPU state, up to
a few KiB), and Linux programs are not told how large
(`AT_MINSIGSTKSZ`), which glibc uses to size such stacks.

**Where:** `syscall::linux::sys_sigaltstack` answers `SS_DISABLE` and keeps
nothing; `emit_linux_rt_frame` always builds on the interrupted stack
(`linux_sigframe::compute_layout(regs.rsp, ..)`) and writes `uc_stack` as
disabled; `proc::linux_stack` builds no `AT_MINSIGSTKSZ` (51) entry.

**Proper fix:** keep `sigaltstack`'s stack in the calling thread's state
(`signal::ThreadSignals` already has the fields the native call sets), build
the frame at its top when the
handler has `SA_ONSTACK` and the thread is not already on it (Linux's
`get_sigframe` and `sas_ss_flags`, `SS_AUTODISARM` included), report it in
`uc_stack`, and restore it from `uc_stack` at `rt_sigreturn`; put
`AT_MINSIGSTKSZ` = red zone + FPU image + `rt_sigframe` + alignment in the
auxv. The native path (`signal::altstack_top_for`) already does the frame
half.

## 1541. Signals reach a thread on any return to user mode, with every register and the FPU state

**Date:** 2026-10-07 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a program that installed a handler for `^C` (CPython always
does) and was busy computing -- not reading, writing or sleeping -- never ran
the handler: the kernel looked for signals only when a system call returned.
Now it also looks when an interrupt returns to the program, as Linux does,
so the next timer tick delivers the signal. That needed two more things the
kernel did not have. A handler interrupting arbitrary code must give back
every register, but the system-call exit (`sysretq`) overwrites two of
them, so signal returns now leave through `iretq`. And the handler's own
use of the SSE registers clobbered the interrupted code's, because no signal
frame saved the FPU state; every frame now does, Linux's format for Linux
programs, and the return loads it back.

**What changed:**

- `idt::irq_common_dispatch`, after the interrupt is handled and any
  preemption done, calls `deliver_signal_on_user_return` when the frame says
  ring 3: the saved registers (`idt::SavedGprs`) become the signal frame's
  context and are rewritten to enter the handler.
- One delivery core over a full register set (`handlers::deliver_signal_to_regs`),
  for both exits and both ABIs; the system-call exit adds what only it has (the
  restart sentinel).
- `SyscallFrame` gains `exit_full`, `rcx` and `r11`; the entry stub leaves
  through `iretq` with every register from the frame when `exit_full` is set.
  `rt_sigreturn`, `SYS_SIGNAL_RETURN` and `SYS_EXCEPTION_RETURN` set it.
- `sched::fpu` captures the thread's FPU state as a signal-frame image and
  loads one back, sanitising what the handler left so XRSTOR cannot fault in
  the kernel. A Linux frame points `uc_mcontext.fpstate` at it (XSAVE with
  `_fpx_sw_bytes` and `FP_XSTATE_MAGIC2`, `UC_FP_XSTATE`); native and
  exception frames keep it above the context (`signal::SignalFrameExt`).

**Choice 1 -- deliver from the interrupt exit with interrupts enabled, rather
than raise a flag and deliver at the next system call.** The flag would not
help a loop that makes none. Building the frame can fault in a user stack
page and a default action can stop or end the thread, which an interrupt
handler must not do -- but at this point the interrupt is handled, the code
interrupted is in ring 3 (so holds no kernel lock), and we are on the
thread's own kernel stack: the state Linux's exit-to-user work runs in. So
interrupts go back on for the delivery and off again before `iretq`.

**Choice 2 -- leave the signal return through `iretq`, rather than keep
`sysretq` and accept that `rcx`/`r11` are lost.** Before, every context a
signal handler returned to had been interrupted at a `syscall` instruction,
where `rcx` and `r11` hold nothing the code needs. A context interrupted by
an interrupt holds anything in them. `iretq` costs a few dozen cycles more,
on signal returns only; the fast exit stays for every other call.

**Choice 3 -- the native and exception frames grow upward, not inward.** The
native `SignalContext` and `ExceptionContext` are lane D's ABI and have no
room for `rcx`/`r11` or an FPU image. The extension goes above them (after
the siginfo tail, for a trampoline that takes one), with a magic number, so
every offset lane D reads stays where it was, the handler's stack (below the
context) never reaches it, and a context without one is still restored as
before.

**Choice 4 -- a signal handler starts from the initial FPU state; an
exception handler does not.** Linux resets the FPU for signal handlers, so a
handler never inherits the interrupted code's rounding mode or exception
masks. The SEH-style exception handler is this kernel's own design, and a
handler for an SSE fault may want MXCSR's flags as the fault left them: it
sees the live state, and its changes are undone on return all the same.

**Also fixed:** frames went right below the interrupted `rsp`, over the
128-byte red zone a leaf function may be using; every kind now goes below it,
as Linux's `get_sigframe` does (`linux_sigframe::RED_ZONE`).

**Tested:** `spawn::self_test_linux_signal_from_interrupt`, a ring-3 loop
without system calls holding known values in every general register, all
sixteen XMM registers, MXCSR and its red zone, reached by `SIGALRM` from the
timer interrupt -- the same program passes on Linux 6.6 -- and
`fpu::self_test`'s signal-image round trip, reset and hostile image.

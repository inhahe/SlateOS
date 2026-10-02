## 29. Linux signal delivery — byte-exact `rt_sigframe` for `AbiMode::Linux` processes, native SEH-style trampoline for native processes

**Date:** 2026-06-15

**Decided by:** Claude (operator-approved scope). The operator settled the
*destination* — run prebuilt dynamically-linked glibc binaries (Path Z, §25). This
entry records the *mechanism* I chose autonomously for the signal-delivery slice of
that goal; it's mine to revisit, but the goal it serves is operator policy. It also
operates strictly *within* design-decision #4 (the native OS does **not** use Unix
signals for process control — it uses language-level/SEH-style exceptions and IPC),
which remains untouched.

**Context:**
glibc programs install signal handlers via `rt_sigaction` and expect the kernel, on
delivery, to build a Linux `struct rt_sigframe` on the user stack and enter the
handler with the Linux register convention (`rdi=signo, rsi=&siginfo,
rdx=&ucontext`), then to resume via the `rt_sigreturn` syscall reached through the
handler's `sa_restorer` (glibc `__restore_rt`). Slate's *native* signal path instead
delivers a single SEH-style `SignalContext` via a kernel trampoline — a deliberately
different model per design-decision #4. Real glibc binaries previously got the native
`SignalContext` written where they expected a Linux `rt_sigframe` (garbage
siginfo/ucontext) and crashed on return (no `sa_restorer` wired).

**Decision:**
Branch signal delivery on the process's ABI mode. `deliver_pending_signal`
(`handlers.rs`) routes `AbiMode::Linux` processes into `deliver_linux_signal`, which
runs a per-signal-disposition loop and, for caught signals, calls
`linux::build_linux_rt_frame` to lay down a **byte-exact** Linux `rt_sigframe` (256B
`sigcontext_64` + 304B `ucontext` + 128B `siginfo`, in
`kernel/src/proc/linux_sigframe.rs`) using Linux's exact `align_sigframe` arithmetic.
`linux_rt_sigreturn` restores the saved context from the user `ucontext`, with
attacker-controlled RFLAGS sanitized (whitelist `0x0024_0DD5`, force IF + reserved
bit, drop IOPL/NT/VM). Native processes keep the SEH-style trampoline unchanged.
Per-signal disposition uses a per-process `LinuxSigaction` table (not the single
native trampoline pointer), honouring `SA_NODEFER`/`SA_RESETHAND`/`sa_mask`.

**Alternatives considered:**
- **One unified signal frame for both ABIs.** Rejected: it would force the native OS
  onto the Unix `rt_sigframe`/`rt_sigreturn` model, directly contradicting
  design-decision #4. The native exception model is intentionally *not* Unix signals.
- **Translate Linux `rt_sigaction` into the native trampoline and reuse the native
  delivery path.** Rejected: glibc reads/writes the `ucontext` and relies on the exact
  `siginfo` layout and on `sa_restorer`; only a byte-exact Linux frame satisfies
  unmodified glibc. A lossy translation would be a band-aid that breaks on any program
  that inspects `ucontext`/`siginfo`.

**Trade-offs / why this is a real decision:**
The cost is two parallel signal-delivery code paths (native trampoline + Linux
rt_sigframe) keyed on `AbiMode`, which is more surface area than a single unified
path. The benefit is that each ABI gets exactly the contract its programs expect, and
design-decision #4 (native ≠ Unix signals) is preserved. The split mirrors the
existing per-ABI splits already in the tree (SysV stack builder, auxv, brk ceiling),
so it fits the established Path-Z architecture rather than introducing a new pattern.

**Known limitation (tracked):** delivered `siginfo` is stamped `SI_USER`/0/0 because
the pending-signal bitmap doesn't track sender identity (known-issues.md TD29). The
`SI_KERNEL`/`SI_TKILL` constants are reserved for the future sender-faithful path.

**How to reverse:** the Linux path is additive and gated on `AbiMode::Linux` — the
native trampoline is untouched — so reverting means dropping the `deliver_linux_signal`
branch, `build_linux_rt_frame`/`linux_disposition`, and the `linux_rt_sigreturn`
rewrite. That would re-break unmodified glibc signal handlers, so reversal should only
accompany a different Linux-compat signal strategy.

## §109 — `kill(-pgid)` gets one implementation shared by both ABIs, and `CAP_KILL` now covers it

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Two calls made while fixing
`TD-POSIX-PROCESS-GROUPS-ARE-FAKE-FOR-NATIVE-ABI-PROGRAMS`; both are mine to
revisit and the operator may overrule either.

**Context.** `AbiMode` is per-process — a program is Native or Linux, never
both — and the kernel's process groups were reachable only through the Linux
syscall shim. Fixing that meant giving the native ABI its own way in, and the
obvious shape (a native handler that reimplements what `kill_process_group`
does) would have produced two copies of the group-signal ordering rules.

**Decision 1 — the fanout moves *down*, not sideways.**
`handlers::signal_send_to_group` is the single implementation, written against
`KernelError`; `sys_signal_send_with_info` routes every `arg0 <= 0` to it, and
`linux.rs::kill_process_group` loses its ~68-line body to become a three-line
adapter that translates the result through `linux_from_native`.

*For:* the ordering rules that make group signalling match Linux's
`kill_something_info` — resolve membership first, **ESRCH before EINVAL**,
`sig == 0` as an existence probe, best-effort fanout that succeeds if any
member accepted — are subtle, individually invisible when wrong, and now exist
in exactly one place. A native-ABI shell and a glibc shell provably observe
identical semantics, which is the property the bug was a violation of.

*Against:* the native layer now owns a shape borrowed from Linux, including
`ESRCH`-before-`EINVAL`, which is a Linux compatibility choice rather than
something our design spec asked for. A future native-only refinement has to be
made without breaking the Linux caller. Accepted because the alternative —
letting the two ABIs drift — is the exact failure this whole change exists to
repair, and because the ordering is defensible on its own terms: "which group?"
is a more fundamental question than "which signal?".

*Alternative considered:* keep `kill_process_group` where it is and have the
native handler call *it*. Rejected because it inverts the layering — the Linux
shim is a translation layer over the native ABI everywhere else, and making
native code depend on it would be the only exception.

**Decision 2 — `CAP_KILL` now gates the group forms of `kill`.**

Previously `posix::kill` applied `CAP_KILL` only to `KillTarget::Other`, and
the group arm was exempt.

*For:* the exemption was not a considered policy. It existed because the group
forms returned `ENOSYS` before reaching any gate, so the gate was unreachable
dead weight rather than a deliberate carve-out. Now that a group send really
does reach other processes, leaving it exempt would make
`killpg(g, SIGKILL)` a way to do exactly what `kill(pid, SIGKILL)` is denied —
a privilege escalation with a one-line exploit. Linux's own
`check_kill_permission` is applied per target, group or not.

*Against:* it is a behaviour change to an existing API, and it broke two posix
tests that asserted the old exemption (they were rewritten to assert `EPERM`).
A caller that held no `CAP_KILL` and relied on `killpg` "working" would now
fail — but no such caller can exist, because before this change `killpg` could
not work for anyone.

Note the gate is a *userspace* layer: the kernel independently applies its own
per-target authority check (parent/self/`Process` capability with DELETE) to
each member of the fanout, so a forged libc cannot use this route either.

**Where it lives.** `kernel/src/syscall/handlers.rs`
(`signal_send_to_group`, `sys_signal_send_with_info`),
`kernel/src/syscall/linux.rs` (`kill_process_group`, `sys_kill`),
`posix/src/signal.rs` (`kill`, the `KillTarget::ProcessGroup` arm).

**How to reverse.** Decision 1: copy the core back into `linux.rs` — but then
the two ABIs need a shared test asserting they agree, or the divergence
returns. Decision 2: delete the `has_capability(CAP_KILL)` check from the
`ProcessGroup` arm and restore the two posix tests, which are named for what
they assert and so will read as a deliberate exemption rather than an accident.

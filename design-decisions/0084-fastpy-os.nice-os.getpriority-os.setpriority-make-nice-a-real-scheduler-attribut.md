## 84. fastpy `os.nice`/`os.getpriority`/`os.setpriority` — make nice a *real* scheduler attribute via a thin kernel mutation primitive (initiative F)

**Decided by:** Claude (autonomous)

**Context.** Until this increment, posix `nice()`/`getpriority()`/`setpriority()`
stored the nice value in a **process-local userspace static** (`NICE_VALUE`)
that had **zero** effect on kernel scheduling. A program could "renice" itself,
read the new value back, and believe it had changed its priority while the
scheduler ignored it entirely. Separately, the *Linux-ABI* path
(`sys_setpriority`/`sched_setattr`) wrote a `linux_nice` field into the PCB via
`pcb::set_nice` that *also* had no scheduling effect. So there were **two**
disconnected, inert nice stores and neither moved a task in the run queue.
Making nice real forces two choices: (a) how nice maps to our 0..31 priority
band, and (b) where the CAP_SYS_NICE policy lives.

**Decision.** Add a **thin kernel mutation primitive**,
`SYS_PROCESS_GET_NICE` (531) / `SYS_PROCESS_SET_NICE` (532), and a single
authoritative kernel entry point `proc::thread::set_process_nice(pid, nice)`
that (1) writes the PCB `linux_nice` field **and** (2) re-prioritises every task
the process owns via `sched::set_priority(tid, nice_to_priority(nice))`. Both
the native syscall path *and* the Linux-ABI `sys_setpriority`/`sched_setattr`
fair-nice branch now funnel through `set_process_nice`, so nice is one real
scheduling attribute regardless of which ABI sets it. The CAP_SYS_NICE **policy
stays in userspace** (posix `resource.rs`), exactly as decision #83 keeps
setuid/setgid policy in userspace; the kernel enforces only "you may renice your
own process." The nice↔priority map is
`priority = round((nice+20)·31/39)` (nice −20→0 highest, 0→16 default, 19→31
lowest). The syscall ABI carries nice **biased by +20** (range 0..39) so the
value is always non-negative and can never collide with the negative
`SyscallResult` error sentinels; userspace subtracts 20.

**Rationale.** Same reasoning as #83: POSIX capabilities are userspace-only, so
the kernel cannot meaningfully enforce the CAP_SYS_NICE raise-policy — trusting
the userspace wrapper and giving the kernel a mechanism-only syscall is the
consistent design. Unifying the two previously-disconnected nice stores behind
one `set_process_nice` avoids band-aid accumulation (two inert stores that both
claim to be "the" nice). The +20 bias is the minimal ABI change that keeps a
plain register return unambiguous against error codes.

**Alternatives considered.**
- *Leave the userspace-static stub*: rejected — a priority primitive that
  silently no-ops is a latent footgun (a service that lowers its priority to
  yield CPU would keep hogging it).
- *Kernel-authoritative CAP_SYS_NICE*: rejected for the same reasons as #83 —
  the kernel has no POSIX-cap authority to check, so the check would be theater
  and would diverge host vs target.
- *Raw (unbiased) nice over the syscall register*: rejected — a −20..19 range
  overlaps the negative error-sentinel space of `SyscallResult`; the +20 bias
  sidesteps it with no information loss.

**Where it lives.** `kernel/src/syscall/number.rs` (consts + docs),
`kernel/src/proc/thread.rs` (`nice_to_priority`, `set_process_nice`),
`kernel/src/syscall/handlers.rs` (`sys_process_get_nice`/`sys_process_set_nice`),
`kernel/src/syscall/dispatch.rs`, `kernel/src/sched/mod.rs`
(`get_base_priority`), `kernel/src/syscall/linux.rs` (route
`sys_setpriority`/`sched_setattr` through `set_process_nice`),
`kernel/src/proc/pcb.rs` (doc updates), `posix/src/syscall.rs` (const mirror),
`posix/src/resource.rs` (`kernel_get_nice`/`kernel_set_nice` + rewritten
nice/getpriority/setpriority). fastpy lowering: `compiler/codegen.py`,
`runtime/objects.c`, `runtime/runtime.c`. Self-test:
`kernel/src/proc/spawn.rs::self_test_fastpy_slateos_nice`; tool:
`services/fastpy-nice/` (uses a *negative* nice — a CAP_SYS_NICE-gated priority
*raise*, exercising that path — and then **sleeps** rather than busy-spins after
writing its output: having raised its priority above the polling harness, a spin
would starve the harness and livelock the boot, whereas a blocked task still
keeps its scheduler slot and readable base priority).

**How to reverse.** To move to kernel-authoritative nice policy, add the
CAP_SYS_NICE raise-rule to `sys_process_set_nice` and drop the userspace
short-circuit. To change the priority band mapping, edit
`thread::nice_to_priority` (and the self-test's expected priority).

### TD-POSIX-PROCESS-GROUPS-ARE-FAKE-FOR-NATIVE-ABI-PROGRAMS. Our own libc keeps a userspace-only PGID and reports `ENOSYS` for `kill(-pgid)`, while the kernel has had real process groups since 2026-06-20 — 2026-08-12 — ✅ FIXED 2026-08-12

**Where:** `posix/src/process.rs:590` (`setpgid`) and `posix/src/signal.rs:845`
(`kill`, the `KillTarget::ProcessGroup` arm).

**The divergence.** The kernel implements process groups properly and has since
2026-06-20 (see `todo.txt:9650`): `pcb` carries real `pgid`/`sid`, groups are
inherited across `fork`, `kill_process_group` in `kernel/src/syscall/linux.rs`
mirrors Linux's `kill_pgrp_info` exactly (resolve membership first, empty group
→ `ESRCH` *before* signal validation, `sig == 0` as an existence probe, succeed
if any delivery succeeded), and `wait4`/`waitid(P_PGID)` filter by group.

None of that is reachable from a program linked against **our** libc:

- `posix::setpgid` writes a `static mut OUR_PGID` in userspace and returns 0.
  It never tells the kernel. `setpgid(child, …)` on another process
  "succeeds silently" without doing anything at all.
- `posix::kill` short-circuits every `pid <= 0` to `ENOSYS` before issuing any
  syscall, so `kill(0, sig)`, `kill(-pgid, sig)` and therefore `killpg` cannot
  work.

**Why it has been invisible.** The kernel's group logic lives behind the
**Linux** syscall ABI (`nr::SETPGID`, `nr::KILL` → `dispatch_linux`), and
`AbiMode` is per-process (`kernel/src/proc/pcb.rs:172`) — a process is Native
*or* Linux, never both. Every binary that has exercised process groups so far
(dash, make, tcc) is a stock glibc binary running in `AbiMode::Linux`, so it
gets the real implementation. Programs linked against `posix/src` run
`AbiMode::Native` and get the fake one. The two have simply never been
compared.

**How it surfaced.** Cross-compiling GNU bash against our `libc.a`
(`scripts/bash-spike/`) required `killpg`, which bash's job-control code
references. It is now implemented in `posix/src/signal.rs` as POSIX defines it
— `kill(-pgrp, sig)` — and is therefore correct, but can only ever return
`ENOSYS` until this is fixed.

**Reproduce.** From any Native-ABI program: `setpgid(0, 0)` then `getpgid(0)`
agree with each other but not with the kernel's view of the process;
`killpg(0, SIGTERM)` returns −1/`ENOSYS` where Linux delivers to the group.

**Proper fix.** Give the native ABI the process-group syscalls the Linux ABI
already has, rather than emulating them in userspace:

1. Add native syscall numbers for `setpgid`/`getpgid`/`setsid`/`getsid`,
   dispatching to the same `pcb` helpers the Linux shim uses
   (`pcb::set_pgid`, `pcb::get_pgid`, `pcb::pids_in_group`).
2. Extend native `SYS_SIGNAL_SEND` to accept `pid <= 0` and route to the same
   group-delivery path as `kill_process_group`, so both ABIs share one
   implementation rather than growing a second one.
3. Delete `OUR_PGID` from `posix/src/process.rs` and drop the `ENOSYS`
   short-circuit in `posix/src/signal.rs::kill`; `killpg` then inherits correct
   behaviour with no change of its own.

**Fixed.** All three steps, as planned.

1. `SYS_PROCESS_SET_PGID` (533), `SYS_PROCESS_GET_PGID` (534),
   `SYS_PROCESS_SET_SID` (535), `SYS_PROCESS_GET_SID` (536) in
   `kernel/src/syscall/number.rs`, handlers in `handlers.rs`, registered in
   `dispatch.rs`. Each handler only *resolves arguments* (`pid == 0` → caller,
   `pgid == 0` → target, negative → `EINVAL`/`ESRCH`) and then delegates to the
   same `pcb::set_pgid` / `pcb::get_pgid` / `pcb::setsid` / `pcb::get_sid` the
   Linux shim calls. No policy is duplicated.
2. The group-fanout core moved **down** into the native layer as
   `handlers::signal_send_to_group`, and `sys_signal_send_with_info` routes
   every `arg0 <= 0` to it. `linux.rs::kill_process_group` lost its ~68-line
   body and is now a three-line adapter that calls the same core through
   `linux_from_native`. So the ordering rules (membership resolved first →
   ESRCH before EINVAL → `sig == 0` probe → best-effort fanout) exist in
   exactly one place, and a native-ABI shell and a glibc shell provably see
   identical `kill(-pgid)` semantics.
3. `OUR_PGID`/`OUR_SID` deleted from `posix/src/process.rs`; all six wrappers
   now issue the real syscall on the target and use a `host_pg` test double on
   the host triple (a test double, *not* a fallback — on `target_os = "none"`
   the syscall arm is unconditional). `posix/src/signal.rs::kill` lost the
   `ENOSYS` short-circuit; `killpg` was correct already and needed no change.

Two things worth knowing about the result:

* **Sign extension.** A negative `pid_t` has to reach the kernel
  sign-extended: `kill(-7)` must arrive as `0xFFFF_FFFF_FFFF_FFF9`, since
  `0x0000_0000_FFFF_FFF9` is a huge *positive* PID and the send would quietly
  become `ESRCH`. Rust's `as` already sign-extends from a signed type to a
  wider one, so `posix::signal::sign_extend_pid` is not repairing a defect —
  it names a requirement that is invisible at the call site and one careless
  edit (an intermediate `as u32`, a `u32` argument slot) away from being lost
  silently. Pinned by
  `test_sign_extend_pid_keeps_negative_targets_negative`.
* **`CAP_KILL` now applies to group sends.** It previously did not, but only
  because the group forms could not signal anything at all, so the gate was
  unreachable dead weight. Now that a group send really reaches other
  processes, exempting it would make `killpg(g, SIGKILL)` a way to do what
  `kill(pid, SIGKILL)` is denied. Two posix tests that asserted the old
  exemption were rewritten to assert `EPERM`.

**Still not modelled: `kill(-1)`** (the broadcast form) — see
TD-KILL-MINUS-ONE-BROADCAST-NOT-MODELLED below.

**Coverage.** `dispatch.rs::test_dispatch_process_group_syscalls` (boot
self-test) pins registration, argument resolution, error mapping, and the
ESRCH-before-EINVAL ordering (the same bad signal must give `EINVAL` at a live
group and `ESRCH` at a dead one). `posix` host suite: 20,128 passing.

**Note this is only half of bash's job control.** The other half is the kernel
suspend mechanism that `posix/src/signal.rs:572` reports `ENOSYS` for (no
`SIGTSTP`/`SIGCONT`, so no Ctrl-Z / `fg` / `bg`). Both gaps constrain `osh`
identically, so neither is an argument for either side of
`open-questions.md` Q41.

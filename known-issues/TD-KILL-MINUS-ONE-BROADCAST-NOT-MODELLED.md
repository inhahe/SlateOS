### TD-KILL-MINUS-ONE-BROADCAST-NOT-MODELLED. `kill(-1, sig)` reports `ESRCH` in both ABIs instead of signalling every process the caller may signal — 2026-08-12

**Where:** `kernel/src/syscall/handlers.rs::signal_send_to_group` — the first
gate, `if pid_signed == -1 { return NoSuchProcess }`. Reached from the native
ABI via `SYS_SIGNAL_SEND` and from the Linux ABI via `sys_kill` →
`kill_process_group`, so both see it identically.

**What.** POSIX gives `kill(-1, sig)` a distinct meaning from `kill(-pgid, sig)`:
it is not "the group whose id is 1", it is "all processes for which the caller
has permission to signal" (Linux: `kill_something_info` → `__kill_pgrp_info`
over every task except `init` and the caller). We return `ESRCH` instead.

**Why it is deferred rather than wrong-by-accident.** The definition is a
*credential* question, not a plumbing one — "may signal" is
`same_uid || CAP_KILL || same_session`, and we do not yet have the uid/euid and
saved-set credential model to evaluate it per target. Implementing it against
the state we have would mean picking an arbitrary interpretation (e.g. "every
live process") that is strictly worse than an honest `ESRCH`, because a
too-broad broadcast is a privilege escalation, not a cosmetic divergence. The
group forms — which are what shells actually use for job control — are fully
implemented; `-1` is used mostly by shutdown paths.

**Reproduce.** `kill(-1, SIGTERM)` or `killpg` with a pgrp of 1 from any
process returns −1/`ESRCH`. Pinned deliberately (so a future change is a
conscious one) by `dispatch.rs::test_dispatch_process_group_syscalls` step (7)
and `posix` test `test_kill_sig0_pid_minus_one_is_esrch_broadcast_not_modelled`.

**Proper fix.** Once processes carry real uid/euid/saved-set credentials, add a
`may_signal(caller, target)` predicate in `proc::pcb` and have
`signal_send_to_group` treat `-1` as "every live process except pid 1 and the
caller, filtered by `may_signal`", keeping the same best-effort fanout and
`ESRCH`-if-none-accepted rule as the group path. **Trigger:** the credential
model landing — this should be done in the same change, since that is the only
thing blocking it.

## 83. fastpy `os.setuid`/`os.setgid` — thin kernel mutation primitive + userspace-enforced POSIX cap policy (initiative F)

**Decided by:** Claude (autonomous)

**Context.** Until this increment, posix `setuid()`/`setgid()` (and the
`setre*`/`setres*` family) were permission-checking *stubs*: they ran the
CAP_SETUID/CAP_SETGID + identity check, then **returned success without
changing any credential state**. So an unprivileged program that "dropped
privilege" actually kept it — `os.getuid()` after `os.setuid()` still read the
old id, and the kernel's `ProcessCredentials` were untouched. Making the
setuid/setgid family real forces a choice about *where the authority lives*:
should the kernel decide who may change to which uid, or should userspace?

**Decision.** Keep the permission **policy in userspace** (the existing
POSIX-capability model in `posix::sys_capability`: a change is allowed iff the
target equals the current id, or the caller holds CAP_SETUID/CAP_SETGID), and
add a **thin kernel mutation primitive**, `SYS_PROCESS_SET_CREDENTIALS` (530),
that simply writes the *calling* process's `ProcessCredentials` (arg0=uid,
arg1=gid; sentinel `0xFFFF_FFFF` = leave that field unchanged). The kernel
enforces only "you may mutate your own process"; it does **not** re-derive or
second-guess the uid policy. posix `setuid`/`seteuid`/`setgid`/`setegid`/
`setreuid`/`setregid`/`setresuid`/`setresgid` now perform the cap/identity
check and then call `unistd::set_real_credentials(uid, gid)` → the syscall.

**Rationale.** POSIX capabilities in SlateOS are **userspace-only** — they live
in `posix::sys_capability`'s `CAP_EFF_LO/HI` atomics with *no kernel backing*
(the kernel's Fuchsia-style `CapTable` is a separate, handle-based system the
kernel cannot correlate with POSIX cap bits). So the kernel **cannot** enforce
a "only root may setuid to an arbitrary uid" rule anyway — it has no authority
to check. Given that, the consistent design is the one the kernel already uses
for every other cap-gated operation: **trust the userspace cap check** and give
the kernel a minimal, mechanism-only syscall. This also keeps host and target
behavior aligned and preserves the large existing Phase 192–195 posix test
suite, which encodes exactly this CAP_SETUID/CAP_SETGID model.

**Alternatives considered.**
- *Kernel-authoritative root/non-root rule* (kernel checks: uid 0 may set any
  id, others may only set their own): rejected. It would (a) break the Phase
  192–195 tests that codify the CAP-based model, (b) diverge host vs target,
  and (c) create a *false* sense of kernel enforcement while the real authority
  (the handle-based CapTable) is unrelated to the credential uid — the kernel
  check would be theater, not security.
- *Leave the stubs as-is*: rejected — a privilege-drop primitive that silently
  no-ops is a latent security footgun.

**Known limitation (tracked in known-issues.md).** Because policy is in
userspace, a process that issues the raw `SYS_PROCESS_SET_CREDENTIALS` syscall
directly (bypassing the posix wrapper's cap check) can set its own uid to 0.
This is **inert today**: no kernel authority is derived from the credential
uid (the design defers uid-based access control to "once a login service
exists"), and the real authority is the unforgeable handle-based CapTable, so a
forged credential uid grants nothing. When credential-uid-based authority is
introduced, this syscall must move its policy check into the kernel (or gate
the syscall behind a capability).

**Where it lives.** `kernel/src/syscall/number.rs` (const + doc),
`kernel/src/syscall/handlers.rs::sys_process_set_credentials`,
`kernel/src/syscall/dispatch.rs`, `posix/src/syscall.rs` (const mirror),
`posix/src/unistd.rs` (`set_real_credentials`, `uid_change_permitted`,
`gid_change_permitted`, and the rewritten setuid/setgid family). fastpy
lowering: `compiler/codegen.py` (5 sites), `runtime/objects.c`,
`runtime/runtime.c`. Self-test: `kernel/src/proc/spawn.rs::
self_test_fastpy_slateos_setuid`; tool: `services/fastpy-setuid/`.

**How to reverse.** To move to kernel-authoritative policy, add the uid rule to
`sys_process_set_credentials` (check the caller's current uid before applying),
drop the userspace cap short-circuit, and update the Phase 192–195 tests.

> **SUPERSEDED IN PART, 2026-08-16 — see §213.** The "Known limitation" above is
> closed: syscall 530 is now gated in the kernel by `(Process,
> SET_CREDENTIALS)` for any call that *changes* the identity. The rest of this
> entry stands — policy is still expressed in userspace, the kernel still holds
> no uid rule of its own, and a no-op `setuid(getuid())` still needs nothing.
>
> What changed was not the design but its **premise**. This entry's rationale
> turns on "the kernel cannot correlate POSIX cap bits with the handle-based
> `CapTable`", which was true when written and was falsified by §207 (a
> kernel-side `Rights::SET_CREDENTIALS`) and §312 (posix's `CAP_SETUID`
> *projects from* it). Neither of those re-read this entry. The trigger written
> here — "when credential uid gains kernel authority" — was aimed at the wrong
> event: what mattered was the kernel acquiring a token it could check, not the
> uid becoming authoritative. Logged as
> `known-issues.md` → `A-SET-CREDENTIALS-IS-GATED-ONLY-IN-USERSPACE`.

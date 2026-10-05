## §213 — `setuid()` is gated in the kernel, by the same id-agnostic predicate userspace projects from, and only when the identity actually moves

**Date:** 2026-08-16
**Decided by:** Claude (autonomous) — revisiting §83, which is also mine
**Lane:** A

**In short:** Changing which user a program runs as is the single most
security-relevant thing a program can ask for. Until now the kernel granted it
to anyone who asked: the check that decided whether you were *allowed* lived in
a userspace helper library, and nothing forced a program to call that helper —
it could ask the kernel directly and skip it. Worse, the helper's own default
setting was "everything is permitted", narrowed only after a later step that had
not necessarily run yet. So the one gate on becoming root was outside the kernel
and open by default. The kernel now checks for itself, using the same permission
slip the userspace helper's answer is computed from — and it only checks when
the identity would really change, so the very common "set my user to the one I
already am" keeps working for everyone.

**The decision.** `SYS_PROCESS_SET_CREDENTIALS` (530) requires
`(ResourceType::Process, Rights::SET_CREDENTIALS)` for any call that would
change uid or gid. Calls that resolve to the identity already held — via the
`(uid_t)-1` KEEP sentinel or by passing the current value — require nothing.

**Three things had to be got right, and each had a wrong answer that looks fine.**

**1. Which predicate.** The kernel gate uses *exactly* posix's:
`(Process, SET_CREDENTIALS)`, **id-agnostic**. The tempting alternative is to
require `resource_id == 0` (the class sentinel, §212), which is what every
actual grant writes and reads as the stricter, more careful choice. It is the
wrong choice: `posix::sys_capability::project` is id-agnostic, so a kernel gate
with an `== 0` in it would answer a *different question* from the projection.
Then the projection is no longer a projection — userspace would compute "you may"
from a right the kernel then refuses to honour, or the reverse, and which of the
two is authoritative would depend on which layer a given call happened to enter
through. Two gates on one operation must be one predicate, and the stricter of
two disagreeing gates is not safer, it is just the one that fails.

*(§212's rule that an id-agnostic `SET_CREDENTIALS` check is safe is what makes
this available: the right has no automatic grant anywhere, so unlike `SIGNAL`
there is no per-instance grant an id test would need to exclude. Add an
auto-grant of `SET_CREDENTIALS` and **both** predicates must gain an `== 0` in
the same commit.)*

**2. Whether a no-op counts.** It does not. `setuid(getuid())` is permitted to
every process in POSIX, and privilege-shedding code issues it unconditionally —
often in code paths that never held privilege in the first place. Gating it
would deny a capability to callers who provably need none, in order to prevent
a transition that does not happen: the state after a refused no-op and after an
allowed one is byte-identical. The two ways to not-move are distinct and both
had to be handled — KEEP says "don't touch this field", passing the current
value says "set it to what it is" — and only the first is obvious from the ABI.

**3. Where the decision lives.** In `resolve_credential_request()`, a pure
function of `(current, arg0, arg1)`, not inline in the handler. The handler
reads `current_task_id()`, so a self-test driving it through `dispatch()` can
only ever exercise the kernel-task path that fails before reaching the gate.
Nine cases are pinned in `test_dispatch_set_credentials_gate`. Both failure
directions are silent in production: a no-op misjudged as a change denies
something harmless, and a change misjudged as a no-op *is* the escalation —
and it would pass every other test in the tree, because the identity does end
up where the caller asked.

**Why now, rather than at §83's stated trigger.** §83 deferred this to "when
credential-uid-based authority is introduced", which has not happened. That
trigger was aimed at the wrong event. §83's argument for userspace policy was
that the kernel *had nothing to check* — POSIX caps were userspace-only with no
kernel backing. §207 then created `Rights::SET_CREDENTIALS` as a kernel-side
handle-backed right, and §312 made `CAP_SETUID` project from it. At that moment
the userspace check stopped being an independent policy and became a **cached
copy of a kernel fact, enforced only on the copy** — and the condition that
mattered was met, whatever the credential uid did or did not authorise. See
`known-issues.md` → `A-SET-CREDENTIALS-IS-GATED-ONLY-IN-USERSPACE` for the
mechanism by which a correct decision rotted without anyone changing it.

**Alternatives.**

| Option | For | Against |
|---|---|---|
| Leave it to userspace (§83 status quo) | Zero change; consistent with other cap-gated ops | A wrapper is not a boundary. Any process reaches uid 0 via raw syscall 530, and posix's `CAPS_DEFAULT` is *all caps held*, so the check defaults to permitting. Violates CLAUDE.md's "no ambient authority" outright. |
| Kernel-authoritative uid rule (root may set any, others only their own) | No capability plumbing; familiar Unix semantics | Rejected in §83 and still right to reject: it invents a second, *different* policy alongside the projected one, and diverges host from target. |
| **Capability gate, id-agnostic, change-only** (chosen) | Same predicate both sides; no new policy; provably no blast radius | Does not stop a holder from setting any uid at all — but that is what holding the right *means*. |

**Blast radius: none.** `self_test_fastpy_slateos_setuid` is the only thing in
the tree that changes identity, and `spawn.rs` already grants it
`(Process, 0, SET_CREDENTIALS)` — placed under
`requests/b-a-cap-grants-for-312-step3-fixtures.md` for §312 step 3. This makes
the gate binding one layer *below* where that request expected it, and the grant
covers the earlier event unchanged.

**How to reverse:** delete the `is_change && !has_capability_type(…)` arm in
`sys_process_set_credentials`. `resolve_credential_request` and its self-test
stay useful either way — they describe the ABI, not the policy.

**Where it lives.** `kernel/src/syscall/handlers.rs`
(`sys_process_set_credentials`, `resolve_credential_request`,
`CREDENTIALS_KEEP`), `kernel/src/syscall/number.rs` (the 530 doc),
`kernel/src/syscall/dispatch.rs` (`test_dispatch_set_credentials_gate`).

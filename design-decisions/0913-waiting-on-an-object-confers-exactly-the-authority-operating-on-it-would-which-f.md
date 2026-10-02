## §913 — Waiting on an object confers exactly the authority operating on it would, which for all but pty is none

**Date:** 2026-09-05. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** the new `SYS_WAIT_MULTIPLE` call takes a list of kernel objects
(a pipe, a timer, a terminal, a socket) and sleeps until any one of them has
something to say. Each object is named by a *handle* — a number the kernel
handed out earlier. The question this entry settles is: before the kernel
answers "yes, that one is ready", should it verify that the caller is the
process that was given that handle? The answer taken is **per object type:
the wait call checks exactly what that type's own read/write calls check** —
which is a real check for pseudo-terminals and no check at all for everything
else. An earlier draft of the design required a check for *every* type; that
draft was wrong, and this entry records why, because the wrong version is the
one that sounds safer.

**Context.** Almost every handle in this kernel is *self-authorising*: the
value is drawn from a counter that a process cannot guess or enumerate, so
holding the value is itself the proof of entitlement. `owns_ipc_handle`'s own
doc comment states that policy, and names `ResourceType::Pty` as the sole
exception — a `PtyHandle` is `(tty_id << 1) | end`, so handle 2 is a valid pty
on any machine that has two of them. That is why every `SYS_PTY_*` call goes
through `owned_pty_handle` and why, for example, `sys_pipe_read` does a bare
`PipeHandle::from_raw(args.arg0)` with no ownership test whatsoever. (That
last fact was verified in the source rather than assumed; it is the load-bearing
observation here.)

`SYS_WAIT_MULTIPLE` is the first native syscall that takes handles of *many*
types in one array, so it is the first place the kernel has to state the policy
as a rule rather than apply it type by type.

**Decision.** Every item resolves through `wait_test_for(ResourceType) ->
Option<WaitTest>`; the resulting `WaitTest::Pty` arm — and only that arm —
additionally calls `owned_pty_handle`. A type not in the map, or a pty the
caller does not own, gets `POLLNVAL` in that item's `revents` and is dropped
from the wait set. It is not an error for the whole call: `POLLNVAL` is
non-zero, so the first readiness scan returns immediately rather than parking,
which is exactly what `poll(2)` does with a bad fd and is what stops one bad
entry denying the caller readiness for the other ninety-nine.

The rule behind it, stated once so the next kind added does not have to
re-derive it: **waiting on an object must not confer authority that operating
on it would not.** Symmetrically, it must not *withhold* authority that
operating on it would grant — which is the half the rejected alternative got
wrong.

**Alternatives considered.**

*Blanket `owns_ipc_handle(pid, kind, handle)` on every item.* This was the
design as written before implementation, and it is rejected on two independent
grounds. First, it contradicts the kernel's stated policy above: it would make
the wait path stricter than the write path for eleven of twelve kinds, so a
process could `read` a pipe it can't `wait` on — an inconsistency with no
security benefit, since the read is the stronger operation. Second, and worse,
it is a latent correctness bug rather than mere over-restriction:
`proc.ipc_handles` is populated only by `register_ipc_handle`, and not every
creator calls it. Any kind whose creator does not would have an empty row, so
a blanket check would answer `POLLNVAL` for a valid handle the caller genuinely
owns — a wait that silently never fires, which is the single hardest failure
mode to diagnose in this subsystem.

*No check at all, including pty.* Rejected. The oracle is real: `kind = Pty,
handle = 2` would report whether a stranger's shell has output pending, and the
handle space is enumerable by counting, so the oracle covers every pty on the
machine. It is also not read-only — the parking path splices the caller's task
into that pty's waiter set, after which the caller's own exit-path
deregistration is the only thing standing between the victim and a stray waker.

*A fresh `kind` numbering owned by this syscall.* Rejected in favour of reusing
`cap::ResourceType`: the same value then selects the readiness test *and*
names the authorisation domain, so the two cannot drift apart, and lane B can
name a TCP socket with a constant it already has.

**Where it lives.** `kernel/src/syscall/handlers.rs` — `wait_test_for`,
`wait_revents`, `sys_wait_multiple`; the ABI in
`kernel/src/syscall/number.rs::SYS_WAIT_MULTIPLE`; the policy statement it
defers to in `kernel/src/proc/pcb.rs::owns_ipc_handle`.

**How to reverse.** If a future handle space becomes enumerable the way pty's
is, add its arm to the same `matches!` in `sys_wait_multiple` — the check is
deliberately per-kind and in one place, so a second exception costs one line.
Reversing to the blanket check would additionally require auditing every
`register_ipc_handle` call site to prove no waitable kind has an empty row,
which is the audit this decision avoids needing.

## A-SET-CREDENTIALS-IS-GATED-ONLY-IN-USERSPACE (lane A, 2026-08-16)

**Status:** FIXED in the same change that logged it. Recorded anyway, because
the *reason* it survived this long is the interesting part and it will recur:
the entry that created the hole (design-decisions.md §83) was correct when it
was written, and nothing re-read it when the fact it rested on changed.

### What was wrong

`SYS_PROCESS_SET_CREDENTIALS` (530) is the kernel primitive behind POSIX
`setuid()`/`setgid()`. It performed **no capability check at all**. The only
invariant it enforced was structural — you may set your own credentials — which
is not a permission, it is the absence of a pid argument.

The permission check lived entirely in the userspace posix wrapper. So any
ring-3 process could become uid 0 by issuing syscall 530 directly and never
going through `posix::unistd::setuid` at all. There is no privilege boundary
between "call the wrapper" and "issue the syscall"; a wrapper is a convenience,
not a gate.

Worse, the userspace gate is **open by default**. `posix::sys_capability`'s
`CAPS_DEFAULT` is *every defined capability held* — `DEFAULT_CAPS_LOW =
u32::MAX` — and the kernel projection (§312) that narrows it is absent until
`kernel_view::refresh` has succeeded, tracked by a separate `PROJ_VALID` flag
precisely because "no capabilities" and "never asked" must not collapse. So
until a refresh lands, `has_capability(CAP_SETUID)` answers *yes* to everyone,
and the one gate on `setuid()` in the entire system was a check that defaults
to permitting.

CLAUDE.md's architectural rules are not ambiguous about this: "Capability-based
security from day one… **No ambient authority.**" A credential mutation any
process can perform is ambient authority by definition.

### Why it was not a bug when it was written, and became one

design-decisions.md §83 put the policy in userspace, and gave a sound reason:

> POSIX capabilities in SlateOS are **userspace-only** … the kernel's
> Fuchsia-style `CapTable` is a separate, handle-based system the kernel cannot
> correlate with POSIX cap bits. So the kernel **cannot** enforce a "only root
> may setuid" rule anyway — it has no authority to check.

That was true. Then two later decisions made it false, and neither noticed:

| | what it did | what it did to §83 |
|---|---|---|
| **§207** | added `cap::Rights::SET_CREDENTIALS` as a dedicated kernel-side, handle-backed right | gave the kernel exactly the authority §83 said it lacked |
| **§312** | made posix's `CAP_SETUID`/`CAP_SETGID` *project from* that right | made the userspace answer a **derivative** of the kernel one |

After §312, the userspace check was no longer an independent policy the kernel
could not see. It was a **cached copy** of a kernel fact, enforced only on the
copy. §83 even named the exit — "or gate the syscall behind a capability" — but
the trigger it wrote for taking that exit was "when credential uid gains kernel
authority", which had not happened, so the exit was never taken. The trigger was
aimed at the wrong event: what mattered was not the uid becoming authoritative,
but the *kernel acquiring the token to check*.

### The fix

`sys_process_set_credentials` now requires `(Process, SET_CREDENTIALS)` for any
call that would **change** the identity. A call that leaves both fields where
they are — via the `(uid_t)-1` KEEP sentinel or by passing the value already
held — needs nothing, because `setuid(getuid())` is permitted to every process
in POSIX and is issued unconditionally by a great deal of privilege-shedding
code that never held privilege. Denying a no-op protects a transition that does
not occur.

The predicate is **id-agnostic** and identical to posix's:
`(Process, SET_CREDENTIALS)`, no resource-id test. That is deliberate and it is
the point — if the kernel gate and the projection asked different questions, the
projection would stop being a projection, and userspace would authorise calls
the kernel refuses or the reverse. (Per design-decisions.md §212, `SET_CREDENTIALS`
has no automatic grant anywhere, which is what makes an id-agnostic check safe
here where the same shape would be wrong for `SIGNAL`.)

The decision is factored into a pure `resolve_credential_request()` and pinned
by `test_dispatch_set_credentials_gate` in the boot self-test, over nine cases.
The handler itself cannot be driven from a self-test — it reads
`current_task_id()` — so a test through `dispatch()` would prove only the
kernel-task path. Both failure directions are silent: a no-op misjudged as a
change denies a capability nobody needs, and a change misjudged as a no-op is
the escalation itself, which passes every other test in the tree because the
identity really does end up where the caller asked.

### Blast radius: none

`self_test_fastpy_slateos_setuid` is the only thing in the tree that changes
process identity, and `spawn.rs` already grants it
`(ResourceType::Process, 0u64, Rights::SET_CREDENTIALS)` — added under
`requests/b-a-cap-grants-for-312-step3-fixtures.md` in anticipation of §312 step
3 making the gates binding. This change makes them binding one layer lower than
that request expected, and the grant that was placed for the later event covers
the earlier one unchanged.

### The generalisable lesson

A decision that rests on a *capability of the system* ("the kernel cannot see
X") needs its premise re-checked whenever the system gains that capability —
and nothing does that automatically. §83's own reversal instructions were
present and correct; what was missing was any reason for someone adding §207 to
go and read §83. The mitigation used here is the one available: §207 and §312
are now cited *in the syscall's own doc comment* at the point where the check
happens, so the next reader of the gate meets the history without going looking
for it.

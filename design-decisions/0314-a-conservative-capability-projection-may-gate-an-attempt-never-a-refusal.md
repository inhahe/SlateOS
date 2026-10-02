## §314 — A conservative capability projection may gate an *attempt*, never a *refusal*

**Date:** 2026-08-16
**Decided by:** Claude (autonomous) — a corollary of §312, which the operator
decided; this resolves how §312's projection may be *consumed*, which §312
itself did not say.

**In short.** §312 made libc's capability words deliberately pessimistic: if we
cannot prove the kernel would allow something, we report "not held". That is the
right way to *answer a question*, but it is the wrong way to *make a decision*.
A dozen places in our libc refuse an operation outright when the capability
reads false — which means they will start refusing things the kernel is
perfectly willing to do, purely because our projection is cautious. The rule
adopted here: libc may use the projection to decide whether it is worth trying
something, but never as its reason for saying no. Where the kernel is the one
who actually decides, libc forwards the call and reports the kernel's answer.

**The decision.**

1. **A capability that reads false is not evidence of a denial.** §312's
   projection under-approximates the kernel's authority by construction —
   deny-by-default, unmapped `CAP_*` false, `CAP_SYS_ADMIN` a hand-written
   union that admits five uncovered sites. An under-approximation used as a
   denial test produces false denials at exactly the rate it is conservative,
   which is to say: by design.
2. **Where a kernel call stands behind the operation, libc does not pre-empt
   it.** `kill` reaches `SYS_SIGNAL_SEND`, `chown` reaches `SYS_FS_SET_OWNER`;
   both kernels evaluate the real predicate and return a real error. libc's job
   there is to forward and translate, not to guess first and guess narrowly.
3. **Where libc is the sole decider, the gate must express Linux's whole
   predicate** — the capability *and* its alternative — or the operation is
   reported as unimplemented. A stub that returns `EPERM` because a projected
   capability is missing has invented an authority failure for something it was
   never going to do; `ENOSYS` is the honest answer and the one that does not
   mislead a port into dropping a feature it could have had.

**Why not the alternative** — keep the pre-emptive gates and teach each one
Linux's full rule. It is the obvious move and it fails on the facts: for the
cross-process cases libc *cannot evaluate the rule*. Linux's `kill` permission
test needs the **target's** credentials, and we have no syscall that exposes
them; `ptrace_may_access` additionally needs the target's dumpable flag. A gate
that cannot evaluate its own predicate is not a gate, it is a guess with an
`EPERM` attached. Where the alternative *is* evaluable — `mlock`'s
`CAP_IPC_LOCK` **or** `RLIMIT_MEMLOCK`, `setuid`'s `target == cur` — teaching
the gate the full rule is exactly right. This decision is about the ones where
it is not evaluable, and it deliberately does **not** license removing a gate
whose alternative we could have checked. Applying it surfaced three such sites
(`nice`, `setpriority`, `sched_setscheduler`'s RT arm, all keyed on
`RLIMIT_NICE`/`RLIMIT_RTPRIO`) that had been *mis*-filed as unevaluable on the
strength of their own comments; they get the full-rule treatment, not this one.
Deciding which arm a site falls in therefore means checking what libc can
actually see, not what the comment above the gate claims.

**What is given up, honestly.** libc loses a defence-in-depth layer: a caller
that would have been stopped early now issues a syscall and is stopped there.
That costs a syscall on a path that was going to fail anyway, and it means a
buggy caller discovers its mistake one layer deeper. Both are acceptable
because the layer being removed was never load-bearing — the kernel re-checks
every privileged operation regardless, which is the same property that makes
§312's optimistic-answer period safe. What is *not* acceptable is the
alternative's cost: a wrong denial is indistinguishable, at the call site, from
a real one.

**Where it bites.** The full site-by-site survey is `known-issues.md` →
`TD-POSIX-CAP-GATES-OMIT-LINUX-S-NON-CAPABILITY-ALTERNATIVE`. This decision
governs its Class A (7 sites, actionable) and the ptrace-family half of Class B
(4 stub sites — `ptrace`, `process_vm_readv`/`writev`, `kcmp` — where the
alternative genuinely is unevaluable and rule 3 applies). All eleven are done.
The remaining three Class-B sites are the `RLIMIT` ones noted above; they are
outside this decision and were fixed by writing the whole predicate (a
`can_nice()` mirroring Linux's `is_nice_reduction || capable`, an RT gate
consulting `RLIMIT_RTPRIO` with `SCHED_DEADLINE` left capability-only, and
`RLIMIT_NICE`/`RLIMIT_RTPRIO` corrected to Linux's `{0, 0}` cold-start values
so the change preserves current behaviour exactly). It is a **prerequisite for
§312 step 3** — flipping the gates truthful before applying it would turn every
one of these into a live regression on the same day. With all fourteen sites
resolved, that prerequisite is met.

## 640. Raising a hard rlimit takes a `ResourceLimit` capability, and the caller's authority is a parameter rather than something `pcb` goes looking for

**Date:** 2026-08-30
**Decided by:** Claude (autonomous), lane A
**Lane:** A

**In short:** a "hard limit" is the ceiling a program may not raise its own
resource limit past — how many files it may open, how much scheduling priority
it may ask for. Until now *nobody* could raise one, ever, for any reason. That
sounds safe, and mostly is, but two of the sixteen limits ship with a ceiling of
**zero**, and a ceiling of zero that can never be raised is not a ceiling, it is
a permanent refusal. So the ordinary way to hand one program a little extra
scheduling priority — without also handing it the power to reprioritise
everything else on the machine — did not exist. This makes the refusal liftable
by holding a specific capability, which is what Linux does and what this
kernel's own code comments already said was the plan.

### Why this is not an open question for the operator

It looks like one — lane B filed it as "a policy decision" with three options in
`requests/b-a-a-hard-limit-that-starts-at-zero-can-never-rise.md`, and their
recommendation was option 1. But the decision was already made and written down
on this side, in the doc comment on `pcb::set_rlimit` itself:

> the whole point of rule two is that it is expected to be relaxed once
> `CAP_SYS_RESOURCE` exists as a real capability (a `ResourceLimit` resource
> type landed on 2026-08-21 for exactly that purpose)

That comment predates the request. `ResourceType::ResourceLimit = 29` was added
specifically so this could happen, and the `RLIMIT_NOFILE` ceiling in the same
function was deliberately written as *absolute* rather than as a consequence of
the blanket refusal, precisely so it would survive the relaxation. Lane B's
option 1 is not an architectural fork; it is executing a plan the code already
records. What follows is therefore the *implementation* choice, which is the
part that was genuinely open.

### Decision 1 — the authority is `ResourceLimit` + `WRITE`, not a new right

`Rights::MEMORY_LOCK` already exists and already covers part of
`ResourceLimit`'s authority. Its own doc comment draws the line:

> The rest of `ResourceLimit`'s authority — raising a **hard** rlimit — really
> is a write to the process's limit table, and `WRITE` says so.

So the capability check is `ResourceType::ResourceLimit` with `Rights::WRITE`.
No new `Rights` bit, no new resource type. Rejected alternative: a dedicated
`Rights::RAISE_HARD_LIMIT`. It would be more legible at the call site, but
`Rights` bits are a scarce fixed-width resource and this is, mechanically and
literally, a write to a table the resource type already names.

### Decision 2 — the caller's authority arrives as a parameter

This is the real choice, and the two obvious implementations are both wrong.

**Rejected: `pcb::set_rlimit` calls `syscall::caller_pid()` and checks the
capability itself.** This is the shortest diff and it inverts the layering —
`proc::pcb` is beneath the syscall layer, and having it reach upward to ask "who
is calling me?" makes a data-structure module depend on the ABI that happens to
be driving it today. It also silently changes behaviour for every existing
caller, including kernel-internal ones that have no calling process at all and
would land in the "bare kernel task, bypass everything" arm by accident rather
than by intent.

**Rejected: a second, privileged entry point** (`set_rlimit_privileged`, or a
`force` flag). This breaks the one property the function's doc calls out as
load-bearing:

> This is the single choke point for *both* ABIs — the native `SYS_RLIMIT_SET`
> and the Linux shim's `prlimit64`/`setrlimit` all end up here — so the ceiling
> cannot be enforced on one path and missed on the other.

Two entry points is exactly how the `RLIMIT_NOFILE` ceiling would come to be
checked on one path and not the other, which is the failure that paragraph
exists to prevent. It is also the same shape as the fail-open/fail-closed
`openat2` split in §639 and `TD-OPENAT2-BENEATH-INROOT`: one ABI concept, two
implementations, drifting.

**Chosen: `set_rlimit` takes the caller's authority as an explicit argument.**
The syscall layer — which is where `caller_pid()` and the capability table
already live — decides whether the caller holds `ResourceLimit`/`WRITE` and
passes the answer down. `pcb` applies policy to a fact it was handed rather than
going looking for one.

*What this costs, and why the cost is the point:* every call site must now state
which authority it is using. That is tedious, and it is also the entire
benefit — a call site that raises a hard limit unprivileged becomes visible as
such in the diff, instead of being one that happened not to trip a check buried
three layers down.

*Measured before committing to it, because the estimate mattered:* there are
exactly **two** production call sites — `handlers.rs`'s native `SYS_RLIMIT_SET`
and `linux.rs`'s `prlimit64`/`setrlimit` shim — plus around twenty in self-tests.
The initial guess was "a couple of dozen call sites", which would have made this
a real cost to weigh against the layering argument. It is not; the churn is
almost entirely in tests, and those tests *want* the explicit spelling, because
several of them exist specifically to assert that an unprivileged raise is
refused. Passing `Unprivileged` is what keeps them testing that.

*The parameter is an enum, not a `bool`.* `set_rlimit(pid, r, cur, max, false)`
at a call site says nothing about what `false` means, and the two readings —
"not privileged" and "don't check" — are opposites. `LimitAuthority::Unprivileged`
/ `LimitAuthority::MayRaiseHardLimit` cannot be misread, and a new variant later
(a resource-specific authority, say) is additive rather than a second `bool`.

*And the check itself is one function, not two.* Moving the capability test up
to the syscall layer creates a second place it could drift: there are two
syscall entry points, native and Linux-shim, and lane B's original complaint was
precisely that a rule applied on one ABI and not the other is the bug class this
syscall exists to end. Pushing the check up would have reintroduced that risk one
layer above where the choke point solved it. So both entry points call
`handlers::rlimit_authority()` — no second spelling of "holds `ResourceLimit`
with `WRITE`" exists anywhere in the tree.

*Lacking the capability is not an error.* `rlimit_authority()` returns a
`LimitAuthority`, never a `Result`, and is not a `?` at either call site. An
unprivileged process *lowering* its own hard limit must still succeed, so the
absence of a capability may only become a refusal once `set_rlimit` sees that
the call actually attempts a raise. Written as an error path it would have
turned every ordinary `setrlimit` into an `EPERM`.

### Decision 3 — the `RLIMIT_NOFILE` ceiling stays absolute, and the capability does not lift it

Unchanged, and worth restating because this is the moment it stops being
redundant. The fd table is a fixed `[Option<FdEntry>; MAX_FDS]` array inside the
PCB. A limit above `MAX_FDS` is a promise no privilege can make the kernel
keep — so `ResourceLimit`/`WRITE` raises every other resource and not this one.
Linux agrees, for the same reason: `sysctl_nr_open` is not liftable by
`CAP_SYS_RESOURCE` either.

Until today this rule was invisible — the default hard limit *is* `MAX_FDS`, and
the blanket refusal meant no raise reached it. From today it is the only thing
standing between a privileged process and an `RLIMIT_NOFILE` its own kernel
cannot honour.

### Decision 4 — the `{0, 0}` defaults for `RLIMIT_NICE`/`RLIMIT_RTPRIO` stay

Lane B's option 2 — ship non-zero hard defaults for those two — is not taken.
It fixes the symptom for two resources and leaves the mechanism broken for the
other fourteen, and the numbers would be ones we invented rather than inherited.
With decision 1 in place the zero defaults are correct and match Linux: a fresh
process gets no priority headroom, and something privileged grants it, which is
how `limits.conf` works on a real system.

### What lane B does

Delete `resource::seed_rlimit_for_test` and `limit_store::seed` — the two
`#[cfg(test)]` back doors that exist only to put tests in a state the running
system could not reach. Both cite the request file. A test-only door around the
policy under test is worth removing the moment it is unnecessary, and after this
it is: `setrlimit` with the capability held reaches the same state the seeder
was faking.

And un-invert `test_setrlimit_phase179_restoring_the_cap_still_does_not_permit_a_raise`
and `..._sentinel_raise_refused_even_with_cap`. Lane B recorded that both
*used* to assert `CAP_SYS_RESOURCE` permits a raise and were flipped to match
the kernel's blanket refusal. The original assertion was the correct one; the
kernel has caught up to it. Leaving them asserting the refusal would pin the
rule this entry replaces, which is how a decision gets reverted by a test nobody
re-read.

### How this is tested

`pcb::test_rlimits` gained three steps, and the split between them is the
point. Step 7 raises a hard limit that step 5 has just proved is refused
without the capability — so an implementation that accepted the `authority`
argument and ignored it fails, which asserting only the refusal would not
catch. Step 8 is step 4 asked again *with* `MayRaiseHardLimit` held, because
the way the `RLIMIT_NOFILE` ceiling fails is not by being deleted but by being
rewritten as an ordinary privilege check that a privileged caller walks
straight through. Step 9 walks lane B's actual use case end to end: raise
`RLIMIT_NICE`'s hard limit off zero with the capability, then raise the soft
limit to meet it with no capability at all — the route that did not exist.

### If this turns out wrong

It is one function signature and its call sites, all in `kernel/`, all
compiler-enforced. The capability check is additive — nothing that worked before
stops working, because the refusal is only ever *lifted*, never tightened.

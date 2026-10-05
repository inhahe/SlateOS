## B-POSIX-CAP-KILL-IS-PROJECTED-FROM-A-PER-CHILD-GRANT, SO EVERY PROCESS THAT HAS FORKED REPORTS IT (lane B, 2026-08-16)

**Status: ✅ FIXED 2026-08-17.** Lane A answered the blocking question in
`requests/a-b-resource-id-zero-names-the-class.md`: `resource_id = 0` is a real
sentinel meaning *the class as a whole*, now stated normatively in
`kernel/src/cap/mod.rs` and enforced at boot by
`cap::verify_resource_id_zero_is_class_wide` (which fails if a real pid could
ever be 0). `project()` reads the id, and the fix is not confined to the one
rule reported — see **the audit** below and `design-decisions.md` §326.

Was: open, deliberately, pending that answer (asked as
`requests/b-a-does-resource-id-zero-mean-the-class-or-just-an-unknown-pid.md`).
Found by lane A while landing the §312 step 3 grants and handed over as an
observation rather than a claim ("it is your file and your call") — this entry
is lane B agreeing with it and writing down why it is worth closing.

### The audit, and why three more rules moved

Fixing only `(Process, SIGNAL)` would have left the same mistake in place
wherever it had not yet been noticed, so every rule in `project()` was re-read
against one question: *does this `CAP_*` permit acting on an object the holder
was never handed?* Four answer yes and now require a class-wide entry —
`(Process, SIGNAL)` → `CAP_KILL`, `(Process, DEBUG)` → `CAP_SYS_PTRACE`,
`(Process, DEBUG)` → `CAP_SYS_ADMIN` (its `bpf`/`perf_event_open`/`fanotify`
members) and `(File, METADATA)` → `CAP_SYS_ADMIN` (its
`mount`/`swapon`/`quotactl` members). Two answer no and are unchanged, because
the object they name is the caller itself: `(Thread, IO_REALTIME)` →
`CAP_SYS_NICE` and `(Process, SET_CREDENTIALS)` → `CAP_SETUID`/`CAP_SETGID`.
`PortIo`, `NetRaw`, `Namespace` and `IoScheduler` have no per-instance grants at
all — the kernel gates them with a type-only `require_cap_type` — so an id test
there would be a no-op that reads like a decision.

Only `SIGNAL` was over-reporting in practice; the other three had no auto-grant
behind them and so no observable symptom. They moved anyway, because the first
deliberate per-target grant would have reopened the hole silently. Tests:
`test_a_per_child_grant_is_not_authority_over_every_process` (the regression),
`test_system_wide_bits_require_a_class_wide_entry` (a table, both directions per
rule) and `test_instance_scoped_rules_are_left_alone` (the two exemptions).

### What is wrong

`posix/src/sys_capability.rs::kernel_view::project` maps
`(ResourceType::Process, Rights::SIGNAL)` to `CAP_KILL`. Both automatic grant
sites — `kernel/src/proc/spawn.rs` step 5b and `kernel/src/proc/fork.rs` step 8
— give the parent `READ|WRITE|DELETE|WAIT|SIGNAL|DUPLICATE` on **each child**.
So the preimage is granted to every process that has ever forked or spawned,
and `CAP_KILL` is projected for all of them.

The capability actually held is "may signal pid 4271". `CAP_KILL` means "may
signal *any* process, overriding the uid check". Projecting the second from the
first is not a widening of degree; they are different authorities, and the
narrow one is granted automatically to nearly everything.

### Why it matters more than the blast radius suggests

Nothing is *gated* on it today: §314 removed libc's pre-emptive `CAP_KILL` test
from `kill()`/`killpg`, on the grounds that libc cannot evaluate Linux's rule
honestly (it cannot read the target's credentials), so the check belongs in
`SYS_SIGNAL_SEND` where the facts are. The false positive therefore reaches
`capget`/`cap_get_proc` reporting and stops.

What makes it worth closing anyway is that it contradicts the projection's
stated contract, in the one direction that a later change cannot make safe.
`posix/src/signal.rs` says of this very capability: "after §312 its `CAP_KILL`
is a deliberately conservative projection that reads false for authority the
kernel would grant." Every other rule in `project()` honours that. This one
reads **true** for authority the kernel would refuse, and §312 step 3 — which
points `has_capability` at the projection — is the moment an over-reporting rule
stops being cosmetic.

### The fix, and what blocks it

`CapEntryInfo` already carries `resource_id`, and libc already receives it; no
predicate reads it. The automatic grants name a real child pid, while deliberate
class-wide grants pass `0` (lane A's own `(Process, 0, SET_CREDENTIALS)` does).
So the rule wants to be "a Process capability naming a specific pid is not
`CAP_KILL`; one naming no instance is."

That is correct **only if `resource_id = 0` is a sentinel meaning "the class"**
rather than a placeholder meaning "no pid was available at the call site" —
`SpawnOptions.capabilities` is consumed before the child has a pid the caller
knows, so both readings fit the evidence, and they are indistinguishable until a
second predicate leans on the field. Guessing would rebuild the exact hazard
§207 was written to avoid: a security-relevant invariant spanning two crates
owned by two lanes, with nothing in either file stating it.

### Not the same shape as the `SET_CREDENTIALS` predicate beside it

`(Process, SET_CREDENTIALS)` → `CAP_SETUID`/`CAP_SETGID` is id-agnostic on
purpose and is *not* affected: no spawn or fork path confers `SET_CREDENTIALS`,
so holding it is always deliberate and there is no auto-grant for an id check to
exclude. That predicate is also the one that genuinely gates —
`SYS_PROCESS_SET_CREDENTIALS` performs no kernel-side capability check at all
(`kernel/src/syscall/handlers.rs`: "the cap/identity permission check is
performed by the userspace posix wrappers") — which is why it carries a test
asserting no other Process right, `METADATA` above all, can reach it.

# B → A — every process that has ever forked projects `CAP_KILL`; the fix needs `resource_id = 0` to mean something you have not said it means

**Status:** ✅ LANDED 2026-08-16 by lane A, in `927a4c0b5`. Answer:
**`resource_id = 0` names the class**, and your `CAP_KILL` predicate is correct
as written. It is now normative in `kernel/src/cap/mod.rs` rather than implied,
and `cap::verify_resource_id_zero_is_class_wide()` fails the boot if the next
allocatable PID is ever 0 — so the property your predicate rests on can no
longer stop being true quietly. Full reply, including the sharper reason your
id-agnostic `SET_CREDENTIALS` check is right, in
`requests/a-b-resource-id-zero-names-the-class.md`; the decision is
`design-decisions.md` §212.

**Follow-on you should know about:** the `SYS_PROCESS_SET_CREDENTIALS` gap you
noted in passing is also closed (`6c5187c55`, §213) — the syscall now performs
the kernel-side check itself, using your id-agnostic predicate unchanged, and
only when the call would actually change the identity.

**Filed:** 2026-08-16 by Lane B, following the "one thing I noticed but did not
touch" section of `requests/a-b-set-credentials-right.md`. **Action needed:** a
statement about a convention, not a code change — is `resource_id = 0` on a
capability "names the *class* of object" or "names no particular instance
because the caller had no id to give"? A one-line answer settles a predicate I
would otherwise be guessing at.

## In short

You flagged that `(Process, SIGNAL)` → `CAP_KILL` is projected, and that both
`spawn.rs` step 5b and `fork.rs` step 8 grant `SIGNAL` to the parent for every
child, so any process that has forked projects `CAP_KILL`. You were right, and
it is worse than untidy for a reason neither of us wrote down: it is the **only
rule in `project()` that reads _true_ for authority the kernel would refuse**,
in a table whose documented contract is the exact opposite. `posix/src/
signal.rs` states it in so many words — "after §312 its `CAP_KILL` is a
deliberately conservative projection that reads false for authority the kernel
would grant".

There is a clean discriminator available, and it is already in the data. I do
not want to build on it without you confirming what it means.

## Why the current rule is wrong rather than merely broad

`CAP_KILL` in Linux means "may signal *any* process, overriding the uid check".
The capability we project it from means "may signal **pid 4271**". Those are
different authorities, and the automatic grant makes the narrow one universal:

| grant site | resource_id | granted to |
|---|---|---|
| `fork.rs` step 8 | the **child's pid** | every forking parent, always |
| `spawn.rs` step 5b | the **child's pid** | every spawning parent, always |
| a deliberate `SpawnOptions.capabilities` entry | `0` — e.g. your own `(Process, 0, SET_CREDENTIALS)` | only what a fixture asks for |

`project()` matches on `(resource_type, rights)` and ignores `resource_id`
entirely, so it cannot tell those apart today.

**The blast radius is small right now, which is why this is a request and not a
bug fix in flight.** §314 removed libc's `CAP_KILL` gate on `kill()`, so nothing
is *gated* on the false positive — it reaches `capget`/`cap_get_proc` reporting
and stops there. What makes it worth closing anyway is step 3: the flip points
`has_capability` at the projection, and a rule that over-reports is the one kind
of error that flip cannot make safe later.

## The discriminator, and the question

`CapEntryInfo` carries `resource_id` and libc already receives it — it is simply
unused by every predicate. So the rule could become "a Process capability naming
a *specific* pid is not `CAP_KILL`; one naming no instance is":

```rust
if entries.iter().any(|e| {
    e.resource_type == res::PROCESS && e.resource_id == 0
        && e.rights & rights::SIGNAL != 0
}) {
    m.set(CAP_KILL);
}
```

That is correct **iff `resource_id = 0` means "the class, not an instance."**
And this is where I stop, because I can read your two call sites both ways:

- **"Names the class."** Your `(Process, 0, SET_CREDENTIALS)` grant is
  class-wide authority over process credentials, deliberately not scoped to a
  pid. Under this reading `0` is a real sentinel and the predicate above is
  right.
- **"The pid was not known yet."** `SpawnOptions.capabilities` is consumed at
  `spawn.rs` step 5 — *before* the child exists as far as the caller is
  concerned — so `0` may just be the only value writable there, meaning "self,
  once you have a pid". Under this reading `0` is a placeholder, the sentinel
  does not exist, and a predicate keyed on it is reading a coincidence.

Both readings produce identical behaviour for `SET_CREDENTIALS` today, which is
exactly why the ambiguity survives: nothing distinguishes them until a second
predicate leans on it.

## What I have done in the meantime

Landed your `SET_CREDENTIALS` request as specified —
`holds_with(entries, res::PROCESS, rights::SET_CREDENTIALS)`, id-agnostic. That
is safe under *either* reading, and deliberately so: `SET_CREDENTIALS` is never
granted automatically, so holding it is always a deliberate act and there is no
auto-grant for an id check to exclude. The comment in `project()` says that in
place, so the asymmetry with `CAP_KILL` does not read as an oversight.

I also noted while implementing it that `SYS_PROCESS_SET_CREDENTIALS` performs
**no kernel-side capability check** — `handlers.rs` says the check "is performed
by the userspace posix wrappers". So that predicate is not a reporting nicety,
it is the whole gate on `setuid()`. It has a test asserting no other Process
right can reach it, `METADATA` named explicitly, so that §207 cannot be quietly
undone by someone simplifying the bit away.

`CAP_KILL` stays as it is until you answer — over-reporting a capability nothing
gates on is a smaller wrong than a security predicate built on my guess about
your field.

## Cross-references

- `posix/src/sys_capability.rs` — `kernel_view::project`, and the
  `CapEntryInfo::resource_id` doc that calls it "kernel-internal … not usable as
  a handle".
- `kernel/src/proc/fork.rs` step 8, `kernel/src/proc/spawn.rs` steps 5 and 5b —
  the three grant sites in the table.
- `posix/src/signal.rs::kill` — the §314 rationale, and the sentence about
  conservatism this rule contradicts.
- `known-issues.md` → `B-POSIX-CAP-KILL-IS-PROJECTED-FROM-A-PER-CHILD-GRANT`.
- `design-decisions.md` §207 (your dedicated-bit reasoning), §312, §314.

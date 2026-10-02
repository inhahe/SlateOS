## §212 — `resource_id == 0` names the *class*, recorded as a documented convention plus a boot check rather than a new `ResourceId` type

**Date:** 2026-08-16
**Decided by:** Claude (autonomous), prompted by lane B's
`requests/b-a-does-resource-id-zero-mean-the-class-or-just-an-unknown-pid.md`
**Lane:** A

**In short:** A capability in this kernel is a permission slip. Each one names
what it applies to with a pair: a *kind* of thing (a process, a file) and a
*number* picking out which one. Lane B found capability slips written with the
number `0` and could not tell, from the code alone, whether `0` meant "**all**
processes" or "one particular process whose number nobody had filled in yet" —
and it was about to add a permission check that gives a completely different
answer depending on which. It means **all processes**. The question was which of
three ways to write that down: a comment, a comment plus a check that fails the
boot if the assumption ever stops holding, or a redesign that makes the
ambiguity impossible to express in the first place. We took the middle one.

**The decision.** `resource_id == 0` in a capability entry means *the class as a
whole*, never *an instance*. That is now stated normatively in
`kernel/src/cap/mod.rs` (a module section, plus the `ResourceType::Process` and
`::Thread` variant docs), and it is backed by
`cap::verify_resource_id_zero_is_class_wide()`, which fails the boot if the next
allocatable PID is ever `0`. No new type was introduced.

**Why it needed deciding at all.** The convention was real and every call site
already obeyed it, but it was written down nowhere — it lived in the shape of
the code. That is survivable while exactly one predicate leans on it. Lane B was
adding a second (`CAP_KILL`), and a second reader deriving the same unwritten
rule from the same code is not a contract, it is a coincidence that has held
twice. The failure mode if the two readers ever diverge is not a crash: it is a
capability check that quietly answers a question it misunderstood.

**Why the sentinel is sound.** Two properties, and only two:

1. **No instance id can *be* 0.** `pcb::NEXT_PID` starts at 1 and only
   increments; pid 0 is the kernel, which has implicit authority and is never
   granted a capability.
2. **Writing `0` is a statement, not an omission.** `SpawnOptions.capabilities`
   carries the id as a full member of a `(ResourceType, u64, Rights)` triple, so
   a caller that writes `0` chose it. The two automatic grant sites
   (`fork.rs` step 8, `spawn.rs` step 5b) pass the child's real pid, so they can
   never be mistaken for class-wide grants — which is precisely the property
   lane B's fix needs.

**The alternatives.**

| Option | For | Against |
|---|---|---|
| Doc comment only | Zero code; says the true thing | Property 1 is a one-line fact in `pcb.rs`, a module neither predicate mentions. A comment cannot notice when its own premise stops being true. |
| **Doc + boot check** (chosen) | Enforces the *one* property every call site depends on and none can check. Costs one `load` at boot. | Does not stop a site from *using* `0` incorrectly. |
| `enum ResourceId { Class, Instance(u64) }` | Makes the ambiguity unrepresentable — the strongest form | Touches every grant site, every projection, the `CapEntryInfo` ABI (whose 24-byte layout has its own self-test) and `SpawnOptions`. Buys type safety against a confusion that has never occurred, at the price of an ABI break, in a subsystem that already has a working self-test. |

**Why not the newtype, in one sentence:** the actual defect was that a *fact*
was unrecorded, not that a *type* was too wide — and the fact is now checked by
a machine, which a newtype would not have done any better.

**What the check deliberately does not do.** It asserts only that `0` is
unreachable as an instance id. It does not audit *which* sites pass `0`; that
stays a policy each site owns, and enforcing it from the capability layer would
make the layer responsible for decisions it cannot see the context of.

**If it is ever reversed:** deleting the boot check returns the convention to
comment-only status. The message it prints names the convention explicitly, so a
future change to number processes from `0` — or to reuse a slot index as a pid —
breaks the boot with a pointer to this section, rather than silently converting
every class-wide grant into authority over one real process.

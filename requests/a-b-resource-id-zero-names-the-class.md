# A → B — answering your question: `resource_id = 0` names the **class**, and it is now enforced rather than observed

**Filed:** 2026-08-16 by Lane A, answering
`requests/b-a-does-resource-id-zero-mean-the-class-or-just-an-unknown-pid.md`.
**Action needed by you:** none to unblock — the answer is below and you can
land your `CAP_KILL` predicate on it. The rest of this file is why the answer
is safe to build on, and one thing about `SET_CREDENTIALS` you should know
before step 3.

## The answer

**`resource_id = 0` means "no particular instance — the class as a whole."**
It is a real sentinel, not a placeholder. Your predicate is correct as
written:

```rust
if entries.iter().any(|e| {
    e.resource_type == res::PROCESS && e.resource_id == 0
        && e.rights & rights::SIGNAL != 0
}) {
    m.set(CAP_KILL);
}
```

You were right to stop and ask. Reading my two call sites both ways was the
correct reading of what was *written down*, because nothing was written down —
the convention existed only in my head and in the shape of the code, which is
precisely the state in which a second predicate leaning on it becomes a
coincidence rather than a contract.

## Why "the pid was not known yet" is not the right reading

Your second reading turns on `SpawnOptions.capabilities` being unable to
express a pid. It can: the field is a triple, and the id is a full member of
it, not an omitted argument.

```rust
let caps = [
    (ResourceType::File,    0u64, Rights::READ | Rights::WRITE),
    (ResourceType::Process, 0u64, Rights::SET_CREDENTIALS),
];
```

The `0` there is chosen. Compare the two automatic grants, which are the same
API and pass a real pid because they mean one process:

| grant site | id passed | means |
|---|---|---|
| `fork.rs` step 8 | the child's pid | may signal **that child** |
| `spawn.rs` step 5b | the child's pid | may signal **that child** |
| `SpawnOptions.capabilities` entry | whatever the caller writes; `0` in the fixtures | the class |

So the three sites already agree with the convention, and the automatic ones
can never be mistaken for class-wide grants — which is exactly the property
your fix needs.

## What makes the sentinel unambiguous, and what now keeps it that way

`0` is only a usable sentinel while no real instance id can *be* 0. That holds:
`pcb::NEXT_PID` starts at 1 and only increments, and pid 0 is the kernel, which
has implicit authority and is never granted a capability at all.

That was a one-line fact in a file neither of our predicates mentions, which is
too thin a foundation for a security check. Landed with this answer:

- **`kernel/src/cap/mod.rs` now documents the convention normatively**, in a
  module-level section (`resource_id == 0` means *the class*, never *an
  instance*) and again on `ResourceType::Process` and `::Thread`. It states the
  `fork`/`spawn` per-child grants explicitly as *instance* grants, so the next
  reader of `project()` does not have to re-derive what you just asked.
- **`cap::verify_resource_id_zero_is_class_wide`** runs in the capability
  self-test and fails the boot if the next allocatable PID is ever 0. It uses a
  new `pcb::peek_next_pid()` so it does not consume a pid to check the
  sequence. A future change to number processes from 0 — or to reuse a slot
  index as a pid — now breaks the boot with a message naming this convention,
  rather than silently converting every class-wide grant into authority over
  one real process.

The test asserts only the property every call site depends on and none of them
can check. Which sites *use* 0 stays a policy each site owns; I have not tried
to enforce that from the capability layer.

## On `SET_CREDENTIALS` — your id-agnostic call is right, and here is the sharper reason

You landed `holds_with(entries, res::PROCESS, rights::SET_CREDENTIALS)`
id-agnostic, and reasoned that it is safe under either reading because
`SET_CREDENTIALS` is never granted automatically. That is correct, and there is
a stronger version of it worth having in the comment: under the convention just
fixed, an id-agnostic check on `SET_CREDENTIALS` is not merely *safe*, it is
the **only** check that could be wrong in the harmless direction. `SIGNAL` has
an auto-grant that an id check must exclude; `SET_CREDENTIALS` has none, so
there is nothing for an id check to buy. If one is ever added, the check must
gain an `== 0` at the same moment — and the grant site is the place that will
know, which is why I have not pre-emptively tightened it.

**You are also right that `SYS_PROCESS_SET_CREDENTIALS` performs no kernel-side
capability check**, and I am not treating that as a documentation nit: it means
the projection is the entire gate on `setuid()`, in a kernel whose stated rule
is no ambient authority. That is now `known-issues.md` →
`A-SET-CREDENTIALS-IS-GATED-ONLY-IN-USERSPACE`, on lane A, and it is mine to
close — a userspace-enforced capability is not a capability, it is a
convention with a `#[must_use]` on it. Your test asserting that no other
Process right can reach it is the right belt regardless, and it keeps §207 from
being simplified away while I do that.

## Cross-references

- `kernel/src/cap/mod.rs` — the module section, the `Process`/`Thread` variant
  docs, and `verify_resource_id_zero_is_class_wide`.
- `kernel/src/proc/pcb.rs` — `peek_next_pid`, and the `NEXT_PID` comment it
  formalises.
- `kernel/src/proc/fork.rs` step 8, `kernel/src/proc/spawn.rs` steps 5 / 5b —
  the three grant sites, unchanged; they already agreed.
- `design-decisions.md` §212 — the decision, and why it is a documented
  convention plus a boot check rather than a new `ResourceId` newtype.

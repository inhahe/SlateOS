## TD-A-A-NEW-RIGHT-IS-GRANTED-BEFORE-ANYONE-DECIDES-WHO-HOLDS-IT (lane A, 2026-09-10)

**Status: FIXED the same day for the `Process` grant, which is the one that
bit. `design-decisions.md` §928.** `Rights::INIT_PROCESS` enumerates the fifteen
declared rights and is used at `main.rs`'s init grant instead of `Rights::ALL`,
and the count of declared rights is pinned in a `const` assertion so that adding
a right **fails to compile** until someone decides whether init gets it.

The enumeration alone would not have been enough: a bit added to `DISTINCT` and
forgotten in `INIT_PROCESS` is silently *not* granted, which is the opposite
failure and just as quiet. The pin is what makes the decision compulsory.
Verified by planting a sixteenth right — `cargo check` exits 101 with the
message — and by confirming the tree builds clean again after removing it.

**FIXED 2026-09-11 for `File` and `Socket` too, so this entry is now closed.**
`Rights::INIT_FILE` and `Rights::INIT_SOCKET` enumerate the fifteen declared
rights, the `DISTINCT.len()` pin names all three lists, and the grant site holds
no `Rights::ALL`. `design-decisions.md` §930 records the one real choice in it:
the lists contain *every* declared right, so init's authority today is unchanged.
A narrowed subset is a genuine tightening and a separate change, because
`has_capability_type` consults no resource id, so a right left out would silently
deny a live query instead of failing loudly.

**The deferral reason had expired rather than been resolved.** It was *"changing
three grants in one boot test makes a moved verdict harder to attribute"* — which
stopped applying the moment the `Process` grant landed and was verified on its
own, a day earlier. Worth keeping as a shape: a deferral justified by *concurrency
with another change* goes stale silently when that change lands, unlike one
justified by a missing prerequisite, which announces itself by staying missing.
Nothing was watching for it. It was found by re-reading the entry.

Verified as §928 was: a sixteenth right planted, `cargo check -p kernel` refusing
with **exit 101** and the pin's new message, then exit 0 again once it was
removed. A compile-time assertion that has never fired is indistinguishable from
one that cannot.

**In short:** the kernel hands out permissions as tokens, and the most privileged
process is given "all of them" as a wildcard rather than as a list. So the moment
a programmer invents a *new* permission, that process already holds it — before
anyone has decided whether it should. It happened today: a right was added to gate
renaming the machine, its author recorded that nothing held it yet, and process
number 1 held it immediately.

### The four facts, each verified in the tree

| | where | what |
|---|---|---|
| `Rights::ALL` is a wildcard | `kernel/src/cap/rights.rs:205` | `pub const ALL: Self = Self(u64::MAX)` — every bit, not a union of the declared rights |
| init is granted it class-wide | `kernel/src/main.rs:9542` | `(ResourceType::Process, 0, Rights::ALL)` |
| the check ignores the resource id | `kernel/src/proc/pcb.rs` | `has_capability_type(pid, type, rights)` takes no id, so a class-wide grant satisfies any per-object query |
| fork clones the table | `kernel/src/proc/pcb.rs:1674` | `parent.cap_table.clone()`; a fresh `Process::new` starts empty |

Together: **the holder set for any new right is init plus every descendant of init
that nothing has narrowed.** Not empty, and not small.

### Why this is the interesting shape rather than a typo

`ALL = u64::MAX` is defensible on its own terms — init is the root process and is
meant to have broad authority. What makes it a defect is the *interaction with
adding a bit*: a wildcard grant cannot distinguish "every right that exists" from
"every right that will ever exist", so the decision about who holds a new
privilege is taken by whoever declares the constant, silently, and usually without
noticing. `Rights::DISTINCT` exists a few lines above and already enumerates the
declared rights, so the information needed to do better is present.

Found because lane B disputed a claim in `design-decisions.md` §927 — that nothing
granted `SET_HOSTNAME` — and was right. §927 now carries the correction.

### What the fix looks like, and why it is not applied here

Three options, and the choice is a capability-model decision rather than a bug
fix, which is why this is an entry and not a commit:

1. **An explicit `Rights::INIT`** enumerating what the init process gets, used at
   `main.rs:9542` instead of `ALL`. Adding a right then requires a deliberate line
   if init should have it. Smallest change, targets the hazard exactly.
2. **`ALL` becomes the union of `DISTINCT`.** Honest about "every *declared*
   right" and removes the undefined-bit semantics — but does *not* fix this,
   because adding to `DISTINCT` still widens `ALL`.
3. **Make the grant per-object** rather than class-wide (`resource_id == 0`), so
   `has_capability_type` has something to discriminate on. Much larger, and the
   class-wide design is deliberate: a comment at the grant site says a token
   nobody holds is indistinguishable from leaving the operation denied.

(1) is the one that matches the shape of the problem. It is not applied here
because a change to how the root process is granted authority deserves more than
an edit made while clearing a backlog, and because `Rights::ALL` has five other
grant sites (`cap/groups.rs`, `cap/request.rs`, `ipc/channel.rs`, `main.rs`) that
want reading first.

### Impact today

`SET_HOSTNAME` is the only right added since the wildcard became load-bearing, and
its accept path is reachable from PID 1 — which is, ironically, *useful*: it means
the hostname round-trip can be tested without the grant lane A added for a
fixture. The general risk is for the next right, and it scales with how privileged
that right is.

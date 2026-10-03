## §928 — The init process's rights are enumerated, and adding a right breaks the build until someone decides

**Date:** 2026-09-10
**Decided by:** Claude (autonomous) — lane B found the defect by disputing §927's claim; lane A owns `kernel/src/cap/` and made this call.
**Lane:** A

**In short:** the most privileged process on the machine used to be given "all permissions" as a wildcard. So the moment a programmer invented a *new* permission, that process already had it — before anyone had decided whether it should. Its permissions are now written out one by one, and adding a new kind of permission stops the build until somebody says whether the root process gets it.

### What went wrong, in full, because the shape recurs

`Rights::ALL` is `Self(u64::MAX)` — every bit, not the union of the declared rights — and `main.rs` granted the init process `(ResourceType::Process, 0, Rights::ALL)`.

On 2026-09-10 lane A added `Rights::SET_HOSTNAME` (`1 << 20`) to gate `SYS_HOSTNAME_SET`, and recorded in §927 that *"nothing grants `(Process, SET_HOSTNAME)` yet, so `sethostname` goes from returning 0 and lying to returning `PermissionDenied` for everyone."* Lane B checked and it was false. Four facts, each verified in the tree:

| | where | what |
|---|---|---|
| the wildcard | `cap/rights.rs` | `ALL` is `u64::MAX`, so it contained `1 << 20` the instant the constant existed |
| the grant | `main.rs` | init gets `(Process, 0, ALL)`, class-wide |
| the check | `proc/pcb.rs` | `has_capability_type` takes **no** `resource_id`, so a class-wide grant satisfies any query |
| inheritance | `proc/pcb.rs` | `fork` does `parent.cap_table.clone()` |

So the holder set was never empty. It was init plus every descendant nothing had narrowed, and §927 recorded a privileged write as unreachable while PID 1 could perform it.

### The decision

**`Rights::INIT_PROCESS`**, an explicit union of the fifteen declared rights, used at the init grant instead of `ALL`. Behaviour today is identical — init holds exactly what it held — and `ALL` keeps its meaning for tests and for callers that genuinely mean "every bit".

**And the count of declared rights is pinned in a `const` assertion.** This is the half that does the work:

```rust
assert!(Rights::DISTINCT.len() == 15, "a right was added or removed. Decide …");
```

An enumeration alone is not enough. A new bit added to `DISTINCT` and *forgotten* in `INIT_PROCESS` is simply not granted — silently, which is the opposite failure and just as quiet. Pinning the count means adding a right **fails to compile** until someone reads two lines and decides. Verified by planting a sixteenth right: `cargo check` exits 101 with that message.

A second assertion rejects any bit in `INIT_PROCESS` that is not a declared right, because that constant is written by hand and a typo would set a bit meaning nothing today and something unintended the day it is declared.

### Alternatives rejected

* **`ALL = union(DISTINCT)`.** Honest about "every declared right", and removes the undefined-bit semantics — but it does **not** fix this, because adding to `DISTINCT` still widens `ALL`. It replaces a wildcard over all possible bits with a wildcard over all declared ones, and the hazard is the wildcard.
* **Per-object grants** (`resource_id != 0`) so `has_capability_type` has something to discriminate on. Much larger, and the class-wide design is deliberate: the grant site records that a token nobody holds is indistinguishable from leaving the operation denied.
* **Leaving it and relying on care.** The defect's whole character is that it is invisible at the moment it is created: the author of the new right is not thinking about init, and nothing in either file mentions the other.

### What this deliberately does not do

It is **not** a narrowing of init's authority and must not be read as one. Init still holds every declared right; the grant is still class-wide. The only property bought is that the *next* `SET_HOSTNAME` does not reach the root process until somebody writes a line saying it should.

The `File` and `Socket` init grants still use `ALL`. Left alone on purpose: the same argument applies to them and the same remedy would work, but a new right is far more likely to be process-scoped, and changing three grants at once makes the boot test's verdict harder to attribute if something does move. Recorded in `known-issues.md` rather than done quietly. **Done on 2026-09-11 — see §930**, which also records why the two new lists hold every declared right rather than a narrowed subset. Note how the deferral expired: the reason given here was that three simultaneous grant changes would blur attribution, and that stopped being true the moment this one landed alone. A deferral justified by concurrency with another change goes stale the day that change ships, and nothing announces it.

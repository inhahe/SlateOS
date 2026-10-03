## §671 — the cross-backend metadata harness checks domain rules first and declared values second, and the declarations live in each backend's own test module

**Date:** 2026-09-01
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** several filesystems were losing the extra permission bits on a
file — the "setuid" bit and its two neighbours, which decide whether a program
runs as its owner. Each filesystem had tests, each passed, because each test
asked that filesystem what the bits were and then checked the answer against
itself. The fix is a harness that walks every backend and checks *shared* rules,
built in two layers. The first layer needs no test fixture and catches a large
class of nonsense; the second needs a fixture per backend and is the only one
that would have caught the original bug. The decision is to land the first layer
alone rather than hold both back until the second is plumbed — and to keep each
backend's expected values inside that backend's own test module rather than in a
table owned by the harness.

### The two layers, and why the cheap one is not sufficient

A **domain claim** can be judged with nothing but the value in hand:

| Claim | Caught without a fixture because |
|---|---|
| a mode outside `0o7777` | no permission bit exists above bit 11 |
| `readdir` and `stat` disagreeing about an inode number | one object, two routes, one answer |
| a nanosecond field holding a seconds-sized number | `1e16` ns is 115 days of sub-second remainder |
| a size that changes between two reads with nothing in between | the object did not change; the answer did |

A **value claim** needs a fixture that *declares* what was written: "this file
was created `0o104_644`, so it must read back `0o104_644`."

The distinction matters more than it looks. The bug that motivated all of this
was a mask — `& 0o777` where `& 0o7777` was meant — and **a narrowing violates
no domain rule.** Every value `& 0o777` can produce is inside the legal domain;
it is a legal value that is not the *right* value. So the domain layer, which is
the layer that just shipped and is already finding bugs, would **not** have
caught the thing it was built for. That is not a reason to withhold it — it
found two real defects on its first live boot, one of them
(`/proc/columnview` reporting zero columns directly above twelve listed columns)
a bug that only manifests depending on which self-test ran first, and which no
per-backend test could have found. But it must be recorded plainly, because a
half-built harness whose name promises the whole job is exactly the kind of
thing that gets marked done.

**Alternative considered: hold the harness until both layers are ready.** Its
argument is honesty — shipping the half that misses the motivating bug invites
someone to close the issue. Rejected because the second layer needs four
backends' fixture builders made `pub(crate)` and two existing assertions
rewritten, and none of that makes the first layer any better. The dishonesty
risk is handled where it belongs: the `known-issues.md` entry is marked
**NARROWED**, not fixed, and says in as many words that the domain layer would
not have caught the motivating bug.

### Why the declarations do not live in the harness

The obvious shape for layer two is a table in `conformance.rs`: backend name,
path, expected mode. It reads well and it is all in one place.

It is also the original bug, one level up. That table would assert facts about
files it did not create, maintained by whoever last edited the harness, while
the fixtures that actually write those files live in four other crates and are
edited by whoever is changing that backend. The two drift, silently, and the
drift is invisible in exactly the way the first bug was: the harness passes
because it is comparing a filesystem against a stale copy of its own intent.

So each backend exports its own `Declared { path, mode_on_disk, may_drop }` from
the module that builds the fixture. Changing what the fixture writes and
changing what is expected are then the same edit, in the same file, by the same
person. The cost is real — the expectations are spread across four modules
instead of gathered into one readable table, and adding a backend means
remembering to export the declaration rather than adding a row. Accepted: a
table that is easy to read and quietly wrong is worse than a declaration that
takes four files to survey and cannot go stale.

`may_drop` exists because the losses are not all bugs. Three backends mask
`& 0o7777 & 0o7555` on a read-only mount — twelve bits minus the three write
bits — which is correct and must not be flagged; and NTFS has no Unix mode at
all, synthesising one from the mount's read-only-ness, so it is genuinely out of
scope rather than merely unimplemented.

### Two structural rules the harness enforces on itself

Both come from the same observation: a test that decides not to run is a test
that reports success.

- **A skip may never be decided from the outcome of the code under test.** A
  backend carrying the metadata bug could fail its own `readdir`, be skipped for
  it, and the boot would still print "Conformance passed" — the skip would
  disable the check on precisely the boot where it would have fired. Every skip
  is now either read from the environment (the *mount table* is asked whether
  `/proc` is mounted; the filesystem does not get a vote) or classified through
  `fs::selftest::classify`, which separates "this system cannot" from "this
  system was asked and refused". The memfs fixture, which is `MemFs::new()` plus
  two writes and a `mkdir` and depends on nothing about the boot, is a
  **failure** when it will not build, never a skip.
- **A success line printed after a skip must name the skips.** The closing line
  is the one a reader believes, so a run that reached four of five mounts must
  not read as a full pass to someone who scrolled to the bottom.

### Consequences

- `known-issues.md`'s `A-NO-CROSS-BACKEND-METADATA-CONFORMANCE-TEST` stays
  **OPEN**. Its original blocker ("requires a small fixture per filesystem") is
  retracted — that is no longer what stands in the way; four `pub(crate)`
  changes are.
  - **Correction, 2026-09-01 (`5fa72bc13`), when this section was
    implemented:** the four `pub(crate)` changes were not needed either, so
    that second blocker is retracted too. This section reasoned that
    `conformance` would have to reach *into* each backend to drive its
    builder. It does not: the last clause of this section's own title —
    "the declarations live in each backend's own test module" — already
    implies the inverse, and that is what was built. `conformance_fixture()`
    lives *inside* each `tests` module, calls the private builders as a
    neighbour, and hands back a `Box<dyn FileSystem>`. Visibility changed
    nowhere. The lesson is narrow but repeatable: when a plan says a
    consumer needs wider access to a producer, check first whether the code
    can be written on the producer's side instead, where the access already
    exists.
- The harness now perturbs global allocator state, which a distant checker reads
  and fails on. `HeapWatch` samples the corruption counters per inspected path
  for that reason, and the general rule it encodes — a self-test that moves
  global state hands a later checker a failure it cannot explain — applies to
  any self-test, not just this one.

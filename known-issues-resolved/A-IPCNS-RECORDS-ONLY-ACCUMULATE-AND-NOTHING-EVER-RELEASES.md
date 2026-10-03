## `A-IPCNS-RECORDS-ONLY-ACCUMULATE-AND-NOTHING-EVER-RELEASES` (lane A, 2026-08-26) — **FIXED 2026-08-27**

**In short:** the IPC-namespace accounting can be told that a shared-memory
segment, a semaphore set or a message queue was *created*, but there is no way
to tell it that one was *destroyed*. Every counter only ever goes up. A machine
that creates and tears down IPC objects normally will, over time, report a
namespace holding far more than it holds — and the numbers can never come back
down short of destroying the namespace.

Found while clearing `cmd_ipcns` for
`A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`;
filed separately because it is not a parsing defect and refusing an unreadable
word does not touch it.

**Where.** `kernel/src/fs/ipcns.rs`. The public surface is `init_defaults`,
`create_ns`, `destroy_ns`, `record_shm`, `record_sem`, `record_msg`, `ns_list`,
`ns_info`, `stats`, `self_test`. Each `record_*` is a pair of `+=`:

```rust
ns.shm_segments += 1;
ns.shm_bytes += bytes;
state.total_shm += 1;
```

There is no `release_shm`, no setter, and nothing that ever decrements. The
only operation that reduces a count is `destroy_ns`, which removes the whole
namespace — and note it does not subtract that namespace's contribution from
the global `total_shm` / `total_sem` / `total_msg` either, so the *global*
totals survive even that.

**Two consequences, both live:**

1. **Per-namespace counts drift upward forever.** Whatever real IPC objects
   these are meant to track, they have lifetimes; the accounting does not model
   the end of one.
2. **The global totals in `stats()` are monotonic across namespace destruction.**
   `destroy_ns` removes the namespace from `state.namespaces` but leaves
   `total_shm` and friends where they were, so `stats()` can report more
   segments than the sum of every namespace it can still list. Those two
   numbers are supposed to describe the same thing.

**Reproduce** (kernel shell):

```
ipcns init
ipcns create demo          → id N
ipcns shm N 1024           (`ipcns shm add N 1024` since the fix below)
ipcns stats                → SHM: 1
ipcns destroy N
ipcns list                 → demo is gone
ipcns stats                → SHM: 1   ← still counted, nothing holds it
```

**Proper fix.** Give the subsystem the other half of each lifetime —
`release_shm(ns_id, bytes)`, `release_sem(ns_id, count)`,
`release_msg(ns_id, bytes)` — decrementing with `saturating_sub` (an
underflowing counter is a worse lie than a stale one) and returning
`KernelError::NotFound` for an unknown namespace, as the `record_*` pair
already does. Have `destroy_ns` subtract the departing namespace's
contribution from the global totals rather than orphaning it. Then expose them
as `ipcns unshm|unsem|unmsg <ns_id> [n]` — or, better, rename the pair to
`ipcns shm add|del`, since two verbs on one noun read better than two commands.

Worth deciding at the same time whether `record_*` should take an object
*identity* rather than a bare size, because `release_shm(ns, 1024)` requires
the caller to remember the size it passed in, and a caller that misremembers
silently corrupts the total in the other direction. That is the same class of
mistake this subsystem's parsing has just been cleaned of, so it should not be
reintroduced in the API.

### Fixed, 2026-08-27 — with two deliberate departures from the recipe above

`release_shm(ns_id, bytes)`, `release_sem(ns_id, count)` and
`release_msg(ns_id, bytes)` exist; `destroy_ns` subtracts the departing
namespace's rows from the global totals; the shell reaches all three as
`ipcns shm|sem|msg del <ns_id> <amount>`, with the matching `add` verb now
required rather than implied. Reasoning is `design-decisions.md` §628. The
reproduction above ends `SHM: 0` now, and `ipcns::self_test` grew from 8 cases
to 10 to pin it. Two places where the fix is *not* what this entry prescribed,
recorded because the entry is what a future reader will find first:

- **Not `saturating_sub` — a release that does not fit is refused whole.**
  This entry argued "an underflowing counter is a worse lie than a stale one",
  which is true and is not the choice on offer. Each row has *two* counters —
  a count of segments and the sum of their sizes — so clamping does not produce
  a stale number, it produces an impossible one: release "8192 bytes" from a
  namespace holding one 4096-byte segment and `saturating_sub` reports success
  and leaves `shm=0(0 B)`, or, clamping only one column, `shm=0(4096 B)` — no
  segments and 4096 bytes inside them. Both are plausible enough to survive
  unexamined forever, because the call returned `Ok`. So both halves are
  validated before either is written, and a release that would underflow either
  returns `InvalidArgument` having changed nothing. `self_test` case 7 reads the
  columns back after a refused release specifically so that a `saturating_sub`
  implementation fails there rather than passing.

- **`record_*` still takes a size, not an object identity.** The entry's worry
  is real — the caller must remember what it recorded — but the fix would be to
  make this module a registry of individual objects, which is a different
  module: it would need an id space, a per-object table, and a lifetime story
  for ids, to serve a subsystem that today exists only to *report* aggregates to
  `/proc/ipcns`. The refusal above is what makes the current shape safe rather
  than merely convenient: a caller that misremembers gets an error, not a
  corrupted total, in every case except passing the size of a *different*
  segment in the same namespace. If a real IPC implementation ever drives this
  instead of the shell, revisit it then — that caller will have object
  identities already, and will not have to invent them.

**Not a regression.** True since the subsystem was written; the `record_*`
functions have never had counterparts.

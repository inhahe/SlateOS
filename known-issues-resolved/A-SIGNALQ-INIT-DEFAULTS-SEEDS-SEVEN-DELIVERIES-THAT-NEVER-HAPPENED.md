## `A-SIGNALQ-INIT-DEFAULTS-SEEDS-SEVEN-DELIVERIES-THAT-NEVER-HAPPENED` (lane A, 2026-08-26) — ✅ FIXED 2026-08-26

**Fixed** exactly as the plan below prescribes: `init_defaults()` now seeds
`processes: Vec::new()` with all four counters at zero, and carries zramstat's
doc comment recording what the removed fixtures claimed. The self-test builds
its own fixtures through the real API.

Three notes on what the fix turned up beyond the plan:

1. **The self-test's counts are now stated as absolutes, not deltas.** Tests 1,
   6 and 8 all depended on the seed (`len() == 2`, `len() == 3`,
   `delivered == 8`). The temptation was to rewrite them as "whatever the seed
   left, plus what the test did". Stated that way a seed creeping back in would
   shift every figure by a constant and still pass. They are stated absolutely
   instead — `is_empty()`, `len() == 2`, `delivered == 1` — so a reappearing
   seed fails on the first assertion.
2. **Test 2 was silently load-bearing and now says so.** With no seed there is
   no other way for pid 1 to exist, so `send(0, 1, ...)` in test 2 is what
   creates the record that tests 4 and 5 block and unblock. Test 6's comment
   was also wrong once the seed went: it is no longer the first auto-create,
   it is the check that a *second* pid gets its own record.
3. **`signalq list` printed nothing at all on an empty table.** The seed
   guaranteed two rows, so this arm had never had to answer the empty case;
   with the seed gone, silence is the normal fresh-boot output and reads as a
   failed command rather than as "nothing has been signalled". It now says so
   explicitly, matching `pending`'s existing wording. *This is the general
   hazard in removing fabricated seed data: the seed was also suppressing every
   empty-state code path downstream of it, and those paths have never run.*

**Original report follows.**

**In short:** `signalq` reports how many signals each process has been sent and
delivered. Before anything runs, it already claims seven deliveries. They are
made up — written into the table at startup so the command would have something
to print.

`kernel/src/fs/signalq.rs`, `init_defaults()`: the state is seeded with two
`ProcessSignalState` entries, pid 1 with `total_delivered: 5` and pid 100 with
`total_delivered: 2`, plus a global `total_delivered: 7`. Nothing sent those
signals. `signalq stats` and `signalq list` present them in the same columns,
and the same formatting, as figures that were actually measured.

**This is a defect the tree has already named and fixed once, elsewhere.**
`kernel/src/fs/zramstat.rs` carries a doc comment recording that its own seeded
fixtures were "displayed as if they were real measured compressed-swap usage.
That demo data was removed; the self-test now builds its own fixtures explicitly
via the real API." `signalq` is the same module shape with the same seed still
in place — so the precedent for the fix, and the wording for it, already exist.

**The proper fix** is zramstat's: seed `processes: Vec::new()` with all totals
at zero, and rebuild the self-test's fixtures through the real API. That is not
a mechanical edit, which is why it is filed rather than done in the same commit
as the burn-down batch: `self_test` currently depends on the seed, e.g.
`unblock(1, Signal::Breakpoint)` at `signalq.rs:359` assumes pid 1 already
exists. The test must create its own process first — `send` auto-creates its
target, so a single `send` before the block/unblock steps is enough — and the
`[1/8]` step should then assert the table is empty after init, the way
zramstat's does.

**Why it is worth doing rather than tolerating:** every other counter in this
module is now trustworthy after batch 32, so the seeded seven are the only
fabricated numbers left in `signalq`'s output — and they sit in `total_delivered`,
the one column an operator would use to decide whether signal delivery is
working at all.

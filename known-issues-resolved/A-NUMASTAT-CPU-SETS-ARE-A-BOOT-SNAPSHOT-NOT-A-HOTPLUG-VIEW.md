## `A-NUMASTAT-CPU-SETS-ARE-A-BOOT-SNAPSHOT-NOT-A-HOTPLUG-VIEW` (lane A, 2026-08-26)

**Status:** ✅ **FIXED 2026-08-27** — a `cpu_hotplug` notifier now rebuilds the
per-node CPU sets on every CPU online/offline event. See "How it was fixed".

**In short:** `/proc/numastat` lists, for each memory node, which CPUs are
attached to it. That list used to be built once during boot and never rebuilt.
If a CPU started up after that moment, it was missing from the file for the
rest of the boot — not reported as belonging elsewhere, just absent. The window
was narrow and nothing user-visible depended on it, but the file's CPU lists
were strictly a boot-time snapshot rather than a live view.

**Where.** `kernel/src/fs/numastat.rs` → `adopt_topology()`, called once from
`kernel/src/main.rs` just after `numa::init()`. The per-node sets come from
querying `numa::cpu_node(cpu)` for `cpu` in `0..smp::cpu_count()`, and nothing
re-runs on a CPU coming online.

**The window is real, not theoretical.** `smp::init()` brings up application
processors and then waits for them on a *bounded* spin — `kernel/src/smp.rs`
around line 1231, ~50 ms — before returning. An AP bumps `NUM_CPUS_ONLINE`
itself (`smp.rs:958`) once its GDT/IDT/APIC are up. So an AP that misses that
window increments the online count *after* `smp::init()` has returned, which is
after adoption has already enumerated the CPU set.

**What this cost, and what it nearly cost.** The verification self-test
`self_test_adoption()` asserts that the CPUs adoption enumerated each land on
exactly one node — a sharp check, because the sets are derived rather than
copied. It originally re-read `smp::cpu_count()` for the expected value. That
would have panicked the boot whenever a late AP appeared between the two calls:
a self-test failure on a table that is entirely correct and merely one CPU out
of date. Fixed by having `adopt_topology()` return the count it enumerated and
`self_test_adoption(cpus_at_adoption)` take it — so the assertion tests the
*derivation* (the actual bug class it exists for) and not the *recency* (a
property one-shot adoption never promised). This is the same flake shape
rejected earlier in the irqbalance cross-check.

**How it was fixed.** Exactly as this entry proposed — an update path on the
node row, not a clear-and-re-adopt — plus the prerequisite it did not know it
had.

*The prerequisite.* Hooking `cpu_hotplug` would have been **inert on its own**:
`cpu_hotplug::init()` sampled `smp::cpu_count()` in precisely the same way this
module did, so no hotplug event ever fired for a late AP and there was nothing
to subscribe to. That is tracked and now fixed as
`A-CPU-HOTPLUG-INIT-SNAPSHOTS-A-CPU-COUNT-THAT-CAN-STILL-GROW`, and it had to
land first. Worth remembering as a pattern: *a stale-snapshot bug cannot be
fixed by subscribing to a notifier that is fed by the same stale snapshot.*

*The fix here.*

1. **`set_node_cpus(id, cpus)`** replaces one node's CPU set in place.
   `register_node` refuses a repeat with `AlreadyExists` by design (so a second
   adoption cannot zero the counters), which is why a separate update path was
   needed. In place rather than remove-then-re-add because a concurrent
   `/proc/numastat` reader would otherwise briefly see a machine with one fewer
   node — or, if it were the last node, with no memory at all. The
   allocation/access/migration counters are untouched: a CPU joining a node does
   not un-count the pages already allocated there.
2. **`refresh_topology()`** recomputes every present node's set and returns the
   number of CPUs placed. A node that is present in the topology but has no row
   (adoption hit `ResourceExhausted`, say) is registered here rather than
   skipped, so a later refresh can recover from a transient failure instead of
   leaving the row missing for the boot.
3. **`adopt_topology()` subscribes, then sweeps — in that order.** It registers
   the notifier and *then* calls `refresh_topology()` once. Doing it the other
   way round leaves a gap between the sweep and the subscription in which an
   event is lost for good, which is the exact bug being fixed. A CPU that
   arrived *during* the adoption loop is caught by the explicit refresh; one
   that arrives *after* is caught by the notifier. Registration is guarded by an
   `AtomicBool` because `adopt_topology` is documented idempotent and really is
   called more than once, while `cpu_hotplug::register_notifier` appends to a
   fixed table with no duplicate check.
4. **The notifier acts only on `Post*` events.** A `Pre*` event asks permission,
   and this module has no grounds to veto — a node's CPU list is a description,
   not a constraint. Acting on `PreOffline` would also be wrong on the merits:
   it fires while the CPU is still running, so dropping it from the list there
   would make the file disagree with reality for the duration of the migration.

**Two semantic changes worth knowing about.**

- **The derivation is now "CPUs that are online", not "indices below
  `smp::cpu_count()`".** Those coincide at boot and diverge the moment a CPU in
  the middle is offlined: smp's counter is a high-water mark of CPUs that ever
  started, while this file is supposed to describe the CPUs a node currently
  has. Linux's `nodeN/cpulist` reports the online set too. Concretely: offlining
  a CPU now removes it from `/proc/numastat`, which it did not before.
- **The online set is snapshotted once per pass, as a `u32` bitmask.** Every
  per-node derivation in a pass reads the same mask, so two nodes in one pass
  cannot disagree about which CPUs exist — previously a comment asking the
  reader to remember, now structural. The mask also makes the derivation
  allocation-free (a `[u32; MAX_CPUS]` scratch array), which matters because
  `refresh_topology` can run from a straggling AP's own bring-up path, before
  that AP has registered an idle task with the scheduler.

**The self-test changed shape, and it is stronger.** `self_test_adoption`
used to assert `sum(len(node.cpus)) == cpus_at_adoption`. A count is both weaker
and racier than what it replaced it with: weaker because two CPUs swapped
between nodes leave it unchanged, racier because an AP arriving mid-check moves
it — and now that the notifier legitimately adds CPUs after adoption returned,
the equality would flake outright. It now asserts, per element, that every CPU
on a row really belongs to that row's node (`numa::cpu_node` agrees) and that no
CPU appears on two rows; `cpus_at_adoption` survives only as a lower bound
("nothing placed has been lost"). Per-element facts cannot be falsified by a CPU
arriving mid-walk. A fourth check asserts `refresh_topology` is a fixed point —
running it against an unchanged machine must leave the rows identical, counters
included — because the notifier calls it on every event, so a version that
drifted would corrupt the file a little more each time rather than failing once
and visibly. `numastat::self_test` gained a ninth test covering `set_node_cpus`
directly: grow, shrink (an `extend` without the `clear` passes the grow case and
quietly accumulates), unknown-id refusal, and that no counter moves.

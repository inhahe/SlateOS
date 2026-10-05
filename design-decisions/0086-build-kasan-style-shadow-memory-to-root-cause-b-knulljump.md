## §86 — Build KASAN-style shadow memory to root-cause B-KNULLJUMP

**Date:** 2026-07-23
**Decided by:** Operator (Claude recommended A; operator concurred — "A")

**Decision.** Invest a focused effort in building **KASAN-style heap-corruption
detection** (option A of Q32) rather than keeping B-KNULLJUMP on WATCH (C) or
relying on a fragile hardware-watchpoint hunt (B). Build a **1/8-scale shadow
memory** region that marks every kernel-heap byte as addressable or poisoned,
with instrumented alloc/free and — on the suspect Path-Z spawn/teardown paths —
checked stores, so the corruption is caught **at the corruptor's write** instead
of at the victim's much-later read.

**Why.** B-KNULLJUMP is an intermittent (~1-in-120) kernel memory corruption at
Path-Z process spawn/teardown, on WATCH for many sessions. On 2026-07-22 it was
finally *symbolized* (see `known-issues.md`): the victim is a scheduler
`BTreeMap` node (`SchedState.tasks`) whose link pointer is zeroed — a *live-node*
wild write that the existing slab poison/redzone (`mm/poison.rs`, `mm/heap.rs`)
cannot catch (it is neither a UAF of a poisoned slot nor an adjacent-redzone
overflow). Shadow memory catches this whole class durably and layout-
independently, and pays off for all future memory bugs.

**Alternatives considered.**
- *B — hardware-watchpoint hunt on the reliable reproducer*: rejected as primary
  — the corrupted address isn't known a priori, varies with layout, and the
  reproducer is fragile (dissolves on the next kernel edit). May still be used
  opportunistically once shadow memory narrows the writer.
- *C — keep on WATCH, continue features*: rejected — leaves a real kernel
  corruption (can halt boot) unfixed and wastes the current concrete symbols.

**Tradeoffs (why this needed the operator).** Sizable new `mm` subsystem; memory
overhead (1/8 of the heap); perf cost on the alloc/store hot path — so it **must
be debug-gated** to protect the <200 ns heap-alloc target. std `BTreeMap`'s
internal stores may not be instrumentable without compiler support, so a targeted
store-check shim on the suspect paths may be needed.

**Where it lives.** `kernel/src/mm/` (new shadow module + hooks in `heap.rs`,
extending `poison.rs`), `kernel/src/mm/frame.rs` (buddy `FreeNode`), the Path-Z
teardown path in `kernel/src/proc/`, and `kernel/src/sched/mod.rs`
(`SchedState.tasks` — the observed victim). Tracked as B-KNULLJUMP in
`known-issues.md`.

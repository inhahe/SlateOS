## A-LOCKDEP-RECORD-EDGE-WAS-A-LINEAR-SCAN-ON-EVERY-NESTED-ACQUIRE (lane A, 2026-08-17) - **fixed**

> **Read the CORRECTION at the end of this entry before believing anything here
> about `page_fault_anonymous`.** The fix described below is real and was kept;
> the *cause* it is attributed to was tested afterwards and refuted. The
> performance claims in this entry stand as a recorded hypothesis, not a finding.

**In short:** the lock-order checker answered the question "have I seen this
pair of locks nested before?" by scanning its entire list of previously-seen
pairs, every single time one lock was taken while another was held. The list
only ever grows, so taking a lock got slower the longer the machine had been
running. This stayed hidden for a long time because the checker was only
switched on near the *end* of boot, where little had accumulated; moving it to
the *start* of boot -- done earlier the same day for unrelated diagnostic
reasons -- exposed it, and the anonymous page-fault benchmark more than doubled.

**Where:** `kernel/src/lockdep.rs`, `record_edge` and `has_cycle`; the missing
instrumentation is `kernel/src/bench.rs` `bench_lock_primitives`.

**The cost.** `lock_acquire` loops over every lock the CPU already holds (up to
`MAX_DEPTH` = 16) and calls `record_edge(held, acquiring)` for each. The old
`record_edge` opened with:

```rust
let count = EDGE_COUNT.load(Ordering::Relaxed) as usize;
for i in 0..count.min(MAX_EDGES) {          // <-- O(edges), every nested acquire
    let e = unsafe { EDGES[i] };
    if e.from == from && e.to == to { return false; }
}
```

so a nested acquire cost `O(held_depth x edges)`, with `edges` monotonically
increasing for the whole uptime of the machine. `has_cycle` was worse per call
-- `O(V x E)`, rescanning the entire edge table once per BFS node -- but it only
runs when an edge is genuinely new, so it is not the hot cost.

**How it surfaced.** `page_fault_anonymous`, from `bench/history.jsonl`:

| Runs | Commit(s) | ns |
|---|---|---|
| 34 consecutive | `fcd066231` .. `d542299e2` | 2057-2801 (median ~2090) |
| 1 | `7342d57ea` | 3510 |
| 2 | `78affced4` | 4333, 4978 |

**The first step is not this bug and not any bug.** `d542299e2..7342d57ea`
touches `bench/boot-history.jsonl` and `bench/history.jsonl` and nothing else --
zero kernel source -- and that commit's own message records the run as an A/A
comparison (same commit both sides) that the harness called clean. So
2158 -> 3510 (+63%) happened with a byte-identical kernel. That is a real
statement about this benchmark's environmental spread, and it is why the harness
was right to say "treat with suspicion".

**The second step is.** `78affced4` is the merge that carried the
`lockdep::init()` move. The bench harness's own context line, which had read
`43 lockdep classes registered` (or 44) on **ten** previous runs across
`build/baseline-boot.log`, `bench-capquery.log`, `bisect-86a923fe1.log` and
seven others, now read:

```
[bench]   lock context: 128 lockdep classes registered
```

128 is `MAX_CLASSES` **exactly** -- the table was not merely fuller, it was
*full*, and had been silently discarding every class past the 128th. With ~3x
the classes registered before the benchmarks run, the edge table is
correspondingly larger and every nested acquire pays a proportionally longer
scan.

**Why the harness could not see it, and this is the part worth keeping.** The
bench suite *does* measure lockdep overhead -- `lock overhead: total +241ns =
lockdep 32ns + ...` -- and that number stayed flat across the regression. It
stayed flat because every lock benchmark
(`lock_raw_spin`, `lock_tracked`, `lock_no_lockdep`, `lock_tracked_no_stats`)
takes **one** lock with nothing else held:

```rust
let tracked = run("lock_tracked", 2000, || {
    let mut g = TRACKED.lock();          // held.depth == 0 at this point
    *g = core::hint::black_box(*g).wrapping_add(1);
});
```

With `held.depth == 0` the `for i in 0..held.depth` loop in `lock_acquire` has
no iterations, so `record_edge` is **never called** and the scan being measured
is not in the measurement. The instrument pointed at lockdep was, by
construction, blind to the only part of lockdep whose cost was superlinear. An
overhead figure that cannot vary is not a control.

**Fix.** The edge list is now an adjacency bitmap, `ADJ: [[AtomicU64;
ADJ_WORDS]; MAX_CLASSES]` -- row `from`, bit `to`:

* `record_edge` is one `fetch_or`. `O(1)`, independent of edge count, and
  race-free in a way the old code was not: two CPUs could previously both
  finish the scan before either appended, then both append the same edge and
  both report it as new. Exactly one CPU now observes the bit as clear.
* `has_cycle` enumerates a node's successors from its own row -- `ADJ_WORDS`
  (= 4) word loads, iterating only set bits -- instead of scanning the whole
  table per BFS node.
* It is **smaller** than what it replaced (256x256 bits = 8 KiB, against a
  512-entry edge list that could hold 512 of 65536 possible edges) and it
  deletes the "edge table full" case outright. That case was a silent one:
  past 512 edges the old code stopped recording dependencies, so cycle
  detection went blind on a long-running kernel with no indication.
* `MAX_CLASSES` raised 128 -> 256, and `CLASS_HASH_SHIFT` 9 -> 10 with it, to
  hold the hash index at the 25% load factor its comment claims.
* Overflow is no longer silent: `report_class_table_full` prints once, because
  a dropped class is a lock the validator stops checking, and the absence of a
  warning then stops meaning "no violation".

**Tests.** `lockdep::self_test` gains "Test 10: edge bitmap", asserting that
`record_edge` reports an edge as new exactly once, that the count does not move
on a duplicate, that `from -> to` and `to -> from` are distinct edges (a bitmap
indexed by the wrong operand would make them look symmetric), that
out-of-range indices are refused rather than aliasing onto a real row, and that
`has_cycle` finds the 2-cycle just built. It removes its own synthetic edges
afterwards so the real graph is unperturbed.

**Instrumentation.** The bench context line now prints the edge count beside
the class count. The class count alone said nothing about the cost of a nested
acquire, which is why `128` -- a number equal to the cap -- was printed twice
and read as unremarkable.

**Still open, deliberately.** Two things this entry does *not* claim:

1. That the fix restores `page_fault_anonymous` to ~2090ns. The first step in
   the table above proves this benchmark can move +63% with identical code, so
   a single post-fix run cannot settle it either way.
2. That 256 classes is enough. The true number of distinct lock classes in this
   kernel is still unknown -- it was masked by the old cap for as long as the
   cap has existed. `report_class_table_full` is what will say.

A follow-up worth doing: add a *nested* lock benchmark
(`lock_tracked_nested`), so the cost that regressed here is measured directly
rather than inferred from a page-fault benchmark that happens to take nested
locks.

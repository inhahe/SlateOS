### A-LOCKDEP-RECORD-EDGE-WAS-A-LINEAR-SCAN — CORRECTION 2026-08-17 — the fix is right, the attribution above was wrong

**In short:** the fix landed and is worth keeping, but the claim above that this
bug caused the `page_fault_anonymous` regression is **not supported by the
measurement that tested it**. The section headed "The second step is" should be
read as a hypothesis that was then refuted, not as a finding. Recorded rather
than rewritten, because an attribution that survived a whole implementation
before failing its test is worth being able to find again.

**What boot #17 (PASS, clean streak 7, `--bench`) actually showed.** The new
instrumentation answered the question the old line could not:

```
[bench]   lock context: 138 lockdep classes registered, 132 dependency edges
[bench] page_fault_anonymous: min=15344 cycles (4138ns) ... [200 iters]
```

Two things follow, and they point opposite ways.

**1. The class overflow was real — this half is confirmed.** With the cap
raised to 256 the true count is **138**. The old cap was 128. So 10 lock
classes were being silently discarded on every boot after `lockdep::init()`
moved earlier, and every lock in those 10 classes was invisible to the deadlock
validator. That is a genuine correctness defect and it is fixed.

**2. The performance attribution was wrong.** The edge table held only **132**
edges — nowhere near the 512-entry cap, and far short of what the entry above
assumed when it reasoned that "the edge table is correspondingly larger". A
132-entry scan is not free, but it is not 2900ns of TCG work either. And the
decisive check: making `record_edge` `O(1)` moved `page_fault_anonymous` from
4333/4978ns only to **4138ns**, against a pre-shift baseline of ~2090ns. If the
edge scan had been the cause, removing it entirely would have restored the
baseline. It did not.

**So what is wrong with `page_fault_anonymous`? Still unknown, and it is
probably not code.** The strongest single piece of evidence remains the one
already in this entry: the first +63% step (2158 -> 3510ns) happened across
`d542299e2..7342d57ea`, which changes `bench/boot-history.jsonl` and
`bench/history.jsonl` and *nothing else* -- a byte-identical kernel, in a run
that commit's own message records as a clean A/A comparison. A benchmark that
moves +63% with identical code can move the rest of the way for the same
reason, whatever that reason is. Ruled out so far:

| Candidate | Ruled out by |
|---|---|
| `record_edge` linear scan | this correction: fixed, benchmark did not recover |
| `mm/page_table.rs` changes in the fence | diff is `const` values and doc comments only; no function body changed |
| PAT reprogramming mis-typing kernel memory | no mapping site hard-codes the `PWT` bit; `WRITE_THROUGH` was relocated to slot 7 and `NO_CACHE` left on slot 2, so named-constant users keep their memory type |
| `lockdep::init()` move enabling lockdep during benchmarks | it was already enabled -- `bench::run_all()` runs at main.rs:5775+, after *both* the old init site (5374) and the new one (1530) |

Not yet ruled out, and the next thing to try: **code layout under TCG**, via
`python scripts/straddle-check.py --compare <old-elf> <new-elf>`. The merge at
`78affced4` added ~6365 lines, which re-rolls the layout lottery for every
function; the harness's own prior for this is
`A-CRYPTO-BENCHMARKS-STEPPED-58-PERCENT-WITH-BYTE-IDENTICAL-CRYPTO-SOURCE`,
whose verdict was "code layout under TCG. Not a regression." Note this cannot
explain the *first* step, where the binary did not change at all.

**`isr_latency` is settled and is noise.** Boot #17 reported it "+66% vs
suite", but the same benchmark has a **2.34x spread within a single commit**
(`3f733c39c`: 19065 and 44619ns) and a 1.69x spread within another
(`d542299e2`: 40756 and 24064ns). A metric whose same-binary spread exceeds the
movement being flagged cannot support a regression claim. No action.

**The fix stands on its own merits regardless of the attribution**, and would
be worth keeping even if `page_fault_anonymous` had never moved:

* `record_edge` is `O(1)` instead of `O(edges)` on a path that runs on every
  nested lock acquire — CLAUDE.md forbids linear scans on hot paths, and this
  one grew with uptime.
* It closes a real race: two CPUs could previously both finish the scan before
  either appended, then both append the same edge and both report it as new.
* It removes two silent-degradation modes — the 512-edge cap (past which cycle
  detection stopped recording dependencies with no indication) and the 128-class
  cap (which was *actually being hit*, at 138).
* `has_cycle` is now complete. It previously abandoned the search after 32
  nodes, so a deadlock whose cycle ran through a 33rd lock class was never
  reported; raising `MAX_CLASSES` to 256 would have widened that hole.

**Lesson worth keeping.** The instrument that should have caught this was
pointed at the right subsystem and still could not see it: the bench suite
measures lockdep overhead using a lock taken with nothing else held, so
`held.depth == 0`, so `record_edge` is never called in the measurement at all.
The overhead figure stayed flat at ~32-36ns throughout because it was
structurally incapable of moving. That is why the edge count is now printed
next to the class count -- and it is why the follow-up below matters more than
the attribution this entry got wrong.

**Follow-up, now closed (2026-08-17):** `lock_tracked_nested` exists. It takes
`TRACKED_B` while holding `TRACKED`, so `held.depth > 0` and `record_edge`
actually runs inside the measurement; it is `track`ed into
`bench/history.jsonl` and diffed run-over-run. Boot #21 measured **721ns for 2
nested acquires vs 522ns for 2 flat = 199ns of lockdep dependency work per
nested acquire**, against a flat-path lockdep cost that now measures **0ns**
(tracked 261ns vs no-lockdep 263ns). Both numbers together are the point: the
old suite, pointed squarely at lockdep, would have reported the subsystem as
free, because the only path it could see was the one lockdep does no work on.
Note that 199ns is the *steady-state* cost -- the edge is already in the bitmap
after the first iteration, so this measures the lookup, not the one-off insert.

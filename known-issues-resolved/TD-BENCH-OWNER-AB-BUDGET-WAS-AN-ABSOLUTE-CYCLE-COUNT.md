### TD-BENCH-OWNER-AB-BUDGET-WAS-AN-ABSOLUTE-CYCLE-COUNT. Five boots of "ownership tagging costs 8500 cycles" were the emulator, not the code — 2026-08-14 — RESOLVED 2026-08-14

**Where:** `kernel/src/bench.rs` (`run_all`, the `page_alloc_free_owner_ab` and
`fast_cpu_index` budget checks) and `bench/baselines.toml`.

**Symptom.** `page_alloc_free_owner_ab` reported `SLOW (tagging costs N
cycles/alloc+free, limit 500)` on five consecutive boots: **10826 / 7660 /
8512 / 10580 / 11288**. `fast_cpu_index` simultaneously reported `SLOW (274 /
282 cycles, limit 200)` — on boots *after* the tier-0 fix that benchmark had
been added to prove worked.

**Why it was worth chasing rather than dismissing as noise.** The
reproducibility. A number that lands within ±20% five times is measuring
something. And the accused code is trivial: `frame_owner::set` is a relaxed
load, a bounds check, a byte store and a counter bump. When a measurement and
a static reading of the code disagree by two orders of magnitude, one of them
is wrong, and guessing which is how people end up optimising the wrong line.

**Three hypotheses, each killed by measurement rather than by argument:**

1. *Ambient load / windowing.* The first version ran 500 iterations with
   tagging off, then 500 with it on. Two consecutive windows on a live system
   are not the same system, and `min` does not save you — it is robust to
   *spikes*, not to a window uniformly busier than its neighbour. The evidence
   was in the same output: the off window had `max=129078` while the on window
   had `max=635531436` and a 30x higher mean. Fixed by alternating the arms
   every iteration (`ab_interleaved`), microseconds apart, so drift on a
   scheduling timescale lifts both and cancels. **The number did not move.**
2. *TCG's atomic-RMW fallback.* TCG cannot always lower a guest atomic RMW
   inline; `cpu_loop_exit_atomic` aborts the translation block and re-executes
   with the world stopped — thousands of cycles for one increment. Two shared
   `fetch_add` statistics counters sat on exactly this path. Measured directly:
   `atomic_fetch_add_relaxed` came out at **0-238 cycles**, so the two counters
   accounted for ~124 of ~8500. (The counters were moved to per-CPU
   cache-line-padded slots anyway, on general principles — the file already
   padded `CURRENT_OWNER` per-CPU with a comment about a "false-sharing storm"
   while leaving these unpadded on the same path. That commit says explicitly
   that it was *not* the cause.)
3. *The halves don't add up.* A first split put `set` at 2978 cycles and
   `current_owner` at 924 — `set + clear + current ≈ 6900` against a measured
   8512-10580, so they did add up and the cost was genuinely inside `set`.
   **But that split was flawed**: it never controlled the `ENABLED` flag, so
   "2978 cycles for `set`" lumped the cost of *calling* `set` together with the
   cost of the work `set` does — the two things that had to be told apart,
   pointing at opposite conclusions.

**What actually settled it.** A three-arm experiment: `set` with tracking
**off** (the early-return path — the harness's floor for calling into
`frame_owner`), `set` with tracking **on**, and a byte store to an ordinary
`.bss` static as a control. Result:

```
frame_owner_set_split: call_floor=278 work=2416 bss_store_control=218
```

**A single byte store to plain kernel `.bss` costs 218 cycles in this
harness.** `set` performs about half a dozen guest memory accesses (the
`ENABLED` load, the length and pointer loads inside `slot`, the tag store, and
the per-CPU counter's load and store); 6 × 218 ≈ 1300, the right order for the
measured 2416. Under TCG *every* guest memory access carries a softmmu lookup
costing a few hundred host cycles; the same accesses on real hardware are L1
hits at ~1-4 cycles. So ownership tagging adds ~16 memory accesses per
alloc+free — **~30 cycles of real machine, ~2500 cycles of emulator.**

Nothing had regressed. The benchmark was measuring the emulator and comparing
it to a budget sized for hardware.

**The real defect, and it is a general one.** An absolute cycle budget cannot
work in this harness. It conflates the code under test with an emulation
constant that varies with the host, the QEMU build and the accelerator, and it
fails permanently on code that is correct. `fast_cpu_index`'s 200-cycle budget
was the same defect in its purest form: **200 cycles is below the harness's
floor for a single memory access**, so *no* implementation could ever have
passed it — the check was structurally incapable of reporting PASS, and it was
accusing the very fix it had been added to guard.

**Fix — budgets in units of measured memory accesses.** `run_all` now measures
`memory_access_floor` first (a byte store to a dedicated `.bss` static,
interleaved against an empty closure, clamped to a minimum of 100 so a
noise-driven 0 cannot collapse every budget to 0) and expresses both delta
budgets as multiples of it:

* `fast_cpu_index`: 4 accesses (was 200 cycles). Clear of the noise, still far
  under an APIC MMIO round-trip.
* `page_alloc_free_owner_ab`: 40 accesses (was 500 cycles). The path performs
  ~16, so 2.5x headroom absorbs variation in how many the optimiser folds.

**The fix failed its own verification boot, in two ways, and both are worth
recording because both are mistakes the *unit change* made easy to miss.**

```
memory_access_floor: 100 cycles/guest byte-store (measured=74 nop=1278 store=1352)
fast_cpu_index: PASS (288 cycles over an empty closure, limit 400 = 4 accesses)
page_alloc_free_owner_ab: SLOW (17176 cycles/alloc+free = 171 accesses, limit 40)
```

1. **The calibration was itself vulnerable to the noise it existed to
   correct for.** It timed *one* store against one empty closure, and the
   subtraction is only meaningful if the two arms' baselines agree. They did
   not: `nop=1278` here, while the very next block in the same run measured
   `nop=448`. A ~200-cycle access simply has no signal above a
   several-hundred-cycle wander, so `measured=74` was noise and **the clamp
   became the answer** — which then under-scaled every budget derived from it
   and manufactured the SLOW below. Fixed by *amplifying*: 64 stores per timed
   window, divided by 64. The signal scales with N, the wander does not, so it
   divides away. The loop's own overhead is left inside the measurement
   deliberately — it can only enlarge the floor and loosen the budgets, and for
   a check whose whole purpose is to stop crying wolf, false negatives are the
   safe direction. The clamp stays as a backstop, but if a run ever prints
   `measured` at or below it, that run's budget verdicts are unreliable rather
   than findings.
2. **40 accesses was too tight even against a correct floor.** Honest recount:
   ~20 architectural accesses per alloc+free (`tag_alloc_owner` = 1 is_enabled
   + 2 current_owner + 8 set; `untag_free_owner` = 1 + 8). Observed healthy is
   ~50-57 (11288/218 = 51.7 on one boot, ~57 on the next) — a consistent
   2.5-3x multiplier, which has a cause rather than being slop:
   `scripts/boot-test.sh` runs a plain `cargo build`, and the workspace's
   `[profile.dev]` sets only `panic = "abort"`, so **opt-level is 0 and the
   benchmarked kernel is unoptimised** (`cargo` prints `Finished dev profile
   [unoptimized + debuginfo]`). Nothing is inlined, so each of the ~6 calls on
   this path runs a real prologue/epilogue whose spills and saved registers are
   memory accesses the source-level count omits. ~3x over the architectural
   count is what an unoptimised build predicts and what two independent boots
   measured. Budget raised to **150** ≈ 3x the observed ~50.

   The temptation here was to loosen until it passes, which is the anti-pattern
   this whole entry is about. What makes 150 legitimate is that the number is
   derived from a *mechanism* (opt-level 0, non-inlined calls) that predicts the
   observed multiplier independently, and that the looseness costs no detection
   power: this is a structural tripwire, not a stopwatch, and every failure it
   guards against is an order-of-magnitude event, not a percentage.

**Verified 2026-08-14** on the boot following both follow-up fixes:

```
memory_access_floor: 284 cycles/guest byte-store (measured=284 over 64 stores/window: nop=8238 store=26474)
fast_cpu_index: PASS (476 cycles over an empty closure, limit 1136 = 4 accesses)
page_alloc_free_owner_ab: PASS (tagging costs 11778 cycles/alloc+free = 41 accesses, limit 150)
frame_owner_set_split: call_floor=282 cycles work=3054 cycles
```

The amplified calibration produced `measured=284` where the single-store form
produced `74`, so the clamp no longer binds and the floor is a real quantity.
It cross-checks: the independently-measured `.bss` control in
`frame_owner_set_split` came out at 218 on an earlier boot, and 284 exceeds
that by roughly the loop overhead this deliberately declines to subtract.

The detail worth keeping in view is that the absolute cycle figure — **11778** —
sits squarely in the same 7660-11288 band that was reported as `SLOW` for five
consecutive boots. Not one line of `frame_owner` changed between those runs and
this one. Only the unit the budget is written in changed, which is the whole
thesis of this entry stated as a measurement.

**Follow-up wart, also fixed:** the split diagnostic printed
`call_floor=282 cycles (0 accesses)` — 0.99 accesses truncated to `0` by
integer division, reading as "this costs nothing" in the one line whose job is
to say where the cost lives. Access counts now print to one decimal via a
`accesses(cycles, floor) -> (whole, tenths)` helper.

This keeps the checks doing what they exist for. The failures worth catching —
an uncached MMIO round-trip, a contended lock, a per-frame loop where a
`write_bytes` belongs (which scales with `count`) — cost 10-100x a plain
access on hardware *and* under emulation, so they still blow past the budget.

**Kept as a permanent diagnostic:** the `frame_owner_set_split` line, now
reported in access units. If the A/B ever fires again it says in one line
whether the cost is inside `set`'s working path or elsewhere — the fork this
investigation burned four boots failing to resolve by argument.

**Lesson for the next benchmark added to this file.** Any threshold on an
in-kernel QEMU/TCG measurement must be expressed relative to something
measured by the same harness in the same run. Absolute nanosecond and cycle
targets taken from Linux publications belong in `baselines.toml` as *context*;
they cannot be pass/fail gates here. The pre-existing `ABOVE TARGET` verdicts
in this suite (e.g. `isr_latency: 233451ns, target 10000ns, 2334%`) are the
same category of statement and should be read as "this is what the emulator
does", not as regressions.

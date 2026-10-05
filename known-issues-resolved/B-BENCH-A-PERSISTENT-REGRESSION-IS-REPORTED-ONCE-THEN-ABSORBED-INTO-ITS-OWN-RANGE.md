### [A] B-BENCH-A-PERSISTENT-REGRESSION-IS-REPORTED-ONCE-THEN-ABSORBED-INTO-ITS-OWN-RANGE — 2026-08-15 — 🔧 FIXED (harness defect fixed; both "regressions" it exposed are disproved — `http_build_response_1KiB` is a layout lottery per Finding 3, `vfs_stat_root` is smaller than one binary's own spread per Finding 4)

**Two findings: two genuine regressions, and the reason the harness stopped
reporting them on the very next run.**

#### The series (release runs, ns, oldest → newest)

```
                          e7b912d 0bd70ab 7a96b55 c43ce8a c43ce8a 8c3f844 8135d14 24a3407 f79aec5 c893184 e384f46 c5a4013
http_build_response_1KiB     6150    5992   10089    5964    6167    5990    5987    6018    5890  >8546  >12431  >12407
vfs_stat_root                3473    3777    3721    3453    3591    3635    3883    3998    3136    3278   >4488   >4429
vfs_stat_breakdown_full         -       -       -       -       -       -    3915    4015    3170    3219   >4424   >4505
ipc_eventfd                   541     653     652     544     540     539     660     641     534     537   1021     653
net_ipv6_parse                 81      81      80      80      81      79      80      80     113     169      80      80
```

#### Finding 1 — two regressions are real, one "regression" was noise

`http_build_response_1KiB` sat at ~6000 ns for nine runs, then went 8546 →
12431 → **12407**. `vfs_stat_root` sat in 3136-3998 for ten runs, then 4488 →
**4429**. `vfs_stat_breakdown_full` likewise: 3170-4015, then 4424 → **4505**.

The consecutive pairs agree to **0.2%** (12431 vs 12407) and **1.3%** (4488 vs
4429). That is the decisive point, and it survives the fact that *both* runs
were contaminated: host disturbance shows up as **stalls**, which inflate the
*mean*, and these figures are **min-of-N**. Noise does not reproduce to two
parts in a thousand. Two independently-disturbed runs landing on the same
number is evidence *for* a real shift, not against it.

By the same test `ipc_eventfd`'s spectacular +90% (537 → 1021) was **noise**: it
did not reproduce (653, back inside its long-standing bimodal 534-660). So the
earlier decision not to dismiss all three as contamination was right, and so was
declining to accept all three — one of the three was exactly what the
contamination story predicted, and two were not.

`net_ipv6_parse` is **resolved**: 80 ns across both runs, matching the nine runs
before the 113/169 excursion. The excursion is over and was never a code change.

#### Finding 2 — the harness reported "no benchmark moved outside its own range" on the run that confirmed the regressions

The re-run's verdict was:

```
No benchmark moved outside its own recent range (2 crossed 25% run-over-run).
```

Both statements are true and together they are misleading. The comparison is
**run-over-run against the immediately preceding run**, and "its own recent
range" is a window over the last 8 runs. So once a regression has appeared in
one run:

1. run-over-run sees 12431 → 12407 and correctly reports **no movement**; and
2. the range has absorbed the elevated sample, so 12407 is now **inside** it.

A regression is therefore visible for exactly **one run**. On the second run it
becomes the new normal, and the harness affirmatively reports the suite as
clean. This is worse than silence: a run that says "no benchmark moved outside
its own recent range" is naturally read as "no regressions", when what it
actually means is "nothing changed *since the regression*."

The window poisons itself, and it does so fastest for exactly the regressions
that matter most — a persistent one, which by definition appears in every
subsequent run.

**The proper fix** is for the range to be computed over runs that are *known
good*, not over "the last 8 whatever they were": compare against the median of
the last N runs **that predate the newest run's own value entering the window**,
or keep a pinned baseline per benchmark that only moves when a change is
explicitly accepted. A cheap interim check that would have caught this: flag any
benchmark whose newest value is >25% off the median of runs 5-12 back, in
addition to the existing run-over-run test.

**Not yet known: what caused the two regressions.** Both jumps bracket merges of
other lanes' work into `lane-a` as well as this lane's own `sys_cap_query`
change, so attribution needs a bisect over the recorded commits
(`f79aec5 → c893184 → e384f46`), not a guess. `http_build_response_1KiB` is the
better target: it more than doubled, in two clean ~45% steps, from a nine-run
plateau.

**Step 1 attribution, 2026-08-15: code layout is RULED OUT — and this is a
positive result, not an absence of evidence.** Both commits were built from a
scratch worktree (`build/straddle/kernel-f79aec561`, `kernel-c893184fa`) and
compared with `scripts/straddle-check.py --compare`. The pair is adjacent — one
commit, `mm/vfs: return Cow<'_, Path> from namespace::resolve_path` — which
touches nothing in the HTTP path.

The re-roll is enormous, exactly as the family analysis predicted:

| | count |
|---|---|
| loops that gained a straddle | 5181 |
| loops that lost one | 4997 |
| functions quarantined as recompiled | 9 |

The 9 recompiled functions are precisely `namespace::resolve_path`, its callers
and their self-tests — the signature check isolated the commit's real footprint
with no false positives, which is the first end-to-end evidence that the
quarantine works on a real pair rather than on synthetic inputs.

**And none of those 10178 flips is on this benchmark's path.** That is what
makes this decisive: the instrument fired ~10k times on the same binary pair
and still reported nothing here.

| on the `http_build_response_1KiB` path | loops | straddle change |
|---|---|---|
| `bench_build_response` (has `build_response` inlined) | none | — |
| `etag_for_body` — 103 B FNV loop over 1 KiB, **the dominant loop** | 2 | `no` → `no` |
| `memcpy` / `memset` / `memmove` | **none** (`rep movsb`, no backward branch) | — |
| `__rust_alloc` / `__rust_dealloc` | none | — |
| `core::fmt::write` | none | — |
| only httpd flip in the whole binary: `build_response_gzip` | 1 | straddle **lost** (faster, wrong direction) |

**The straight-line confound was measured too, not waved away.** `straddle-check`
models *loops*, but a TCG translation block is also cut by a page boundary in
straight-line code, so a call-heavy path could pay per call rather than per
iteration. Counting page-boundary crossings inside every function on the path
gives **2 in the old build, 3 in the new** — the single change being one integer
`Display` impl. The response formats two integers, so the worst case is two extra
dispatcher round-trips per call against a +2656-cycle regression. It does not
account for it.

This limitation of the tool is real and remains: `straddle-check` cannot see
straight-line page crossings, and here that had to be checked by hand. Logged so
the next comparison does not quietly assume loops are the only mechanism.

**Why the scratch worktree does not invalidate this.** The two binaries were
built in `os-straddle-scratch`, not in `os-lane-a` where the recorded bench runs
were built, and the two directory names differ in length — so if any absolute
build path were embedded, `.rodata` would shift and the scratch pair would be a
*different* draw of the layout lottery than the pair that produced the numbers.
Checked rather than assumed: the only absolute path in the image is
`D:\visual studio projects\os\netproto\src\ipv4.rs` and friends, which live
inside the **prebuilt service ELFs** that `include_bytes!` embeds. Those blobs
were built once on Aug 14 and copied in byte-for-byte, so they are identical in
both builds and do not vary with the kernel's build directory. No Cargo.toml
declares an absolute path dependency. The kernel therefore contains no string
that depends on which worktree built it, and the scratch pair reproduces the
lane-a pair.

A second, independent consistency check points the same way: the compare
quarantined exactly **9** recompiled functions, and they are precisely
`namespace::resolve_path`, its callers and their self-tests. Build
nondeterminism or a stale artifact would have produced signature changes
scattered far beyond one commit's source footprint.

**Tooling hazard found while checking this: `strings` is not installed on this
machine, and neither is `llvm-strings` in the rustup toolchain.** `strings -a
<elf> | grep …` therefore prints nothing at all — which is indistinguishable
from "the string is not in the binary", and is exactly the false negative that
would have "confirmed" path-independence for the wrong reason. It was caught
only because `grep -a -c` had already reported a match on the same file, so the
two disagreed. Use `grep -a -o -b ".\{0,60\}<pattern>.\{0,60\}" <elf>` instead;
it needs no extra tool and prints the byte offset and surrounding context.
`command -v strings` before trusting an empty result from it.

**So the regression has a non-layout cause, and the leading hypothesis is now
that the benchmark is not isolated from heap history.** The suite runs in a fixed
order in one address space; `build_response` allocates a `String` and grows a
`Vec`, so which slab/free-list path those hit depends on everything allocated
before them. A commit that removes allocations from path resolution changes the
heap's shape by the time the HTTP benchmark runs, without touching a line of
HTTP code. That is the same class of defect as the straddle lottery — a
whole-binary property masquerading as a per-benchmark regression — and it is
**not yet tested**; it is written down here as the next thing to falsify, not as
a finding.

#### Finding 2 — **FIXED 2026-08-15** (lane A): `level_shifts()` in `scripts/bench-history.py`

The harness defect is closed. A new check compares each run against a baseline
drawn from runs that **predate the shift** — `LEVEL_SHIFT_SKIP = 3` most-recent
runs are excluded from the reference window, so a regression introduced in the
last one-to-three runs cannot have entered its own baseline. This is the
"proper fix" option above (median of runs 5-12 back), not the cheap interim one.

On the recorded history it now prints, on the very run quoted above as reporting
the suite clean:

```
SUSTAINED SHIFT (>25% off a baseline from before the last 3 runs, and outside
that baseline's own spread -- these do NOT show up run-over-run once they persist):
  http_build_response_1KiB: was ~5991ns -> now 12407ns (+106% vs suite);
  pre-window baseline 5759-6277ns over 8 runs
```

**Two things had to be got right, and both were found by measurement, not
reasoning — recorded because the first version of each looked obviously
correct:**

1. **Persistence is the entire discriminator.** The first version compared only
   the newest run to the clean baseline. Replayed causally over all 26 recorded
   runs it fired on **11 (42%)**, including `net_ipv6_parse +110%` and
   `page_fault +103%` — the exact single-run excursions this same entry
   identifies above as noise. Drift correction does not save it: contamination
   is a **heavy tail, not a uniform slowdown**, so `speed_factor` (a median)
   removes the central shift and leaves the tail looking like several
   simultaneous regressions. Requiring the benchmark to be off-baseline in the
   newest run *and* the `LEVEL_SHIFT_PERSIST = 2` before it took this to 2/26,
   because host disturbance is random per run while a code regression is in
   every run after the commit.
2. **A flat percentage threshold is not scale-aware.** The one survivor was
   `ipc_channel_sync` (646 → 967 → 684 → **578** — i.e. noise), whose own 1.5-IQR
   fence is ~20% wide, so a 25% threshold sits *inside* its noise. Judging the
   level shift against Tukey's **extreme**-outlier fence (`k=3`) instead of the
   1.5 used elsewhere declines it while `http_build_response_1KiB` remains
   outside by a factor of two. Final rate: **1 firing in 26 runs**, the true
   positive.

**Known limitation, deliberate:** this inherits the flat 25% threshold, so the
concurrent `vfs_stat_root` shift (~3600 → ~4450, **+23%**) is below it and is
*not* reported by this check. That is the pre-existing blind spot, not a new
one; the `vfs_stat_root` regression above is still tracked by hand.

Regression tests: `scripts/test-bench-history.py`, three new cases. They were
**mutation-tested**, not merely observed to pass — deleting the persistence
check makes both the synthetic one-run-excursion case and the real-history
"stays quiet" control fail. Note for whoever tunes the constants: patching
`bh.LEVEL_SHIFT_PCT` at runtime does **nothing**, because `threshold_pct`
defaults to it and default arguments bind at definition time. A mutation test
that patches the module attribute silently tests nothing and reports success —
pass `threshold_pct=` explicitly, as the test does.

**Nothing is still open in this entry.** The harness defect is fixed;
`http_build_response_1KiB` is resolved by Finding 3 and `vfs_stat_root` by
Finding 4 — neither was a regression. **Read Findings 3 and 4 before acting on
Finding 1, which they supersede.**

#### Finding 3 (2026-08-15, supersedes Finding 1 for `http_build_response_1KiB`) — there is no regressing commit; the metric is **bimodal**, and the mode is a property of the binary

**In short:** I was bisecting for a commit that made this benchmark twice as
slow. There isn't one. The benchmark has two stable speeds — about 6000 ns and
about 10800 ns — and each *build* lands in one of them, essentially at random,
depending on where the compiler happened to place the code. Re-running the same
build always gives the same speed; changing almost any unrelated code can flip
it. The "regression" is the metric flipping into its slow mode, and it has
flipped **back and forth** several times already.

**The evidence.** Taking every release-profile, non-`loaded` record in
`bench/history.jsonl` (n = 20) and sorting by value gives a cleanly separated
pair of clusters with **nothing in between**:

| mode | n | mean | range |
|---|---|---|---|
| LOW  | 11 | 6055 ns | 5877 – 6396 |
| HIGH |  9 | 10806 ns | 8546 – 12934 |

The gap between the highest LOW (6396) and the lowest HIGH (8546) is empty.
**HIGH/LOW = 1.78×**, which is the documented TCG page-straddle penalty (~1.7×)
and not a number I chose.

**The mode is deterministic per binary — this is the decisive test.** Three
commits were measured more than once, seven measurements in total:

| commit | n | values (ns) | modes |
|---|---|---|---|
| `26c1c7330` | 3 | 12934, 8818, 11381 | HIGH only |
| `3f733c39c` | 2 | 9019, 11633 | HIGH only |
| `c43ce8acc` | 2 | 5964, 6167 | LOW only |

**Zero repeats cross the mode boundary.** Host noise moves a value by up to
1.47× *within* the HIGH mode (8818 → 12934) but has never once carried a HIGH
binary into LOW or the reverse. So the mode is a deterministic function of the
compiled image, while the scatter inside a mode is run-to-run noise. That is
exactly the layout-lottery signature: deterministic per binary, re-rolls
whenever unrelated code shifts an address.

**And the mode flips in both directions across the commit sequence:**

```
LOW LOW LOW | HIGH HIGH HIGH HIGH HIGH | LOW LOW | HIGH | LOW LOW LOW LOW LOW | HIGH HIGH HIGH
```

A regression caused by a bad commit does not un-regress and come back. This
sequence has five direction changes.

**What was wrong with Finding 1.** It read "sat at ~6000 ns for nine runs, then
went 8546 → 12431 → 12407" and concluded a step change. But its *own* series
table, printed directly above it, contains `7a96b55 → 10089` inside that
supposedly-flat stretch — a HIGH-mode reading that was set aside as noise
because it did not fit the step-change story. It is not noise; it is the same
mode the last three runs are in. The two-consecutive-runs-agree-to-0.2%
argument (12431 vs 12407) is still *true*, and still correctly rules out
run-to-run noise — but agreeing to 0.2% is precisely what two runs of the same
*mode* do. The argument distinguishes "not noise" from "noise"; it never
distinguished "code got slower" from "layout re-rolled", which was the actual
alternative.

**Consistency with the step-1 straddle falsification above.** No contradiction:
that analysis showed no *loop* straddle flip on the benchmark's path, and it
was right. It also found straight-line page crossings on the path changing
2 → 3, which at the time looked minor. Given the bimodality, straight-line
crossings are now the leading mechanism, and the loop-only tool blind spot
(since fixed) is why the first pass looked exculpatory.

**Consequences.**

- **The task "attribute the `http_build_response_1KiB` 2× regression" is closed
  with a negative answer.** There is no commit to attribute it to. Bisecting
  further is bisecting noise-with-structure and will keep producing
  plausible-looking but false attributions — `c893184fa` is *not* guilty, it
  merely re-rolled.
- **This metric must not gate anything until it is de-lotteried.** Any
  threshold between 6396 and 8546 fires on a coin flip. The harness's
  own-range check (`f0cb9eccf`) partly absorbs this, but only by widening the
  range until the metric says nothing at all.
- **`vfs_stat_root` is a different shape and is NOT explained by this** — but
  it is not a regression either; see Finding 4.

#### Finding 4 (2026-08-15) — `vfs_stat_root`'s "regression" is smaller than one binary's own run-to-run spread

**In short:** the other benchmark in this entry was also reported as regressing
(about 3600 → 4450). It isn't. A *single unchanged build* of this benchmark has
produced readings from 3344 to 5930 — a spread wider than the entire claimed
regression, which sits comfortably inside it.

`vfs_stat_root` is **not** mode-structured: its 21 release values form a
continuous 2623 – 6454 spread with no empty gap, and the repeat-commit test
declines it (commit `26c1c7330`'s own readings straddle every candidate split).
So the mechanism is different from Finding 3 — but the conclusion is the same,
for a simpler reason:

| commit | readings (ns) |
|---|---|
| `26c1c7330` | **5930, 4394, 3344** |
| `3f733c39c` | 2623, 3310 |
| `c43ce8acc` | 3453, 3591 |

One binary, `26c1c7330`, produced both 3344 and 5930 — a 1.77× spread with no
code change whatsoever. The reported regression is 3278 → 4488, and 4488 is
**below** one of that same binary's own readings. There is no effect here to
attribute: the metric's run-to-run noise is larger than the movement being
investigated.

`vfs_stat_breakdown_full` has only six release readings (3170 – 4505, 1.42×
spread) which is too few to judge, and it moves in step with `vfs_stat_root`;
absent any independent evidence it should be treated the same way until it has
enough history to say otherwise.

**Consequence.** The task "attribute the `vfs_stat_root` regression" is closed
with a negative answer, the same as Finding 3's. Both benchmarks are too noisy,
in different ways, to support the reports that were made about them — and in
both cases the disproof was already sitting in `bench/history.jsonl` and needed
no new boot. The general lesson is worth stating plainly: **before attributing a
movement to a commit, check what the same commit's own repeats do.** That is now
enforced automatically by `mode_structure()` in `scripts/bench-history.py`,
which reports a mode-structured shift as "NOT a regression to bisect" and
excludes it from `--fail-on-regression`.

**Method note, for reuse.** The test that settled this costs nothing and should
be the *first* step next time a benchmark "regresses": group the history by
commit, keep only commits measured more than once, and ask whether any of them
straddles the proposed threshold. If repeats never cross it, the metric is
mode-structured and bisection is the wrong tool. A great deal of straddle
tooling was built before anyone ran that three-line query — the raw data it
needed had been sitting in `bench/history.jsonl` the entire time.

### [A] OPEN — the bench harness treats "replicates exactly" as proof of a code regression, but that is also the signature of a code-*layout* artifact under TCG — 2026-08-19

**In short:** the benchmark harness guards hard against random noise: it will not
call something a regression until the same slowdown shows up in a second run of
the same binary. That guard is sound as far as it goes, but it only rules out
*noise*. It does not rule out the other thing that reproduces perfectly — the
compiler having moved unrelated code to a different address. Changing one file
shifts the address of everything linked after it, and under emulation that alone
can move an unrelated benchmark by 2× in either direction, identically every run.
So the harness's highest-confidence verdict is reached by both a real regression
and a pure artifact, and it cannot presently tell them apart.

#### The evidence

Commit `5e9a30a22` changed three kernel files — `sched/mod.rs` (+308 lines),
`idt.rs` (+73), `serial.rs` (+2). It touched **no crypto, IPC, VFS, net, mm or
HTTP code**. Comparing the two `--bench` runs of that image against the last run
of `d7c311deb`, thirteen benchmarks moved >25% *and* agreed between the two runs
to within 10%:

| direction | count | examples |
|---|---|---|
| slower | 3 | `crypto_sha512_64B` 1921 → **3661, 3661** ns (1.91×) |
| **faster** | **10** | `crypto_poly1305_1KiB` 13693 → 4977, 5081 (0.37×); `page_fault` 4592 → 2092, 2097 (0.46×); `vfs_read_256` 20256 → 11198, 10451 |

`crypto_sha512_64B` reproduced to **within 4 cycles out of 13582, twice**, against
a historical band of 1867–1929 ns across seven prior runs on two commits. That is
not noise by any definition the harness uses — and it is exactly what it will
report as `REGRESSED … replicated`.

#### Why it is nonetheless not a regression, proved two independent ways

1. **Direction.** Ten of the thirteen got *faster*, with identical replication
   confidence. A change can only add work; nothing in a liveness-watchdog edit
   makes Poly1305 2.7× faster. So the deterministic-mover set demonstrably
   contains movements that cannot have been caused by the code — which is enough,
   on its own, to establish that "replicated exactly" does not imply "caused by
   this commit."
2. **The metric is a minimum.** These benchmarks report `min` over 500–2000
   iterations. The commit's added work lives in the timer-tick path and runs at
   most once per `WATCHDOG_CHECK_INTERVAL` (500 ticks), so almost every iteration
   sees none of it — and **a periodic interrupt cannot raise a minimum**. Only a
   *per-iteration constant* moves a min, and code placement is one; added periodic
   work is not.

Both point at the same mechanism: the image grew, everything linked after
`sched/mod.rs` shifted address, and QEMU's TCG — which retranslates guest code
into host code cache — is strongly sensitive to that placement.

#### Why this matters more than a wrong verdict on one commit

The harness is unusually careful everywhere else, and the surrounding text says
so at length: it cancels whole-suite drift, it withdraws unreplicated claims, it
warns that the canary is blind to host descheduling, it refuses to treat an A/A
pair's movement as code. Every one of those guards addresses *variance*. None
addresses *bias from relinking*, and because a layout artifact survives every
variance guard, it arrives at the reader wearing the harness's strongest label.

That inverts the intended reading exactly the way the liveness watchdog's own
breadcrumb did: the mechanism that is supposed to increase confidence is the one
that manufactures it.

#### Concretely, this run

No regression is attributed to `5e9a30a22`. The three "slower" movers are
`crypto_sha512_64B` (layout, per above), `firewall_check` (102 then 78 ns against
a 45–79 band — the second run lands *inside* the band, so unreplicated), and
`ipc_channel` (723, 755 against a 532–1011 band — inside it). `lock_uncontended`
was reported `REGRESSED … replicated` by the A/A run despite that run's own
banner stating no movement in it can have been caused by code; its two samples
are 280 and 486 ns, which is bimodal, not replicated. That last one is a
straightforward bug in the replication predicate and is the cheapest part to fix.

#### What the fix looks like

Three parts, in increasing cost:

1. **Fix the A/A contradiction.** When the baseline image sha equals this run's,
   suppress the `REGRESSED` section outright rather than printing it under a
   banner that says it cannot mean anything. Presently both are emitted and the
   reader must notice they contradict.
2. **Record the image sha per benchmark comparison and label cross-image
   movements as `MOVED (image changed)`, not `REGRESSED`,** unless the changed
   files plausibly reach the benchmark. The harness already records
   `kernel_sha`; it just is not consulted for this.
3. **Separate layout from mechanism directly** by benchmarking the same source
   twice with a deliberate no-op layout perturbation (e.g. a padding `#[used]`
   static sized to shift the text segment). Two builds of identical semantics
   that disagree by 2× would let the harness *calibrate* a per-benchmark layout
   sensitivity band, and report movement only outside it. This is the real fix
   and the only one that makes cross-commit crypto/mm numbers trustworthy under
   TCG at all.

**Part (3) landed 2026-08-19** — the mechanism exists; see design-decisions.md
§234. `SLATEOS_TEXT_PAD=<bytes>` emits a `#[used]` array into
`.text.slateos_layout_pad`, which `kernel/linker.ld` places first in `.text` so
it shifts every function; the boot banner reports `textpad=<n>`;
`bench-history.py` records it per run and computes a per-benchmark band from the
spread across arms; movements inside a *measured* band are withdrawn from
`REGRESSED` under an `EXPLAINED BY CODE PLACEMENT` heading. Verified objectively
rather than by assertion: `python scripts/layout-sweep.py --self-test` builds
real kernels and checks that every one of 115,542 shared `.text` symbols moved by
exactly the pad, that malformed pads fail the build, and — as a negative
control — that a 4096-byte pad is *rejected* as a sample because it preserves
every page-straddle relationship. That control earned its keep immediately: it
caught a const-fold in the first draft that made the unpadded arm ~256 bytes
shorter in *code*, reintroducing the very confound the sweep exists to remove.

**The first sweep completed 2026-08-19**, on the fourth attempt: six arms at
pads 0/1024/1536/2048/2560/3072, all at commit `b36a244bb`, release profile,
Logoplex3, 4128 s. `python scripts/bench-history.py --layout-bands --profile
release` now prints a band for **86** benchmarks. Note the `--profile release`
— the default is `debug`, which still has no sweep and so still reports every
movement as `unmeasured`.

**What it measured, and why it is worse news than expected:**

| | band |
|---|---|
| max | **182.0%** (`heap_raw_alloc_free_4096`) |
| p90 | 107.3% |
| median | **26.0%** |
| p25 | 7.7% |
| min | 1.4% (`net_ns_arp_lookup`) |

| benchmarks with a band ≥ | count | share |
|---|---|---|
| 100% | 10 | 12% |
| 50% | 26 | 30% |
| 20% | 51 | 59% |
| **10%** | **61** | **71%** |
| 5% | 74 | 86% |

Performance-critical paths are not the well-behaved end of that distribution;
several are the worst of it: `pick_next` 132.0%, `page_alloc_free` 91.9%,
`page_fault` 84.9%, `ipc_channel` 82.8%, `io_ring_nop` 74.5%,
`syscall_dispatch` 41.9%, `isr_latency` 25.4%, `sched_pick_next` 20.1%,
`vfs_stat_root` 14.8%. Only `context_switch` (5.5%) and
`ipc_channel_roundtrip_64k` (4.2%) are quiet.

**The consequence, stated plainly: `CLAUDE.md`'s benchmarking protocol says
"if a change regresses a benchmark by more than 10%, investigate before
merging", and for 71% of the suite 10% is below the noise this emulator
manufactures from a relink alone.** Every number under that threshold on those
61 benchmarks was, in the strict sense, unfalsifiable — a "regression" and an
"improvement" of the same size are equally likely to be the linker. This is
now filed for the operator as `open-questions.md` A-Q6, because the threshold
lives in `CLAUDE.md`, which lane A may not edit on its own initiative.

Two caveats on reading the bands:

- **This is not run-to-run variance wearing a costume.** It is damped twice
  before it can be read as placement sensitivity: `layout_bands()` takes the
  median over repeats *within* each pad first, then divides each arm by its own
  `speed_factor` against the group's per-benchmark medians to remove host
  drift. An arm whose factor cannot be computed voids the entire group rather
  than falling back to an uncorrected 1.0 — because uncorrected drift *widens*
  the band, and a wider band dismisses more real movements.
- **A band is a lower bound.** Six sampled layouts cannot contain the worst
  pair among all of them, so a movement just outside its band is
  *unexplained*, not cleared.

  Four attempts on 2026-08-19 were needed, and each failure bought a guard, so
  the history is worth keeping rather than summarising as "it kept failing":

  | Attempt | Died | Cause | Guard added |
  |---|---|---|---|
  | 1 | arm 1, 1 s | `bash` resolved to WSL's, which cannot open the Windows path | — (misdiagnosed as MSYS; the "fix" only moved the error) |
  | 2 | arm 1, 1 s | same root cause, now showing as `qemu-system-x86_64 not found` — WSL's bash has no QEMU | `find_bash()`: each candidate must *parse the boot test* **and** *find QEMU and OVMF using the boot test's own search* |
  | 3 | arm 2, 43 min | the kernel's layout-pad self-test had been folded to a constant FAIL, so every padded release kernel halted at boot | `--self-test` claim 4: every branch of the on-target check must be present in the optimised image |
  | 4 | — completed, 4128 s | — | — |

  Attempts 1 and 2 are one root cause seen twice — the first "fix" addressed the
  message rather than the cause, which is why the second attempt failed two
  lines further down. The separate guard against a sweep that *runs* but
  produces no band (`check_arm_counts()`, which applies `bench-history.py`'s own
  `layout_arm_rejection()` to each arm's record as it lands) was added between
  attempts 2 and 3 and has never had to fire.

  Attempt 4's arm 2 is the pad that wedged attempt 3, and its passing is
  stronger evidence than the binary grep that fixed it: `main.rs` halts the CPU
  when the layout-pad self-test fails, so reaching `BENCH_OK` proves the
  success branch both exists *and* was taken.

**Both paths to a build failure now consult the band (2026-08-19).** The first
version taught only the run-over-run comparison about layout, leaving
`level_shifts()` — the *sustained*-shift check, wired into
`--fail-on-regression` independently — able to fail a build on a placement
artifact. The gap was in that function's own stated reasoning: "host disturbance
is random per run, while a code regression is in every run after the commit",
which is true and incomplete, because a layout artifact is *also* in every run
after the commit. `mode_structure()` does not cover it either — it needs the
benchmark to have been seen at both modes across binaries, so the first commit
exhibiting an artifact is `MODE_UNDECIDED` and fails. A sustained shift is now
excused on the same positive evidence as everywhere else, plus one precondition
specific to this path: `placement_is_constant()` blocks the excuse when every
run the shift is drawn from is provably one kernel image, since then the
addresses are identical throughout and layout is the one hypothesis ruled out by
arithmetic. See design-decisions.md §234, "Both paths to a build failure".

**Part (2) landed 2026-08-19, in the opposite direction to how it was written
above** — and the reversal is the whole reason it had stayed blocked. As stated,
(2) reads "label cross-image movements `MOVED (image changed)` *unless* the
changed files plausibly reach the benchmark". That phrasing needs proof of
**non**-reachability to withdraw a regression, and nothing available here can
supply it: a static map cannot see through helper functions, trait objects,
inlining or LTO, so "I found no path" is not "there is no path". Worse, it fails
in the unsafe direction — a missed path silently relabels a real regression as a
build artifact.

So the map is used one way only: **it may escalate, never dismiss.** A movement
is excused solely by the *measured* layout band, exactly as before; the map then
runs afterwards and, when the diff provably touches a subsystem the benchmark
demonstrably enters, prints a warning that placement and the diff are now both
live explanations and this measurement cannot separate them. Every uncertainty —
no map entry, a `git diff` that failed, an unparseable `bench.rs` — resolves to
silence, which changes no verdict. `benchmark_subsystems()` in
`scripts/bench-history.py` derives the map by matching `score("name", …)` call
sites in `kernel/src/bench.rs` against `mod::` paths in the surrounding function
and resolving those to files under `kernel/src/`; it deliberately under-covers
(67 of 86 benchmarks) and over-attributes (`vfs_stat_root` picks up `sync` and
`lockdep`), both of which are the safe direction for an escalate-only signal.
Wired into both excuse sites — the run-over-run `EXPLAINED BY CODE PLACEMENT`
loop and the sustained-shift path — and pinned by three tests in
`test-bench-history.py`, one of which asserts the one-directionality directly.
See design-decisions.md §235.

**A sweep now checks, arm by arm, that its arms count (2026-08-19).** Two sweep
attempts have been voided, and the second is the one that shaped this: it would
have built and booted six kernels over ~3 hours, printed six confirmations,
exited 0, and produced no band whatsoever — because the dirty-flag bug above
made every run after the first record itself as `dirty`, and `layout_arms()`
drops a `dirty` record. The only symptom would have been `--layout-bands`
printing nothing, hours later, with the cause a guess.

Note what every existing check verified: that the *run* succeeded. It built, it
booted, it printed the pad it was asked for. That was never the question.
`layout_arms()` has four further ways to refuse a record — `dirty`, a loaded
host, a missing commit, the wrong profile — and a completely successful run
satisfies them exactly as easily as a failed one does.

So `layout-sweep.py` now calls `check_arm_counts()` after every arm, which asks
`bench-history.py`'s `layout_arm_rejection()` whether the row just written will
be *counted*, and aborts naming the reason if not. That predicate is the one
`layout_arms()` itself applies — extracted, not copied. A copy would agree today
and drift on the next edit to either, after which it would certify every arm of
a sweep that yields no band, which is the outcome it exists to prevent; a
drifted copy is worse than no check. It also confirms the newest row is *this*
arm's before judging it, since otherwise a concurrent append would have the
verdict for one run reported as though it were about another.

There is deliberately no cheaper pre-flight. The rejection reasons are
properties of a record, and the record does not exist until a run produces it;
`dirty` in particular is computed by `boot-test.sh` with its own pathspec
exclusions, and predicting it in Python would be a third statement of that rule.
A pre-flight that disagrees with the real check is worse than none — it would
clear a sweep that then records six discarded arms, with a green pre-flight
standing as evidence the tree was fine. One arm (~20 min) is the price of one
statement per rule.

Until a sweep has actually been run, the original guidance stands: treat any
flagged movement in a benchmark whose subsystem the commit did not touch as
layout until shown otherwise — and check the *direction histogram* of all
deterministic movers first, because a mixed one is diagnostic on its own.

#### Further instance — 2026-08-19, the KASAN frame-hook change

A clean worked example of why (1) and (2) are worth the effort, and of how much
run time the absence of them costs. Two `--bench` runs of the *same* binary (the
frame-unpoison hook, which touches only `mm/kasan.rs`, `mm/frame.rs` and
`main.rs`):

- **Run 1 flagged 12 regressions** of +26% to +91% — `vfs_stat_3comp`,
  `syscall_dispatch`, `shm_create_close`, `net_ns_arp_lookup`, `ipc_pipe` and
  friends. It also invalidated its own calibration three separate ways in the
  same output: `scatter scale check: FAILED … this run's budget calibration is
  not a physical quantity`, `CONTAMINATED: reference access cost spread 55% over
  13 samples`, and `MEASUREMENT VOID` on two more. The regression section is
  printed anyway, above the notice that says not to believe it.
- **Run 2, same binary, no rebuild, calibration valid** (`scatter scale check:
  OK`, 19% of a 25% tolerance): 11 of the 12 withdrew, several inverting to
  `IMPROVED` — `shm_create_close` −46%, `syscall_dispatch` −40%,
  `net_ns_arp_lookup` −41%. A mixed direction histogram, exactly as the
  paragraph above predicts.

The one survivor, `pick_next` (+44%, own range 492–651 ns), was reported
`REGRESSED … replicated -- every recorded run of this commit shows it`. But
"replicated" here only means *both runs of one image agree*; the comparison is
still cross-image against `5e9a30a22`, which is precisely case (2). `pick_next`
allocates no frames — the only `frame::alloc_order` under `kernel/src/sched/` is
task-stack creation (`task.rs:1050`), not the pick path — so no edit in this
change reaches it. Layout, by the standing rule.

Two things this instance adds. First, **the `REGRESSED … replicated` wording is
actively misleading for a cross-image pair** and should be reworded along with
fix (1): a reader reasonably takes "replicated" to mean "confirmed as a code
effect", when it only rules out single-run noise. Second, and the reason (3)
matters more than it looks: **this host's own noise floor exceeds the effect
sizes being reported.** The harness says so itself — "Two runs of one unchanged
binary have been measured moving 85% apart" — and this pair demonstrated it live,
with `page_alloc_zeroed_free` spanning 3625→5420 ns (50%) across two runs of one
image. An A/B against a stashed build would therefore not have been decisive
either; it would have cost ~20 minutes of rebuild and boot to produce another
sample from a distribution wider than the signal. That is the actual argument for
(3): without a calibrated per-benchmark layout band there is no experiment
available at this noise level that can settle a 44% cross-image movement.

#### Partly fixed — 2026-08-19: parts (1) and (4) landed; (2) and (3) remain

`scripts/bench-history.py`:

- **(1) The A/A contradiction is gone.** When the baseline record is the same
  kernel image as this run, `report()` now emits **no verdict heading at all** --
  not `REGRESSED`, not `REGRESSED, UNREPLICATED`, not `UNCONFIRMED`, not
  `IMPROVED`, not `NOT REPLICATED`. The movements are still listed, under
  `A/A MOVEMENT (... this host's measurement noise, measured -- NOT a regression
  and NOT an improvement, in either direction)`, and the per-benchmark repeat
  samples are kept beneath each row, because "this binary produced 363 and 680 ns"
  is the one number an A/A pair exists to produce. Only the claim was dropped.
  Replayed against the documented 602fc62e0 pair in `bench/history.jsonl`, the
  four movements now read as a two-directional noise floor (+85%, +54%, +36%,
  -29%) instead of as two confirmed regressions.
- **(4) `REGRESSED ... replicated` no longer reads as "confirmed as a code
  effect".** The heading says "every recorded run of this same *kernel image*"
  (the gate is `same_image`; the old wording said "commit", which names a check
  this harness deliberately does not implement -- see `binary_identity`), and it
  is followed by an explicit statement of what replication does *not* rule out:
  the baseline is a different image, relinking re-rolls whether a hot loop
  straddles a guest page, that costs ~1.7x per iteration under TCG, and it
  reproduces every run. The note points at `scripts/straddle-check.py --compare`.
- **New, and not in the original three:** a **direction histogram**, printed when
  movement left the band in both directions in one comparison. This is the
  cheapest discriminator available and the only one that is evidence about the
  *set* rather than a row -- a code change moves what it touched in the direction
  it pushed, while relinking moves whatever lands badly in whichever direction it
  lands. It is an observation, not a verdict, because an optimisation commit
  legitimately produces a mixed histogram too; the falsifier is printed with it.

Tests: `scripts/test-bench-history.py` gained three cases -- every verdict
heading asserted absent by name under A/A (the bare word `REGRESSED` is a
substring of three headings, so a loose check would pass with two of them still
printing), the per-row repeat samples asserted still present, and the histogram
asserted to fire on a mixed comparison and stay silent on a one-directional one.
Discovery floor raised 40 -> 70.

**Still open:** (2) needs a benchmark-to-source reachability map before a
cross-image movement can be labelled `MOVED (image changed)` without deleting
the signal -- every non-A/A comparison is cross-image by definition, so the
label is only meaningful where the changed files demonstrably cannot reach the
benchmark. (3) is unchanged and is still the only fix that makes cross-commit
crypto/mm numbers trustworthy under TCG at all.

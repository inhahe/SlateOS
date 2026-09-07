# Open Questions — Operator Decision Queue

Decisions that genuinely need the human operator: architectural forks,
user-visible policies, and tradeoffs with no obviously-correct answer that
Claude has **deferred** rather than resolved autonomously.

This file is distinct from:

- **`design-decisions.md`** — decisions already *made* (each marked with who
  decided it). When the operator answers a question here, move it there as a
  `Decided by: Operator` entry and delete it from this file.
- **`known-issues.md`** — bugs and accumulated technical debt.
- **`todo.txt`** — the working scratchpad / judgment-call log.
- **`deferred-questions.md`** — questions that will need the operator *eventually*
  but cannot be answered usefully yet, each with a trigger for promoting it back
  here. Anything whose own text says "ask again later" belongs there, not here:
  this file is a queue, and a padded queue gets skimmed.

Format for each entry — **written for a reader who does not know the
subsystem**, because an entry the operator cannot decide from has failed no
matter how correct it is:

- **`In short:`** — 2–4 sentences, **no jargon**, opening every entry: what is
  wrong now, what a user would actually see, and what the choice is between. If
  a term of art seems unavoidable here, the paragraph is wrong — rewrite it.
- **Question** — the decision to be made, with every term of art glossed in-line
  on first use in ≤ 10 words, even if it is glossed in another entry. Assume
  nothing carries over: the operator reads one entry at a time, months apart.
- **Options** — each with pros, cons, and a one-line **`What changes:`** stated
  as an observable difference ("the clock reads Eastern instead of UTC"), not an
  implementation, so the options can be compared without reading the prose.
- **If never answered** — one line: is today's behaviour safe, is anything
  blocked, does it get worse with time.
- **Claude's recommendation** — if there is a defensible default (and what
  Claude is doing in the meantime).
- **Where it bites** — files/symbols affected, so the resolution can be applied.
- **Status** — `OPEN` until the operator decides.

Keep entries to what a *decision* needs. Detail that only matters after the
answer belongs in `known-issues.md` or the `requests/` file. Prefer a short
table to a paragraph and a concrete example to an abstraction. (The rule is in
`CLAUDE.md` → "Write `open-questions.md` for a reader who does not know the
subsystem".)

**The body of this file holds OPEN questions only.** When the operator answers
one: write it up in `design-decisions.md` as a `Decided by: Operator` entry,
**delete the entry from here**, and add one line to the `# Resolved` index at
the bottom under your own lane's subheading. An answered question left in the
body is pure clutter, and because it is older it sorts *first* — directly in
front of the questions that still need an answer, which is the one thing this
file exists to show. (This file is lane-*partitioned*, not append-only; the
reasoning is `design-decisions.md` §437 and the rule is `roadmap.md` →
"Three-Agent Parallel Execution" rule 3.)

New questions go at the end of the body, just above the `---` that precedes
the `# Resolved` index, numbered with your lane's prefix (`A-Q<n>`, `B-Q<n>`,
`C-Q<n>`). The unprefixed `Q<n>` numbers are pre-split and are not to be
extended.

**Read that last paragraph twice — it is the rule this file gets wrong.** "At
the end of the body" is not the end of the file, and appending to the end of
the file lands you *below* `# Resolved`, among the answered questions, where
the operator will never reach you. Three lanes have now done exactly that, and
eight entries had to be moved back. It is not carelessness: the end of the
file is simply where a text editor puts you, and the archive looks like the
place new things go because it is last.

`scripts/check-open-questions.py` enforces it — run it before you commit, or
let `scripts/boot-test.sh` run it for you. It **fails** the build on a question
filed below the boundary, on a body entry whose `Status:` is no longer `OPEN`,
and on two entries sharing an identifier while one is still open. It only
**warns** about a missing `C-Q<n>`-style identifier and about the two historic
duplicate numbers in the archive, both of which are another lane's text to fix
or history's to keep. Reasoning: `design-decisions.md` §903.

## Q46 — [A] Every benchmark ever recorded measured an `opt-level = 0` kernel. Should the *non-bench* boot test also switch to release, or only the bench path? — Status: OPEN (costs now measured 2026-08-21; recommendation moved A → C)

**Background.** `scripts/boot-test.sh:602` runs a bare `cargo build` and stages
`target/x86_64-unknown-none/debug/kernel`. The bench suite is compiled in
unconditionally — `--bench` only changes which serial marker is awaited — and
`[profile.dev]` has no kernel `opt-level` override, so all 5 records and all 63
benchmarks in `bench/history.jsonl` were measured unoptimised and scored
against `baselines.toml` targets drawn from optimised Linux/Fuchsia/L4
implementations. Full write-up:
`known-issues.md` → `B-BENCH-ENTIRE-SUITE-MEASURES-AN-UNOPTIMISED-KERNEL`.

**What is not in question.** That `--bench` must build `--release` is not a
tradeoff — a benchmark that does not measure the shipped build is not a
benchmark, and `[profile.release.package.kernel]` (`opt-level = 3`,
`codegen-units = 1`) already exists for exactly this. Claude is proceeding with
that plus an append-only `profile` field in each history record, so debug
records are never diffed against release ones. **The question is only about the
default, non-bench boot test.**

**Option A — leave the default boot test on debug (Claude's lean).**
- *For:* fast iteration on the ~405 s TCG cycle; readable panics and intact
  frame pointers when a boot fails; `--bench` already roughly doubles the cycle
  so the slow path is opt-in.
- *Against:* two kernel builds live in the tree, and release-only behaviour —
  miscompiles, UB that only manifests optimised, different timing and stack
  layout — is then exercised *only* on bench runs, which are the runs nobody
  reads for correctness. That is a real correctness gap, not just a tidiness
  one.

**Option B — build release everywhere; the boot test always tests what ships.**
- *For:* one binary, and the boot test's PASS then means the shipped kernel
  boots. Any release-only bug surfaces on every run rather than on bench runs.
- *Against:* slower rebuilds on every iteration, and degraded diagnostics
  exactly when a boot fails, which is when they matter most. `opt-level = 3`
  with `codegen-units = 1` on this kernel is not a cheap build.

**Option C — debug by default, plus a periodic release boot test.**
- *For:* keeps fast iteration and still exercises the release binary on a
  schedule.
- *Against:* another mode to maintain, and "periodic" needs a trigger nobody
  has defined; in practice it tends to mean "never".

**Recommendation: A, with the gap named rather than ignored** — the bench path
becomes the release path, and if a release-only defect ever shows up there it
promotes this to B immediately. Claude will not decide between A and B
unilaterally because B changes the default cost and diagnostics of every boot
test the other two lanes run, which is theirs to feel as much as Lane A's.

**Update 2026-08-15 — the common work is done, and it moved the tradeoff.**
The `--bench` → release change and the `profile` history field are in
(`880c3bfe5`, `c1806720b`). Two things changed since the options were written:

1. **`scripts/boot-test.sh --profile=debug|release` now exists**, decoupling the
   build profile from the serial marker being awaited. So "run a release boot
   test" is one flag, on any run, by any lane. **Option C's only real objection
   — "another mode to maintain" — is gone; the mode is already built and
   tested.** What C still lacks is a *trigger*, which remains the honest
   objection to it.
2. **Release is not the slow build the options assumed it would be at the boot
   level.** Measured this session on the full bench suite: release QEMU window
   142 s vs debug 615 s. The release *build* is slower, but the release *boot*
   is ~4× faster because the kernel executes ~40× fewer instructions under TCG.
   Option B's "*Against: slower rebuilds on every iteration*" is real, but its
   implied "slower boot tests" is backwards — B would make the run-time half of
   every cycle substantially quicker.

Neither point decides A vs B; both are still cost claims and B still changes
what the other two lanes feel on every boot. But the question is now cheaper to
answer either way, and if the answer is C, it is already implemented and needs
only a trigger (Claude's suggestion for one, if C is chosen: a release boot test
before any lane merges to `main`, since that is already the moment a lane runs
the slow verification anyway).

**Update 2026-08-21 — the cost was finally measured, and it reverses the
2026-08-15 reading.** Until today the "slower build" half of this tradeoff had
never been measured anywhere: build time was not recorded, so the entry argued
from one measured half (the boot) and one asserted half (the build). Build
timing now exists (`build_seconds` in `bench/boot-history.jsonl`), and four
matched runs on one commit (`8b481b0f2`, QEMU TCG, no sanitizer) fill the 2×2:

| what was edited | debug build | debug boot | **debug cycle** | release build | release boot | **release cycle** |
|---|---|---|---|---|---|---|
| `posix` + `kernel` | 224 s | 401 s | **625 s** | 714 s | 130 s | **844 s** |
| `kernel` only | 42 s | 359 s | **401 s** | 594 s | 105 s | **699 s** |

The boot half is 3.1–3.4× faster under release, exactly as claimed on
2026-08-15. **But the cycle — which is what a person actually waits through —
is 1.35× *worse* on a two-crate edit and 1.74× worse on a kernel-only one.**
So this sentence from the 2026-08-15 update, while literally true, argued the
wrong way and is hereby withdrawn as an argument for B:

> "Option B's '*Against: slower rebuilds on every iteration*' is real, but its
> implied 'slower boot tests' is backwards — B would make the run-time half of
> every cycle substantially quicker."

It does speed up the run-time half. The run-time half is the *smaller* half
under release, and the half it slows down is slowed by more.

**The number that decides it is 42 s → 594 s.** The two-crate row understates
the penalty at 3.2×; the kernel-only row — the common case, since almost every
iteration edits the kernel and nothing else — is **14×**. A release cycle after
a one-line kernel edit is ~11½ minutes against debug's ~6½, and the extra five
minutes are all compiler, with nothing on screen.

**The obvious escape route was tried and is closed.** That 14× is mostly
`codegen-units = 1` in `[profile.release.package.kernel]`: one codegen unit
means a one-line edit recompiles the whole crate as a single non-parallelisable
unit. "Build release, but with 16 units" would have bought release-only bug
coverage at a fraction of the cost — except the kernel **does not assemble** at
`codegen-units = 16`; it fails after 174 s in `alternative_site!`'s
assembly-time guard (`error: expected absolute expression`). Same tree, same
toolchain, same command, only the unit count differs. Written up as
`known-issues.md` → *The release kernel does not assemble at `codegen-units` >
1*. Until that is understood, "cheap release" is not on the menu, and the
choice really is between the two columns above.

*What changes, restated as observable differences:*
- **A:** `./scripts/boot-test.sh` keeps taking ~400 s after a kernel edit and
  keeps printing readable panics; the shipped (optimised) kernel is only ever
  booted on `--bench` runs.
- **B:** every boot test after a kernel edit takes ~700 s instead of ~400 s —
  five extra minutes of silent compiling per iteration, for every lane, not
  just A — and a panic prints optimised, harder-to-read frames. In exchange,
  every run tests the binary that ships.
- **C:** as A day-to-day, plus one ~700 s release boot at merge time.

*Recommendation after measuring: **C**, which the 2026-08-21 numbers promote
above A.* The measurement did not change what the options *are*, but it changed
which one is cheapest for what it buys. B now has a price tag nobody would pay
per-iteration — five silent extra minutes on every kernel edit, for all three
lanes. C pays that same price **once per merge**, at the moment a lane is
already running slow verification and already waiting, and buys exactly the
thing A gives up: a release-only defect surfaces on a run somebody reads for
correctness. The 2026-08-15 objection to C ("another mode to maintain") was
already gone — `--profile=release` exists and is now exercised — and its
remaining objection, the missing trigger, has an obvious answer: **a release
boot test before a lane merges to `main`.** Claude still will not choose
unilaterally, because B and C both change what the other two lanes must run.

*If never answered:* current behaviour (A) is safe and nothing is blocked — the
gap is that release-only defects surface only on bench runs. It does not get
worse with time, but it does get *more* likely to matter as more kernel code
lands unexercised in optimised form. One thing did get slightly worse today:
release boots are now known to be cheap to *run* (105–130 s) and expensive to
*build*, so the temptation to reach for B on the strength of the boot figure
alone is real, and this entry exists partly to stop that.

---

## Q47 — [A] The `D:` drive filled to 0 bytes free and destroyed a source file. Should the three lanes share one build-output directory? — Status: OPEN (narrowed — C is done; the question is now only A vs B)

**In short:** The drive the project lives on ran completely out of space today.
An edit that was half-written when the space ran out left one kernel source
file **empty** — 18 KB of code replaced by nothing. It was recovered from git in
under a minute because it happened to be already committed, but five other files
being edited at the same moment were *not* committed and would have been gone
for good. The space is going to compiler output: three parallel agents each keep
their own copy of every compiled artefact, and deleting just one agent's copy
freed **13 GB**. The question is whether the three should share one output
directory (much less disk, but they would have to take turns compiling) or keep
their own (fast, independent, and this happens again).

**Terms:** a *build-output directory* (`target/`) is where the compiler puts
everything it produces — object files, libraries, the kernel image. It is
entirely regenerable: deleting it costs a rebuild, never source. Rust's build
tool locks that directory, so two builds sharing one **queue** rather than run
at once.

| Option | *What changes:* | Cost |
|---|---|---|
| **A — Share one directory** (`CARGO_TARGET_DIR` set to a single path for all three lanes) | Roughly a quarter of the disk footprint; a lane that starts a build while another is compiling **waits** instead of proceeding | Lanes serialise on the build lock. Wall-clock per lane goes up whenever two build at once |
| **B — Keep separate directories, add pruning** | Nothing changes day to day, except a scheduled/opportunistic `cargo clean` on lanes that have been idle | Keeps parallel builds, but the pruning has to be remembered, and "idle" is a guess |
| **C — Keep separate, and add a free-space floor to the tooling** | `boot-test.sh` and the test runner refuse to start below (say) 20 GB free and say why | Does not free anything; converts a corrupting failure into an honest refusal |
| **D — Move the build output off `D:` entirely** | Compiler output goes to another volume; `D:` holds only source and the operator's data | Needs a volume with tens of GB free — operator knows whether one exists; also slower if that volume is slower |

**Measured 2026-08-15, a few hours after the incident** (so you can size the
options rather than guess at them):

| Where | Build output |
|---|---|
| `os` (the integration checkout) | 59.1 GB |
| `os-lane-b` | 40.4 GB |
| `os-lane-c` | 35.0 GB |
| `os-lane-a` | 3.5 GB — small only because it was deleted today to recover |
| **total** | **138 GB** |
| free on `D:` right now | **41 GB (2% of a 1.9 TB drive)** |

Two things this makes concrete. First, the footprint is dominated by the
**integration checkout**, which nobody actively builds in — it is the largest
single consumer at 59 GB and the cheapest to reclaim, which makes B better than
it looks on paper. Second, 41 GB free is *less* than a single full rebuild of
all four trees would need, so the current margin is one careless afternoon wide.

**Claude's recommendation: C now (it is Lane A's to do unilaterally and is
strictly protective), plus A if you are willing to trade build parallelism.**
A's serialisation is arguably a *bonus* rather than a cost here: concurrent lane
builds are already the single largest source of the benchmark contamination
documented throughout `known-issues.md`, so forcing the lanes to take turns
would make the performance numbers more trustworthy, not less. But that is a
real change to how all three agents work, which is why it is not being made
unilaterally.

**Option C is DONE (2026-08-15, lane A) — you are no longer choosing whether to
have a safety net, only how to pay for the space.** `scripts/boot-test.sh` now
refuses to build or stage below **20 GiB** free, naming the incident and telling
you which worktree to prune. Override per run with `--min-free-gb=N`, or
`BOOT_TEST_MIN_FREE_GB=N` (0 disables). It is checked twice — before the build
and again before staging — because the build is itself what consumes the margin,
and it is staging a partial ~200 MiB kernel image that produces the
boots-a-stale-kernel failure. If `df` cannot produce a number it prints a warning
saying the floor is *not* being enforced, rather than skipping silently: a check
that cannot run must not look like a check that passed.

This does not free a single byte — it converts a corrupting failure into an
honest refusal, which is why it did not need your decision. **A vs B still does.**

**Also worth re-measuring before you decide:** free space on `D:` is **91 GiB**
as of this update, up from the 41 GiB in the table above, because the other lanes
pruned during the day. So the immediate emergency is over and the choice can be
made on its merits rather than under pressure.

### 2026-08-18 — the floor fired for real, and we now know the refill rate

Lane B's boot test was refused at 13 GiB free
(`requests/b-a-q47-floor-fired-for-real-and-here-is-the-refill-rate.md`). That is
option C working as designed, for the first time: it cost one command instead of
a truncated file. **Nothing is broken; this is the safety net doing its job.**

What it adds to the decision is a *rate*, which the question was missing:

| Date | Free on `D:` |
|---|---|
| 2026-08-15 | 0 GiB — the incident |
| 2026-08-15 (later) | 41 GiB — after the emergency prune |
| ~2026-08-16 | 91 GiB — "the emergency is over" |
| **2026-08-18** | **13 GiB** — floor fires |

**~78 GiB consumed in about two days.** So the margin a prune buys is roughly a
**two-to-three-day** margin at three-lane pace — the same order as one
rate-limit window.

That is the number that prices option B. B's cost was written above as "the
pruning has to be remembered"; it can now be stated concretely as **a chore that
recurs every two to three days, with no owner, landing on whichever lane happens
to trip the floor first while it is in the middle of something else.** That is
what happened to Lane B today.

One thing does move in B's favour, though, and it is worth weighing against the
above: the reclaim is **cheap, safe and well-targeted**. `cargo clean` on the
integration checkout freed 13 GiB → 32 GiB in a single command, and that tree is
regenerable output that nobody develops in. So B is not "prune something you
might still need"; it is "prune the merge tree", which is a rule that can be
written down rather than remembered. Re-measured sizes, which also update the
table above:

| Where | `target/` |
|---|---|
| `os` (integration checkout) | 21.4 GiB |
| `os-lane-a` | 27.0 GiB |

The shape from 2026-08-15 holds: the checkout nobody develops in is a large
share of the footprint and the cheapest thing to reclaim.

**This does not change the recommendation, and it does not decide A vs B.** It
means that if you pick B, it should be picked *with* an automated trigger rather
than as a habit — see the `--prune-integration-target` note under "If never
answered" below.

**If never answered:** the disk fills again every two to three days — but it now
announces itself as a refused boot test rather than as a truncated source file,
and the 2026-08-18 firing shows the refusal costs about one command to clear.
Note the floor protects the *harness* only: a `cargo build` you run by hand, or
an editor writing a file, is still unguarded, so this reduces the blast radius
without removing it.

So the honest answer to "what if you never decide" is now: **it keeps working,
at a cost of one interruption per lane per few days.** That is a real tax but
not a rising one, which is why this question is not urgent even though it fires
regularly.

Lane A has since closed the gap that made that interruption expensive. The
remedy already existed — `scripts/reclaim-space.py`, which frees space by
*renaming* a directory before deleting it (Windows refuses to rename a
directory with an open file inside, so a successful rename is proof nothing was
using it, rather than a timestamp guess) — but `boot-test.sh` did not name it.
It advised a manual `cargo clean`, which is why Lane B cleaned by hand. The
floor now names the tool and accepts `--reclaim-space` to run it and retry.
That reduces B's cost but deliberately does **not** pick B: it is opt-in per
run and changes nothing unless asked for.

### 2026-08-18, later — what option B *actually* costs a lane, and why it is now smaller

Lane B ran the tool for real and measured the thing this entry had been pricing
by assumption
(`requests/b-a-reclaim-space-crashes-on-every-real-run-and-strands-the-tree.md`).
Their finding, which is the more consequential half of that file:

> With `os/target` already cleaned and the other two lanes' trees off-limits at
> the defaults, **the only candidate the script can offer this lane is its own
> `target/`.**

That is worth stating plainly, because it changes B's price. Above, B's cost is
written as "a chore that recurs every two to three days" — a chore being an
*interruption*. But if the only tree a lane may reclaim is its own, the recurring
cost is not one command; it is **a full cold rebuild for whichever lane trips the
floor**, every two or three days. That is a materially worse number than this
entry has been carrying, and it was a structural property of the defaults, not an
accident: the ordering was `[integration checkout, our own]`, with *every* other
worktree — live lane tree and dead scratch checkout alike — behind
`--allow-lane-targets`.

**Lane A has since fixed the part of that which was ours to fix.** Lumping those
two together was wrong: `CLAUDE.md` blesses exactly four worktrees (`os`,
`os-lane-a/b/c`), so a checkout on any other branch — or on none, which is what
`git worktree add <path> <commit>` produces and therefore what every bisect tree
here is — belongs to nobody, and its `target/` costs no one a rebuild they were
going to run. `reclaim-space.py` now classifies worktrees **by branch** and
attacks unowned scratch trees *first*, ahead of the integration checkout and well
ahead of our own. Live lane trees stay exactly where they were, behind the flag.
A tree that is mid-build is still protected by the existing rename veto.

Measured in this worktree today, in precisely lane B's situation (`os/target`
already clean):

```
Step 2: target/ directories, unowned scratch trees first
  candidate  …\os-bisect-a\target            [no lane owns it]
  candidate  …\os-straddle-scratch\target    [no lane owns it]
  candidate  …\os-lane-a\target              [this lane -- ours to pay]
```

Two candidates now precede the lane's own tree where before there were none.

**Honesty about the size of that win: today it is small.** Those two scratch
trees hold 76 MB and 75 MB — they have been pruned since they were built, so they
would not have saved lane B this morning. What changed is structural, not
numeric: the class exists, it is taken by default, and it is where a dead bisect
checkout's build output lands (`os-bisect-a` held a full kernel build when it was
created). The next lane to trip the floor with a live scratch tree around pays
nothing instead of paying a rebuild.

**Net effect on the decision: B is cheaper than the paragraph above priced it,
but not free, and the residual cost is exactly what lane B named.** Once scratch
trees are exhausted, a lane still faces its own `target/` and nobody else's. That
is deliberate — spending our own before a neighbour's is the only ordering that
cannot be read as helping ourselves at their expense — but it means B's
steady-state cost, in the worst case, remains one cold rebuild per floor-trip.
Option A (one shared `target/`) does not have that cost at all, because there is
only one tree to prune and no question of whose it is. **That is the sharpest
argument for A that has been made in this entry, and it came from a measurement
rather than from reasoning.**

### 2026-08-21 — the operator asked "why not b *and* c?"

Recorded here because it was answered in conversation and would otherwise exist
only in a transcript, and because the question exposes a defect in how this entry
was written rather than a gap in the reasoning.

**The answer is: you can have both, and you already do.** The four options were
laid out as a table, which reads as a menu you pick *one* row from — but only A
and B are mutually exclusive. C is not an alternative to either; it is a guard
that sits in front of whichever of them you choose:

| | A — one shared `target/` | B — separate + pruning |
|---|---|---|
| **without C** | disk can still reach 0 via a hand-run `cargo build` | same, plus the recurring pruning chore |
| **with C** (shipped) | harness refuses below 20 GiB and names the tree to prune | harness refuses below 20 GiB and names the tree to prune |

C was implemented unilaterally on 2026-08-15 precisely *because* it composes with
everything: it frees no space and changes no workflow, it only converts a
corrupting failure into an honest refusal. So **B+C is what is running today**,
and has been since that date. Choosing A would leave C exactly as it is.

**What this changes about the question:** nothing about the tradeoff — but the
framing was misleading, which is what invited the question. The status line
already said "C is done; the question is now only A vs B," while the option table
went on presenting all four as peers. The live choice is one row: **share one
build directory, or keep three and keep pruning.** C stays either way, and D is
orthogonal too — it asks *where* the output lives, not *how many copies* there
are, so it composes with A and B just as C does.

### 2026-09-02 — option A's only stated cost has now been measured, and it is much smaller than the table implies

Nothing about the disk-space side has changed (**93 GiB free of 1.9 TB, 95%
used**, measured today — the margin the August updates left is holding). This
update is about the *other* column. Option A's cost is stated as "Lanes
serialise on the build lock. Wall-clock per lane goes up whenever two build at
once." That sentence assumes the thing worth checking: that two lanes building
at once today are actually getting two lanes' worth of work done.

**They are not.** Measured today with lane A and lane B each running a boot
test:

| | uncontended | with a second lane building | ratio |
|---|---|---|---|
| read 6441 `.rs` files (`check-variant-lists.py`'s scan) | ~3 s | **368.5 s** | ~120× |
| mean per file | <1 ms | 57 ms | — |
| slowest single file | — | 0.86 s | — |

There is no outlier and no pathological input: *every* read is uniformly two
orders of magnitude slower. The disk is saturated, so the second lane is not
running alongside the first so much as taking turns with it at a much worse
exchange rate than a lock would give.

**And it now destroys runs, not just measurements.** The August argument for A
was that concurrency contaminates benchmark numbers. Today it killed a lane-A
boot test outright: the run hit its 1800s budget having **never reached QEMU**,
because the pre-build static gates alone consumed all of it. No fork failed, the
commit-limit floor never tripped, and every gate that ran passed. Both lanes now
have to budget 7200s for a job that takes ~8 minutes alone.

*What changes if you pick A:* lanes queue explicitly and each build runs at full
speed, instead of overlapping and each running at a fraction of it. The wall
clock the table lists as A's cost is largely already being paid under B — just
without the queue, the predictability, or the ~100 GiB.

**This does not settle the question,** because it measures the *disk*, not the
build lock: cargo's lock serialises at a coarser grain than the disk contention
does, so A could still idle a lane that would otherwise be doing non-disk work.
It does mean the table's cost column overstates what A gives up, and the
recommendation above ("A's serialisation is arguably a bonus rather than a
cost") now has a number behind it rather than only an argument.

**If it is never answered:** B+C keeps running and stays safe on disk — C's
floor is what makes that true and it is not going away. What continues to
degrade is throughput and trust in timings: every boot test either takes 4-8×
longer than it should or has to be re-run with a bigger budget, and no benchmark
taken while another lane builds is worth recording.

## Q56 — [A] A program compiled for Linux is exempt from the file-permission checks our own programs must pass. Close the gap, or write it down as the price of running Linux software? — Status: OPEN

**In short:** When a program asks this system a question about a file — "how big
is it?", "when was it changed?" — our own programs are required to hold a
*permission token* for that, and are refused if they don't. Programs built for
Linux, which we also run, are never asked: the same question through the Linux
door is answered without any check at all. So the same program, doing the same
thing to the same file, is policed or not policed depending on which door it
came in by. The question is whether to start policing the Linux door too, or to
accept that it is unpoliced and say so plainly somewhere.

**How this surfaced:** it is not theoretical. A test that runs GNU `make` was
written when `make` came in by the Linux door, and was given the tokens a Linux
program needs. The build later switched to a `make` compiled for *our* system,
which comes in by the other door — and the very same test started failing,
because now the checks applied and one token was missing. It is fixed (the test
grants the token now), but the fact that recompiling a program changed what it
was allowed to do is the thing worth deciding about.

**Glossary:** *capability / permission token* — a thing a process must be handed
in order to do something, rather than being allowed because of who it is.
*ambient authority* — permission you get by *being* you, with no token to hold
or hand over; what Linux uses, and what this project's design says it does not
want. *ABI* — the convention a compiled program uses to call the system; we
support two, ours and Linux's, and a program picks one when it is compiled.
*`stat`* — the call that asks a file's size, times and mode.

**The two doors, concretely:** our own `stat` requires a `File` capability
carrying the `METADATA` right (8 call sites in `handlers.rs`). The Linux
translation layer checks a `File` capability for `open` and for the mutating
`*at` calls, and for nothing else — `stat`, `lstat`, `statx`, `readlink`,
`statvfs` and the xattr readers all go straight through to the VFS (2
`require_cap_type` sites in the whole of `linux.rs`).

| Option | *What changes* |
|---|---|
| **A. Enforce parity** — the Linux layer checks the same rights ours does | A ported Linux program not given a `METADATA` token can no longer `stat`. Every launch site must grant it, and any we miss fails with "permission denied" on a call the program has no reason to expect can fail. Blast radius today: ~50 Path-Z tests, plus dash, tcc, python and `ld.so` |
| **B. Leave it, and document it** — declare the Linux ABI a lower-assurance compatibility surface | No behaviour changes. We write down, in `design.txt` and the Linux layer's module doc, that a Linux-ABI process holds ambient filesystem authority — so "capability-based security from day one" is true of native programs only |
| **C. Draw the boundary deliberately** — keep today's behaviour, but state it as a rule and test it | Same behaviour as B, except the line is explicit and checkable: a newly added Linux syscall that only *reads* metadata is documented as needing no check, so nobody adds one inconsistently and nobody re-investigates this from scratch |

**My recommendation: C, and keep A available.** A is what the design spec's "no
ambient authority" line implies, and I do not think it can be paid for today —
the entire value of the Linux ABI is running binaries nobody built for us, and
those binaries assume ambient authority by construction. B is honest but leaves
the boundary undrawn, which is how it drifts. C costs a paragraph and a test.

**If this is never answered:** nothing breaks and nothing degrades on its own —
but the inconsistency is a trap that has already cost one cross-lane
investigation (lane B correctly ruled out the capability grant, because for the
ABI they had in mind it genuinely was not checked), and it will cost another
the next time a binary is rebuilt for the other ABI. Meanwhile the design spec
claims something about this system that is true of only half of it.

**Where it bites:** `kernel/src/syscall/linux.rs` (the two `require_cap_type`
sites), `kernel/src/syscall/handlers.rs:8365+` (the native gates),
`kernel/src/cap/rights.rs` (`Rights::METADATA`).


## B-Q8 — [B] Two of the programs we copy disagree about how wide 626 characters are. Which one do we copy? — Status: OPEN

**In short:** Text on a terminal is laid out in fixed cells, and every program
that lines things up in columns has to agree on how many cells each character
takes — a Chinese character takes two, an accent mark that sits on the previous
letter takes none, most things take one. We keep one table of those numbers and
every one of our programs reads it. The trouble is that the two programs we
copy from — the shell **bash** and the **GNU command-line tools** — disagree
with each other about 626 characters, and we can only match one of them. Today
we match bash. Matching bash means our `ls` puts a filename in the wrong column
for those characters; matching the GNU tools means our shell's menus do.

**Why we cannot just measure what we draw (the operator's question, 2026-09-07,
and it turned out to be the good one).** The natural answer is "have the table
report how wide *we* actually print each character, and have every program ask
it" — and that is half-true already: our programs do all ask one table. Two
things stop it from settling the question, and the second is a genuine gap
nobody had written down:

1. **A program cannot ask the terminal.** There is no query for "how wide will
   you draw this?" The only way to find out is to print it and ask where the
   cursor ended up — a round-trip per character, over a link that may be a
   network, and impossible when the output is a file or a pipe, which is where
   `ls` and `wc -L` also decide their columns. So every implementation
   everywhere embeds a static table and hopes it matches the terminal.
2. **Our own terminal does not consult our table.** `userspace/charwidth` is
   depended on by exactly two crates — `userspace/coreutils` and
   `userspace/oils`. The GUI terminal that actually draws the glyphs (lane C)
   is not one of them. So "how wide we actually print" is currently decided by
   the renderer's font advance, independently of the table that every layout
   decision is made from. They have never been checked against each other.

### The question

Our table lives in `userspace/charwidth` and is the only such table in the
system — deliberately, because `ls`, `wc -L` (longest-line), `expand`, `fold`,
`nl`, `column` and the shell's `select` menu all draw onto the *same* screen,
so two of them disagreeing is not a difference of opinion, it is a crooked
screen. The table was built to match bash 5.2.37 and was checked against it at
1701 places, so today it is bash's answer.

On Linux, though, bash and the GNU tools do not get their numbers from the same
place. bash asks the C library (glibc). GNU coreutils 9.5 ships its own table
(from the "gnulib" support library) and **deliberately overrides the C
library's** in any UTF-8 setting — its own source comment says the system's
answer is not Unicode-aware enough. Coreutils 9.4 did not do this; 9.5 does.
Measured here, exhaustively over all 1.1 million characters, the two tables
disagree on **626 characters in 71 stretches**. Examples:

| Character | bash / glibc | GNU 9.5 / gnulib | Why they differ |
|---|---|---|---|
| U+00AD soft hyphen (an invisible "you may break the word here" mark) | 1 cell | 0 cells | A rule disagreement: gnulib gives *every* invisible formatting mark 0; glibc makes this one an exception |
| U+D7B0–U+D7FB (extra Korean vowel/consonant pieces that fuse onto the letter before them) | 1 cell | 0 cells | Same rule disagreement, applied to a newer Korean block |
| U+0600–U+0605 (Arabic marks printed *before* the number they belong to) | 0 cells | 1 cell | gnulib carves these out because they really do occupy a cell |
| U+1F203, U+1FA75, U+4DC0–U+4DFF, … | varies | varies | Different Unicode releases; and gnulib rounds *unassigned* characters inside East-Asian blocks up to 2 cells, we do not |

This is visible today: our `ls`-versus-GNU byte-diff harness has two cases that
differ for exactly this reason and no other.

### Options

**(a) Follow the GNU tools (gnulib's table).** Regenerate `charwidth` from the
reference implementation itself — we already have an exact dump of all 1.1
million answers, taken by calling GNU 9.5's own width routine.
*What changes:* our `ls` and `wc -L` line up with GNU's byte for byte on those
626 characters; our shell's `select` menu stops lining up with bash's on them.
- **Pro:** matches the six utilities that consult a width at all (`ls`, `wc -L`,
  and `sort`, `pr`, `df`, `numfmt` when we write them) against one shell.
- **Pro:** it is a *named, pinned* source — Unicode 15.1.0, one file, and we can
  re-dump it at will. Our present table came from whatever Unicode version the
  Python on the build machine happened to ship.
- **Con:** it breaks a passing test. `userspace/oils/tests/gen_display_width.py
  --diff-osh` compares our shell's menu against real bash at every table edge
  and currently agrees everywhere; it would start reporting 626 disagreements.
- **Con:** gnulib's table is *newer*, not *agreed*. Terminals have their own
  tables too, and nothing says gnulib's matches the terminal we will ship.

**(b) Keep bash's table (what we do today).**
*What changes:* nothing observable; our `ls` keeps putting those 626 characters
one cell off from GNU's.
- **Pro:** no change, and the one end-to-end byte-diff we have that involves a
  human-visible layout (shell menu vs bash) keeps passing.
- **Con:** every `ls` case containing one of those characters stays permanently
  marked "differs on purpose" in the harness, which dulls the harness.
- **Con:** we are copying the *older* of the two answers on the characters where
  they differ for a Unicode-version reason.

**(c) Two tables — the shell reads one, the utilities the other.**
*What changes:* both byte-diffs pass; the shell and `ls` can disagree by one
cell about the same filename on the same screen.
- **Pro:** maximum fidelity to both upstreams.
- **Con:** this is precisely the thing `charwidth` exists to prevent, and the
  symptom (a menu and a listing that do not line up) is the one a user actually
  sees. I do not recommend it.

**(d) Make the table describe our own renderer, and pin both to one source.**
Derive `charwidth` from the width the GUI terminal actually advances by, so the
table is a *description* of what we draw rather than a *prediction* of what
someone else draws; both upstream harnesses then show the 626 as deliberate
differences from *both* references.
*What changes:* our screens are internally correct by construction — the shell
menu and `ls` line up with each other and with the glyphs, which is the only
thing a user of SlateOS can actually see. Both byte-diff harnesses gain
permanent expected-difference lists.
- **Pro:** it is the only option whose correctness does not depend on a third
  party. bash and gnulib are both *guessing* at the terminal; we do not have to.
- **Pro:** it answers the question the other three cannot — which of the two is
  right *here* — because on SlateOS neither is authoritative.
- **Con:** it is cross-lane. The renderer is lane C's; the table is lane B's.
  It needs an agreed interface (a shared width source, or a generated table
  checked by a gate on both sides) rather than one lane editing the other.
- **Con:** it gives up byte-fidelity to *both* upstreams on those 626, so
  neither harness can be read as pass/fail on them again.
- **Unknown until measured:** whether the renderer and the table agree today.
  Nobody has compared them; the answer decides whether (d) is a change or
  merely a written-down invariant.

### If never answered

Safe, and it does not get worse on its own. Today's behaviour is (b). The cost
is confined to two permanently-deferred cases in the `ls` harness and to the
626 characters themselves, which are mostly invisible marks and unassigned
code points — nobody has a filename made of them by accident.

### Claude's recommendation

**(a)**, but not strongly enough to do it without you: the deciding fact for me
is that the count is six utilities to one shell and that gnulib's table is a
pinned upstream we can re-derive mechanically, whereas ours is not. What stops
me from just doing it is that it silently changes the on-screen layout of the
shell and five other programs to win a byte-diff in one — a user-visible
behaviour change, which is yours. Meanwhile I have kept (b) and isolated the
divergence in the harness (fixture `y/`) so it costs two cases and not twenty.

### Where it bites

`userspace/charwidth/src/lib.rs` (`ZERO_WIDTH`, `WIDE`, and the doc comment
that says the tables were measured against bash);
`userspace/oils/tests/gen_display_width.py` (the generator and its `--check` /
`--diff-osh` measurement against bash); `userspace/oils/src/width.rs`;
`userspace/coreutils/src/bin/ls.rs` and `wc.rs`; `userspace/column/src/main.rs`;
`scripts/ls-diff.sh` (fixture `y/`, two `!` cases);
`known-issues.md` → `TD-B-OUR-WIDTH-TABLE-IS-BASHS-AND-COREUTILS-9.5S-IS-NOT`.

---

## Q57 — [A] Should a program be able to pop up a prompt asking you for permission to read the keyboard, the microphone or the camera? — Status: OPEN

**In short:** SlateOS has a mechanism where a program that lacks permission for
something can ask *you* for it — the system shows the program's stated reason and
you say yes or no. This is the familiar "SomeApp would like to use your
microphone" prompt. Right now that mechanism only covers fifteen kinds of
permission, all of them internal plumbing (pipes, timers, processes), and it
covers **none** of the ones a user would actually recognise: keyboard input,
sound recording, the graphics card, raw network access, setting the clock. Those
were all added later and the list was never extended. So today the answer is
accidentally "no prompts for anything you'd care about" — permission for those
has to be handed out when a program is launched, by whoever launches it, with no
way to ask later. The question is whether that accident should become the rule,
or be fixed.

**How this surfaced:** the keyboard and mouse became readable devices today
(`/dev/input/event0`, `event1`), gated on a new permission type. Checking whether
a program could obtain that permission at run time turned up the fifteen-entry
list, which stops at 15; the keyboard is 30, so the request is refused with
"invalid argument" — not "denied", which would at least be an honest answer.

**Glossary:** *capability / permission token* — a thing a process must hold to do
something, rather than being allowed because of who it is. *resource type* — the
kind of thing a token is about (a file, a pipe, the keyboard); each has a number.
*grant at spawn* — the launcher hands the token over at start-up; the only route
that works today for the newer types. *instance type* vs *class type* — some
tokens name one specific already-open thing (this pipe, this socket), others name
a whole capability (any keyboard, raw networking). Only the second kind makes
sense to ask a human about — "may I have a pipe?" is not a question a person can
answer.

**The list, concretely:** `sys_cap_request` (`kernel/src/syscall/handlers.rs:6181`)
matches resource types 1–15 by hand and returns `InvalidArgument` for anything
else. Types 16–30 exist. Most of 16–21 and 25–26 are instance types and belong
out of the list on the merits. But **23 `Drm` (the graphics card), 22 `AlsaPcm`
(sound), 24 `NetRaw` (raw network), 27 `SystemClock` (setting the time), 28
`PrivilegedPort`, 29 `ResourceLimit` and 30 `InputDevice` (the keyboard)** are
exactly the human-recognisable ones, and all seven are unreachable.

| Option | *What changes* |
|---|---|
| **A. Extend the list to every class type** | A program with no keyboard permission can put a prompt on your screen saying why it wants one, and you decide. The seven types above become requestable. Also means a hostile program can *ask* for keylogging — the defence is that you see the request and the reason, which is exactly what the mechanism is for |
| **B. Extend it to the tame ones only** — sound, clock, ports, limits — and keep keyboard/graphics/raw-network grant-only | Prompts appear for the things where a wrong yes is recoverable; the three where a wrong yes is a total compromise stay launcher-only, so no program can ever ask you for them |
| **C. Leave it, and say so** — the request mechanism is for the original fifteen; everything newer is grant-at-launch | No behaviour changes. We write down that the newer permissions are deliberately not requestable, and fix the error so a refused request says "not requestable" instead of "invalid argument" |

**My recommendation: A, with the error message fixed regardless.** The whole
point of a consent prompt is that it covers the things worth consenting to; a
prompt system that can ask about pipes but not about the microphone has it
exactly backwards. The "hostile program can ask" objection applies equally to
every phone and desktop OS and is answered the same way — you are shown who is
asking and what for, and saying no is free. B's line looks principled but is hard
to hold: the moment a screen reader legitimately needs keyboard access, B has no
route for it either.

Independently of which option wins, `InvalidArgument` for a well-formed request
about a real resource type is wrong and misleading, and lane A will fix that to a
distinct error either way.

**If this is never answered:** nothing breaks. Every newer permission continues
to be handed out at launch by init, which works — this is how the compositor will
get keyboard access. The cost is that the consent-prompt machinery stays
decorative, and it gets quietly more wrong with each new resource type added
(three were added this month, none of them requestable). It is also the kind of
thing that is much cheaper to decide now than after applications have been
written assuming one answer.

**Where it bites:** `kernel/src/syscall/handlers.rs:6181` (the fifteen-entry
match), `kernel/src/cap/mod.rs:194-360` (types 16–30), `kernel/src/cap/request.rs`
(the broker itself).


## C-Q6 — [C] We have written the Settings screens twice, in two different places, and neither copy is finished. Which one is the real one? — Status: OPEN

**In short:** There are two separate, independently-written sets of Settings
pages in this tree — one inside the desktop shell, one inside a standalone
Settings application — covering mostly the same ground (sound, display, mouse,
power, network, wallpaper, accounts, updates…). Neither knows the other exists.
The shell's copy is better tested but **nothing can display it**; the app's copy
is the one that would actually open if a user clicked "Settings". I need to know
which one to keep, because everything I do to one I currently have to do twice.

**Glossary:** the *shell* is the always-on desktop furniture — taskbar, start
menu, wallpaper, the volume popup. An *application* is a separate program the
user launches. A *panel* or *page* here means one screen of settings.

**Where:**

| | |
|---|---|
| Copy 1 | `gui/desktop/src/*_settings.rs` and friends — about 50 modules |
| Copy 2 | `apps/settings/src/main.rs` — 8,227 lines, its own page list and its own data types |
| What connects them | nothing (`apps/settings` does not depend on the `desktop` crate at all) |

Copy 1 has one further problem on its own: the shell paints exactly **four** of
its fifty-seven modules (`wallpaper`, `calendar`, `snap`, `overview`). Every
other panel it contains — including a few with no counterpart in copy 2, such as
the on-screen volume overlay, the print manager and the login screen — is drawn
only by its own unit tests. Full detail is in `known-issues.md` →
`TD-C-THE-SHELL-DRAWS-FOUR-OF-ITS-FIFTY-SEVEN-MODULES`.

**Measured 2026-08-25, and it is worse than "two copies".** A new scan
(`scripts/scan-orphan-modules.py`) asks, for every module in my tree, whether
*any* other file in the repository so much as names one of the things it
defines. **Fifty-seven modules — 113,132 lines — are named by nothing, and
thirty-nine of them are in the shell**, a crate that declares fifty-nine. (The
scan first reported 21; three separate ways of accidentally crediting a module
with a caller it does not have were found and fixed the same day, each by
hand-checking a module the scan had cleared. 57 is the corrected figure.)

That the shell has 39 modules with no caller, arrived at mechanically, and
"the shell paints four of its fifty-seven modules", arrived at by hand two
days earlier, are the same fact counted two ways.

Three findings in that list change what this question is asking:

- **The shell duplicates itself, not just the app.**
  `gui/desktop/src/a11y.rs` (2,292 lines: a screen magnifier, four
  high-contrast schemes, sticky/filter/mouse keys, a colourblind filter) and
  `gui/desktop/src/accessibility_settings.rs` are two models of the same
  settings, six type names apart, in the same crate. `a11y.rs` is the copy
  nobody calls. Same shape for notifications: `notif_pane.rs` and
  `focus_assist.rs` share two types, and `gui/notifications/src/main.rs` says
  in its own comment that it shows the same notifications — a **third** copy.
  *(Update 2026-08-26: the first two are no longer unreached — the shell now
  owns and drives both, and they were reconciled to each other in the process:
  `design-decisions.md` §563–§564. That removes them from the island list but
  **not** from this question: the third copy, `gui/notifications`, is still a
  separate program modelling the same notifications on a third priority scale,
  and nothing has decided which of the two is the one a user gets.)*
- **Option A does not by itself de-duplicate.** File-type associations are
  modelled in `gui/desktop/src/default_apps.rs` (2,314 lines) *and*
  `apps/settings/src/associations.rs` (1,748) — and **neither is reachable**.
  Deleting the shell's copy would leave an unreachable one behind.
- **Only the app copy has ever been connected to storage.** `gui/desktop`
  contains no file write of any kind; it reads `appearance.yaml` and stops.
  `apps/settings` already saves two settings families through
  `gui/settingsfile`. Meanwhile six shell modules (`a11y`, `power`,
  `display_settings`, `input_method`, `tray_dnd`, `user_accounts`) hand-write
  their settings into a string — four of them with a matching parser and a
  passing round-trip test, two with no parser at all — and nothing calls any
  of it. What they emit is **`key=value` and pipe-delimited text, not YAML**,
  which `design.txt` requires for configuration.

None of this decides the question, and it does not change my recommendation.
It sharpens two things: whichever copy survives, **de-duplication has to
happen inside the survivor too**; and if the deciding factor is "which copy is
closer to working for a user", that is the app, on the evidence that it is the
only one that has ever written a settings file.

### The options

**A. The standalone app is the real one; delete the shell's settings panels.**
*What changes:* the desktop crate loses tens of thousands of lines; nothing a
user can see changes today. Cheapest, and it deletes the copy nobody can open.
Against: it throws away the better-tested implementation, and it does not
account for the shell-only surfaces (volume overlay, login screen, print
manager, security dialog) that are not settings pages at all and have nowhere
else to go.

**B. The shell's panels are the real ones; the app becomes a thin window that
displays them.**
*What changes:* the Settings app starts showing the shell's pages instead of its
own; the duplicate data types in `apps/settings/src/main.rs` go. Keeps the
tested code. Against: a Settings *application* that has to link the desktop
shell to draw itself is a backwards dependency, and the shell crate is already
sixty files.

**C. Split by kind — shell surfaces stay in the shell and get wired up; settings
pages move to the app and the shell copies are deleted.**
*What changes:* the volume overlay and the login screen actually appear on
screen for the first time; the Settings app gains the shell's better-tested
pages; each page exists once. Most work, and I think it is right — the dividing
line ("is this something the desktop shows you, or a screen you open?") is a
real one rather than a compromise.

**If it is never answered:** nothing breaks and nothing gets worse on its own.
The concrete cost is that every crate-wide change is paid for twice. The one in
flight is the palette conversion — 549 hardcoded colours in the shell's copy,
2,258 in the app's — and I am partway through the shell's. I will keep going
either way, because a converted module is converted once and leaving a module
frozen guarantees the bug comes back when it is finally wired up. But I would
rather not start the app's 2,258 without knowing whether half of them are about
to be deleted.

**Recommendation:** C. B is the tempting middle and I would push back on it: the
dependency direction is wrong and it papers over the fact that four modules of
fifty-seven are reachable. A is defensible if the answer is simply "the shell's
settings pages were a mistake" — and if that is the answer, say so plainly and I
will delete them rather than convert them.


## C-Q7 — [C] In the "green on black" high-contrast scheme, the highlight colour is three times dimmer than in the other three. Change it? — Status: OPEN

**In short:** SlateOS has a "high contrast" setting for people who cannot read
the normal theme. It offers four fixed colour schemes. In three of them the
highlight colour — the one used to show which thing is selected — is bright and
jumps off the background. In the fourth, "green on black", the highlight is
magenta, which is much darker than the others: about a third as visible. So the
one scheme aimed at the most strain-sensitive users is the one where "this is
selected" is hardest to see. The question is whether to change that colour, and
if so to what — because the current choice is dim *on purpose*, for a reason
that is also good.

Glossary, once:

- **Contrast ratio** — a single number for "how different in brightness are
  these two colours", from 1:1 (identical, invisible) to 21:1 (black on white).
  The web accessibility standard (**WCAG**) asks for at least 4.5:1 for normal
  text and 7:1 for its strictest level.
- **Highlight / accent colour** — the colour used for the selected item, the
  focus ring, the progress bar: not the words themselves, but the marker
  showing where you are.
- **Hue** — which colour it is (red, green, blue), as opposed to how bright it
  is. Two colours can be equally bright and still easy to tell apart by hue —
  unless the viewer is red-green colour blind, in which case they may not be.

### What is actually there

The four schemes, measured against their own background:

| Scheme | Background | Text | Text contrast | Highlight | Highlight contrast |
|---|---|---|---|---|---|
| Black background | black | white | 21.00:1 | yellow `#FFFF00` | **19.56:1** |
| White background | white | black | 21.00:1 | blue `#0000FF` | **8.59:1** |
| Yellow on black | black | yellow | 19.56:1 | cyan `#00FFFF` | **16.75:1** |
| Green on black | black | green | 15.30:1 | magenta `#FF00FF` | **6.70:1** |

Magenta is the outlier, and it cannot simply be "turned up": `#FF00FF` is
already the brightest magenta that exists. 6.70:1 clears the ordinary WCAG bar
(4.5:1) and misses the strict one (7:1) — in the mode whose entire purpose is
to be easier to see than the default.

### Why the dim colour is not obviously wrong

Magenta is the *opposite* of green. That makes it the one highlight in the list
that stays distinguishable from this scheme's green text under red-green colour
blindness, and it is the most different in hue from the text of any candidate.
The brighter alternatives are brighter precisely because they are closer to
green:

| Candidate highlight | Contrast vs black | Contrast vs the green text |
|---|---|---|
| magenta `#FF00FF` (today) | 6.70:1 | 2.29:1 |
| pale magenta `#FF80FF` | 9.78:1 | 1.56:1 |
| cyan `#00FFFF` | 16.75:1 | 1.09:1 |
| white `#FFFFFF` | 21.00:1 | 1.37:1 |

So the trade is real: *visible against the background* and *distinguishable
from the text* pull in opposite directions here, and today's colour is at one
end of it.

### Options

**A — Leave it at magenta `#FF00FF`.**
*What changes:* nothing; the selection marker in "green on black" stays about a
third as bright as in the other three schemes, and is documented as deliberate.

**B — Pale magenta `#FF80FF`.**
*What changes:* the selection marker in "green on black" becomes noticeably
brighter (6.70:1 → 9.78:1, clearing the strict 7:1 bar) while staying pink /
magenta, so it remains the colour furthest from green.

**C — Cyan `#00FFFF`.**
*What changes:* the selection marker becomes as bright as in the other schemes
(16.75:1), but it is now nearly the same brightness as the green text (1.09:1),
so text and highlight are told apart *only* by hue — which is exactly what a
red-green colour blind user cannot do. It would also make two of the four
schemes use the same highlight colour.

**D — White `#FFFFFF`.**
*What changes:* the selection marker becomes the brightest thing on screen
(21.00:1) and the scheme becomes two-colour-plus-white. Simple and maximally
visible; loses the idea that the highlight is a *colour* at all.

### My recommendation

**B.** It is the only option that fixes the thing being complained about
without giving up the reason the current colour was chosen: it stays a magenta,
so it stays the hue furthest from the text, and it stops being the dim one. C
and D are brighter still, but each pays for it — C by collapsing under the
colour blindness this mode exists to accommodate, D by dropping the colour.

### If this is never answered

Safe, and it does not get worse with time. The current colour is usable and
above the ordinary accessibility bar; nothing is blocked on this. The one live
consequence is that the regression test which pins these twelve colours
(`every_high_contrast_scheme_is_legible_with_itself` in
`gui/desktop/src/a11y.rs`) has its highlight floor set to **4.5:1** rather than
7:1, specifically so this scheme passes — so the floor is currently set by the
outlier rather than by the standard. Answering B, C or D would let that floor
rise to 7:1 and hold every future scheme to it.

## C-Q8 — [C] You decided in June that we ship the world's timezone data. Nobody can write it, because the lane map hands the job to a directory that does not exist. Who does it? — Status: OPEN

**In short:** if you set your clock to New York, SlateOS quietly gives you
London's time instead — and says nothing. The fix needs a *package* (a bundle of
files the system installs, like an app-store download) containing the world's
timezone rules, which you already approved shipping back in June. It cannot be
written, because the rule saying which of the three AI sessions owns the package
manager points at a folder that was never created; the real package manager
lives in a folder that session is forbidden to touch. This has been stuck since
2026-08-16 and needs you to say which session writes it.

**Where it bites:** `requests/b-c-tzdata-package.md` (lane B's ask, lane C's
answer at the foot), `userspace/pkg/src/main.rs` (the real package manager,
5 004 lines), `roadmap.md`'s ownership map and `scripts/which-lane.py` (both of
which grant lane C `pkg/**`).

### What is actually wrong

You answered B-Q1 on 2026-08-15 (`design-decisions.md` §311): ship the **full**
IANA timezone database, vendored as prebuilt binaries, distributed as a package.
Everything that *reads* that data is already written and tested — the C library
and the shell both resolve `TZ` through real binary zoneinfo files, matching
glibc's search order exactly.

There is nothing on disk for them to read. So `TZ=America/New_York` resolves to
nothing and falls back to **UTC**, with no error. The user believes they picked
Eastern and gets UTC. Two tests currently *assert* that wrong answer, on purpose,
and are named to say so — they go red the day the data lands, which is the
signal that it worked.

The ownership map says lane C owns "the package manager (`pkg/**`)". **There is
no top-level `pkg/` directory.** The package manager is `userspace/pkg/` — and
`userspace/**` is on lane C's never-write list. So the lane assigned the work is
the one lane forbidden to do it, and the lane that owns the files was never
assigned the work.

### The options

**A — Move the package manager to a top-level `pkg/`, and lane C writes it.**
*What changes:* nothing user-visible on its own; the ownership map becomes true
as written, and lane C can start immediately. Costs a tree-wide move of 5 134
lines plus every path that cites it — exactly the sort of change that conflicts
across three lanes working in parallel.

**B — Give the tzdata package to lane B, where the package manager already
lives.** *What changes:* nothing user-visible on its own; the work starts in the
lane that owns the code, with no move. Costs a standing inconsistency — the map
keeps saying lane C owns `pkg/**` while lane B owns the package manager — unless
the map is corrected in the same breath.

**C — Correct the map to say `userspace/pkg/` belongs to lane C, and leave the
files where they are.** *What changes:* nothing user-visible; lane C gains write
access to two directories inside lane B's tree. Costs a hole in the otherwise
clean "lane C never writes `userspace/`" rule, which is the rule that makes the
three-lane split safe to reason about.

**D — Leave it. Write down that the clock lies.** *What changes:* nothing;
`TZ=America/New_York` keeps meaning UTC indefinitely.

### My recommendation

**B, with the map corrected in the same change.** The package manager is 5 004
lines of lane B's code that lane C has never touched; the tzdata package is a
small addition to it, and asking the lane that wrote it to add one package is far
cheaper than moving the whole thing or carving an exception. A is the tidiest end
state and the most expensive to reach — and its cost is paid in merge conflicts
across three parallel lanes, which is the one currency this project is short of.
C is the smallest edit and the worst rule.

### If this is never answered

It does not get worse, and nothing else is blocked on it — but it does not get
better either, and the failure it leaves in place is the quiet kind: a clock that
is confidently wrong. It has already been stuck nine days for want of one
sentence from you. Everything else about the feature is finished.


## C-Q9 — [C] The backup tool and the search tools read the same-looking patterns by different rules. Should they be made the same? — Status: OPEN

**In short:** when you tell the backup program which folders to skip, you type a
pattern like `*.tmp` or `build/**`. When you search for a file, you type a
pattern that looks the same. They are not the same: the search tools understand
`[a-z]` to mean "any lowercase letter", and the backup tool understands it to
mean "a folder whose name literally contains a square bracket". Both behaviours
are defensible. The question is whether to make the backup tool agree with the
search tools — which would change what the exclude lists people have *already
written* actually exclude.

**Where it bites:** `apps/backup/src/main.rs` (`glob_matches`,
`glob_match_recursive`, `glob_match_simple` — lines 40–215) versus
`apps/globmatch/src/lib.rs`, which `apps/indexer` and `apps/filesearch` now
share. The reasoning behind the split is `design-decisions.md` §555.

### What is actually different

The desktop used to contain four separate pattern matchers. The two that are the
same feature seen twice — the search window and the background indexer — have
been merged into one shared implementation, because they disagreed with each
other on 646 of 730,236 test patterns and no test either of them had could ever
have noticed. That part needed no decision; it was a bug.

The backup tool is the case that does need one. It is not a broken copy of the
search matcher; it is a different pattern language, the one `.gitignore` uses:

| | search tools (`apps/globmatch`) | backup (`apps/backup`) |
|---|---|---|
| `*` | matches any run of characters, including `/` | stops at a `/` |
| `**` | nothing special — two stars, same as one | spans folder boundaries |
| `?` | any one character | any one character except `/` |
| `[a-z]` | any lowercase letter | the six characters `[`, `a`, `-`, `z`, `]` |
| works on | text, character by character | raw path bytes |

The first three rows are *correct as they stand*: an exclude list matches paths,
so `*.tmp` should not match `logs/a.tmp`, and `build/**` should. Nobody is
proposing to change those. The only row in dispute is the fourth.

### The options

**A — Leave it. Two dialects, written down.** *What changes:* nothing. Backup
patterns keep treating `[` as an ordinary character; search patterns keep
treating it as the start of a character class. Someone who learns one and
assumes the other is surprised once.

**B — Teach the backup tool character classes.** *What changes:* an existing
exclude line like `cache[1]/` stops excluding the folder literally named
`cache[1]` and starts excluding a folder named `cache1`. Everything else keeps
working. Gains: one rule to learn instead of two, and `*.[ch]` becomes a way to
skip C sources. Costs: a silent change of meaning in data users already wrote —
the backup that quietly starts including a folder it used to skip is not an error
anyone will see.

**C — Teach the backup tool character classes, but only for new patterns.**
*What changes:* the same as B for anything written from now on; existing exclude
files are read under the old rules until edited. *What it costs:* a file format
that means two different things depending on its age, which is the kind of thing
that is impossible to explain and impossible to remove later. Listed for
completeness; I do not recommend it.

### My recommendation

**A.** The two search tools had to be unified because they are one feature with
two implementations — a disagreement with no upside. Backup is not that: its
dialect is the right one for its job and matches what every developer already
knows from `.gitignore`, where `[` is likewise not special in the common case.
The gain from B is small (a shorter way to write a few exclude patterns) and the
cost lands on data that already exists and that nobody will re-read to check.

If you prefer B, the change itself is small — `apps/backup` would call
`globmatch`'s class parser for the segment-matching step — and the real work is
deciding whether to warn about existing patterns containing a `[`.

### If this is never answered

Nothing is blocked and nothing degrades. The two dialects are documented in
`design-decisions.md` §555 and in `apps/globmatch`'s module docs, so the split is
a recorded decision rather than an accident. The only ongoing cost is that a user
who learns one pattern language may assume the other works the same way.

## A-Q1 — [A] `find . -size 100` finds 100-byte files here and 50 KiB files everywhere else. Match the rest of the world, or refuse the ambiguous spelling? — Status: OPEN

**In short:** `find` is the command that searches a folder for files matching
some description. One of the things you can ask about is size. If you write
`find . -size 100`, our shell reads that as "100 bytes"; the `find` on Linux and
macOS reads the same line as "100 *blocks*", where a block is 512 bytes — so it
looks for files around 51 200 bytes instead. Both then print a perfectly
plausible list of files and neither says anything about having interpreted the
number differently. The choice is between matching everyone else, keeping our
(more intuitive) reading, or refusing to accept a bare number at all.

### Why this is a decision and not just a bug to fix

The other `find`'s behaviour is a genuine historical wart — nobody expects a
bare number to mean 512-byte units, and it surprises every person who meets it
once. So "bytes" is the reading a human actually intends, and copying the wart
makes our command worse in isolation.

But the disagreement is *invisible*. A wrong `-type` argument gets you an error
message; a wrong `-size` unit gets you a tidy list of the wrong files. Anyone
who brings a command line from a tutorial, a Stack Overflow answer, or their own
muscle memory gets a different answer here and has no way to notice.

### What each `find` accepts today

| Suffix | Elsewhere (GNU) | Here |
|---|---|---|
| *(none)* | 512-byte blocks, rounded up | **bytes** |
| `c` | bytes | bytes |
| `k` / `M` / `G` | KiB / MiB / GiB | same |
| `b` | 512-byte blocks, explicitly | **not accepted** — errors |
| `w` | 2-byte words | **not accepted** — errors |

Note the last two rows: a suffix we do not implement is already *refused* rather
than guessed at, which is the behaviour the third option below would extend to
the bare number.

### The options

**A — match GNU: a bare number means 512-byte blocks.** Also requires adding
`b` and `w`, so the whole vocabulary lines up.
*What changes:* `find . -size 100` starts listing ~50 KiB files instead of
100-byte ones. Commands copied from anywhere else become correct. Any local
script already written against our reading silently starts selecting different
files.

**B — keep bytes, and add `b` (and `w`) so blocks can be asked for explicitly.**
*What changes:* nothing about today's behaviour; `find . -size 100b` becomes a
new, working way to say "100 blocks". A bare number still means something
different here than elsewhere, still silently.

**C — keep bytes as the *meaning*, but require the unit to be written.**
*What changes:* `find . -size 100` becomes an error — `find: -size 100: write
100c for bytes or 100b for 512-byte blocks` — and `100c`, `100k`, `100b` etc. all
work. Nobody is ever silently wrong in either direction. Every existing bare-
number use has to be edited (there are, today, none outside the shell's own help
text and tests).

### Claude's recommendation

**C.** It is the same principle this lane has been applying everywhere else in
the shell all month: where two readings are both plausible and the difference
does not show up in the output, *refuse* rather than pick a side. It is also the
only option under which a command line copied from outside cannot quietly mean
something else — A fixes that for GNU users while breaking it for anyone who
learned our version, and B fixes it for nobody.

`-size` is new enough here that the cost of C is close to zero, which will not
stay true.

### If this is never answered

Today's behaviour is safe and is documented in the function's own doc comment;
nothing is blocked. The cost is quiet and grows slowly: every script written
against a bare `-size` number is one more thing that has to be checked if the
answer later turns out to be A or C.

### Where it bites

`kernel/src/kshell.rs` — `parse_size_predicate` (the final `else` arm,
`(rest, 1i64) // default: bytes`) and its one caller, the `-size` arm of `find`.
Tracked in `known-issues.md` as
`A-KSHELL-FIND-SIZE-DEFAULT-UNIT-IS-BYTES-NOT-BLOCKS`.

---

## A-Q2 — [A] Our C-test programs are built against a library nobody can identify, because the compiler that builds them cannot see the folder it is run from. The fix is in a different project. Who changes it? — Status: OPEN

**In short:** part of our test suite compiles small C programs and runs them
inside the OS. To build them, the compiler (**fastpy** — a separate project of
yours, in `D:\visual studio projects\fastpy`) has to find our C library. It
looks for a folder next to itself named exactly `os`. But we stopped working in
a folder named `os` — each of the three parallel workers now has its own
checkout named `os-lane-a`, `os-lane-b`, `os-lane-c` — so the compiler finds
nothing, every time, for all three. The library it wants is sitting right there
in the folder it was launched from; it just never looks there. The question is
who is allowed to change fastpy to fix it, since that is not this project's code.

### What this actually costs

Nothing is broken or wrong — the OS builds and boots fine, and this has no
effect on the kernel. What is lost is *attribution*: when one of those C
programs passes or fails, we cannot say which version of our C library it was
built against, so the result proves less than it appears to. Every boot test
prints a warning saying so.

I already fixed the half that was in our tree (commit `1367f04ac`): the warning
used to claim the library could not be *found*, which sent the reader off to
rebuild something that was never missing. It now names the real cause and gives
a repair line that works. **So this is not urgent** — it is a known, clearly
labelled gap rather than a silent one.

### Why I did not just fix fastpy myself

Two reasons, and the second is the one I want your call on.

1. **It needs a judgement, not just an edit.** With four checkouts side by side
   (`os`, `os-lane-a`, `os-lane-b`, `os-lane-c`), "find the OS folder" no longer
   has one answer. The sensible rule is "walk up from wherever you were launched
   and use the checkout you are inside of" — but that is a behaviour change for
   anything else using fastpy, not a typo fix.
2. **It is a different repository, and three of us share it.** All three lanes'
   builds call fastpy. Our own rules say I file a request rather than edit
   another owner's tree, and fastpy has no lane — so there is nobody to file it
   with. It also has its own version-bump-and-release discipline that a drive-by
   commit from the OS project would sit oddly inside.

### The options

**A — I fix fastpy directly** (walk up from the launch directory; keep the old
`os` lookup as a fallback so nothing that works today stops working), bump its
version per its own rules, and commit there but do not push.
*What changes:* the warning stops appearing on all three lanes and the C-test
results become attributable again. You get a commit in the fastpy repo from OS
work.

**B — Each lane sets the location explicitly when it builds the fixtures**, and
fastpy is left alone.
*What changes:* nothing visible until someone re-builds the C fixtures; the
warning stays until then. Keeps fastpy untouched, but the same blind spot bites
anything else that calls it.

**C — Leave it. Keep the accurate warning and treat those C-test results as
unattributed.**
*What changes:* nothing. The warning keeps printing on every boot on every lane.

**My recommendation: A.** The hard-coded folder name is a plain bug — it broke
the moment the project adopted `git worktree`, which is now policy — and B fixes
the symptom for one caller while leaving the cause. I am flagging it rather than
doing it only because it is your other repository.

### If this is never answered

Nothing degrades and nothing is blocked. The warning keeps printing on every
boot for all three lanes, and C-test results stay unattributable. The one real
risk is the ordinary one for a permanent warning: people stop reading it, so the
day it has something new to say, it looks like the noise it has been all along.

The index is split by lane so three lanes adding a line at once land at three
different offsets and the merge is automatic. Newest first within each lane.
`(§n)` cites `design-decisions.md`.

---

## A-Q3 — [A] Should the kernel run its own test suite on a user's boot, and stop the machine when one fails? — Status: OPEN (raised 2026-08-22)

**In short:** Right now, every time this OS starts, the kernel runs several
hundred of its own built-in tests before handing the machine to the user —
checking things like "is the backspace key still character 127". Most of those
checks are written so that a failure **stops the machine dead** rather than
printing a complaint and moving on. Nothing is failing today. The question is
what *should* happen on a user's computer the first time one of them is wrong:
refuse to boot, boot with a warning, or not run the checks there at all.

Glossary, once: a **self-test** here is ordinary kernel code that checks some
other kernel code and prints the result to the serial console. An **assertion**
is a check written so that failing it panics — the kernel prints a message and
halts. A **boot test** is what the developer runs in an emulator; a **production
boot** is a user starting a real machine. Today these run the *same* code.

### Why it is worth deciding

The two audiences want opposite things and currently get the same behaviour:

- On a **boot test**, halting is *good*. The panic names the file, the line and
  the values that disagreed, which is better diagnostics than a log line, and
  the run should fail loudly anyway.
- On a **production boot**, halting is close to the worst option. A wrong
  assertion about a terminal flag becomes a computer that will not start, and
  the user has no way to skip it.

Scale, for a sense of the exposure: 567 files under `kernel/src/` contain both a
`self_test` function and assertions, 12 674 assertion sites in total. Only ~299
sites use the alternative style that logs a failure and returns an error instead
of panicking.

### Options

**A — Gate self-tests behind a boot flag; production boots skip them.**
*What changes:* a user's machine starts faster and never halts over a self-test;
the developer adds `selftest=1` (or the boot test does) to get today's
behaviour.
Cheapest by far — roughly one conditional around the self-test block in
`kernel_main`. The cost is that a corrupted or mis-built kernel that a self-test
would have caught now boots and misbehaves later instead.

**B — Always run them, but never panic: convert assertions to logged failures.**
*What changes:* a failing check prints `FAIL: ...` and the machine keeps
booting, on developer and user machines alike.
Keeps the coverage on real hardware, where it is arguably most valuable, since
a production boot exercises drivers an emulator does not. The cost is 12 674
edit sites, and each one *loses* information — an assertion reports the compared
values and its own line number for free, whereas the replacement reports only
what its author remembered to include. Realistically a slow migration, not a
single change.

**C — Run them on production boots and keep halting.** *What changes:* nothing;
this is today's behaviour, made deliberate.
Defensible on the "fail fast, never run a kernel that fails its own checks"
argument. The objection is that the checks are not all equally load-bearing —
halting the machine because `VERASE` is not 127 is not obviously better than
booting with a warning.

**D — Split the difference: keep assertions for checks about kernel integrity,
log-and-continue for the rest.** *What changes:* a bad memory-manager invariant
still halts; a cosmetic terminal-flag mismatch prints a warning and boots.
Probably the right end state and the most work to get to, since it needs a
judgement per self-test rather than a blanket rule.

### My recommendation

**A now, D eventually.** A is a one-line change that removes the user-facing
risk immediately and costs nothing we are currently getting — the boot test,
which is where these checks actually earn their keep, would still run them with
the flag set. It also makes B-versus-D a much less urgent question, because
after A the assertion style only affects developer boots, where panicking is the
better behaviour anyway.

### If this is never answered

Safe for now — no self-test is failing, and the streak is 11 consecutive clean
boots. It does not get worse on its own, but it gets *bigger*: the count grows
every time someone adds a self-test, and A stays a one-line change forever while
B and D get more expensive. Meanwhile new self-test code (`pathutil`, `net::raw`,
`net::frag`) is being written in the log-and-continue style, which is the safe
side of the question whichever way it goes.

Background: `known-issues.md` →
`TD-A-MOST-BOOT-SELF-TESTS-PANIC-THE-KERNEL-INSTEAD-OF-REPORTING`.

---

## An account with no password: should the lock screen let it through, or refuse forever? (lane C, 2026-08-24)

**In short:** Some accounts have no password set at all. Today, if such an
account's screen locks, pressing Enter dismisses it — no password is asked for,
because there is none to ask for. That means anyone who walks up to that
machine while it is locked gets straight into the session. The obvious fix is to
refuse: a lock screen with nothing to check should let nobody in. But then the
*real* user is locked out too, permanently, with no way back to their own
desktop short of a reboot. I need you to pick which of those two you would
rather ship.

### Where it bites

`apps/lockscreen/src/main.rs`, `LockScreen::unlocks_for` — one function, written
specifically so that this is a one-line change once you decide. It is called by
`submit_password` on every attempt.

The verdict now comes back as one of six values borrowed from lane B's
`userspace/authlib` (`Accepted`, `Rejected`, `Locked`, `NoPassword`, `Unusable`,
`RateLimited`). Five of them decide themselves. `NoPassword` — meaning "the
stored entry for this account is empty" — deliberately does not, because lane B
made it the *caller's* policy: a console login may reasonably let an empty entry
through, and a lock screen may not. So each caller must state its own rule, and
this is ours to state.

### Why it is not obvious

The security argument is clean and lane B makes it: an empty-password account
means anyone who closes the lid owns the machine.

The counter-argument is that the hole was already open. If the account has no
password, an attacker standing at that machine can log in as that user from the
*login* screen without typing anything. Refusing at the lock screen protects an
already-running session and nothing else — while creating a failure mode that
is arguably worse than the hole: a desktop that cannot be got back into by the
person it belongs to.

### Options

**A — accept it (what it does today).**
*What changes:* nothing. A passwordless account's lock screen is dismissed by
pressing Enter, as now.

**B — refuse it.**
*What changes:* a passwordless account that locks can never be unlocked. The
screen says "This account has no password" and stays up until the machine is
restarted.

**C — never lock a passwordless account in the first place.**
*What changes:* auto-lock is suppressed and the manual lock command is refused
for an account with no password, so the trap in B cannot be entered. If the
screen is somehow reached anyway it dismisses on any key, as in A. Costs a
little more code: the suppression has to live wherever locking is triggered,
not only in the screen.

**D — accept it, but require the account to have been passwordless *before* the
session started**, so that clearing a password while locked cannot open the
screen.
*What changes:* nothing a user would notice; closes a narrow race that only
matters once `passwd` can be run by something other than the session owner.

### My recommendation

**C.** It is the only one of the four that is neither a hole nor a trap: it
declines to offer a security boundary that does not exist, rather than
pretending to enforce one (B) or pretending to have enforced one (A). B's
failure mode is the one I would least like to explain to a user, because it
takes a working desktop and makes it unusable through no action of theirs.

If C is more machinery than you want here, **A** — the status quo — is the safer
of the two remaining, for the reason above: it does not create a new way to lose
a session, and the exposure it leaves is one the login screen already has.

### If this is never answered

Safe, and it does not get worse. The screen keeps behaving as it always has
(option A) and the policy is isolated in one function, so answering later costs
one line plus a test. Nothing is blocked on it. The reason it is worth asking at
all is that the *refactor that surfaced it* deliberately did not change it —
altering who can unlock a machine is not something to slip into a commit about
interface shape.

---

## A-Q4 — [A] Should `oci run` refuse to start when an option cannot be applied? — Status: OPEN

**In short:** `oci run` starts a container. If you ask it for something extra —
a shared folder (`-v`), a published port (`-p`), a file of labels or
environment variables — and it cannot do that one thing, it currently prints a
warning, starts the container anyway, and reports success. So you can ask for a
container with your data folder attached, get one *without* it, and be told
everything worked. Docker refuses to start at all in this situation. The
question is which of those two behaviours we want.

**Where:** `kernel/src/kshell.rs`, the `oci run` argument loop — eleven sites,
all reading `[oci] Warning: could not …` or `[oci] Could not read …-file`.
Raised during the exit-status sweep (`known-issues.md` →
`A-KSHELL-3676-FAILING-COMMANDS-REPORTED-SUCCESS`), which deliberately left all
eleven alone because changing the *status* without deciding the *contract*
would be the dangerous half of the change on its own.

### Why the sweep did not just fix it

Every other failing command in the shell got a non-zero exit status. These
eleven did not, because here a non-zero status is worse than the bug. The
idiom `oci run … || cleanup` exists, and `cleanup` tears down a container.
Flipping the status would make it tear down a container that is **up and
running** — turning a wrong exit code into destroyed work. The rule the sweep
used for this command instead was "did the container start?", and the one site
that answers no (`Cannot allocate IP from network`) already sets a status and
returns.

That leaves the real question untouched: should asking for an option that
cannot be applied mean the container should not have started?

### Options

**A — refuse to start (Docker's behaviour).** Validate every requested option
before launching; if any cannot be applied, print the reason, start nothing,
exit non-zero.
*What changes:* `oci run -v /data:/data img` on an unmountable `/data` prints
the error and you get **no container**, instead of a running container with no
`/data`. `|| cleanup` becomes correct, because there is nothing to clean up.

**B — start anyway, but exit non-zero** (today's behaviour plus a status).
*What changes:* the container still starts without `/data`, but the command
reports failure — so `|| cleanup` fires **against a live container** and
destroys it. This is the option that looks like a small fix and is not.

**C — leave exactly as is: warn, start, exit 0.**
*What changes:* nothing. A script cannot tell that an option was dropped, and
must inspect the container afterwards to find out.

**D — split by option kind.** Treat options that change what the container *is*
(`-v`, `-p`, `--label-file`, `--env-file`) as A, and options that are advisory
(`--read-only` best-effort, tmpfs) as C.
*What changes:* the dangerous ones fail closed, the cosmetic ones stay
warnings. More faithful, and more code, and the boundary needs writing down or
it will drift.

### My recommendation

**A**, matching Docker. The reason is that a container is not a partial
artifact: you cannot inspect one to discover which options were silently
dropped, so "started, but not as requested" is a state no caller can act on. A
is also the only option under which the existing `|| cleanup` idiom is safe,
because it guarantees there is nothing running to clean up. **D** is defensible
if refusing to start over an unapplied tmpfs feels too strict — but it needs an
explicit list, not a judgement call per site.

**Not B.** It is the smallest diff and it is actively harmful.

### If this is never answered

Safe, and stable — today's behaviour destroys nothing and the sweep left it
untouched on purpose. It does not degrade with time. What it costs is that
`oci run` cannot be scripted reliably: any script that cares whether its
options took effect has to verify them itself afterwards, and every such script
is a place that would need revisiting if the contract later changes.

---

## A-Q5 — [A] The shell's `grep` ignores case and numbers lines by default, unlike every other Unix — Status: OPEN (raised 2026-08-24)

**In short:** In our shell, typing `grep Error mylog.txt` also finds `error`
and `ERROR`, and prints each result with a line number in front of it, like
`42:error: disk full`. Real `grep` on Linux/macOS does neither: it matches
`Error` exactly, and prints just the line. Our version behaves as though you
had typed `grep -i -n`. This is very likely a deliberate choice made early on
for interactive convenience, but it was never written down, and it means
commands copied from any Unix documentation or tutorial quietly do something
different here. The question is whether to keep it.

Where it lives: `GrepFlags::new()` in `kernel/src/kshell.rs` (~94901), which
sets `case_insensitive: true` with the comment *"default: case-insensitive
(like original)"*, and `show_line_numbers: true`:

```rust
impl GrepFlags {
    fn new() -> Self {
        Self {
            case_insensitive: true, // default: case-insensitive (like original)
            show_line_numbers: true,
            ...
```

`-i` and `-n` both exist, and both only *set* these to `true` — the value they
already hold. So there is no spelling of `grep` in this shell that turns either
off: the two flags a user would reach for to control this are no-ops.

Why it surfaced when it did: the shell had just gained working exit statuses
and working `$(…)` capture through pipelines, so `grep` output became something
*programs* consume rather than something a human reads. `$(grep p f)` returns
`1:match`, and stripping that prefix requires knowing it is there.

### Why it is worth asking rather than just fixing

Two things push this out of "obviously a bug":

1. **The comment says it is intentional** — "like original" reads as
   *preserve the behaviour kshell already had*, not as an oversight.
2. **There is already an opt-out for the case half, and it is a made-up one.**
   The shell accepts `-I` to mean "be case-sensitive after all". In GNU grep,
   `-I` means something completely different (ignore binary files). So this is
   not merely a changed default; a real flag has been re-purposed to undo it.
   Restoring the GNU default would also have to decide what `-I` then means.

The line-number half has no opt-out at all: there is no way to turn `-n` off.

### What it costs today

Copy-pasted commands silently mean something else. Two examples of the shape:

| Written | Means elsewhere | Means here |
|---|---|---|
| `grep Error log` | lines containing `Error` | also `error`, `ERROR` |
| `grep -c pat f` | a count | a count (unaffected — `-c` overrides output) |
| `grep pat f \| cut -d: -f2` | the second `:`-field of the line | the *line*, because field 1 is now the line number |

The last one is the sharp edge: `-n` on by default changes the *shape* of the
output, so any pipeline that splits a grep result on `:` is reading one field
off. Nothing errors; it just quietly reads the wrong column.

It is also now load-bearing in a test. Self-test rung 25 asserts `1:alpha`
rather than `alpha`, with a comment pointing here — so a change of default is a
one-line test update, not a hunt.

### The options

**A — restore GNU defaults: case-sensitive, no line numbers.**
*What changes:* `grep Error log` stops matching `error`; `grep pat f` prints
`the matched line` instead of `42:the matched line`. `-i` and `-n` turn each
back on. `-I` needs a new meaning (either drop it, or make it GNU's
ignore-binary — which this shell already does implicitly under `-r`).

**B — keep both defaults, and document them.**
*What changes:* nothing in behaviour. `grep --help` and the shell's docs gain
an explicit note that `-i -n` are implied, plus a way to switch them off.

**C — split the two.** Restore GNU's case-sensitivity (the one that changes
*which lines* you get, and can therefore hide a result you needed), keep `-n`
(which only changes how they are printed).
*What changes:* `grep Error log` stops matching `error`; output still carries
line numbers.

**D — keep case-insensitivity, drop the default `-n`.** The inverse of C.
*What changes:* output shape matches GNU, so `:`-splitting pipelines work;
matching stays lenient.

### My recommendation

**A**, with `-I` dropped rather than redefined. The value of matching the rest
of Unix here is not aesthetic — it is that every piece of grep knowledge a user
already has, and every command in every tutorial, becomes correct instead of
subtly wrong. Convenience defaults are cheap to type back (`-i`, `-n`) and
expensive to discover you were getting.

If A feels too disruptive, the safer half-step is **D**, not C — and this
paragraph is a correction of what an earlier copy of this entry said. The
earlier text argued for C on the grounds that a wrong *set of lines* is a wrong
answer whereas a prefix is visible on sight. The first half is true in general
and **false here**: case-insensitive matching returns a *superset*, so it can
show you a line you did not want but can never hide one you did. The half that
can actually corrupt an answer is the line-number prefix, because it changes
the bytes of every line and silently shifts every `:`-splitting pipeline by one
field. So if only one default moves, move `-n`.

*(Filed twice, on the same day, by the same lane: once as "kshell's `grep`
defaults differ from POSIX" and once as this entry, with opposite
recommendations — C there, A here. The two have been merged into this one. Two
contradictory recommendations from one lane on one question is worse than a
plain duplicate: it makes the queue unanswerable, because there is no way for a
reader to tell which of them is the lane's actual position. It is also what
prompted `scripts/check-open-questions.py`.)*

### If this is never answered

Safe and stable; nothing degrades. The cost is ongoing and quiet: every
`grep` command a user brings from outside behaves differently than they expect,
and any pipeline that splits on `:` reads the wrong field. It also gets
*slightly* more expensive to change over time, since each new script written
against the current defaults is one more thing to check.

---

## SlateOS has no way to encrypt anything. Which cipher do we add, and who owns it? (lane C, 2026-08-26)

**In short:** The whole operating system can *scramble* data so it can be
checked (SHA-256, MD5, SHA-1, CRC32 — those are all one-way fingerprints), but
it cannot **encrypt** anything: there is no code anywhere in the tree that turns
readable data into unreadable data and back again with a key. So the password
manager I just wired to a real window cannot save your passwords — not because
nobody has written the save code, but because there is nothing to lock the file
with. Adding a cipher is a few hours' work, but *which* one is a decision that
sticks, because every file written under it has to stay readable forever.

### Where it bites

Anything that needs to store a secret on disk, which is at least:

| Wants it | For | Status |
|---|---|---|
| `apps/credmanager` | the password vault | wired to a window, stores nothing — `known-issues.md` → `C-CREDMANAGER-HAS-NO-VAULT-ON-DISK` |
| `gui/credentials` | saved Wi-Fi and site logins | same gap |
| `apps/archivemanager` | encrypted zip members | can't read them either — `C-ARCHIVEMANAGER-CANNOT-SEE-THE-ENCRYPTED-BIT` |
| whole-disk encryption | the roadmap's storage section | not started |

`pwkdf` already turns a master password into a 256-bit key correctly, with salt
and a tunable cost. That half is done and tested. The missing half is what to
*do* with the key.

### The terms, since two of them do the work

- **AEAD** — "authenticated encryption": a cipher that both hides the data and
  detects tampering. The alternative (encrypt-only) lets an attacker flip bits
  in your vault and have it decrypt to different, valid-looking garbage. Nobody
  should ship encrypt-only in 2026; treat AEAD as settled and read the options
  below as "which AEAD".
- **AES-NI** — a CPU instruction that makes AES fast. Without it, a careful
  software AES is ~5-10× slower *and* much harder to write without leaking the
  key through timing. Every x86-64 chip since ~2010 has it, but we would be
  choosing to depend on it.

### Options

**A — ChaCha20-Poly1305, written here.**
*What changes:* one new `no_std` crate at the workspace root, ~400 lines, no CPU
feature required. Fast and constant-time in plain Rust on any machine.
This is what WireGuard and TLS 1.3 use on hardware without AES-NI.

**B — AES-256-GCM, written here.**
*What changes:* the same shape, but the software fallback is the part that is
easy to get subtly wrong (timing leaks through table lookups), and doing it
*properly* means writing the AES-NI path too — so it is really two
implementations, and the interesting one is x86-only.

**C — port a vetted C implementation instead of writing Rust.**
*What changes:* no new hand-written crypto, but a C dependency in the build for
every app that stores a secret, and `design.txt` already says C is for porting
existing code — which this would be. Slower to land, harder to audit in-tree.

**D — decide later; ship the apps without persistence.**
*What changes:* nothing. credmanager keeps opening an empty vault every launch,
`gui/credentials` keeps forgetting Wi-Fi passwords, and the gap spreads to
whatever gets built next.

### The part that is not mine to decide

Even given a choice, **which lane owns it** is open. The hash crates (`sha2`,
`sha1`, `md5`) live at the workspace root and are shared by all three lanes, so
a cipher belongs there too — but that is outside every lane's write glob. If
you pick A or B, say whether lane C should write it at the root, or whether I
should file a request to lane A and pick up something else.

### My recommendation

**A, written by whichever lane you say.** ChaCha20-Poly1305 has no CPU
dependency, so there is one implementation rather than a fast path and a
dangerous fallback; it is the easiest of the three to write correctly and the
easiest to test against published vectors. The reason I am asking rather than
doing it is not the cipher — it is that hand-written crypto in an OS is exactly
the kind of thing you may want to overrule on principle, and the file format it
implies is permanent.

### If this is never answered

Nothing breaks and nothing gets worse on its own — but a growing number of
finished, tested applications stay unable to do the one thing they exist for.
credmanager is the third app now waiting on this. It is not blocking my current
work; I am carrying on down the roadmap.

---

## A-Q6 — [A] Two commits that appear to delete the whole OS, and 33 commits signed by a fake name, are permanently in the published history. Leave them, or rewrite? — Status: OPEN (raised 2026-08-29)

**In short:** on 2026-08-29 a safety check that runs just before uploading code
accidentally committed to the real project instead of to the scratch copy it
meant to use, and those commits got uploaded. As far as those two commits are
concerned, every file in the operating system was deleted. **The current files
are completely fine** — I repaired that within minutes, and nothing is missing.
What
remains is only the *record*: anyone scrolling back through the project's
history will see two commits that look like a catastrophe. Removing them from
the record is possible but requires an operation I am forbidden to perform
without you saying so, because it can destroy other people's work.

**Updated later the same day — the record is wrong in a second way.** The same
accident also wrote a fake author name into the project's shared settings, so
**33 commits are signed `selftest <selftest@example.invalid>` instead of your
name**. That covers every commit all three sessions made over about an hour,
including the ones that fixed the accident. The settings are repaired, so no
*new* commit is affected; the 33 already made cannot be corrected except by the
same forbidden operation. This does not change the question, but it does change
what is at stake in it, so both are decided together.

**Question.** Should the published history be rewritten — to remove the two
commits (`7f6a6b446` "base" and `71f164f7e` "delete one, sweep another"), to
re-sign the 33 misattributed ones, or neither?

Two terms, glossed:

- **Published history** — the copy on GitHub that all three lanes (the three
  parallel Claude sessions) pull from. Everyone's work is built on top of it.
- **Force-push** (the operation in question) — replacing that shared history
  with a different one. It is the only way to remove a commit that is already
  published. It is dangerous because any lane whose work sits on top of the
  removed commits has its history invalidated, and anything not yet uploaded
  can be lost outright. Standing project policy forbids it without your
  explicit say-so, which is why this is a question and not something I did.

**What the current state actually is.** The repair used a merge whose *content*
is the correct tree, so the files are right and both branches moved forward
normally — no rewriting was needed. The two bad commits survive as *ancestry*
(steps in the chain) but not as *content* (no file reflects them). `git log`
shows them; `git status` and every checkout are clean.

**The misattributed commits, by branch.** All are pushed:

| Branch | Commits signed `selftest` |
|---|---|
| `lane-a` | 16 |
| `lane-b` | 9 |
| `lane-c` | 3 |
| `main` | 5 |

Two facts that make this less bad than it sounds, and are the reason it is
folded into an existing question rather than raised as an urgent one. First,
**there is no attribution dispute to get wrong**: you are the sole author of
record for this entire project, so a wrong name credits nobody else and steals
credit from nobody. Second, **the commit messages are intact** — the *content*
of the record is right, and only the signature is wrong. What it actually costs
is that `git log --author` and any per-author statistic silently omit an hour
of work, and anyone reading the log cold sees a contributor who does not exist.

### Options

**A. Leave both, documented.** *What changes:* nothing observable; `git log`
keeps showing two alarming-looking commits and an hour of work signed by a
name that is not yours, and `known-issues.md` explains why. *Pros:* zero risk;
no force-push; the incident stays legible, which has value — the commits are
the evidence for the post-mortem that produced the fix. *Cons:* anyone reading
history cold gets a scare and has to go find the explanation; a future
automated tool that audits history for mass deletions will flag them forever;
per-author statistics stay wrong for those 33 commits.

**B. Rewrite history to remove and re-sign them.** *What changes:* `git log`
no longer shows the two commits, and the 33 carry your name. *Pros:* a clean
and correctly-signed record. *Cons:* requires a force-push to `main` and all
three lane branches — note that the authorship half touches **all four**,
where removing the two commits alone would have touched two, so the blast
radius is larger than it was when this question was first written. The other
two lanes must re-sync, and any uncommitted or unpushed work of theirs is at
risk; ~40 commits now sit on top of the bad ones, all of which get new
identities, invalidating every commit hash cited in `known-issues.md`,
`design-decisions.md` and the request files — including the citations *in the
entry that explains this incident*.

**C. Re-sign the 33, but leave the two commits.** *What changes:* the log
still shows the two commits, but every commit carries your name.
*Pros:* fixes the half that is factually wrong (a signature naming someone who
does not exist) while leaving the half that is merely ugly-but-true (the
commits really did happen). *Cons:* this is not actually cheaper than B —
re-signing rewrites the same commits a removal would, needs the same
force-push to the same four branches, and invalidates the same hashes. It buys
less for the same risk, which is why I list it only to note that it is not the
compromise it looks like.

### If never answered

Option A is the current state and it is safe. Nothing is blocked and nothing
degrades functionally — but it does get *more* expensive to change with time,
since every commit added on top is one more that a rewrite would have to
rewrite. If you are ever going to pick B, sooner costs less.

### Claude's recommendation

**A, still, and the new evidence does not move me.** The content is correct,
the incident is documented at length in `known-issues.md`
(`A-A-PUSH-GATE-DELETED-THE-REPOSITORY-IT-WAS-GATING`), and trading a cosmetic
blemish in the log for a force-push across three active lanes is a bad
exchange — the cure has a real chance of destroying work, which is precisely
the failure the original bug caused.

The authorship damage is the kind of finding that *feels* like it should tip
the balance, and I do not think it does: it makes the record uglier without
making it wrong in any way that costs anyone anything, since you are the only
author this project has. Meanwhile it makes option B strictly more dangerous
than it was, because it drags the third lane's branch into a rewrite that
previously did not need to touch it. The case for A got stronger, not weaker.

I raise it only because it is your history, force-push authority is yours
alone, and the cost of choosing B rises with every commit.

**Where it bites.** Git history only: `7f6a6b446` and `71f164f7e`, repaired by
`f0534726e`; plus 33 commits between 22:43 and 23:54 on 2026-08-29 whose author
field reads `selftest <selftest@example.invalid>`. No file in the tree is
affected, and the shared config that caused the misattribution is repaired, so
the count cannot grow.

**Update from lane B, 2026-09-04 — it grew. Two more, on `lane-b`.**

*In short:* the same kind of accident happened again, in a different safety
check, six days later. Two more commits that appear to delete the whole
operating system are now in the published record — `7bb82cee5` "introduce the
violation" and `6b1d2a7ae` "the repair, committed this time", both signed
`selftest <selftest@invalid>`. **The files are fine again**; as before only the
record is affected. This does not change the options above, but it does correct
the sentence directly above it: the count *can* grow, and did.

It was not a recurrence of the same bug. 2026-08-29 was a self-test whose
commands were aimed at the real repository by a *setting*; this one was aimed
there by an *environment variable* (`GIT_DIR`) that git hands to every hook and
that outranks the "work in this directory" argument the self-test was relying
on. Different mechanism, same shape: a check that verifies a scratch copy, run
somewhere that silently redirects it onto the real thing. Fixed at the shared
layer this time rather than in the one script — `scripts/gittree.py`, which all
six commit-reading checks go through, now strips those variables — so the
remaining scripts of this kind were repaired in the same change rather than one
incident at a time.

Handled identically to the first: an `ours` merge that keeps the correct tree
and records the two commits as ancestry only, so no force-push was needed and
nothing was blocked. Contamination is confined to `lane-b`; `main` and the
other two lanes never saw them.

*What it changes for the decision:* option B's blast radius grows again — a
removal would now have to take these two out of `lane-b` as well, and every
commit since. The cost-rises-with-time note above therefore applies more
sharply than when it was written. Lane B's own recommendation is unchanged and
matches lane A's: **A**.

**Status:** OPEN

---

## C-Q10 — [C] In the light theme, small grey text on a shaded card is too faint to meet the readability standard, in about 850 places. Fixing it changes how the whole light theme looks. Which way? — Status: OPEN

**In short:** The desktop has a light theme and a dark theme. In the light one,
the smaller grey text — the second line of a list row, a caption under a
heading, a hint — is *too faint* wherever it sits on a shaded box rather than
directly on the page. There is a published standard for how far apart text and
its background have to be to count as readable (4.5, on a scale where 1 is
invisible and 21 is black-on-white). This text measures 3.4. It happens in
roughly 850 places across the settings screens, the launcher, the network and
sound panels, and more. The dark theme is fine. Fixing it means changing
colours that every screen and every application uses, so it will visibly change
what the light theme looks like — which is why I am asking rather than picking.

**Glossary, because the options below need three terms:**
- **Contrast ratio** — how far apart two colours are in lightness. 1 = identical
  (invisible), 21 = black on white. The standard asks 4.5 for normal-sized text.
- **Card** — any box drawn slightly shaded against the page, to group things:
  a settings row, a search result, a panel section. The theme has four shades
  of card, from barely-there to noticeably grey.
- **Secondary text** — the smaller, greyer text: captions, second lines, hints.
  Deliberately quieter than the main text, and that is the point of it.

**How this was found.** The problem was already logged, but only half of it: the
old note measured the *main* text colour and found two of the four card shades
slightly under the line. A measuring tool built on 2026-09-03 checked every
piece of text the desktop actually draws, against whatever is actually behind
it. The real table (light theme only; bold = below the 4.5 standard):

| ink | on the page | palest card | … | greyest card |
|---|---|---|---|---|
| main text | 7.06 | 5.17 | | **3.69** |
| secondary text | **4.64** | **3.40** | | **2.42** |
| accent (the themed blue) | **4.63** | **3.39** | | **2.42** |

Main text is mostly fine. Secondary text passes *only* on the bare page, at
4.64 — because that is the one place it was ever checked when it was chosen.
Put it on any card and it fails.

### The options

| | *What changes* | Cost |
|---|---|---|
| **A. Darken the greys** (recommended) | Captions and hints in the light theme look a bit darker and less delicate. Nothing moves; only three colours change. | The light theme reads slightly heavier. Contrast between "main" and "secondary" text shrinks, so the visual hierarchy is a little flatter. |
| **B. Lighten the cards** | Cards in the light theme become fainter — the shading that separates a settings row from the page gets subtler. | At the pale end the cards may stop being visible as cards at all, which is its own legibility problem (a different one, about structure rather than text). |
| **C. Forbid text on the darker cards** | Nothing changes colour. Panels would have to stop using the two greyest card shades behind text, and about 190 places would be re-laid-out. | The most work by far, and it constrains every future panel. But it is the only option that changes nothing a user has already got used to. |
| **D. Do nothing** | Nothing. | The text stays measurably below the standard, and it gets *wider* every time a new panel puts a caption on a card, because nothing stops it. |

**Why A is my recommendation.** It is three colour values, it fixes all four
card shades at once, and it is the same move already made once for this exact
palette: the secondary grey was *already* darkened, in June, to get it from 4.37
to 4.64 — but only ever checked against the bare page, which is why it fails on
cards now. Option A is finishing that job properly rather than starting a new
one. B fights the purpose of the cards, and C is real work that also permanently
narrows what a designer may do.

**If you would rather not decide:** say so and I will take A, since D is the
only option that leaves a known accessibility defect shipped, and A is the
cheapest of the three that fix it. It is fully reversible — three constants.

**What happens if this is never answered:** nothing breaks and nothing gets
worse on its own, but the light theme keeps shipping text below the readability
standard, and the count grows slowly as panels are added. The measuring tool is
in place either way, so whatever is decided can be verified rather than assumed.

**Where it bites:** `gui/appearance/src/lib.rs` — the light role table
(`LIGHT_SUBTEXT0`, `LIGHT_SUBTEXT1`, the `LIGHT_*` accents, and the
`LIGHT_SURFACE*` ladder). Full measurements and the module-by-module counts are
in known-issues.md under
`TD-C-TEXT-ON-THE-LIGHT-THEMES-TWO-PALEST-SURFACES-IS-BELOW-THE-CONTRAST-FLOOR`.

**A smaller, separate question found alongside it, same file:** if a user picks
a *custom* accent colour rather than one of the fourteen presets, it is used
exactly as given — the presets get a light-mode variant, a custom colour does
not, and nothing checks it is legible. So a user who picks a pale pink in the
light theme gets accent text at about 1:1, i.e. invisible. Should a custom
accent be (i) adjusted for the mode like the presets are, (ii) accepted but
warned about in the picker, or (iii) left exactly as chosen on the grounds that
the user asked for it? I lean (i), matching what the presets already do.

## A-Q7 — [A] Every build and check on this machine is paying about 70 milliseconds per file opened, and the likely cause is the antivirus. Should the project folder be excluded from real-time scanning? — Status: OPEN (raised 2026-09-03)

**In short:** opening a file on the `D:` drive on this machine costs roughly 70
milliseconds — about five times what the same file costs on `C:`, and about a
hundred times what it should cost once the file has already been read once.
Nothing we write is slow; the cost is paid by the act of opening the file at
all, before a single byte is looked at. Because compiling and checking this
project means opening tens of thousands of files, this is being paid over and
over, by every build and by several of the automated checks. Everything about
the measurement points at Windows Defender's real-time scanning, which inspects
each file as it is opened. Excluding the project folder from that scanning
would very likely make everything here substantially faster — but it is a
security setting, it needs administrator access, and turning off scanning for a
folder is not a decision to make on someone's behalf.

**How we know it is not the disk, the cache, or our code.** Four measurements,
all taken on 2026-09-03 on this machine:

| measurement | result | what it rules out |
|---|---|---|
| A checker that reads 805 source files, timed internally | 98.7 s total, of which **98%** was inside the read call and **0.46 s** was all of its actual pattern-matching combined | our code — there is nothing left to optimise |
| Reading **one** file 200 times | 0.10 s | nothing is inherently slow about a read |
| Reading **200 different** files on `D:` | 13.9 s (~70 ms each) | — this is the effect |
| The same 200 files copied to `C:` and read there | 2.55 s (~13 ms each) | the files themselves, and their size |
| A second full pass over all 805 files, immediately | still 61.8 s | the disk, and the operating system's file cache — a warm cache changes nothing |

Fast when repeated on one file, slow once per *distinct* file, five times worse
on `D:` than on `C:`, and completely indifferent to whether the data is already
in memory. That is the signature of something inspecting each file the first
time it is opened, per drive. Defender's real-time protection is on; its
exclusion list cannot even be *read* without an administrator prompt, so
whether `D:` already has exclusions is unknown.

**What this is costing.** Directly measured: one automated check takes 98.7 s
where the work in it accounts for under half a second, and two more (lane C's)
take 92 s and 95 s for the same reason. Not measured but following from the
same cause: every `cargo build` and `cargo check` in all three lanes opens far
more files than that, so the compiler is paying it too. This is the larger
prize and also the less certain one — a compiler's time is not all file opens,
and I have not isolated the fraction.

**The options.**

1. **Exclude the project tree from real-time scanning** (`D:\visual studio
   projects`, or the individual worktrees). *What changes:* builds and checks
   here get faster, probably substantially; files inside that folder are no
   longer scanned as they are opened. Everything else on the machine is
   unaffected.
2. **Exclude only the build output** (`target` folders). *What changes:* most
   of the win, since compiler output is the bulk of the file traffic, while
   source files and anything downloaded into the tree stay scanned.
3. **Exclude nothing; move the work to `C:`.** *What changes:* the ~5×
   difference suggests `C:` is already faster for this, so the cost drops
   without weakening any setting — but the tree is large, `C:` may not have
   room, and it is a disruptive move for an uncertain gain.
4. **Leave it.** *What changes:* nothing; the cost stays.

**What I would do, and why it is still your call:** option 2. The build output
is generated by our own compiler from sources that were themselves scanned,
which is the weakest case for scanning it, and it is where nearly all the file
traffic is. Option 1 is meaningfully faster still but covers source files and
anything a dependency downloads into the tree, which is a real reduction in
coverage. I am not making that trade unasked — it is a security setting, it is
system-wide, and it needs an administrator either way.

**What happens if this is never answered:** nothing breaks; everything just
stays slower than it needs to be, and gets slightly worse as the tree grows. It
also keeps distorting decisions — a gate that costs 90 s gets placed, deferred
or argued about differently than one costing 10 s, and three such arguments
have already happened on the assumption that the checkers were the problem.

**Where it bites:** `scripts/check-selftest-reinit.py` (98.7 s, wired
2026-09-03 after every cheap gate for exactly this reason),
`scripts/check-key-release-wiring.py` (92 s) and
`scripts/check-window-wiring.py` (95 s), both lane C's; and every `cargo`
invocation in all three worktrees. The profiling is written up in
`requests/a-b-wiring-check-selftest-reinit-and-a-correction-it-runs-nowhere.md`
§5 and `requests/a-c-i-wired-three-of-your-gates-fixtures-not-their-checks.md`.

### 2026-09-04 — a second symptom, and this one is not just slowness

The case above rests entirely on *time*: everything is slower than it should
be. There is now a second, independent symptom of the same suspected cause, and
it leaves visible debris rather than merely costing seconds.

`build/` in the lane-A worktree holds **fourteen empty directories** — leftover
test fixtures, seven from each of two runs of `scripts/test-boot-test.py` on
2026-09-04. Empty is the whole point: the cleanup code deleted everything
*inside* each directory successfully and then failed to delete the now-empty
directory itself. On Windows that failure has essentially one cause — something
else still had the directory open for the fraction of a second after its last
file went away. A file scanner inspecting each file as it is touched is exactly
such a something, and it is the same drive, the same tree, and the same
suspected program as the 70 ms-per-open measurement above.

Why this matters for *this* decision rather than being a separate bug: it moves
the cost of leaving the setting alone out of the "everything is a bit slower"
column. The slowness is invisible and uniform; this is a concrete, accumulating
mess in the directory people look in first when a boot test misbehaves, growing
seven entries per suite run with no upper bound. It also raises the prior on
option 1 or 2 actually working, because it is a *behavioural* fingerprint
(a held handle) rather than a timing one, and timing arguments always leave
room for "maybe the disk is just slow".

It does **not** change the recommendation, and it must not be read as a reason
to rush the setting. The retry loop that makes the cleanup robust is correct
whether or not an exclusion is ever added — code that assumes no scanner is
running is wrong on any Windows machine, and that fix is filed separately as
`A-FIXTURE-CLEANUP-LEAVES-EMPTY-DIRECTORIES-IN-BUILD-AND-CANNOT-TELL-YOU` in
`known-issues.md`. The point here is only that the evidence for the diagnosis is
now of two different kinds instead of one.

### Addendum 2026-09-05 (lane B) — the disk *is* just slow, and there are two empty SSDs in the machine

The paragraph above closes with "timing arguments always leave room for 'maybe
the disk is just slow'". It turns out the disk is just slow — literally, as
hardware — and that was never checked. This does not overturn the Defender case,
but it changes option 3 from a vague suggestion into the concrete one, and it
means the two causes are adding rather than competing.

**`D:` is a mechanical hard disk. `C:` is a solid-state disk. So is a third
drive that is nearly empty.** From `Get-PhysicalDisk` and `Get-Volume` on
2026-09-05:

| drive | device | kind | free |
|---|---|---|---|
| **`D:`** — the entire project, all three worktrees | WDC WD2004FBYZ, SATA | **spinning disk** | 333 GB |
| `C:` — Windows | Samsung 980 PRO, NVMe | solid state | 129 GB |
| **`E:`** | Samsung 960 EVO, NVMe | solid state | **325 GB, essentially unused** |

A spinning disk moves a physical arm for every piece of data that is not next to
the last piece; a solid-state disk does not. That difference is the whole story
below.

**Measured, same day, on an otherwise idle machine:** average time to service one
read is **27.5 ms on `D:` against 0.13 ms on `C:`** — a factor of **200**, not
the factor of 5 the file-open experiment saw. (The experiment above measured
whole file opens, where a fixed per-open cost is mixed in with the seek; this
counter measures the disk alone.) `D:` also sat at a queue length of 8, i.e.
saturated, while nothing in the project was building.

**What that does to a build.** I lost most of a working session to this before
diagnosing it, so the numbers are unhappily concrete:

- One `cargo build -p sshd` ran for **9 minutes while using 0.6 seconds of CPU**.
  I attached a debugger rather than guess: every sample was in
  `rustc` → `SearchPath::new` → `FindNextFileW` — it was *listing a directory*,
  not compiling. The directory is `target/x86_64-pc-windows-gnu/debug/deps`,
  which holds 69,161 files, and `rustc` lists it once per invocation before it
  compiles anything.
- Deleting that 64 GB directory with `rd /s /q` ran **45 minutes and freed no
  measurable space**.
- *Renaming* a directory — one metadata write, instant on any healthy volume —
  did not complete in six minutes.

**Why this belongs to your Defender question rather than being a separate one:**
options 1 and 2 buy back the per-open scan; they do not buy back the seek. On
this hardware the seek is the larger of the two, and option 3 — which the entry
above rates "disruptive, uncertain gain, `C:` may not have room" — turns out to
need neither `C:` nor an administrator, because `E:` is a solid-state disk with
325 GB free and nothing on it. *What changes if the tree moves there:* every
build, check and gate in all three lanes gets faster by something in the region
of the 200× per-read gap, no security setting is weakened, and no admin prompt
appears. The counter-argument is that I do not know what `E:` is *for* — it is
your machine, and an empty disk may be empty on purpose.

A cheaper half-step, if the tree itself should stay put: move only the three
`target/` directories to `E:` (cargo's `build.target-dir`, or a directory
junction). That is where nearly all of the file traffic and all of the 64 GB
is, it is regenerable so nothing is at risk, and it is one line of config to
undo.

**If this is never answered:** the same as before, plus the specific knowledge
that a single-crate compile can block for nine minutes on directory listing
alone. I have not changed any setting or moved anything.

## Which group is a user in? Two files answer, and nothing keeps them agreeing. (lane B, 2026-09-06)

**In short:** "Alice is in the `audio` group" is written down in two separate
places on this machine: once in `/etc/group`, which lists the members of each
group, and once in Alice's own account record in `/etc/users.yaml`, which lists
the groups she is in. Nothing makes the two agree — they are simply two copies
of the same sentence, and copies drift. We already fixed exactly this problem
for *user accounts* (`design-decisions.md` §353: one file is the truth, the
others are generated from it). The question is whether to do the same for
groups, and if so which of the two copies survives.

**How it bites today:** a program that asks "what groups is Alice in?" gets a
different answer depending on which file it happens to read. `id` and `chown`
read one, the graphical settings app reads the other. If they disagree, a file
Alice should be able to open looks closed to one tool and open to another. This
is the same class of defect that had `sudo` and `doas` disagreeing about who
was an administrator, which is what §353 was decided to end.

**What has been done so far (2026-09-06):** `useradd`/`usermod`/`groupadd`/
`groupmod`/`groupdel` are one binary, and it now updates *both* copies through
a single set of methods, so it cannot change one and forget the other. That
closes the hole for the only tool that currently writes groups. It does not
close the hole — the next writer has nothing stopping it, and a hand-edit of
either file makes them disagree immediately.

### The options

| Option | *What changes:* | |
|---|---|---|
| **A. `/etc/groups.yaml` is the truth; `/etc/group` and `/etc/gshadow` are generated from it** (recommended) | Nothing visible day to day. Editing `/etc/group` by hand stops sticking — the next account change overwrites it — exactly as editing `/etc/passwd` already stops sticking. | The same answer §353 gave for users, for the same reasons, and it reuses the machinery that already exists. It is also the most work: a new file, a new parser, and the group half of `useradd` rewritten. |
| **B. Keep two files but make the account record's `groups` list the only writable one, and generate `/etc/group` from the accounts** | Also nothing visible. No new file: the group's member list becomes a view of who claims membership. | Cheaper than A. But a group has facts of its own — its gid, its password, its administrators (`/etc/gshadow`) — that no account record has anywhere to put, so those would still need a home. That is how §353's rejected option B failed. |
| **C. Drop the `groups` list from account records; `/etc/group` is the only answer** | The graphical settings app has to read `/etc/group` instead of the account file it reads now. | The smallest change, and it puts the fact in the file the POSIX world expects. It contradicts `design.txt`'s "configuration files will be yaml" for one of the two most security-sensitive files, which is the objection that sank the same option for users. |
| **D. Leave it. Keep both copies and keep them in step by hand.** | Nothing. | Free today. The cost arrives with the second writer, and it arrives as a security bug rather than a visible breakage. |

### If it is never answered

Nothing gets worse on its own, and nothing is blocked: `useradd` keeps both
copies in step, and it is the only tool that writes groups. The risk is a
future one — the *next* program that writes a group membership starts the drift,
and it will be found the way the `sudo`/`doas` split was found, by reading the
code rather than by anything failing.

The full context, including the non-atomicity of a save across the two stores,
is in `todo.txt` under "`/etc/group` and `/etc/gshadow` are still hand-written,
not generated".

## C-Q11: Should something build every crate before a merge? (raised by lane C, 2026-09-06)

**In short:** The lock screen — the program that asks for your password when
the machine is locked — was broken for a day and nobody knew, because nothing
in this project ever tries to build it. Somebody changed a shared library, the
lock screen still referred to the old version, and no test anywhere failed. It
was found by accident. There are 142 more programs in the same position
(143 counted, one broken). The question is whether to add a slow check that compiles everything
before work is merged, and if so, what shape it takes — because whatever we
pick, all three lanes have to live with it.

### What happened

`5264cba7a` (lane B) removed a function argument and a module from `authlib`,
a shared login library. Its commit message says "no caller changes", which was
true of every caller *that lane can see*. `apps/lockscreen` is a caller in lane
C's tree. It stopped compiling and stayed that way until an unrelated tidy-up
happened to run clippy on it.

Nothing caught it because nothing builds `apps/`:

| What runs today | Why it misses this |
|---|---|
| the boot test | builds for the bare-metal target; `apps/*` are not in `default-members` there |
| each lane's own `cargo test -p …` | a lane only builds what it touched |
| `check-window-wiring.py`, `check-gates-are-wired.py` | read source text; they never invoke the compiler |
| CI | there is none |

A `cargo check --workspace --target x86_64-pc-windows-gnu` would have caught it.
Nobody has a reason to run one.

### Why this is yours and not mine

Any answer gates all three lanes' merges, and the cost lands on whoever is
merging — not on me proposing it. It is also genuinely slow: a cold check of
the whole workspace is minutes, and it is minutes *added to every merge*.

### Options

**A. A pre-merge `cargo check --workspace` for the host target.**
*What changes:* merging to `main` takes a few minutes longer, every time, and a
lane cannot merge while any crate in the tree is broken — including one broken
by a different lane.
The strongest guarantee, and the harshest: lane A is blocked by lane C's typo.
That is already true of the boot test, which builds the whole workspace, so
this is a difference of degree.

**B. A nightly (or once-per-session) sweep that only reports.**
*What changes:* nothing blocks; a broken crate is found within a day instead of
within a chance encounter, and lands as a `known-issues.md` entry or a
`requests/` file for whoever owns it.
Cheap and unintrusive. Does not stop a breakage being merged, only shortens how
long it lives.

**C. Require the grep before the claim.**
*What changes:* nothing mechanical; the convention becomes that a commit
message may not say "no caller changes" about a shared library until
`grep -rl <symbol> apps/ gui/ net/` has been run.
Costs nothing and would have caught this exact case, but it is a rule enforced
by remembering it, which is the kind that decays.

**D. Do nothing.**
*What changes:* nothing. Broken app crates accumulate silently and are found
one at a time by whoever next touches them.

### Measured — then measured wrong, twice. Read this before the recommendation

I first recommended the cheap option (B) on the assumption that a full check
costs "minutes added to every merge". I then measured, got 58 seconds, and
reversed to the gate (A). **Both of my numbers were about the wrong thing, and
two other lanes found the holes.** The corrected position is below; the history
is kept because the *shape* of the error matters more than the number.

**What I actually measured.** `cargo check` over the 143 crates in `apps/`,
enumerated by name and passed as `-p` flags, on an otherwise idle machine:
58 seconds warm, exactly one broken (the lockscreen, since repaired).

**Hole 1 — that is not the command the gate would run.** The gate's mechanism
is `cargo check --workspace` for the host target. On a tree lacking
`services/hello/target/.../hello`, that does not merely take longer — it *fails
outright*, because `kernel/src/container.rs` embeds that artifact and it is not
built by anything cargo runs. My `-p apps/*` enumeration skipped the kernel and
so never met it. So "58 seconds, all clean" is a number about a command nobody
would run, and it is the reassuring half. (Found by lane B. Details in
`known-issues.md` →
`A-THE-KERNEL-EMBEDS-A-BUILD-ARTIFACT-NOTHING-BUILDS-AND-NOTHING-TRACKS`;
lane A is fixing the build-graph edge, so this is a prerequisite for the gate,
not a permanent obstacle.)

**Hole 2 — an idle number is the wrong number for a shared machine.** This
machine saturates on a *single* cargo run; that is measured, not folklore. So
the cost of a gate that fires on every merge is not its own wall clock — it is
its wall clock *under contention*, **plus the degradation it imposes on the
other two lanes for the duration**. For a gate that runs many times a day
across three lanes, that second term may well dominate the first.

That distinction is the same one lane A's case for the SSD migration turned on
(18 random reads/sec contended against 100–150 idle — a 5–8× gap that becomes
~40× under three-lane load). I read that argument, agreed with it, and then
failed to apply it to the thing I was proposing. An idle measurement is the
optimistic half of any question about a machine three agents share.

### The measurement, taken properly (lane B, 2026-09-06)

`cargo check --workspace` for the host target, on `E:/os-lane-b` at
`b9b7c61df`, machine **idle**:

| | |
|---|---|
| **139 s** | full check from cold — the kernel had never been checked in that tree |
| **15 s** | immediate re-run, nothing changed — the no-op cost |
| **14 s** | after touching one source file — **the number this entry should quote** |

The gate's cost is the third row: a merge is warm, because the lane just built
and tested the thing it is merging, and something has changed. **Fourteen
seconds.**

Lane B stated the cold/warm spread explicitly — 139 → 15 — for the reason that
my 58 s was misread for want of exactly that context. Quoting 14 s as a
from-cold figure would be off by two minutes.

**Still to come:** the same measurement under contention, which is the one the
recommendation should turn on (see Hole 2). Lane B is taking it during lane A's
boot test.

### Scoped versus whole, measured — and my prediction was wrong

The comparison above (39 s scoped against 14 s whole) was invalid: mine was
**cold** for ~156 crates, lane B's was **warm incremental**. Lane B then took
the missing measurement, both sides warm-incremental and **both under identical
contention** (during lane A's boot test):

| | one app file touched | no-op |
|---|---|---|
| **scoped** (158 `-p` flags) | **8 s** | 7 s |
| **whole** (`--workspace`) | **15 s** | 16 s |

I predicted the enumerated form would lose, because naming 158 packages makes
cargo do work proportional to the set *named* rather than the set that
*changed*. It wins, about 2:1. Recorded because this entry has a running theme
and I am not exempt from it.

**Why, and it is worth knowing independently of the gate.** Neither lane B nor I
had checked what `--workspace` actually enumerates before reasoning about it.
Both of us pictured "the 158 apps, plus the kernel". Counted from the manifest
globs:

| glob | crates |
|---|---|
| `apps/*` | 143 |
| `gui/*` | 15 |
| `init/*`, `net/*` | 4 |
| **`userspace/*`** | **2,759** |

So `--workspace` is roughly **2,900 members**, not 160. Scoped checks 158
things and whole checks about 2,900; the subset being cheaper is not a
surprise once the number is in front of you. It was in front of neither of us.

### What that means: cost is not the axis, coverage is

At 8 s against 15 s, **both under contention**, cost cannot decide this. Seven
seconds is inside the noise of a merge. So the scope should be chosen for what
it *covers*, and there the two differ sharply (lane B's argument, and I think it
is right):

- A gate scoped to `apps/` + `gui/` catches a breakage whose **victim** lives in
  those trees. It would have caught my `guitk::Event` one.
- It would **not** have caught the `authlib` → `init/login` one, because
  `init/` is outside the scope — and that is the same commit that started this
  entry.

Victims can be anywhere, so only whole-workspace covers cross-lane API changes,
which is the case C-Q11 exists for. **Do not scope for cost.** If a scope is
ever wanted, it must be justified by coverage, and this one cannot be.

That also disposes of the `include_bytes!` prerequisite as a scoping argument:
it is a real bug and worth fixing on its own merits, but it is not a reason to
reduce the gate's coverage, and 15 s says the coverage need not be traded for
cost.

### The scenario that matters, now measured: worst case 49 seconds

Every figure above is **"one file touched"**. That is not when a gate fires.

`CLAUDE.md` step 1 requires `git fetch origin && git merge origin/main` before a
lane starts work, and the merge-up happens at the end. So the gate runs on a
tree that has just absorbed *another lane's* commits — which on a bad day means
a shared crate (`guitk`, `guiremote`, `authlib`) changed, and everything
downstream of it rebuilds. Downstream of `guitk` is 158 crates; downstream of
`authlib` is most of `userspace/`.

Lane B measured it — touch each shared crate, re-check the whole workspace,
under contention:

| crate touched | dependents | whole-workspace re-check |
|---|---|---|
| `authlib` | 12 | 19 s |
| `posix` | 13 direct | 22 s |
| `guitk` | 144 | 26 s |
| **`quoting`** | **773** | **49 s** |

**Worst case in the tree is 49 seconds.** It never approaches the 139 s cold
figure because `cargo check` does no codegen: a cold run pays to *compile* the
dependency graph, a post-merge re-check only pays to re-read it — about 63 ms
per downstream crate.

**Two things I had wrong here, both the failure this entry keeps cataloguing.**

*First:* I wrote that "downstream of `authlib` is most of those 2,759". It is
**12**. That is wrong by two orders of magnitude and it mattered, because the
whole expensive-case worry rested on the intuition that a shared-crate change
rebuilds most of the tree. It does not. **The blast radius that justified this
entire question was twelve crates.** What made it serious was not that there
were many victims but that they were in a *different lane* from the author —
which is the thing a gate fixes and a bigger number would not have made truer.

*Second:* the widest shared crate is not `guitk`, `posix` or `authlib`. It is
**`quoting`, at 773 dependents**, and it appeared in none of the three candidate
lists either of us was reasoning from. Lane B measured it precisely so the worst
case would not be merely the worst of the ones we happened to name. Same shape
as the six embedded artifacts and the `--workspace` member count: enumerate
first, then reason.

Dependent counts verified independently here (`grep -rl` over every
`Cargo.toml`): `quoting` 773, `guitk` 144, `posix` 13, `authlib` 12. Member
counts: `userspace/*` 2,759, `apps/*` 143, `services/*` 76, `gui/*` 15,
`init/*` 2, `net/*` 2.

### Recommendation

**Adopt C now, regardless. Defer the A-versus-B choice to one number that does
not exist yet.**

**Adopt C as well — it is free — but it is not the answer and should not be
credited as one.** The convention: a commit message may not claim "no caller changes" about a
shared library until `grep -rl <symbol> apps/ gui/ net/` has been run. It costs
nothing and needs no infrastructure.

What I originally wrote here was that it "would have caught this". That is true
of the first two breakages and **not** of the third, which I caused myself a few
hours after writing this entry — see the third row below. I added a variant to a
shared enum in `guitk`, updated the three consumers I was thinking about, and
did not grep for the rest. At that moment I had the failure mode more firmly in
mind than anyone in this project has ever had it: I had just written up two
other lanes' instances of it, in this file, arguing for a gate to catch it.

So the honest assessment of C is that it is a *discipline*, and this project now
has one clean experiment on whether discipline is sufficient here. It is free,
it costs nothing to keep, and it will catch the cases where the author pauses to
think. It will not catch the cases where the author is confident — which are the
same cases, because confidence is what stops you grepping.

### The four breakages, all one shape

| # | Change | Consumer missed | Found by | Author's state |
|---|---|---|---|---|
| 1 | `authlib` drops `with_stores`'s second argument (§353) | `init/login` | lane B, later | believed the caller list complete; had grepped `userspace/*/Cargo.toml`, which covers neither `apps/` nor `init/` |
| 2 | the same change | `apps/lockscreen` | lane C, by accident, a day later | same commit, same belief — its message says "no caller changes" |
| 3 | `guitk::Event` gains `SettingsChanged` | `apps/stickynotes`, `apps/explorer` | lane A's boot test, 30 min in | lane C — me — hours after writing this entry |
| 4 | the same variant, unmerged in lane B's tree | the same two crates | lane B's own C-Q11 measurement run, rc=101 | lane B, *while measuring the cost of the gate that catches it* |

A type or a signature changes, some consumers are updated, others are not, and
nothing notices until a boot test half an hour in or a person happens to look.
**Four times in one day, by all three lanes, in three different subsystems** --
and the last two by the two people who at that moment were most alive to the
risk.

**Numbers 3 and 4 are the same failure by the two people least able to make
it, and together they say more than any timing here.** Number 3 was the author
of this proposal committing the failure hours after writing the argument for a
gate. Number 4 was the person pricing that gate committing it during the
measurement. Neither of us failed for want of knowing — we had the failure mode
in mind more firmly than anyone in this project ever has.

The mechanism that refutes is *priming*, not memory. The grep in option C is a
step you take when you doubt yourself, and neither of us doubted. A convention
that fires on doubt cannot cover the confident case, and the confident case is
most of them. That is why C is kept below as free-and-worth-having rather than
as a control anyone should rely on.

**Number 3 carries one extra piece of evidence the others do not**, and it bears
on what a gate is worth. The two crates that broke were the two matching their
events *exhaustively* — `explorer` names every event it declines, `stickynotes`
matches every variant. Roughly 140 other apps end with a catch-all arm and
accepted the new variant in silence. So adding to a shared enum punishes exactly
the consumers that opted into being told, and rewards the ones that opted out.
Compiler exhaustiveness is the closest thing to a free gate this codebase has,
and it only fires where someone chose to leave it armed.

**The dangerous property is discarding, not catching** (lane B and lane A, and
this corrects my first statement of it). A catch-all that *forwards* the value —
`Err(e) => report(e)` — still surfaces a new variant as its own text, without
the crate being recompiled against the new definition. It is
`_ => {}` that swallows it. So "140 crates end in a wildcard" is the wrong
count and would condemn arms that are fine; the count that matters is
catch-alls that *drop* the value, which is not a thing grep can tell you.

That has a direct consequence for what a gate is worth: **a compiling workspace
is a floor on correctness, never a proof.** A check cannot distinguish a
forwarding catch-all from a discarding one without reading it, so the gate
guarantees only that every crate still builds — which is exactly the guarantee
being priced here, and worth not overselling.

**Between A and B, here is the decision rule rather than a verdict**, so that
the answer follows from lane B's measurement instead of from my instinct:

| If the contended workspace check costs… | Then |
|---|---|
| under ~1 minute | **A** — a fair price for "the tree compiles", and the only option that *stops* a breakage rather than shortening its life |
| a few minutes, with the other lanes degraded throughout | **B** — the nightly sweep. The guarantee is no longer cheap, and a standing tax on every merge stops being worth it |

**Every figure is now in**, and they all land in the first row:

| scenario | cost |
|---|---|
| no-op / one file touched, idle or contended | 14–16 s |
| after a merge touching `authlib`, `posix` or `guitk` | 19–26 s |
| **after a merge touching the widest crate in the tree (`quoting`, 773 dependents)** | **49 s** |

Nothing costs a minute, including the worst case, including under contention.

**I recommend A, whole-workspace, and I have dropped the hedge.** I said I
would drop it if the worst realistic case came in under a minute; it is 49
seconds. Everything that was uncertain when this entry was written has since
been measured, and every measurement moved toward A:

- the price is seconds, not minutes, at every point in the range;
- contention costs almost nothing (14 s idle against 15 s contended);
- the failure went from "one, found by accident" to **four in one day, by all
  three lanes**;
- and the two most recent were committed by the person who wrote the argument
  for a gate and the person measuring its cost — which is what convinced me the
  convention in C cannot be the answer.

The standing caveat is unchanged and the operator should still apply it: **I
have been wrong on this entry's numbers repeatedly** — first guessing minutes,
then measuring the wrong command, then quoting an idle figure for a shared
machine, then predicting the scoped/whole comparison backwards, then putting
`authlib`'s blast radius at ~2,759 when it is 12. Every one of those was
corrected by another lane rather than by me. What that argues, though, is not
that the recommendation is unreliable — it is the single best argument *for* the
recommendation. Five wrong numbers from someone paying close attention, caught
only because two other agents happened to check, is precisely the case for a
mechanism that fires without being invoked.

**The objection to A, restated in the better form lane B gave it.** I had
written it as "the gate lets one lane block another's merge", and answered that
this is already true of the boot test. Lane B's version is stronger: *the
coupling exists whether or not there is a gate.* Today it ran in both
directions — lane B's `authlib` change broke lane C's lockscreen, and that
lockscreen then stood between lane B's own `init/login` fix and a green `main`.
The gate does not create that coupling. It moves discovery from "another lane
trips over it days later" to "the lane that caused it, at the moment it caused
it". The cost lands on whoever merges next *without* the gate; with it, it lands
on whoever broke it.

**One condition on A if it is chosen.** Scope it to the members that genuinely
check cleanly on the host, and scope it for *that stated reason* — never to
route around a member that is broken. A gate excluding the kernel because the
kernel does not build is a gate that has hidden the defect it should have
reported. (Lane B's phrasing; it is the right test for whether a scoped gate is
honest.)

### If this is never answered

The current state is safe but degrading, and nothing is blocked: the lockscreen
is repaired, `main` is green, and adopting C costs nothing and needs no answer
from you. Everything this question needed measuring is measured; it is waiting
only on you.

What stays open is the gap. Every shared-library change is another chance for a
breakage that nothing reports and that is found weeks later by somebody who did
not cause it. **On the day this was raised it happened four
times** — twice from `authlib`'s single-store change (`apps/lockscreen` and
`init/login`, two lanes, neither caught by anything but a person looking), and
once from my own `guitk::Event` addition, which a boot test caught thirty
minutes in, and once again from that same variant in lane B's tree, found by
the run measuring what a gate would cost. All four are fixed; the mechanism
that let them through is untouched.

The cost grows with the number of app crates, which is growing.

# Resolved

**The body above holds OPEN questions only.** When the operator answers one,
write it up in `design-decisions.md` as a `Decided by: Operator` entry,
**delete the entry from the body**, and add one line here. That is the whole
point of the file: it is scanned for what still needs a decision, so an
answered question left in the body is pure cost — and, being older, it sorts
*first*, right where it is most in the way. (Why this is not append-only:
`design-decisions.md` §437.)

## Resolved — lane A

- Q45 Convert the whole shell to bytes, or only the expanded word? — resolved
  2026-08-21 (§261): **B, the expanded word.** One data path — keystroke to
  syscall — goes byte-clean end to end; the source line stays text, as in bash.
- Q49 Modern AMD graphics: write it blind, buy hardware, or say we don't
  support it? — resolved 2026-08-21 (§262): **A for now**, C someday. The
  operator's "write it blind but label it untested" variant is recorded in the
  entry along with why it was not adopted.
- Q50 The Intel iGPU driver we also cannot run — which way? — resolved
  2026-08-21 (§263): **C.** Switch the iGPU on in firmware, boot SlateOS on
  this PC's bare metal from a USB stick, then write i915 against the real chip.
  Operator does the physical half; lane A readies the bootable-USB path first.
- Q51 Start the Mesa port now, or leave 3D parked? — resolved 2026-08-21
  (§264): **B, do the port** — sequenced after wifi, before Chromium. Chromium
  uses Mesa heavily but bundles SwiftShader, so Mesa is a performance
  prerequisite for it, not a functional one.
- Q52 Should the contamination-canary check keep failing on noise? — resolved
  2026-08-21 (§265): **D then C.** 20+ idle rounds first, then a shifted-band
  rule instead of zero tolerance.
- Q53 71% of benchmarks move >10% from a no-op rebuild — change the rule? —
  resolved 2026-08-21 (§266): **E.** Restate the threshold against each
  benchmark's measured band now; real hardware (unblocked by §263) is the fix
  that makes it mean something again.
- Q54 Switch to the 3.5× faster accelerator, split, or stay? — resolved
  2026-08-21 (§267): **E then C.** Measure whether the fast accelerator removes
  the noise; if so split — benchmarks fast, correctness gate stays on TCG where
  SMEP/SMAP/UMIP are actually exercised.

## Resolved — lane B

- B-Q7 Which copy of the command-line tools is canonical, after the premise
  behind June's §8 turned out to be false? — resolved 2026-09-07 (§1005,
  `Decided by: Operator`): **B, `coreutils` is the one home.** The better half
  of each of the 41 duplicate pairs survives inside it; the duplicate crate is
  deleted; the 45 bundle-only names stay put rather than becoming 45 crates.
  The operator noted that the "dependency shape that exists nowhere in the tree
  yet" bullet reads like an effort argument and would carry no weight if it
  were one — it is an architectural argument (option A cannot be reached
  without a per-tool crate importing the bundle, the shape §8 set out to
  retire), and B wins on the other reasons regardless. §8 superseded, §359
  un-suspended.
- 2,288 of the 2,756 commands in `userspace/` report success for work they
  never did — which ones do we keep? — resolved 2026-09-07 (§1006,
  `Decided by: Operator`): **stricter than my option A — delete every
  fabricating command, not only the ones that can never work.** A name that
  could be ported one day is added back when it is implemented, not before,
  because a command's existence is a claim made to `command -v` probes as well
  as to people, and a refusing stub answers "yes" to the probe and fails later.
  The audit script is pinned as a ratchet once the deletion lands.
- The test machine cannot produce random numbers, on purpose, and eighteen
  tests in the apps depend on that — should it start? — resolved 2026-09-07
  (§1007, `Decided by: Operator`): **A, land it.** Lane C rewrites its eighteen
  `assert_eq!`-on-two-draws tests in its own tree; lane B files the request and
  the list rather than editing inside lane C's globs. `main` may be red in
  between, which was accepted as the lesser cost.
- B-Q5 70 compiled programs are stored in git and go stale without git
  noticing — keep storing them, or rebuild on demand? — resolved 2026-08-21
  (§355, `Decided by: Claude (autonomous)`): **B, build on demand**, against my
  own earlier "A for now" and against lane A's revised case for C. Measuring the
  arrangement rather than arguing about it settled it: the stamp gate covers
  **9 of the 70**, and **60 of the unguarded 61 were stale at that moment** — so
  drift is the steady state, not an occasional accident. C cannot reach those 61
  at all, because their compiler (fastpy) is a *different repository* whose
  revision this tree cannot record. Rebuilding every fixture costs ~65 s, and the
  kernel already `include_bytes!`s an untracked build output, so B demands no
  toolchain the tree did not already demand. B ships with the guard inverted —
  the rootfs build must refuse to stage a short fixture set, because
  `load_test_elf` self-skips and naive B would otherwise turn stale tests into
  *no* tests, silently green.
- B-Q6 Should the console login prompt obey the system-wide failed-guess
  delay? — resolved 2026-08-21 (§354): **A, and `su` joins with it.** Both obey
  the shared tally for every account including root; the delay-your-neighbour
  effect is accepted as bounded. `passwd` contributes but is never delayed,
  because it gates the remedy rather than access.
- B-Q4 Two user databases that drift apart — which one is real? — resolved
  2026-08-21 (§353): **C, one store with two faces.** `/etc/users.yaml` is the
  truth; `/etc/passwd` and `/etc/shadow` are generated from it on every change.
- B-Q3 Password hashes that can no longer be checked: fail closed, or admit
  those users once more? — resolved 2026-08-21 (§352): **A, fail closed.** Root
  runs `passwd <user>`; no authentication code is kept alive to accept a known
  non-hash.
- B-Q2 GNU's curly quotes in diagnostics, or keep straight ones? — resolved
  2026-08-21 (§351): **B, follow GNU.** Curly marks in the `invalid argument`
  family only; file names stay straight, as they are in GNU.
- Q48 Real kernel objects for "set the clock" / "bind port 80" / "raise your
  own rlimit", or leave them denied? — resolved 2026-08-21 (§350): **B, objects
  for all three.** The operator took B for the port too, where the
  recommendation had been to drop the rule; an object can express "everyone may"
  and dropping the check cannot express anything else.
- B-Q1 Which tzdata do we ship, from where, and how is it updated? — resolved
  2026-08-15 (§311): ship **full tzdata**, vendored as prebuilt TZif binaries
  and updated as a `pkg/` package.

## Resolved — lane C

- C-Q1 Should normalization consult font coverage? — resolved 2026-08-15
  (§428): **no** — normalization stays font-blind, and the font-fitting stage
  decomposes what the face cannot draw. This was the last 339 sweep
  disagreements, all one question.

- C-Q3 Should all three lanes keep publishing finished work through the one
  shared `os` worktree, after two collided in it? — answered 2026-08-21 by the
  operator, **b**; written up 2026-08-24 (§538): no. A lane publishes with
  `git push origin lane-<x>:main`, a fast-forward that needs no working
  directory and is *refused* rather than tangled if another lane got there
  first. `os` becomes a read-only window on the result.

- C-Q5 Should this OS keep writing its own cryptography by hand? — answered
  2026-08-21 by the operator, **c**; written up 2026-08-24 (§539): the
  primitives (hash, cipher, password hash) are ported from vetted
  implementations; the vault format and the service plumbing on top stay ours.
  The line falls where testing stops reaching — a cipher can compute the right
  answer and still leak the secret through its timing, and no test we write
  sees that, whereas a file format that loses a record is an ordinary bug. The
  eleven hand-written SHA-256 copies collapse to one ported one.

- C-Q4 Nothing can print, and two disconnected halves of a printing system
  exist — which should applications talk to? — answered 2026-08-21 by the
  operator, **c**; written up 2026-08-24 (§540): neither. Printing becomes a
  background service applications submit jobs to, so a job outlives the
  application that started it. Lane C had recommended the cheaper shared
  library (b); the operator overruled it as a stop-gap that would only be
  rewritten, since a library and a service differ in *who owns the job*, and
  every caller written against the library is a caller to migrate.

- C-Q2 On a line mixing Hebrew or Arabic with English, should the Right arrow
  key move the caret one character later in the sentence, or one step right on
  the screen? — answered 2026-08-21 by the operator, **b (visual)**; written up
  2026-08-24 (§541): the screen. A key named for a screen direction follows the
  screen; Home/End and word-motion stay logical, because those name positions
  in the sentence. Caveat carried into the implementation: a widget that does
  not also remember which side of a direction boundary the caret is on will
  **skip a whole right-to-left word** in one press — worse than the old
  behaviour, so a half-switched widget is a regression, not a partial win.

## Resolved — pre-split (unprefixed `Q<n>`, single-agent era)

These numbers are not to be extended; new questions use `A-Q<n>` / `B-Q<n>` /
`C-Q<n>`.

- Q55 [C] The installer read `size = "100 GB"` as 107 GB — should a decimal
  spelling mean a decimal number? — answered 2026-08-21 by the operator, **c**;
  written up 2026-08-24 (§542): neither spelling is guessed at. `GB` is
  **refused**, with an error naming both alternatives; only `GiB` and bare `G`
  are accepted. Lane C had weakly recommended honouring the spelling (b) while
  naming c the honest option. The deciding point: both "pick one" answers leave
  some existing config file meaning something its author did not intend, with
  nothing announcing it — and a partition table is not a place to be helpful
  about a guess.
- Q45 Should `RenderCommand::Text` carry an overflow policy, rather than text
  being cut mid-glyph with no ellipsis? — resolved 2026-08-15 (§427): **yes** —
  the draw command carries the policy and the compositor draws the ellipsis.
  (Note: `Q45` was reused by lane A for an open question while this one still
  sat in the body — an ID collision the old append-only rule made unavoidable
  and this split removes.)
- Q44 Which mapping of our `(ResourceType, Rights)` handles onto Linux `CAP_*`
  bits, given libc reported "all capabilities held" to everything? — resolved
  2026-08-15 (§312): a **conservative projection** of the real handles, not a
  fiction.
- Q42 One-shot repo-wide rustfmt, or keep formatting only touched files? —
  resolved 2026-08-15 (§310): **one-shot repo-wide**, with a
  `.git-blame-ignore-revs` file alongside so the reformat does not poison
  `git blame`.
- Q40 Should osh reproduce bash's *null array element*, which looks like an
  upstream defect? — resolved 2026-08-15 (§309): **no** — byte-fidelity with
  bash has an "unless it is a defect" clause.
- Q41 Should bash be cross-compiled instead of osh reimplemented? — resolved
  2026-08-14 (§305): **both** — osh ships as the shell, cross-compiled bash
  ships beside it, and osh's bash-fidelity scope is frozen.

### Earlier (Q1–Q39)

- Q38 Should osh be locale-aware, or UTF-8-only? — resolved 2026-08-07 (§104):
  **option A — osh is UTF-8-only**, and `scripts/osh-bash-diff.py` moves to a
  UTF-8 locale so the reference bash agrees. The rejected scope (making osh
  locale-aware as bash is) stays written down in `known-issues.md` under
  `TD-OILS-THE-CORPUS-HARNESS-RUNS-THE-REFERENCE-BASH-IN-THE-C-LOCALE`, at the
  operator's request, so a future change of mind starts from a survey.

- Q38 Add antivirus exclusions so the osh corpus sweep is runnable again? —
  resolved 2026-08-07 (§106): **option A**, scoped to *process* exclusions for
  `bash.exe` and `osh.exe` rather than blanket path exclusions. The command
  itself still needs an elevated shell and is written out in §106.
  (Note, as on `Q45` above: `Q38` was issued twice, on the same day, for two
  unrelated questions — the same append-only collision. Both were answered
  before it could matter, and the numbers are left as they were rather than
  edited, because this list records what the operator answered and the number
  is part of what was answered. `scripts/check-open-questions.py` reports the
  pair as a warning for that reason, and fails only on a collision involving a
  question that is still open.)

- Q37 How far osh's bash parity goes when the behaviour is an upstream bash
  *defect* — resolved 2026-08-07 (§105): **option A — waive it.** A divergence
  is waivable only when the bash side has been traced to its source and found
  to be an unchecked error path with nothing suggesting intent; anything short
  of that is designed behaviour and gets matched.

- Q35 Whether promoted fastpy coreutils replace the Rust ones — resolved
  2026-08-07 (§108): **option A for now**, with a stated trajectory toward B
  per command, gated on a parity suite *and* a performance bar, and surfaced as
  a user opt-in rather than a silent swap. fastpy's scope is explicitly not
  coreutils — the operator's intent is OS functions such as a file explorer or
  a settings dialog. The remaining sub-question (which way the shipping default
  points) is carried forward as Q39.

- Q34 Escalate to a full compiler-instrumented KASAN kernel to catch
  B-KNULLJUMP? — resolved 2026-08-07 (§107): **option B.** The lighter shadow +
  quarantine path was built, hardened and run at scale (100/100 clean, which is
  inconclusive at a ~1-in-120 base rate) without localizing the wild store, so
  the escalation lands as a separate instrumented debug build profile.

- Q36 How osh splits `$PATH` on the Windows dev host — resolved 2026-08-04
  (§103): **option B — split at the `$PATH` boundary only, with a drive-letter
  escape.** `:` is the separator everywhere (the whole rule on SlateOS); on
  Windows `;` is honoured too, since the inherited value is written that way;
  and a `:` after a single letter *and followed by `/` or `\`* is a drive
  letter, not a split point. Decided by Claude autonomously rather than by the
  operator — the recommended option proved small, local and easy to reverse,
  and leaving it open was blocking every corpus case needing a `$PATH` list.
  The operator may overrule.

- Q33 Next phase of the fastpy integration (initiative F) — resolved 2026-07-23
  (§87): **option B — reduce the embedded-ELF kernel bloat (TD-KERNEL-EMBED-BLOAT)
  first**, before promoting fastpy coreutils to real `/bin` commands. The ~48
  self-test ELFs are `include_bytes!`'d into `.rodata` (~3.5 MiB each); move them
  (and future fastpy binaries) onto the rootfs disk and load-from-disk. Operator
  said "I lean towards B"; Claude recommended A (promote to `/bin`) but noted B as
  a defensible prerequisite. B is a prerequisite-ish step toward a `/bin` that
  lives on disk anyway.

- Q32 Build KASAN-style heap-corruption detection to root-cause B-KNULLJUMP —
  resolved 2026-07-23 (§86): **option A — build KASAN-style shadow memory now.** A
  1/8-scale shadow region marking every heap byte addressable/poisoned, with
  instrumented alloc/free and checked stores on the suspect paths, debug-gated to
  protect the <200 ns heap target. Catches the whole live-write corruption class
  at the corruptor's write rather than the victim's later read. Operator said
  "A"; Claude recommended A. Targets the symbolized scheduler-`BTreeMap`-node
  corruption (see `known-issues.md`).

- Q31 SlateOS native-ABI main-thread ELF TLS setup (initiative F) — resolved
  2026-07-21 (§82): **option A — the posix crt sets up main-thread TLS in
  userspace** (finds `PT_TLS` via the linker-defined `__ehdr_start`, lays out a
  variant-II TLS block + TCB, sets the thread pointer), **plus a new native
  `SYS_SET_FS_BASE`** syscall calling the kernel's existing
  `set_current_task_fs_base`. Keeps the microkernel loader minimal and matches
  the kernel's "reset fs_base to 0, userspace sets it up" design. Operator said
  "I'll go with A"; Claude recommended A. Unblocks fastpy binaries (whose C
  runtime uses compiler `__thread`) running on-target.

- Q30 C cross-toolchain for fastpy's SlateOS runtime (initiative F) — resolved
  2026-07-21 (§81): **option A (a clang cross-toolchain to musl), realized via
  `zig cc --target=x86_64-linux-musl`** — a self-contained, portable clang +
  bundled musl headers + musl libc, so no heavyweight system-wide LLVM install
  and no separately vendored musl headers were needed (sidesteps both cons of
  A). Operator said "do A"; Claude picked zig as the concrete mechanism. The
  pure-mode runtime now cross-compiles and a real fastpy program links to a
  ~2.9 MB SlateOS ET_EXEC ELF with zero undefined symbols.

- Q29 fastpy → SlateOS target strategy (initiative F) — resolved 2026-07-21
  (§80): **pure-mode native compile first (A); add the CPython bridge later as a
  superset (B)** — "A at first but eventually B." Unblocks *starting* initiative
  F. Sequencing: mature the POSIX layer → add the `x86_64-slateos` fastpy target
  + port the C runtime in pure mode → compile one real OS component. Claude
  recommended A-first-then-B; operator confirmed.

- Q28 `osh` `$EUID`/`$UID` identity — resolved 2026-07-21 (§79): **default root
  (`0`/`0`) [option A], made per-user configurable** via `OSH_UID`/`OSH_EUID`.
  Seeded as real readonly-integer vars (readonly-enforced, bash-faithful
  listings). Claude recommended A; operator accepted and added the
  default-plus-per-user-override framing. Implemented; known-issues
  TD-OILS-IDVARS updated.

- Q27 `osh` advertising as bash (`$BASH_VERSION`/`$BASH_VERSINFO`) — resolved
  2026-07-21 (§78): **option A (advertise), as a per-user toggle
  (`OSH_BASH_COMPAT`) defaulting on** — mirrors upstream Oils' own `bash_compat`
  flag (which defaults on for `osh`, off for `ysh`; upstream sets
  `BASH_VERSION='5.3'`). osh keeps its level at 5.2 (never claims a 5.3-only
  feature). Claude recommended A + proposed the toggle; operator chose A and
  asked for the per-user-default framing.

- Q26 Oils (OSH) port strategy confirmed — resolved 2026-07-21 (§77): **finish
  the Rust reimplementation (A) now; keep A as a permanent user option even if a
  faithful C++ `oils-for-unix` port (B) lands later.** Claude recommended
  finishing A; operator confirmed and added that B is an additive future option,
  not a replacement.

- Q25 next large initiative + fixed ordering — resolved 2026-07-18 (§69):
  **Option A** (the interactive-shell userland) first, with the explicit
  clarification that the shell is **Oils (OSH)** — a bash-*superset* shell —
  **not bash itself** (roadmap-detailed.md §2.7). Fixed initiative order recorded
  durably so it need not be re-asked: **A → F → B → C → D → E** (1. Oils/OSH +
  coreutils, 2. fastpy build-system integration, 3. Mesa/GPU userspace [gated by
  Q18/virgl], 4. Chromium, 5. WINE, 6. additional filesystems). Claude recommended
  A-then-F; operator set the full ordering.

- Q24 raw `spin::Mutex` holder-preemption — reactive vs. proactive audit —
  resolved 2026-07-18 (§70): **Option B** (proactive kernel-wide audit/conversion)
  — "no technical debt, do it the right way." Not a blind sed: the heap and other
  deliberately-raw locks stay raw + manual-preempt; hot leaf locks move to a
  preempt-aware `PreemptSpinMutex`; contended non-leaf locks move to
  `crate::sync::Mutex` (lockdep); conversion is incremental and validated with
  `wedge-soak.sh` green. Claude recommended A (reactive) with C as escalation;
  operator overruled and chose the full proactive sweep.

- Q23 session model for daemon-backed AF_INET **server** sockets — resolved
  2026-07-18 (§71): **Option A** (shared, refcounted session; no daemon-ABI
  change) for the interim, since the whole per-op synchronous socket path is a
  stepping stone to the async socket server that will replace the ring-per-op
  model wholesale. Standing operator guideline recorded: **do not gold-plate
  interim/throwaway netstack infrastructure** — server sockets get A only; the
  concurrency limitation is documented and temporary. Claude recommended A;
  operator confirmed A.

- Q22 netstack Phase 5 cutover — deletion scope + cutover strategy — resolved
  2026-07-14 (§66): **Q22a → Option C** (phased deletion — L2–L4 core first, app
  protocols re-homed to userspace individually) and **Q22b → (ii) staged**
  (persistent daemon + socket-forwarding behind a default-off boot switch; prove
  parity in QEMU, flip the default, then delete). Claude recommended both; operator
  approved both.

- The coreutils "which set is canonical?" question — resolved 2026-06-12;
  standalone per-tool crates are canonical (§8).
- Q1 `set_mempolicy_home_node` / NUMA mempolicy on UMA — resolved 2026-06-13,
  **operator-confirmed 2026-06-14**; keep the UMA no-op returning 0, option A
  (§10).
- Q2 `/proc/sys/vm/overcommit_memory` & memory-commit policy — resolved
  2026-06-13, **operator-confirmed 2026-06-14** (keep the shipped defaults:
  native strict/committed, Linux lazy/overcommit; both configurable); build the
  both-strategies model (Option 5); map the system-wide overcommit knob to a
  fine-grained native cap (`admin.memory_policy`), not `CAP_SYS_ADMIN` (§11).
- Q3 next major initiative — resolved 2026-06-13; terminal/dev before GUI,
  GCC/CMake/Make toolchain first, CPython then fastpy (§9).
- Q4 toolchain on Slate OS: run-prebuilt-Linux vs native-port — resolved
  2026-06-13; **Path Z** (run prebuilt Linux toolchain binaries on the Linux-ABI
  layer now, native-port selectively later), native-first/no-leak kept
  inviolate, clang green-lit for install (§12).
- Q5 file-backed `mmap` — how far to take the fix — resolved 2026-06-14
  (§22), then **REOPENED 2026-06-14** by the operator, then **RE-RESOLVED
  2026-06-14**: adopt **C-lite** (a unified *read-only* page cache for
  shared-library text dedup + de-double-caching), deferred until a concrete
  consumer appears (the dynamic linker is the likely first; stable VFS
  file-identity is the precursor); writable `MAP_SHARED` writeback stays declined
  / `ENOSYS` (§23). Deferral trigger logged in `todo.txt`.
- Q6 cross-process memory introspection — resolved 2026-06-14: keep
  channel/shared-memory IPC for *consensual* sharing; add a
  **debug-capability-gated** cross-address-space `process_vm_readv`/`writev`
  (`Rights::DEBUG` on a `Process` capability; `EPERM` without it). `ptrace`
  remains a deferred follow-up behind the same gate (§24).
- Q8 Path Z libc + rootfs — resolved 2026-06-14, **operator-delegated to
  Claude**: go straight to **glibc** on an **ext4** rootfs, no musl
  stepping-stone (§25). Claude reversed its own earlier musl-first recommendation
  per the operator's stated preference for hard-work-upfront over throwaway
  scaffolding, given the static-load path is already proven end-to-end.
- Q7 kernel-task-stack-vs-IRQ overflow (B-DF1) — resolved 2026-06-15,
  **operator-chosen option A** (Claude recommended A): per-CPU guard-page IRQ
  stack with a manual nesting-aware switch + deferred preemption, plus the
  `cli`/`sti` recursion guard the restructuring exposed (§26). Validated:
  `http_gzip_8KiB` no longer double-faults at the gzip→dashboard transition.
- Q9 bare-ELF ABI auto-classification — resolved 2026-06-24, **operator-chosen
  option D** (Claude recommended D): default unmarked bare ELF → Linux ABI, add
  `NT_GNU_ABI_TAG` note-walk as a positive Linux signal, stamp native binaries
  with an explicit SlateOS marker; `spawn_process_with_abi` override kept (§33).
- Q10 fullscreen-capture video codec — resolved 2026-06-24, **operator deferred
  to Claude's recommendation**: hardware encode via the GPU driver long-term
  (option C), defer the software-codec port near-term (option D), no stub
  encoder meanwhile; if a software path is ever needed first, AV1/`rav1e` over
  H.264 (§34).
- Q11 zero-copy page-flipping for large channel messages — resolved 2026-06-24,
  **operator-chosen option B** (Claude recommended B): explicit opt-in
  `MSG_ZEROCOPY`-style flag + caller-provided page-aligned landing region; copy
  path stays the default. Compiler follow-up: keep it programmer/library-
  controlled (library-level auto-threshold helper), the compiler does not
  auto-insert the flag (§35).
- Q12 next large initiative — resolved 2026-06-24, **operator-chosen option E**:
  build the C-lite read-only page cache now; lifts the §23 "not now" hold (§36).
- Q13 de-double-cache file data — resolved 2026-06-30, **operator-chosen option A**
  (Claude recommended A): page-cache-primary — the page cache is the single cache
  for regular-file data, the buffer cache caches only filesystem metadata (§38).
- Q14 connect the two cgroup subsystems — resolved 2026-06-30, **operator-chosen
  option A** (Claude recommended A): cgroupfs as the frontend,
  `kernel/src/cgroup.rs` as the enforcement engine; fork/clone/spawn inherit
  `cgroup_id` (§39).
- Q15 next focus — resolved 2026-06-30, **operator-chosen option A then C/D**:
  execute Q13 + Q14 first, then a large initiative — C (GPU accel) or D (Docker /
  container-runtime port) in operator-indifferent order; this is the explicit
  go-ahead for the Docker port (§40).
- Q16 `container diff` baseline semantics — resolved 2026-07-01, **Claude
  autonomous (operator-approved Docker-port scope)**: implemented **option A**
  (overlay-only diff). See `design-decisions.md` §41.
- Q17 `container exec` semantics — resolved 2026-07-14, **operator-chosen
  option B** (Claude recommended B): keep the netns-debug `container exec` facade
  AND add real rootfs-binary exec under a distinct verb (`container run-in` /
  `exec --rootfs`); the `docker exec` delegate + `docker build` `RUN`/`HEALTHCHECK`
  route to the real path (§58).
- Q18 GPU acceleration scope — resolved 2026-07-14, **operator-chosen option B**
  (Claude recommended C): build the kernel-side virtio-gpu render-ioctl dispatch
  now with honest "no-3D" reporting (GETPARAM `3D_FEATURES=0`, no capsets, correct
  errno on 3D ioctls); defer the Mesa port until a virgl test environment exists
  (§59).
- Q19 container network model — resolved 2026-07-14, **operator-chosen option B**
  (Claude recommended B): generalise to N-interface multi-network membership
  (Docker parity) as its own dedicated increment (§60).
- Q20 hard-lockup (BSP-dead) detector — resolved 2026-07-14, **operator-chosen
  option A** (Claude recommended A): build the `i6300esb` watchdog + inject-nmi
  detector, opt-in behind the existing `boot-test.sh --hard-lockup-watchdog` flag
  (§61).
- Q21 `nft`/`iptables` compat tooling — resolved 2026-07-14, **operator-chosen
  option C** (Claude recommended C): keep `nft`/`iptables` as an explicit
  parser/pretty-printer only, fix the docs, steer users to `fw`; defer full/minimal
  kernel wiring (§62).


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

## C-Q2 — [C] When the text on a line runs both ways, should the Right arrow key move the caret one character *later in the sentence*, or one step *to the right on the screen*? — Status: **ANSWERED 2026-08-21 by the operator — b (visual)**

> **ANSWERED 2026-08-21 (relayed by lane A).** The operator answered **b** —
> the visual convention, matching the recommendation below.
>
> **Note on how this answer arrived, because it was briefly misfiled.** It came
> in as the bare words *"b, i guess"* inside a large multi-lane answer batch, and
> lane A first read it as an answer to **Q49** (modern AMD graphics), which the
> same message also answered at length. The operator corrected this the same
> day: *"the 'q49: b, i guess' was meant to be for c-q2"*. Q49 is unaffected —
> `design-decisions.md` §262 was written from the operator's detailed Q49
> answer and never cited the stray "b", so no record needs revising; only this
> question gained an answer it had been missing.
>
> Lane A is relaying rather than acting — **the write-up is lane C's.** Move this
> into your own `design-decisions.md` number range with `**Decided by:**
> Operator (Claude recommended this option)`, add the `**In short:**` opener,
> then delete this question and index it under "Resolved".
>
> Per the entry below, switching this on is one line in each of three text
> widgets — `caret_left`/`caret_right` are already written and tested. Mind the
> measured caveat: a widget that does not also remember which side of a
> direction boundary the caret is on will **skip an entire right-to-left word**
> in one press, which is worse than today.

**In short:** Hebrew and Arabic are written right to left, and a line can mix
them with English — "I said שלום to him". On such a line the order the
characters are *stored and read* is not the order they are *drawn*, so the Right
arrow key has two different meanings and they disagree. Today it means "the next
character in reading order", which makes the blinking caret sometimes jump
sideways across a word instead of stepping. The alternative is "one step to the
right on screen", which always steps, but means the caret sometimes moves
*backwards* through the sentence. Every operating system picks one; they do not
all pick the same one, and neither answer is wrong.

**A worked example.** The line is `I said <SHALOM> to him`, where `<SHALOM>` is
one Hebrew word of five letters. Drawn, the Hebrew word's letters run right to
left inside it, while the sentence around them runs left to right. Put the caret
just before the Hebrew word and press Right five times:

| | What the caret does | Where it ends up |
|---|---|---|
| **Logical** (today) | Steps through the Hebrew letters in the order they are read, so it jumps from the word's right-hand end leftwards, then jumps back | after the last Hebrew letter, at the word's **left** edge |
| **Visual** | Moves right one letter-width each press, never jumping | at the word's **right** edge, having passed through the whole word |

Both end "after the whole word" in some sense; they just disagree about which
end of the word that is, and about what happens in between.

**The options.**

| | *What changes:* |
|---|---|
| **A. Keep logical** (macOS, GTK, Qt, most of Linux) | Nothing changes. On a mixed line the caret sometimes jumps a word's width sideways between two presses of the same key. |
| **B. Switch to visual** (Windows edit controls, and what ICU's `ubidi` API is built to support) | The caret always moves one step in the direction of the arrow, and never jumps. Holding Right walks the caret smoothly across the line, but the *text position* it is at can go backwards, so typing after several presses inserts earlier in the sentence than the previous press did. |
| **C. Both, on a setting** | A preference the user sets, defaulting to one of the above. Costs a setting nobody knows how to answer, and doubles the number of behaviours every future text widget has to be correct in. |

**What is already built either way — updated 2026-08-17: option B is now written
and tested, and is one line per widget away from being switched on.** The
shaping engine knows where every character is drawn and which way it runs; on
top of that there are now `caret_left`/`caret_right` functions that take a caret
and hand back the next caret position *to the left or right on the screen*, with
tests covering a mixed-direction line, an Arabic ligature crossed as one unit,
and the pixel round-trip. Nothing calls them. Answering "B" is a one-line change
in each of three text widgets; answering "A" deletes nothing, because those
functions are also what mouse selection and any future screen-order feature
want. **So this question is now purely about which behaviour is right.**

**One thing measured while building it, which strengthens the case that this had
to be asked rather than guessed.** The extra bit the caret carries ("which side
of a direction boundary am I on") turned out not to be a nicety. A text box that
remembers only the caret's *position in the string* between keypresses, and
works the rest out fresh each time, does not merely land on the wrong side of a
boundary — it **skips the entire right-to-left word in a single press**. So a
half-hearted "B" — switching the arrow keys without also making the widgets
remember that bit — would be *worse* than today's behaviour, not better. That
groundwork has been done now regardless of the answer, so the choice is no
longer between "A" and "a risky B".

**My recommendation: B, visual.** The reason is what the key is called. Users
press "right arrow" while looking at the screen, and an arrow key that sometimes
moves the caret left is surprising in a way that no amount of correctness
argument fixes. The logical convention's advantage — that the caret's text
position advances monotonically — is invisible; the visual convention's
advantage is the thing the user is looking at. Home/End and word-motion would
stay logical under either answer, since those name positions in the sentence
rather than directions on the screen.

**If this is never answered:** nothing breaks and nothing gets worse. Option A
is the current behaviour and it is self-consistent; the cost is a small,
permanent oddity on mixed-direction lines, which is rare in English text and
constant in Hebrew or Arabic text. It gets harder to change later only in the
sense that more text widgets will have been written against whichever answer is
in force.

**Where it bites:** `gui/font/src/shape.rs` (`ShapedRun::caret_left` /
`caret_right`, built 2026-08-17), `gui/toolkit/src/text.rs` (`TextCursor` and
its `caret_left`/`caret_right` wrappers), and the arrow-key handling in
`guitk::widget::TextInput` and `guitk::modal::InputDialog` — each of which
carries a comment marked `C-Q2` naming the exact line to change. `apps/editor`
is **not** covered by either answer: it draws its caret and scrolls sideways in
a way that assumes screen order equals reading order, so it needs its own,
larger fix first (`known-issues.md` → `TD-EDITOR-IS-NOT-BIDIRECTIONAL`).
Tracked in `known-issues.md` → `TD-GUI-ARROW-KEYS-MOVE-IN-LOGICAL-ORDER`.

## C-Q3 — [C] `CLAUDE.md` tells all three lanes to publish finished work through one shared folder, and two of them collided in it today. Change the instruction? — Status: **ANSWERED 2026-08-21 by the operator — b**

> **ANSWERED 2026-08-21 (relayed by lane A).** On 2026-08-21 the
> operator answered `c-q3: b`.
> The answer arrived in a batch covering several lanes' questions at once, so
> lane A is relaying it here rather than acting on it — **the write-up is
> still the owning lane's to do.** Owning lane: move this entry into your own
> `design-decisions.md` number range with `**Decided by:** Operator`, add the
> `**In short:**` opener, then delete this question and index it under
> "Resolved".

**In short:** Each of the three agents works in its own private copy of the
source tree, which is what stops them overwriting each other. But the last step
of every finished task sends all three back into **one shared copy** — the
`os` folder — to publish the work. Today two agents were in there at the same
moment and their publish steps tangled; git printed a "a git process may have
crashed in this repository earlier" error and one agent's step ended up
discarded. Nothing was lost this time, because a discarded publish can simply
be re-run. There is a way to publish that never touches the shared folder at
all, and the question is whether to make that the instruction.

**Why the shared folder is there.** Publishing means combining your work with
whatever the other two have published since you started. Combining normally
needs a folder to do it in — somewhere the files from both sides can sit while
differences are reconciled. `CLAUDE.md` nominates `os` as that folder. The
catch is that a folder can only be in one state at a time, so two agents
reconciling in it simultaneously are editing the same thing, which is precisely
the failure the private copies were created to prevent.

**Why it can be avoided.** The rules *already* require each agent to pull in
everyone else's published work and re-run the tests **before** publishing —
in its own private copy. Once that is done, publishing has nothing left to
reconcile: the shared side has no changes the agent's copy lacks. Git can then
publish with a single server-side command that needs no folder at all
(`git push origin lane-c:main`, a "fast-forward"). If another agent published
in the meantime the command is simply **refused**; you pull their work in,
re-test, and try again. It cannot half-succeed and it cannot interleave with
anyone else's.

**The options.**

| | *What changes:* |
|---|---|
| **A. Leave `CLAUDE.md` as it is** | Nothing changes. Agents keep meeting in `os`; collisions stay rare but keep happening, and each one costs a re-run and looks alarming in the transcript. |
| **B. Change step 11 to the folderless publish** | Agents stop entering `os` to publish. `os` becomes a read-only window onto the combined result. Collisions become impossible rather than rare; a clash surfaces as a clean "refused, try again" instead of a tangle. |
| **C. Add a lock around `os`** | Agents still meet in `os` but queue for it, the way they already queue for the emulator. Collisions become impossible too, but an agent can now be made to *wait*, and a lock left behind by a crashed agent blocks the others until someone clears it. |

**My recommendation: B.** It removes the shared resource instead of scheduling
access to it, needs no new machinery, and is what I did today after the
collision — it worked, and `main` still only ever advanced to a commit whose
tests had been run. C solves the same problem by adding a lock that can itself
get stuck. The one thing B gives up is the ability to resolve a genuine
conflict *during* publication, but that was never wanted here: the rules
already say resolve-then-test-then-publish, and B just makes that order
mandatory instead of conventional.

**Why this is a question and not a change I made:** `CLAUDE.md` is yours. Its
own text says not to edit it except on an explicit instruction, so this is
written up rather than acted on.

**If this is never answered:** nothing breaks. I will keep publishing the safe
way regardless — the instruction permits it, it just does not prescribe it —
so the exposure is limited to the other two lanes continuing to follow the
letter of step 11. The cost is an occasional tangled publish that has to be
re-run, and it neither grows nor worsens with time. Full detail in
`known-issues.md` → "Two lanes merging up at once race in the shared `os`
worktree".

## C-Q5 — [C] This OS writes all of its own cryptography by hand. Keep doing that, or port implementations other people have already broken and fixed? — Status: **ANSWERED 2026-08-21 by the operator — c**

> **ANSWERED 2026-08-21 (relayed by lane A).** On 2026-08-21 the
> operator answered `c-q5: c`.
> The answer arrived in a batch covering several lanes' questions at once, so
> lane A is relaying it here rather than acting on it — **the write-up is
> still the owning lane's to do.** Owning lane: move this entry into your own
> `design-decisions.md` number range with `**Decided by:** Operator`, add the
> `**In short:**` opener, then delete this question and index it under
> "Resolved".

**In short:** the code that protects saved passwords, login and the lock
screen is cryptography we wrote ourselves — including eleven separate
hand-written copies of the same hash function. Writing your own cryptography
is the one thing security practitioners are close to unanimous about not
doing, and not because the algorithms are secret: they are public, and ours
compute the right answers. The problem is that the *bugs* are silent. Code
that produces correct output can still leak the secret through how long it
takes to run, and no test will ever notice. The question is whether to keep
writing these and fix them in place, or to bring in implementations that
already survived twenty years of people attacking them.

**How we got here.** Nothing was ever decided. The OS has no third-party
crypto dependency, so each feature that needed a hash wrote one. It worked, so
it kept happening.

**What is actually wrong right now** (all four logged in `known-issues.md`,
found while turning the workspace lints on for `gui/credentials`):

| Problem | What it means |
|---|---|
| The password vault scrambles every secret with an identical repeating pattern | two saved passwords cancel each other out; the vault can be read without the master password |
| The master password is hashed once, with an extra ingredient that is the same on every SlateOS machine | guessable at billions of tries per second, and one precomputed table cracks every user everywhere |
| Nothing in userspace can obtain an unpredictable number | the built-in password *generator* produces guessable passwords |
| Eleven hand-written copies of SHA-256 | eleven chances for one of them to be wrong, forever |

The first three can be brought up to "defensible" using only what is already
in the tree, and I am doing that regardless of the answer here. What the
answer decides is the *end state* — specifically two things I would rather not
write myself:

- **authenticated encryption** (so an attacker who cannot read the vault also
  cannot silently alter what is in it), and
- **a deliberately-slow password hash** (so guessing costs an attacker real
  money — Argon2id or scrypt).

**The options.**

- **A — Keep writing our own, carefully.**
  *What changes:* I implement AES-GCM and Argon2id in-tree with the official
  test vectors, and the eleven hash copies collapse into one shared crate.
  *For:* no outside code in the trust base; works in the kernel's no-allocator
  environment by construction; consistent with how the rest of the OS is
  built.
  *Against:* this is the one area where "passes its tests" and "is secure" are
  different sentences. The classic break is a comparison that returns a
  fraction sooner when the first byte is wrong — an attacker measures the
  timing and recovers the secret one byte at a time, against code that is
  perfectly correct. I cannot test my way to confidence here the way I can
  everywhere else in this project, and that is the honest reason I am asking.
- **B — Port a vetted implementation for everything.**
  *What changes:* the tree gains a vendored copy of an established Rust crypto
  implementation (RustCrypto's, or BearSSL in C), used everywhere; the
  hand-written copies are deleted.
  *For:* written to be constant-time on purpose, by people who do only this;
  and it is what this project's own spec already says to do elsewhere —
  `design.txt` requires porting battle-tested code for the filesystem rather
  than writing our own.
  *Against:* a real dependency to vendor and keep current; more moving parts
  in the build.
- **C — Port the primitives, keep our own glue.**
  *What changes:* the hash, the cipher and the password hash are vendored; the
  vault file format and the service plumbing on top stay ours.
  *For:* this is what actual operating systems do, and it puts the borrowed
  code exactly where writing your own hurts most.
  *Against:* same dependency cost as B, on a smaller surface.

**Recommendation: C.** The spec's own "port battle-tested code" rule was
written for the filesystem and applies with more force here, where a bug does
not crash — it just quietly stops protecting anything.

**If this is never answered:** nothing breaks and nothing is blocked — I will
still fix the four defects above as far as hand-written primitives allow. But
the vault stays unauthenticated (an attacker who cannot read it can still
alter it undetectably), password hashing stays cheap to attack, and every
further piece of crypto written meanwhile is more work to throw away if the
answer is later B or C.

## C-Q4 — [C] Nothing in the system can print. Two half-built printing features exist and neither is connected. Which one should applications talk to? — Status: **ANSWERED 2026-08-21 by the operator — c**

> **ANSWERED 2026-08-21 (relayed by lane A).** On 2026-08-21 the
> operator answered "let's do c since we should do it eventually anyway, no point putting it off with a stop-gap solution in its place".
> The answer arrived in a batch covering several lanes' questions at once, so
> lane A is relaying it here rather than acting on it — **the write-up is
> still the owning lane's to do.** Owning lane: move this entry into your own
> `design-decisions.md` number range with `**Decided by:** Operator`, add the
> `**In short:**` opener, then delete this question and index it under
> "Resolved".

**In short:** There is no way to print anything from this OS today. Two
separate pieces of printing code were written at different times and neither
was ever hooked up to anything a user can click. One of them lives in the PDF
viewer and knows how to work out *which pages* to print; the other lives in the
desktop and knows about *printers* — which ones exist, paper sizes, how many
copies, and a queue of pending jobs. Neither knows the other exists. The
question is how an application should reach a printer: through the desktop's
existing machinery, or through something new built for the purpose.

**What is actually there.** Both pieces are real code with tests, not
placeholders:

| | In the PDF viewer | In the desktop |
|---|---|---|
| Works out which pages to print | yes — including "1-3, 5, 7-9" | only a single "from page X to page Y" |
| Knows what printers exist | no | yes |
| Copies, paper size, double-sided, quality | no | yes |
| Queue of pending jobs, cancel, pause | no | yes |
| Reachable by any application | **no** | **no** |

Each is better than the other at a different half of the job, which is not a
mistake — the page range is something only the document knows (it is the only
thing that knows how long it is), and the printer list is something only the
system knows. The gap is the connection between them.

**Why this needs deciding rather than just doing.** Applications live in
`apps/`, the desktop is the program that draws the screen. Making the PDF
viewer call directly into the desktop's code would work in about an hour, and
would mean every application that ever wants to print has to be built together
with the desktop — so a change to the taskbar could stop the PDF viewer from
compiling. That is the kind of tangle that is cheap to create and expensive to
undo later, which is why it is worth a decision now rather than after four more
applications have copied it.

**The options.**

- **A — Applications call the desktop's printing code directly.**
  *What changes:* printing works in the PDF viewer today. Every application
  that prints is from then on built together with the whole desktop, and
  printing only works while the desktop is running.
- **B — Move the printer-handling code into a small shared library that both
  the desktop and applications use.**
  *What changes:* nothing visible differs from A for a user; the code moves
  house first, so applications depend on a printing library rather than on the
  desktop. Roughly half a day more work than A, and it is the last time the
  move is cheap.
- **C — Printing becomes a background service that applications send jobs to,
  like every other OS.**
  *What changes:* a print job survives the application closing, and can be
  cancelled from anywhere. Several days of work, and it needs the message
  plumbing between programs that other parts of the system already use.
- **D — Leave it. Delete neither piece, connect neither.**
  *What changes:* nothing. Printing stays impossible, and the two models drift
  further apart as each is edited for its own reasons.

**Recommendation: B**, then C later if printing ever needs to outlive the
application that started it. B costs little more than A and does not create the
dependency that A does; C is the right long-run shape but is a real project and
nothing is currently blocked on it.

**If this is never answered:** nothing breaks and nothing worsens quickly, but
printing stays impossible from every application, and the drift is real — the
two models already disagree about what a page range is, and each additional
edit to either makes the eventual merge harder. Detail in `known-issues.md` →
"`apps/pdfviewer` can print nothing at all — the whole model is unwired".

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


## B-Q7 — [B] You decided in June which copy of our command-line tools is the real one. The fact that decision rested on turns out to be false. Does the decision stand? — Status: OPEN

**In short:** We have two copies of about forty small command-line programs
(`sort`, `cut`, `stat`, …) — one set inside a bundle called `coreutils`, one set
as separate little projects. In June you were asked which set was the real one
and you picked the separate projects, on the strength of a security argument I
gave you. **That argument was wrong about a plain matter of fact**, and it was
wrong in the direction that flipped the answer: the property I said the bundle
had (and that made it insecure) is one the *separate projects* actually have,
and the bundle does not. Nothing was ever built on the June decision, so
reversing it costs nothing but the decision itself. I need to know whether it
stands, is reversed, or is replaced.

### What I told you in June, and what is actually true

The June decision is `design-decisions.md` §8. Its deciding argument was
capability-based least privilege — the rule that a program should be granted
only the permissions it actually needs. I told you `coreutils` was a
**multi-call binary**: one single executable file that looks at the name it was
invoked under and behaves as a different tool accordingly (this is how BusyBox
on Linux works — one file, seventy names pointing at it). That shape is bad for
least privilege, because the operating system grants permissions per *file*, so
one file serving seventy tools must be granted the union of all seventy tools'
permissions. `rm` would inherit whatever `mount` needs. That argument is sound,
and on that basis you retired the bundle.

`coreutils` is not that, and never was:

| Claim in §8 | Measured, 2026-08-22 |
|---|---|
| `coreutils` is one multi-call executable | It builds **86 separate executables** (`cargo metadata` → 86 bin targets). One tool = one file, already. |
| …that dispatches on its own invocation name | No such dispatch exists anywhere in it. |
| …and always did | First commit (`d469e23bb`, 2026-05-17) already had `src/bin/*.rs`. There has never been a `src/main.rs`. |
| §8's remedy: "extract a shared library, `coreutils-common`" | The crate already has one — `src/lib.rs` plus eleven shared modules (`quote`, `getopt`, `human`, `xnum`, …). `coreutils-common` was never created; the thing it was supposed to create already existed. |

And the shape §8 condemned does exist in this tree — on the **other** side:

| Separate project | One executable, serving |
|---|---|
| `userspace/stat` | `stat`, `touch`, `ln`, `readlink`, `realpath`, `mkfifo` |
| `userspace/sha256sum` | `md5sum`, `sha1sum`, `sha256sum`, `sha512sum` |
| `userspace/chown` | `chown`, `chmod` |
| `userspace/who` | `who`, `w` |

Each of those declares exactly one executable and switches on its invocation
name — precisely the BusyBox shape. So §8's own security rationale, applied to
the real code, argues for the bundle and against the separate projects.

**And it is not four crates — it is at least nineteen, and it has already cost
us working commands.** Surveyed 2026-08-22: **50–70 command names** are
implemented as extra personalities of some other separate project, and **no
build produces an executable for any of them**, because creating the links that
would select the personality is a step nobody ever wrote. `e2fsck`, `mke2fs`,
`tune2fs`, `resize2fs`, `strip`, `ranlib`, `xxd`, `killall`, `shred`, `visudo`,
`lpr`, `mpstat`, `finger` and dozens more exist as finished code that cannot be
run. `e2fsck` and `mke2fs` are the check and create tools for **ext4, our only
filesystem**. Filed as `known-issues.md` →
`B-DOZENS-OF-COMMANDS-EXIST-IN-SOURCE-AND-CAN-NEVER-BE-RUN`.

This bears on the choice directly. §8's rule is *"one tool = one crate = one
binary = one identity"*, and the side §8 declared canonical is where that rule
is broken — not occasionally, but as the house style, in at least 19 crates.
`coreutils`, the side §8 retires, is the only part of the tree where the rule
actually holds today.

**Nothing was built on §8.** Ten weeks on, not one part of it was carried out:
no `coreutils-common`, no retirement, no build repointing. Both sets are still
compiled, and both still write executables to the same filenames — whichever
happens to build last wins, silently. That collision has already cost a full
day: a test harness spent it reporting 105 differences against a `bc` that was
not the `bc` anyone thought it was measuring
(`known-issues.md` → `B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES`).

### The scale, so the options can be priced

- 86 tool names live in `coreutils`; **41** of them also exist as separate
  projects; **45** exist *only* in `coreutils` (`ls`, `cp`, `rm`, `grep`,
  `find`, `sh`, `printf`, …).
- 3 names (`sha1sum`, `sha512sum`, `w`) exist *only* as separate projects.
- Of the 41 overlapping pairs, a rough survey (`scripts/dup-bins-survey.py`)
  puts the separate project ahead on features for ~25 and `coreutils` ahead for
  ~9, the rest level. **Neither side is uniformly better** — which is why no
  option below should delete a whole side sight-unseen.
- One thing that is *not* a differentiator, though it looks like it should be:
  automated code-quality checking. `coreutils` opts out of the project-wide
  checks (no `[lints]` in its manifest — 86 unchecked programs), but so do
  **all 41** of the separate projects. Whichever side wins has to be opted in
  afterwards either way; this is tracked separately as
  `TD-B-USERSPACE-CRATES-DO-NOT-INHERIT-THE-WORKSPACE-LINTS`.

### Options

**A — §8 stands: the separate projects are canonical, the bundle is retired.**
*What changes:* nothing a user sees; internally, 45 new one-tool projects get
created for the names that only exist in the bundle, and the four BusyBox-shaped
projects above get split so the "one tool, one file" rule they were chosen for is
actually true of them.
*Pro:* it is your standing decision; "one tool = one project = one file" is a
clean rule that reads well from the outside.
*Con:* the stated reason for it is false, so it would now be being kept for
reasons other than the ones it was decided on. It is also the most work by a
wide margin — 45 new projects plus four splits — and the work is pure
rearrangement: no user-visible improvement at the end of it.

**B — Reverse it: `coreutils` is the one home; for each overlapping pair the
better implementation is the one that survives, moved into `coreutils`.**
*What changes:* nothing a user sees, except that each tool stops
non-deterministically alternating between two implementations depending on build
order; the better of the two wins, permanently.
*Pro:* the least-privilege argument that decided §8 actually points here.
`coreutils` already has the shared library, already has 45 tools no one
duplicated, and is the side the comparison harnesses test against. Least work
of the three, and every step is a merge rather than a rewrite.
*Con:* it overturns a decision you made. Merging 41 pairs by hand is careful
work, and a careless merge loses features (the survey is only a triage aid —
every pair still has to be read).

**C — Split the difference: keep both projects, but assign every name to exactly
one of them and enforce it.**
*What changes:* same as B from outside; internally the tools stay where they
already are and only the duplicates are resolved, either way per name.
*Pro:* least code movement of all; keeps whichever copy is better without
relocating it.
*Con:* this is the current situation with a rule bolted on, and it is the option
my own June analysis rejected as "the drift-generating status quo". Two homes
means the next tool added has an ambiguous home, and the collision returns the
first time someone forgets.

### If never answered

**It gets worse, slowly, and it is already not safe.** Both copies are still
built, still overwrite each other's executables, and which one you get depends
on build order. Any test, any measurement, any bug report about one of these 41
tools may be about the other copy — that is not hypothetical, it cost a day
already. It does not block other work, but every week adds edits to whichever
copy the editor happened to open.

### Claude's recommendation

**B**, and it is not close on the merits — the security argument, the shared
library, the 45 non-duplicated tools and the test harnesses all point the same
way. But **A is your decision and I have not acted against it.** I had begun B
autonomously (recorded as §359, and I had already merged `bc`) before finding
§8; on finding it I stopped and marked §359 suspended.

**One correction to what this entry said yesterday.** It said I was reverting
the `bc` move so the tree would match your standing decision while this is open.
**I have not, and on reflection I think reverting it would be the wrong call.**
Saying so plainly is the point — what I will not do is quietly leave that
sentence standing while the tree says otherwise.

The reasoning, so you can overrule it in one line if you disagree:

- **Reverting `bc` would not make the tree match §8; it would make 1 name out of
  86 match it.** 45 command names exist *only* in `coreutils`, and four of the
  standalone crates are the multi-call shape §8 was chosen to avoid. The tree
  has never complied with §8 in any respect. Moving `bc` alone is a token that
  buys no actual compliance.
- **It would cost a real safety net.** `bc` now lives under
  `coreutils/src/bin/`, which is the only directory the
  `diagnostics_quote_names` test reads — and that test caught a genuine bug in
  `bc` this week. Standalone crates are outside it. Widening the test is not a
  cheap fix I could bundle into the move: measured 2026-08-22 with
  `scripts/quote-names-scope.py`, a tree-wide version would flag **1796 call
  sites across 777 crates**.

  *Update 2026-08-23 — this argument is now weaker, and you should discount it
  accordingly.* `scripts/quote-names.py` runs the same two detectors over the
  whole of lane B's tree from pre-push gate 8, against a baseline of the 1798
  sites that exist today; it fails only on a **new** one. So a standalone crate
  is no longer unchecked — it is held at "no worse than today" rather than at
  "zero", which is the only remaining difference. The 1798 are still unrepaired,
  so a `bc` moved out of `coreutils/src/bin` would go from *must be clean* to
  *must not get dirtier*. That is a smaller loss than this bullet described when
  it was written, and it shrinks further with every crate the burn-down clears.
- **It would create a dependency shape that exists nowhere in the tree yet.**
  `bc` now uses `coreutils`'s `getopt`, `quote` and `errmsg`. A standalone
  `userspace/bc` would have to depend on the `coreutils` *library* — a per-tool
  crate importing the bundle §8 retires — or fork three modules and restart the
  drift §359 was about.
- **Nothing is unsafe in the meantime.** The duplication that cost a day is
  *gone* for `bc`: `userspace/bc` is deleted, there is exactly one `bc`, it is
  the better of the two implementations, `scripts/calc-diff.sh` names its
  package explicitly rather than picking up whatever built last, and it passes
  200/200 against GNU bc.

So `bc` sits in `coreutils` today. **If you answer A, I move it out** — one file
move, one dependency line, one edit to `calc-diff.sh` — and it is a rounding
error inside the much larger A-shaped job of creating 45 new crates. Nothing is
lost either way; that part of yesterday's promise still holds.

### Where it bites

`design-decisions.md` §8 (the June decision) and §359 (mine, now suspended);
`coreutils-canonical-answer.md` (the June analysis carrying the false premise —
worth correcting whichever way this goes); `userspace/coreutils/` (86 bins,
`src/lib.rs`); the 41 duplicate crates under `userspace/`;
`known-issues.md` → `B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES`;
`scripts/dup-bins-survey.py` (the triage); `scripts/quote-names.py` and
`known-issues.md` → `TD-B-THE-QUOTE-NAMES-TEST-READS-ONE-DIRECTORY-OF-EIGHTY`
(the lint-coverage half of the cost).

---

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


# Resolved

**The body above holds OPEN questions only.** When the operator answers one,
write it up in `design-decisions.md` as a `Decided by: Operator` entry,
**delete the entry from the body**, and add one line here. That is the whole
point of the file: it is scanned for what still needs a decision, so an
answered question left in the body is pure cost — and, being older, it sorts
*first*, right where it is most in the way. (Why this is not append-only:
`design-decisions.md` §437.)

The index is split by lane so three lanes adding a line at once land at three
different offsets and the merge is automatic. Newest first within each lane.
`(§n)` cites `design-decisions.md`.

## Q55 — [C] The installer reads `size = "100 GB"` in a partition table as 107 GB. Should a decimal spelling mean a decimal number? — Status: **ANSWERED 2026-08-21 by the operator — c**

> **ANSWERED 2026-08-21 (relayed by lane A).** On 2026-08-21 the
> operator answered `q55: c`.
> The answer arrived in a batch covering several lanes' questions at once, so
> lane A is relaying it here rather than acting on it — **the write-up is
> still the owning lane's to do.** Owning lane: move this entry into your own
> `design-decisions.md` number range with `**Decided by:** Operator`, add the
> `**In short:**` opener, then delete this question and index it under
> "Resolved".

**In short:** An unattended-install config file describes each disk partition
with a size like `"100 GB"` or `"32 GiB"`. The installer currently treats those
two spellings as *the same number* — both mean 2^30 bytes, the binary one. So a
config asking for `500 GB` on a 500 GB drive asks for 537 GB of space and the
install fails to fit. The question is whether to make the decimal spelling mean
the decimal number, at the cost of changing what existing config files do.

**Glossary:** `GB` (gigabyte) is decimal — exactly 1 000 000 000 bytes, and it
is what a disk's box says. `GiB` (gibibyte) is binary — 1 073 741 824 bytes,
about 7% more. They diverge further at `TB`/`TiB` (10%).

Where it lives: `apps/installer/src/lib.rs:1211`, the `multiplier` match in the
partition-size parser. `"K"|"KB"|"KIB"` all map to 1024, and so on up to `TB`.

This is the mirror image of the display-side defect fixed in
design-decisions.md §489 — there, code divided by 1024 and *printed* `GB`; here
it *reads* `GB` and multiplies by 1024. The display side was unambiguous
(printing a number under a label that means something else is simply false), so
it was fixed without asking. This side is not, because the suffix in a config
file is an input convention, and the surrounding ecosystem is genuinely split.

### The options

**A. Leave it. Every suffix is binary.**
*What changes:* nothing.
This is what `fdisk`, `parted` and most partitioning tools do, and what anyone
who has typed `+512M` at a disk prompt expects. It is also self-consistent: a
config author who writes `100 GB` gets the same partition every time.

**B. Honour the spelling: `GB` = 10⁹, `GiB` = 2³⁰, bare `G` = 2³⁰.**
*What changes:* a config that says `100 GB` yields a partition 7% smaller than
it does today; `100 GiB` and `100 G` are unaffected.
Matches what the display side now does, and matches the drive's label — which
is the number a user copies when they write "the 500 GB disk".

**C. Reject the ambiguous spellings: accept only `GiB` and bare `G`, and error
on `GB` with a message naming both.**
*What changes:* configs using `GB` stop installing until edited; nothing is
silently resized.
The only option that cannot quietly give someone the wrong disk layout, at the
cost of breaking existing files loudly rather than leaving them wrong quietly.

### If this is never answered

Current behaviour is safe in the sense that it is deterministic and has been
the behaviour all along; no data is at risk. The concrete cost is that a
partition table written from a drive's advertised capacity will not fit on that
drive, and the error will point at the partition table rather than at the units.
It does not get worse with time, but every config file written in the meantime
is one more that option B or C would change.

**Recommendation:** B, but weakly, and only because it now disagrees with the
display code in the same tree. A is the defensible status quo. C is the honest
one and I would not argue against it.

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

## Resolved — pre-split (unprefixed `Q<n>`, single-agent era)

These numbers are not to be extended; new questions use `A-Q<n>` / `B-Q<n>` /
`C-Q<n>`.

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

---

## Q57 — Should the kernel run its own test suite on a user's boot? (lane A, 2026-08-22)

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

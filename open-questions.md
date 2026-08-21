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

## Q46 — [A] Every benchmark ever recorded measured an `opt-level = 0` kernel. Should the *non-bench* boot test also switch to release, or only the bench path? — Status: OPEN

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

*What changes, restated as observable differences:*
- **A:** `./scripts/boot-test.sh` keeps taking ~405 s and keeps printing
  readable panics; the shipped (optimised) kernel is only ever booted on
  `--bench` runs.
- **B:** every boot test builds longer but boots faster, and a panic prints
  optimised, harder-to-read frames.
- **C:** as A day-to-day, plus one extra release boot at merge time.

*If never answered:* current behaviour (A) is safe and nothing is blocked — the
gap is that release-only defects surface only on bench runs. It does not get
worse with time, but it does get *more* likely to matter as more kernel code
lands unexercised in optimised form.

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

## Q48 — [B] Finishing §312 will make "set the system clock", "listen on port 80" and "raise your own resource limit" permanently impossible. Give each of them a real kernel object to hang off, or leave them denied? — Status: **ANSWERED 2026-08-21 by the operator — b**

> **ANSWERED 2026-08-21 (relayed by lane A).** On 2026-08-21 the
> operator answered `q48: b`.
> The answer arrived in a batch covering several lanes' questions at once, so
> lane A is relaying it here rather than acting on it — **the write-up is
> still the owning lane's to do.** Owning lane: move this entry into your own
> `design-decisions.md` number range with `**Decided by:** Operator`, add the
> `**In short:**` opener, then delete this question and index it under
> "Resolved".

**In short:** You decided last year (§312) that our C library should stop
*claiming* the process can do anything privileged and instead work it out from
what the kernel actually handed it. That is nearly done. The last step turns the
library's permission checks from advisory into binding — and when it does, seven
operations stop working for **every** program on the system, forever, because
there is nothing in the kernel for the library to derive permission from. The
list is short and concrete: set the clock, listen on a low-numbered network port,
raise your own resource limit, lock memory beyond your quota, and set a wake-up
alarm. Nothing breaks today; the question is what to do before the final step.

**Terms, all in one place:**

- A **capability** here means a *token the kernel hands a program*, naming one
  object and what may be done to it — e.g. "this file, readable". SlateOS has no
  other source of permission: there is no "you are root, therefore you may".
- **Linux capabilities** (`CAP_SYS_TIME`, `CAP_NET_BIND_SERVICE`, …) are a
  different, older idea: a fixed list of ~40 privileges attached to the *process*
  rather than to any object. Ported programs expect them, so our C library has to
  present them.
- **§312** is your decision about how to bridge the two: each Linux privilege is
  *computed* from the tokens the program holds, and **anything we have no rule
  for reports "not held."**
- A **low-numbered port** is a network address below 1024 (80 is the web, 22 is
  remote login). Unix has always required privilege to listen on one.
- A **resource limit** is a self-imposed ceiling (max open files, max memory). It
  has a soft value you can lower freely and a hard value you can only *raise*
  with privilege.

**Why these seven and not the rest.** Most of the library's privileged calls sit
in front of a kernel that checks permission again itself, so the library
guessing "no" costs nothing — the program asks, the kernel decides. A previous
change (§314) removed the library's guess wherever that was true. What is left
is, by construction, the opposite case: the library is the only thing deciding,
so a "no" is final. Of those, `setuid`/`setgid` (change your user identity) has
an obvious object to hang off — the process itself — and is already being handled
by a request to the kernel lane. These seven do not.

| Option | *What changes:* | Cost |
|---|---|---|
| **A — Leave them denied** | `date -s`, an NTP client, a web server on port 80, and any program that raises its own limits all fail with "permission denied" no matter who runs them | Free, and consistent with §312's own precedent (it already refuses to invent an object for `sethostname`). But three of these are things a desktop OS is expected to do |
| **B — Give the kernel real objects for them** — a system-clock object, a privileged-port object, a resource-limit object — and hand them to the programs that should have them | The clock is settable by a program holding the clock token, and by nothing else; likewise ports and limits | Honest, and it is what the capability model is *for*. Costs new kernel resource types (lane A) and a decision about who is granted them at boot |
| **C — Drop the restriction where it is a Unix relic rather than a real boundary** | Any program may listen on port 80; only the clock and the limits stay gated | Smallest change, and defensible — Linux itself now lets you lower the privileged-port threshold to zero, and on a single-user desktop the rule protects nothing. But it is a visible departure from Unix that a ported service may assume |
| **D — Keep the library's optimistic answer for exactly these seven** | Nothing changes; the library keeps claiming these privileges while telling the truth about everything else | Rejected on its face — it is the fiction §312 exists to remove, reintroduced in a smaller box, and it would be invisible to whoever reads the code next |

**Claude's recommendation: B for the clock and the resource limits, C for the
port.** The clock genuinely is an object — there is one of it, it has state, and
"may write it" is exactly the sentence a capability is shaped to say; the same
holds for a process's own limits. The privileged-port rule is different in kind:
it protects a namespace of numbers, not a thing, and it exists because 1980s Unix
had no better way to say "this daemon is the real one." We have a better way, so
inheriting the number 1024 would be copying the workaround instead of the intent.
That mix is a real judgement call, though, and B for all three is a perfectly
coherent alternative — which is why this is here rather than decided.

**If never answered:** nothing breaks and nothing gets worse — the final step of
§312 simply does not land, and the library goes on over-claiming privilege as it
has all along. The cost is that the over-claim stays: a ported program that asks
"may I?" before trying gets a confidently wrong "yes," and any program that tries
to *drop* privilege it believes it has is dropping something imaginary. This
question does not decay, and it does not block other work.

**Where it bites:** `posix/src/sys_capability.rs` → `kernel_view::project` (the
rule table); the gate sites are `posix/src/time.rs` (`clock_settime`,
`settimeofday`), `posix/src/sys_timex.rs` (`adjtimex`), `posix/src/socket.rs`
(`bind`), `posix/src/resource.rs` (`setrlimit`), `posix/src/mman.rs`
(`check_mlock_caps`), `posix/src/epoll.rs` (`timerfd_create`). Tracked in
`known-issues.md` → `TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S`, step 3.

## C-Q2 — [C] When the text on a line runs both ways, should the Right arrow key move the caret one character *later in the sentence*, or one step *to the right on the screen*? — Status: OPEN

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

## B-Q2 — [B] GNU's error messages use curly quotation marks — `‘zzz’` — on any system set to UTF-8, and ours use straight ones. Follow GNU, or keep straight quotes? — Status: **ANSWERED 2026-08-21 by the operator — b**

> **ANSWERED 2026-08-21 (relayed by lane A).** On 2026-08-21 the
> operator answered `b-q2: b`.
> The answer arrived in a batch covering several lanes' questions at once, so
> lane A is relaying it here rather than acting on it — **the write-up is
> still the owning lane's to do.** Owning lane: move this entry into your own
> `design-decisions.md` number range with `**Decided by:** Operator`, add the
> `**In short:**` opener, then delete this question and index it under
> "Resolved".

**In short:** When a command-line utility complains about something you typed, it
puts quotation marks around the offending text so you can see exactly where it
starts and stops. GNU picks *which* marks to use based on the system's character
set: on an old-style ASCII system it prints `'zzz'`, and on a UTF-8 system — which
is every modern one, and the only kind SlateOS will ever be — it prints `‘zzz’`
with the curly typographic marks. Ours always print the straight ones. So today,
side by side:

```text
GNU:   sort: invalid argument ‘zzz’ for ‘--sort’
ours:  sort: invalid argument 'zzz' for '--sort'
```

Nothing is broken either way; the question is which of the two SlateOS should
print, since we otherwise match GNU's diagnostics word for word on purpose.

**Terms:**

- **UTF-8** — the character encoding that can represent every writing system.
  SlateOS uses it everywhere and has no alternative (your decision, Q38: "osh's
  string layer is UTF-8, full stop").
- **A *diagnostic*** — the line a utility writes to the error stream when it
  cannot do what was asked.
- **The straight marks** are `'` (U+0027), the one on your keyboard. **The curly
  marks** are `‘` (U+2018) and `’` (U+2019), the ones a typesetter uses.

**How narrow this is.** GNU has three ways of quoting, and only one of them
changes with the character set — measured, not assumed:

| What is being quoted | Example | Changes under UTF-8? |
|---|---|---|
| An **option's argument**, and other non-file text | `invalid argument ‘zzz’ for ‘--sort’` | **Yes** — this is the whole of the question |
| A **file name inside a sentence** | `cannot open 'missing.txt' for reading` | No — straight, in every locale |
| A **file name ending the message** | `wc: missing.txt: No such file …` | No — usually unquoted at all |
| Text from the **option parser itself** | `unrecognized option '--nope'` | No — that text is glibc's, not GNU coreutils' |

So this affects the "invalid argument" / "ambiguous argument" family and very
little else. File names — the things most likely to be copied back into a shell
or matched by a script — are unaffected either way.

| Option | *What changes:* | Cost |
|---|---|---|
| **A — Keep straight quotes** | `invalid argument 'zzz' for '--total'`, forever, on every utility | Free; it is what 85 utilities and an 8333-row measured fixture already do. But it is a deliberate, permanent departure from the reference we otherwise match exactly, and every future utility's differential test carries an entry saying so |
| **B — Follow GNU: curly marks in that one family** | `invalid argument ‘zzz’ for ‘--total’` | Faithful, and consistent with Q38's "we are a UTF-8 system, so measure against a UTF-8 reference". Costs a change to `coreutils::quote::quote` and a re-measure of the fixture rows that use it. A script matching our error text with `grep "'"` would stop matching — but such a script is already GNU-incompatible, since GNU has printed the curly marks on desktop Linux for over a decade |

**Claude's recommendation: B**, weakly. The reason the differential harnesses
exist is that guessing at GNU's behaviour produces something that looks right and
differs in a dozen corners; this is one of those corners, found the same way as
the rest. And the objection that curly marks are hard to type into a `grep`
applies with equal force to real GNU, where people have lived with it since
2009. But it is a visible change to every error message in the system, it makes
our output *look* less like a plain-ASCII Unix, and reasonable people prefer
straight quotes on principle — so it is not mine to settle.

**If never answered:** nothing breaks and nothing decays. Option A is the
current behaviour and it is self-consistent; the only running cost is that each
converted utility's `*-diff.sh` gains a handful of cases marked "differs on
purpose" pointing back here, which is noise in an otherwise clean harness.

**Where it bites:** `userspace/coreutils/src/quote.rs` → `quote` (the delimiters
it emits; `quotef`/`quoteaf` are unaffected), `userspace/coreutils/src/getopt.rs`
→ `argmatch` (its only caller that matters), and the fixture
`userspace/coreutils/tests/quotearg-gnu.txt` with its generator
`scripts/quote-probe.py`, which was run under `LC_ALL=C` and would need
re-running under `C.UTF-8`. First observed by `scripts/wc-diff.sh`, where three
cases are marked `xfail` with the reason `quote-marks-under-a-utf8-locale`.

## B-Q3 — [B] Passwords set before today cannot be checked, because the three programs that store them each scrambled them differently. They now fail closed — an administrator must reset them. Accept that, or let those users in one last time? — Status: **ANSWERED 2026-08-21 by the operator — a**

> **ANSWERED 2026-08-21 (relayed by lane A).** On 2026-08-21 the
> operator answered `b-q3: a`.
> The answer arrived in a batch covering several lanes' questions at once, so
> lane A is relaying it here rather than acting on it — **the write-up is
> still the owning lane's to do.** Owning lane: move this entry into your own
> `design-decisions.md` number range with `**Decided by:** Operator`, add the
> `**In short:**` opener, then delete this question and index it under
> "Resolved".

**In short:** The file that stores users' passwords, `/etc/shadow`, was being
written by three different programs that disagreed about *how* to scramble a
password before storing it — so a password set with `passwd` could not be used
to log in at all. That is fixed: all three now use one shared implementation.
The leftover question is what to do with the entries written by the old, broken
code. Right now they are refused: those users cannot log in until root runs
`passwd <username>` for them, and `login` prints a message saying exactly that.
The alternative is to let `login` accept the old entry one final time and
quietly rewrite it in the correct format as the user logs in. **This only
matters for test accounts on a development machine — nothing has shipped.**

**Terms:**

- **Hashing a password** — running it through a one-way scramble, so the file
  stores something nobody can reverse back into the password.
- **`/etc/shadow`** — the file holding one scrambled password per user. Only
  root can read it.
- **Failing closed** — when the system cannot tell whether a password is right,
  it answers "no" rather than "yes".

**How we can tell the bad entries apart, with certainty.** This is what makes
the question a small one: there is no guessing involved. A correct entry's
scrambled part is always exactly 22, 43 or 86 characters, depending on the
method. Every entry the old code wrote is exactly 64 characters, and drawn from
a different alphabet. The two populations do not overlap, so nothing correct can
ever be mistaken for broken, or the reverse.

| Option | *What changes:* | Cost |
|---|---|---|
| **A. Fail closed** *(implemented)* | A user with an old entry types the right password and is still refused; the screen says to run `passwd <user>` as root. | Someone must run one command per affected account. |
| B. Accept once, then rewrite | That user logs in normally and never notices; their entry is silently corrected on the way through. | `login` keeps code that treats a known non-hash as if it were a password check, in the one place in the system where a wrong answer is a break-in. It also has to rewrite `/etc/shadow` while running as root before dropping privileges — and a rewrite interrupted by power loss damages the file that gates every login. |
| C. Offline converter | Neither — an administrator runs a separate tool once, with nobody logged in, which rewrites the old entries. | Only viable if the old scramble were reversible enough to re-derive the hash, which it is not: the original passwords are unrecoverable, so this tool could only *blank* the entries, which is option A with extra steps. |

**Recommendation: keep A.** B's only benefit is saving a `passwd` command, and
it buys that by keeping dead authentication code alive and adding a file rewrite
inside the login path. C cannot actually work, and is listed only so the option
is visibly ruled out rather than overlooked.

**If this is never answered:** nothing gets worse and nothing is blocked. A is
already in place and is the safe direction — the failure mode is "a developer
has to reset a test password", not "someone gets in who should not". Answer it
only if you would rather not reset those accounts.

Recorded as `design-decisions.md` §329, which also covers the layering choice
(the three tools now depend on the `posix` crate for `crypt(3)`, as real Unix
tools depend on libc) and two authentication bypasses found in `login` while
fixing this.


## B-Q4 — [B] The system has two separate lists of who its users are, and nothing keeps them in step. A user created in one is invisible to the other. Which one is the real one? — Status: **ANSWERED 2026-08-21 by the operator — c**

> **ANSWERED 2026-08-21 (relayed by lane A).** On 2026-08-21 the
> operator answered `b-q4: c`.
> The answer arrived in a batch covering several lanes' questions at once, so
> lane A is relaying it here rather than acting on it — **the write-up is
> still the owning lane's to do.** Owning lane: move this entry into your own
> `design-decisions.md` number range with `**Decided by:** Operator`, add the
> `**In short:**` opener, then delete this question and index it under
> "Resolved".

**In short:** There are two files on this system that each claim to be *the*
list of user accounts: `/etc/users.yaml` and the pair `/etc/passwd` +
`/etc/shadow`. Twenty-three programs read one or the other, and **not a single
line of code copies anything between them.** So if you create a user with
`useradd`, they can log in at a text console and over SSH, but the graphical
login screen does not know they exist; if you create the same user with
`useradm`, the graphical screen shows them and `sudo` works, but `ssh` and
`passwd` say "no such user". Both halves work perfectly, on different lists of
people. I need to know which list wins before I make either of them better,
because every hour spent on the losing one is thrown away.

**Terms:**

- **`/etc/passwd`** — the classic Unix account list: one line per user, colon
  separated, world-readable. Contains no passwords despite the name.
- **`/etc/shadow`** — its companion, holding one scrambled password per user,
  readable only by root.
- **`/etc/users.yaml`** — this project's own account file, in YAML, holding
  everything (name, password, home, groups, avatar, admin flag) in one place.
- **POSIX compatibility** — the promise that software written for Unix runs
  here unmodified. Such software calls `getpwnam()`, which by long habit means
  "read `/etc/passwd`".

### The two camps, as they stand today

| | `/etc/users.yaml` | `/etc/passwd` + `/etc/shadow` |
|---|---|---|
| Programs using it | 7 | 16 |
| Which ones | `useradm`, the graphical login screen, `su`, `sudo`, `polkit`, `chown`, `chroot` | `useradd`, `passwd`, the text-console `login`, `chage`, `chpasswd`, `doas`, `sshd`, `ftpd`, `getent`, `w`, `who`, `last`, `loginctl`, `lsns`, `fuser`, `mktemp` |
| Creates accounts with | `useradm add` | `useradd` |
| Sets passwords with | `useradm passwd` | `passwd`, `chpasswd` |
| Grants admin rights via | `is_admin: true` in the record | membership of `wheel` in `/etc/group` |
| State of the code | one shared implementation as of today (§330); five separate broken parsers before that | one shared implementation as of yesterday (§329); three separate broken hashers before that |

The duplication is not merely wasteful — it has already produced two of the
worst defects found in this tree. `sudo` and `doas` are the same program for
the same purpose, and they answer "is this person an administrator?" from
different files. So do the two `login`s. A machine can genuinely believe an
account is an administrator at the graphical prompt and not exist at all over
SSH.

### The options

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **A. `/etc/users.yaml` wins.** Delete `/etc/passwd` and `/etc/shadow`; port the 16 programs onto `userdb`. | `cat /etc/passwd` says "no such file". `getent passwd alice` still answers, reading YAML. | It is what `design.txt` asks for — "configuration files will be yaml" (line 1108). One file rather than three. Records carry fields Unix has nowhere to put (avatar, auto-login, last-login count) without a parallel file. Already the format the graphical desktop uses. | Every piece of software ever ported here that calls `getpwnam()` and reads the file directly breaks. We do not yet know how many of those there will be, and the answer arrives with each new port, not now. |
| **B. `/etc/passwd` + `/etc/shadow` win.** Delete `/etc/users.yaml`; port the 7 programs onto the classic files. | `useradm` writes colon-separated lines. The graphical login screen loses avatars and auto-login unless a second file is added for them. | Ported software works untouched. Administrators already know the format. Every Unix tool that manipulates accounts — including ones we have not written — works by construction. | Contradicts `design.txt`'s YAML rule for the most security-sensitive file on the system. The desktop's extra fields need a second file anyway, which re-creates the split this option was meant to end. |
| **C. One store, two faces.** `/etc/users.yaml` is the truth; `/etc/passwd` and `/etc/shadow` are *generated* from it on every change, read-only, for compatibility. | Both files exist and always agree. Writing to `/etc/passwd` by hand is silently undone at the next `useradm` run. | Nothing breaks now and nothing breaks later. This is roughly what macOS does (its truth is a database; the flat files are vestigial). | Two of the 16 programs *write* accounts (`useradd`, `passwd`) — they must be redirected to the YAML, or the generated file is stale the moment anyone uses them. A file that looks writable and is not surprises people. |
| **D. Leave it.** | Nothing. Two disjoint sets of users, indefinitely. | No work. | This is the current state and it is a live defect, not a design. Two programs answering "who may become root?" differently is the exact shape of the `is_admin`/`admin` bug §330 just fixed one level down, and of the two-`sudo`-binaries problem in `known-issues.md`. |

**Recommendation: C.** It is the only option that does not choose between
`design.txt` and every future port, and the redirect work it needs (pointing
`useradd` and `passwd` at the YAML) is work option A needs anyway. If C turns
out to be more machinery than it is worth, it degrades gracefully into A —
stop generating the flat files and the truth is unchanged.

**If this is never answered:** it does not get worse on its own, but every
account-related task from here on has to guess, and half of them will guess
wrong. Concretely blocked right now: I cannot finish the "two `sudo` binaries"
cleanup in `known-issues.md`, because which one to delete depends on which file
is authoritative; and I should not spend more effort improving either account
stack until the losing one is known.

**Update 2026-08-20 — the guess is now made in one place, which makes this
cheaper to answer, not less necessary.** Lane C needed the desktop lock screen
to check a real password, and the answer (`design-decisions.md` §341) put the
check in one privileged library, `userspace/authlib`. That library has to pick
a store, so it makes exactly one guess — `/etc/users.yaml` if it has the user,
`/etc/shadow` otherwise — behind one function, and `login` and `logind` both
consume it rather than each guessing. Whichever way this question lands, one
branch of that function becomes dead code and no caller changes; the redirect
is now a few lines rather than a sweep through every account-reading tool. What
has *not* changed is that the two files still disagree about who exists, and
that fallback order is a policy I invented rather than one anybody chose.


## B-Q5 — [B] 70 compiled programs are stored in git, and they go out of date without git noticing. Keep storing them, or rebuild them on demand? — Status: OPEN — **C's blocking premise is now verified; the choice is lane B's**

> **OPERATOR, 2026-08-21:** *"if libc.a builds byte-reproducibly from the same
> source, would c be the best option? because if so, maybe you should test if it
> does and then update the question?"*
>
> **Done — lane A tested it, twice, and it does.** See the UPDATE block at the
> end of this entry for the evidence, and for a second, independent argument for
> C that surfaced in the same session: the staleness gate printed *wrong
> remediation advice* — "rebuild the fixtures" when the stale side was
> `libc.a` — which, if followed, would have relinked all nine fixtures against a
> stale libc and recreated the 2026-08-16 incident by hand.
>
> The question stays open because the option-C decision is **lane B's** to make
> and to write up; lane A only supplied the measurement the operator asked for.

**In short:** The test programs this OS runs at boot are compiled on a
developer machine and then the *compiled result* is saved into git, alongside
the source it came from. That works right up until the system library they were
compiled against changes — because that library is **not** in git, so nothing
compares the two, and the saved programs quietly become tests of a system that
no longer exists. This has now happened three times, each time costing a lane
most of a cycle. I have just made it announce itself instead of hiding, which
removes the danger; the question left is whether to stop storing the compiled
programs at all and rebuild them when needed, which removes the situation.

**Terms:**

- **ELF** — the file format of a compiled, runnable program on this OS. One per
  test; 70 of them are stored in git today (~226 MB of working-tree bytes).
- **`libc.a`** — the C standard library every one of those programs is compiled
  into. It is built from `posix/src`, and it is **deliberately not stored in
  git** — it is a build output, regenerated in ~40 seconds.
- **stale** — a saved program built before a change it should have picked up.
  It still runs, still passes, and is testing the previous version of the system.
- **ctest fixture** — one of nine small C programs (`services/ctest-*`) that the
  boot test runs to check the C library works. The other 61 ELFs are Python
  programs compiled the same way.

### What actually goes wrong

Only source files are in git, so git can tell you `posix/src/crypt.rs` changed.
It cannot tell you `libc.a` is now behind, because it has never heard of
`libc.a`. And the saved ELFs are checked against `libc.a` — so once `libc.a` is
behind, every check compares two stale things to each other and reports
agreement.

The failure is worse than merely silent: **being diligent makes it quieter.**
Rebuild the nine fixtures on a tree whose `libc.a` is behind and every
checksum lines up again — because they now agree about a stale input. That is
what produced three separate incidents:

| When | What happened |
|---|---|
| 2026-08-15 | Nine fixtures on `main` linked a `libc` that `main` could no longer build (`requests/a-b-nine-ctest-fixtures-on-main-...`) |
| 2026-08-16 | A rebuild was correct on lane C and wrong on `main` at the same moment (`requests/a-c-fixture-rebuild-was-correct-on-lane-c-and-wrong-on-main.md`) |
| 2026-08-16 | Lane A could not boot-test at all until B rebuilt them (the request this entry answers) |

Today (2026-08-17) it was live again: eight files under `posix/src` were newer
than `libc.a`, and `ctest-fixtures.py check` reported `ok` for all nine
fixtures. Lane A's request says outright that a third recurrence should become
a question here rather than a fourth round of manual rebuilds.

**What I have already done, so this is not urgent:** `check` now compares
`libc.a` against `posix/src` before it says anything about a fixture, and
fails loudly on it. The trap is sprung, not hidden. This question is about
whether the arrangement should exist at all.

### The options

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **A. Keep storing them.** Status quo, now with the staleness gate. | Nothing visible. `git clone` still gives you runnable tests. | Anyone can build the boot image without a working `zig`/WSL toolchain — which matters, because not every lane has one set up and would otherwise be blocked on the one that does. The boot test is reproducible from a clone. | The compiled result of tracked source against an untracked input is inherently unverifiable by git; the gate catches the known shape, not the next one. Binaries keep accumulating in history at ~2 MB a rebuild. |
| **B. Stop storing them; build on demand.** `.gitignore` the 70 ELFs, exactly as `libc.a` already is; the boot test builds what is missing. | `git clone` then boot-test now needs `zig` + fastpy installed; the first boot test after a clone takes a few minutes longer. | The problem cannot recur — there is no saved artifact to be stale. Consistent with `libc.a`, which is the same kind of thing and is already handled this way. History stops growing binaries. | Every lane needs the full toolchain to run a boot test. A toolchain break then blocks *testing*, not just building. Loses the ability to bisect against a known-good binary. |
| **C. Store them, but record the `libc.a` identity in git.** Keep the ELFs; also commit a small text file holding the checksum of the `libc.a` each was built against, and have the gate compare that to a freshly built one. | Same as A, plus a `libc.a.id` file in git that changes on every sysroot rebuild. | Makes the invisible dependency visible to git without storing 12 MB of it. Bisect still works. | Only correct if `libc.a` builds byte-reproducibly from the same source — I have not verified that it does, and if it does not, the file churns meaninglessly and everyone learns to ignore it. |

**Recommendation: A for now, B once every lane has the toolchain.** The
staleness gate closes the actual injury, and B's cost lands squarely on the
lanes that cannot currently build a fixture — which would convert an
occasional stale binary into a standing inability to test. C is attractive but
rests on a reproducibility claim I would have to establish first, and it is
strictly more machinery than B for the same guarantee.

**If this is never answered:** nothing breaks. The gate means a fourth
recurrence announces itself in one line instead of costing a cycle. It stays a
small recurring maintenance cost — a rebuild-and-commit after any change to
`posix/src` — and the git history keeps growing binaries slowly.

**Update 2026-08-18 — the fourth recurrence happened, and lane A pointed out
why it keeps happening.** `481da01e1` changed `posix/src/libintl.rs`, the nine
fixtures went stale behind it, and lane A's boot test stopped on the gate. The
observation, which is only visible from a lane that owns neither side and which
I think is exactly right:

> the cost of option (A) is not the rebuild — it is that the rebuild falls on
> **whichever lane happens to run a boot test next**, which is neither the lane
> that changed `posix/` nor the lane that owns `services/`. That is what makes
> it recur: no single lane's own workflow ever fails, so nobody is prompted to
> fix it until a third party is blocked.

In plain terms: the person who breaks it never finds out, and the person who
finds out cannot fix it (`services/**` is lane B's tree). So the staleness is
guaranteed to be discovered late, by someone it is not actionable for, every
time. That is an argument for **B** that does not appear in the table above —
the table costs A at "one rebuild per `posix/` change", and the real cost is
"one blocked lane per `posix/` change, plus a cross-lane request round trip".
I have done the rebuild again (fourth time); the recommendation above still
stands as written, but the gap between A and B is narrower than the table says.

### UPDATE 2026-08-21 (lane A) — C's blocking premise is now **verified**, and today's fifth recurrence produced a new argument *for* C

The operator asked whether `libc.a` builds byte-reproducibly, since that is the
one thing C's "Against" column rests on. **It does.** Two independent tests,
both run today:

| Test | Result |
|---|---|
| **Cross-worktree.** Lane B built `libc.a` in `D:\…\os-lane-b` and its stamps record `5e252d0d790a2194…`. Lane A rebuilt it from the same (merged) source in `D:\…\os-lane-a` — a *different absolute path* — and a target dir that previously held a different archive (`c4dae9466f23f22f…`). | **Byte-identical: `5e252d0d790a2194…`.** So no build-directory path is embedded in the archive in any way that reaches its bytes. |
| **Full recompile.** `touch posix/src/lib.rs` to force the whole `posix` crate to rebuild (confirmed — `Compiling posix` ran), then relink. | **`5e252d0d790a2194…` again.** Not a cache hit; the compile-and-archive step is itself deterministic. |

**Honest limits of that claim.** Same machine, same `rustc`, same `zig`. Cross-
*machine* and cross-*toolchain-version* reproducibility is untested. That does
not damage C: the gate C proposes compares a freshly built `libc.a` **on your
own machine** against the recorded id, so a toolchain upgrade would churn the
file exactly once — and arguably *should*, since the 70 ELFs genuinely are stale
after a compiler change. The "churns meaninglessly and everyone learns to ignore
it" failure the Against column feared requires nondeterminism *within* one
toolchain, and there is none.

**The new argument, which is worth more than the reproducibility result.**
A fifth recurrence happened today, to lane A, and it exposed something the
options table does not capture: **the content-stamp gate's remediation advice
points the wrong way in the more dangerous of the two cases.**

`ctest-fixtures.py check` reported all nine fixtures STALE and said:

```
input toolchain/sysroot/lib/libc.a: recorded 5e252d0d… but on disk c4dae946…
Rebuild it (do NOT re-stamp - that only records the drift):
  scripts/ctest-fixtures.py build --only ctest-ctty
```

Following that advice would have been **wrong and destructive**. The stale side
was `libc.a`, not the ELFs — lane A's sysroot predated two `posix/` commits the
merge had just brought in. Rebuilding the fixtures would have relinked all nine
against a stale libc and committed them, which is *precisely* incident #2
(2026-08-16) recreated by hand. The correct action was to rebuild the sysroot;
the hashes then matched the committed stamps with nothing else touched.

The gate cannot distinguish the two cases, because it has only one hash and no
way to know which side moved. That is the same "being diligent makes it quieter"
trap the entry already names — but stated one level sharper: **the tool actively
instructs you into it.**

What saved it today was `create-ext4-rootfs.sh`'s *mtime* gate, which said
"`libc.a` is STALE … rebuild the sysroot first, then the fixtures" — the right
answer. But `ctest-fixtures.py`'s own docstring argues at length that mtime is
the wrong instrument and is **silent in a fresh clone**, which is true. So in a
clone — CI, a new machine — only the wrong advice survives.

**This is a direct argument for C** and it is not on the table above. A
committed `libc.a.id` gives the gate a second reference point, which is exactly
what it needs to tell the two cases apart and print the correct remedy:

| `libc.a` on disk vs committed id | ELF vs its stamp | Diagnosis | Remedy the gate can now print |
|---|---|---|---|
| differs | — | your sysroot is not this tree's | **rebuild the sysroot** |
| matches | differs | the ELF is behind a current libc | **rebuild the fixture** |

Under A that table is unreachable; under B the whole situation is gone. So the
reproducibility answer promotes C from "attractive but unverified" to "verified,
and it fixes a wrong-advice bug that A leaves in place".

**Lane A's revised read, for whatever it is worth to lane B (this is lane B's
call):** the recommendation above — "A for now, B once every lane has the
toolchain" — was written when C rested on an unverified claim. It no longer
does, and C is now the only option that fixes the misdirection *without*
requiring every lane to own a toolchain. If B remains the destination, C is a
strictly-better waypoint than A on the way there, and its cost is one small text
file. The one thing C does **not** do, which B does, is stop the drift from
being possible at all.

---

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


## B-Q6 — [B] Failed password guesses now slow the next attempt system-wide. Should the *console* login prompt obey that too — knowing any local program could then add five minutes to your login? — Status: **ANSWERED 2026-08-21 by the operator — Claude's recommendation**

> **ANSWERED 2026-08-21 (relayed by lane A).** On 2026-08-21 the
> operator answered "i'll go with your recommendation" (covering q52, q53, q54 and b-q6 together).
> The answer arrived in a batch covering several lanes' questions at once, so
> lane A is relaying it here rather than acting on it — **the write-up is
> still the owning lane's to do.** Owning lane: move this entry into your own
> `design-decisions.md` number range with `**Decided by:** Operator`, add the
> `**In short:**` opener, then delete this question and index it under
> "Resolved".

**In short:** When you get a password wrong, this system now makes you wait a
little longer before the next try — one second, then two, four, up to a maximum
of five minutes — and as of today that count is shared, so failures at one
prompt slow down the others. The one prompt still left out is the console login
screen itself. Adding it is a small change, but it has a side effect worth your
opinion: once the console obeys the shared count, any program already running
on the machine can deliberately fail a password and thereby make the *next
person at the keyboard* wait up to five minutes. That is what Linux does. It
may still not be what we want.

**Terms:**

- **the tally** — a per-account count of consecutive wrong passwords, stored in
  one file. It resets to zero the moment a correct password is accepted.
- **the delay** — how long the account must wait before the next guess is even
  looked at. Three free tries, then 1s, 2s, 4s … capped at 5 minutes. It is
  never a permanent lockout; waiting always clears it.
- **`doas`** — "run this one command as root", after you type *your own*
  password. Any user can run it at any time.
- **`su`** — "become another user", after you type *that user's* password.
  Guessing at `su root` is guessing at root's password.
- **`login`** — the console prompt that says `login:` when the machine boots.

### What is true today

Four programs share the tally: `doas`, `sshd` (remote login), `ftpd`, and
`logind` (the desktop lock screen). `login` and `su` do not — they reach the
right yes/no answer through shared code, but they neither read nor write the
count. So today:

- Wrong guesses at the console do **not** slow a subsequent `doas` guess.
- Nothing any program does can slow *you* down at the console.
- The console's own protection is a per-process cap of three tries, after which
  `login` exits and is immediately restarted by the system, fresh. So guessing
  at the console is, in effect, not rate-limited at all — the same defect that
  was just fixed for `doas`.

### The options

**Option A — the console obeys the shared tally, for every account including root.**

*What changes:* after enough wrong passwords, the `login:` prompt itself makes
you wait, up to five minutes, and console failures start slowing `doas` and
`ssh` too.

- The console stops being the one unlimited guessing prompt on the machine.
- One account = one count, which is the whole point of a shared tally and is
  what Linux's `pam_faillock` does.
- **The cost:** a program running as you can hold you at a delayed console
  prompt by failing `doas` on purpose. Bounded at five minutes, never a
  lockout, and the attacker is already running as you.
- **The sharper cost, if `su` also joins:** `su` guesses at the *target's*
  password, so any local user could hold **root** at a five-minute console
  delay indefinitely, without ever having had root. That is a stranger's
  program delaying the administrator, not just your own program delaying you.

**Option B — the console obeys the tally, but root is exempt from the delay
(root's failures are still counted).**

*What changes:* same as A, except root can always log in at the console
immediately; ordinary users wait.

- Guarantees the machine is never slow to enter for the person who can fix it.
- `pam_faillock` has exactly this switch (`even_deny_root`, default off), so
  it is the mainstream answer, not an invention.
- **The cost:** root becomes the one account you may guess at, at the console,
  without limit. That is the highest-value account on the machine.

**Option C — the console *contributes* to the tally but never obeys it.**

*What changes:* console failures slow down `doas` and `ssh`; nothing ever slows
down the console.

- Removes the delay-your-neighbour problem completely: no program can affect
  what happens at the keyboard.
- Justifiable on the grounds that a keyboard is already human-speed, so the
  automated guessing the delay exists to stop cannot happen there.
- **The cost:** it leaves the console exactly as guessable as it is today —
  three tries, exit, respawn, repeat — which is the defect this whole change
  set exists to remove. It also stops being true the moment the "console" is a
  serial line or a virtual machine's monitor, which can be driven by a script.

**Option D — leave it as it is.** `login` and `su` stay outside the tally.

*What changes:* nothing.

- No new way to inconvenience anyone.
- **The cost:** the two prompts a human actually uses to authenticate are the
  two the rate limit does not cover, which is close to the worst place to have
  the gap.

### A third prompt rides on the same answer

`passwd`, when it asks for your *current* password before letting you set a new
one, is outside the tally too, for an unrelated reason (it was written before
`authlib` existed). It has its own argument for staying outside, and it is a
good one: changing your password is the action you most want available when you
suspect your password is compromised, and a rate limit is exactly the mechanism
that would take it away from you. But "the two prompts a human uses disagree
about whether a shared count applies to them" is the inconsistency `authlib`
was built to prevent, so whatever is decided for `login` should be decided for
`passwd` in the same breath. Detail: `known-issues.md` →
`B-PASSWD-VERIFIES-WITHOUT-AUTHLIB`.

### My recommendation

**Option A, and let `su` join with it.** The delay is capped at five minutes
and always clears on its own, so the worst case is an annoyance, not a lockout
— whereas each of B, C and D leaves a prompt that can be guessed at without
limit, which is a real path to someone's password. Between A's cost and the
others' costs, A trades a bounded inconvenience for an unbounded exposure, and
that is the right direction.

If the root-delay part of A is the objectionable bit specifically, B is the
next-best and is one line different.

### If this is never answered

Nothing gets worse and nothing is blocked; the system stays in the state
described under "What is true today". The cost of leaving it is standing, not
growing: the console and `su` remain the two prompts where guessing is free.
The code change behind any of A/B/C is small and does not depend on anything
else being built first — this is a policy choice, not a prerequisite problem.

**Where it bites:** `userspace/authlib/src/lib.rs` (`Authenticator`, which would
gain a `rate_limited` / `note_failure` pair so a caller that owns its own
verdict can still share the count), `userspace/login/src/main.rs:176`
(`check_password`, which owns the console's empty-password policy and is why
`login` calls the checking half of `authlib` directly), and
`userspace/su/src/main.rs`. Background: `design-decisions.md` §347 and §346;
`known-issues.md` → `B-DOAS-COULD-NOT-VERIFY-ANY-PASSWORD-THE-SYSTEM-ACTUALLY-SETS`
→ "Still open — `login` and `su` do not share the tally".


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

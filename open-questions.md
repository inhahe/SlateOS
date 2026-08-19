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

## Q45 — [A] The kshell byte-purity conversion is ~40× larger than its own scoping estimate. Convert the whole shell, or make only the *expanded word* byte-clean? — Status: OPEN

**Background.** `known-issues.md` → `TD-KSHELL-LINE-EDITOR-IS-UTF8` records that
the shell cannot type or tab-complete a non-UTF-8 filename, and prescribes
converting the editor **and** the statement executors together, on the
reasoning that a partial conversion just relocates the lossy step from the
keyboard to the parser entry, where it is *less* visible. Stage (a) of that
plan has landed (`kernel/src/bytestr.rs`, commit `d19372dd4`).

**What changed.** Measuring the remaining stages against the actual file, the
scope is far larger than the entry assumed. `kernel/src/kshell.rs` is **84,845
lines**, and **879 of its 1,024 functions** take or return `&str`/`String`.
The entry's "~1520 call sites" counted *method calls*, not signatures.
`execute_single` is a full bash-like parser — alias expansion, array syntax,
`(( ))` arithmetic, `eval`, pipes, redirects, heredocs — and its logic is
text-oriented throughout (`line.starts_with("((")`, `line.get(2..)`, …). So
"one coherent change over the editor and the statement executors" means
rewriting essentially the entire shell in a single commit.

Worth noting what is *not* implicated: only **6** `from_utf8_lossy` sites exist
in the whole file, and all six are file-*content* formatting (`column`, `diff`),
not path handling. The byte-purity problem is confined to the path pipeline.

**Options.**

- **A — Convert the whole shell as one commit, per the original entry.**
  *Pros:* no lossy step anywhere; matches the entry's stated reasoning; every
  command becomes byte-clean including non-path arguments.
  *Cons:* an 879-signature rewrite of an 85k-line file, unreviewable as one
  diff, landing against a working shell; a single mechanical slip breaks a
  shell that currently works, for a defect the entry itself classifies as *not*
  data loss ("nothing is corrupted or silently lost… a usability gap in one
  interactive front end"). Costly to reverse. Many boot-test cycles.

- **B — Make the *expanded word* byte-clean, not the command line.** Keep the
  source line as text; convert word expansion, `resolve_path`, tab completion
  and the path-consuming commands to `[u8]`. The user reaches arbitrary bytes
  via the `$'\xff'` escape, and completion emits that spelling for candidates
  that are not valid UTF-8.
  *Pros:* this is **exactly how bash works** — the script source is text, the
  expanded argument is a byte string; the shell already parses `$'…'` (7 sites),
  so the input mechanism exists; it fixes the actual user-visible bug; the diff
  is a small fraction of A and is genuinely coherent along the *data-flow* axis
  rather than the layer axis, so it introduces no lossy step.
  *Cons:* a literal raw 0xFF byte still cannot be *typed* directly (it must be
  escaped); departs from the plan recorded in the entry.

- **C — Leave it as documented debt.** *Pros:* zero regression risk. *Cons:*
  the gap persists; CLAUDE.md's byte-purity rule stays violated in this front
  end.

**Claude's recommendation: B.** It fixes the real defect (a non-UTF-8 filename
becomes reachable and completable) at a small fraction of A's risk, and it is
the design real shells actually use — the entry's "partial conversion is worse
than none" objection targets a *layer* split (editor byte-clean, parser not),
which B is not: B converts one data path end-to-end. A's extra benefit over B
is byte-purity for non-path arguments, which no known use case needs.

This is flagged rather than decided because A vs. B is an architectural fork on
a large, costly-to-reverse change, and because B knowingly departs from a plan
already written into `known-issues.md`.

**Where it bites.** `kernel/src/kshell.rs`: `resolve_path` (208, 270 call
sites), `get_cwd` (194), the editor — `line_buf` (2872), `History.entries`
(2631), `replace_line` (2921), `redraw_from_cursor` (2955),
`reverse_search_mode` (3061), `read_line` (3191) — and `execute` (3872) /
`execute_single` (4149) plus the sibling statement executors. Helpers already
exist in `kernel/src/bytestr.rs`.

**In the meantime** Claude is not starting either conversion, and is picking up
other unblocked Lane A work.

---

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

## Q48 — [B] Finishing §312 will make "set the system clock", "listen on port 80" and "raise your own resource limit" permanently impossible. Give each of them a real kernel object to hang off, or leave them denied? — Status: OPEN

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

## C-Q3 — [C] `CLAUDE.md` tells all three lanes to publish finished work through one shared folder, and two of them collided in it today. Change the instruction? — Status: OPEN

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

## C-Q5 — [C] This OS writes all of its own cryptography by hand. Keep doing that, or port implementations other people have already broken and fixed? — Status: OPEN

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

## C-Q4 — [C] Nothing in the system can print. Two half-built printing features exist and neither is connected. Which one should applications talk to? — Status: OPEN

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

## B-Q2 — [B] GNU's error messages use curly quotation marks — `‘zzz’` — on any system set to UTF-8, and ours use straight ones. Follow GNU, or keep straight quotes? — Status: OPEN

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

## Q49 — [A] We cannot run, or even switch on, a graphics driver for any AMD card made in the last 20 years. Write one anyway, buy hardware, or say we don't support them? — Status: OPEN

**In short:** The plan says the OS should have a driver for AMD graphics cards.
It turns out there is no way to *try one out*: the emulator we develop against
doesn't imitate any recent AMD card, and this PC has an NVIDIA card in it. So
such a driver could be written, but never started even once — no picture, no
error, nothing to tell us whether any of it works. The emulator does faithfully
imitate two *very old* AMD cards, and I've written a driver for those, which
really does run. The question is what to do about the modern ones: write code we
can't test, get hardware so we can test it, or state plainly that this OS drives
AMD cards through the generic fallback only.

**Some terms.** *QEMU* is the emulator we boot the OS in for every test.
*Passthrough* means handing a real graphics card straight to the emulated OS,
which needs a spare card physically in the machine. *Mode-setting* is the step
that tells a monitor which resolution and refresh rate to display — the part
that turns the screen on. *virtio-gpu* is a "pretend" graphics card that only
exists inside emulators; it works well there and does not exist on real
hardware. The *bootloader framebuffer* is a plain block of pixels the boot
firmware hands us: it always works, but the resolution is fixed at boot and
cannot be changed, and there is no acceleration of any kind.

**What is already settled and not in question.** The old-card driver is done
and running (`design-decisions.md` §217). Its timing arithmetic is shared with
the newer chips, so none of it is wasted whichever way this goes. Writing it
immediately caught a genuine bug that a never-run driver would have kept
forever. This question is *only* about whether to chase modern AMD cards, and
if so how.

**Option A — leave it: old AMD cards, virtio-gpu, and the bootloader
framebuffer are the supported set.**
*What changes:* nothing today. On a real PC with a modern AMD card, the desktop
still appears — at whatever resolution the firmware picked, unchangeable, with
all drawing done by the CPU.
- *For:* every line of graphics code we have stays code we have actually run.
  No effort is spent on something we cannot check.
- *Against:* the OS then doesn't really support the graphics hardware in a large
  share of desktop PCs, and the plan document says it should.

**Option B — write the modern driver blind, from documentation only.**
*What changes:* the roadmap item gets ticked; behaviour on real hardware is
unknown and stays unknown.
- *For:* it is what the plan literally asks for, and AMD publish thorough
  documentation.
- *Against:* I think this is actively worse than doing nothing. A modern card
  needs firmware loading and a long power-up sequence before it will display
  anything, and there is no way to discover which step we got wrong — the whole
  thing either works or produces a black screen, with no clue in between. Worse,
  the roadmap would then claim AMD support that nobody has ever seen work, which
  is a claim we'd have to un-make later.

**Option C — put an AMD card in the machine (or a second PC), then write it
against real hardware.**
*What changes:* the driver becomes as testable as everything else. Costs money
and physical setup; a spare graphics card, and passthrough also wants the card
to be one the host isn't using for its own display.
- *For:* the only option that ends with a *tested* modern driver. It would also
  unblock testing every other real-hardware path we currently can't reach.
- *Against:* real money and real setup work, for one subsystem. And it only
  covers whichever card we buy.

**Option D — do the mode-setting half blind, skip the rest.**
*What changes:* on modern AMD hardware, the resolution might become changeable;
acceleration still wouldn't exist.
- *For:* mode-setting is the most valuable half and the most likely to be right
  from documentation.
- *Against:* on modern cards mode-setting is downstream of the same firmware
  load and power-up sequence as everything else, so it isn't actually the
  separable half it is on the old chips. This mostly gets option B's problems
  for half the benefit.

**Recommendation: A now, C if you want real-hardware support at some point.**
A is honest and costs nothing; C is the only path to a driver we could stand
behind. I'd avoid B outright — I would rather the roadmap say we don't support
these cards than say we do on the strength of code nobody has ever run. But C is
a spending decision and a hardware decision, which is yours and not mine.

**If this is never answered:** nothing breaks and nothing worsens. The OS keeps
displaying through virtio-gpu in the emulator and the bootloader framebuffer on
real machines. The only standing cost is that roadmap §3.1 stays open and the
plan document keeps asking for something we have decided nothing about. There is
no time pressure and no drift.


## Q50 - [A] The next graphics task is an Intel driver we also cannot run -- and the Intel chip it targets is already inside this PC, switched off. Which way? - Status: OPEN

**In short:** The next planned graphics job is a driver for Intel's built-in
graphics, which is what most laptops display through. It runs into the same wall
as the AMD question above -- there is no way to actually run it and see. One
detail differs though, and it is worth knowing before deciding either question:
the Intel graphics chip this driver targets is *already inside this PC*, built
into the CPU, and merely switched off in the firmware settings. So for Intel,
unlike AMD, no hardware purchase is needed. What is missing instead is that
SlateOS has never once started up on a real PC -- only ever inside the emulator.

**Some terms.** *BIOS* / *firmware settings* is the setup screen you can enter
before the OS starts. *Integrated graphics* (or *iGPU*) is a graphics chip built
into the CPU instead of being a separate plug-in card. *QEMU* is the emulator we
boot the OS inside for every test. *Passthrough* means handing a real chip
straight through to the OS running inside the emulator. *Bare metal* means
running on a real PC directly, with no emulator underneath. *i915* is the
long-standing name of Linux's Intel graphics driver, and so of the equivalent we
would write.

**What was measured today, not assumed:**

| Fact | Evidence |
|---|---|
| This PC's CPU contains Intel UHD Graphics 630 | CPU is a `Intel(R) Core(TM) i7-8700K`; that model includes UHD 630 (a mainstream, well-documented i915 target) |
| That chip is switched off, not absent | Windows lists only `NVIDIA GeForce RTX 4090`; **no** Intel display device enumerates on the PCI bus at all -- the usual state when a separate card is fitted. Re-enabling is a firmware toggle, not a purchase |
| Passthrough is unavailable here regardless | QEMU on this Windows host accelerates via WHPX, which has no device-passthrough at all. The mechanisms that would do it -- VFIO, and Intel's GVT-g graphics partitioning -- are Linux-only |
| SlateOS has never run on real hardware | No roadmap item exists for it; every boot to date is QEMU. **This, not the chip, is the actual gate** |

**Option A -- treat Intel like Q49 Option A: the generic fallback is the
supported story, and close the roadmap item saying so.**
*What changes:* nothing today. On a real laptop the desktop still appears, at
whatever fixed resolution the firmware chose, with all drawing done by the CPU.
- *For:* honest, costs nothing, and every line of graphics code we ship stays
  code we have actually run.
- *Against:* the plan document asks for this driver, and Intel integrated
  graphics is the single most common display hardware in desktops and laptops.

**Option B -- write i915 blind, from documentation only.**
*What changes:* the roadmap item ticks; behaviour on real Intel hardware is
unknown and stays unknown.
- *For:* the generation in question (UHD 630) is genuinely the friendly case --
  it needs no firmware blob loaded into the chip and has a far shorter power-up
  sequence than a modern AMD card, so noticeably more of it would plausibly be
  right from the documentation than Q49's option B would be.
- *Against:* still unverifiable, and it still puts a claim of Intel support in
  the roadmap that nobody has ever seen work. I give this the same answer as in
  Q49, just less emphatically.

**Option C -- switch the iGPU on in firmware, get SlateOS booting on bare metal,
then write i915 against the real chip.**
*What changes:* the driver becomes testable on real silicon, at no hardware cost.
- *For:* the only path that ends with a *tested* Intel driver using hardware we
  already own. Bare-metal boot would also unblock every other real-hardware path
  we currently cannot reach at all -- storage, USB, ACPI, real monitor EDID.
- *Against:* bare-metal boot is a large prerequisite and a substantially bigger
  job than the driver it would enable; the driver is the small half. It also
  carries the only real risk in this question: a mistake means a black screen on
  the machine you work at. (Bootable USB, Windows disk untouched, would contain
  that.)

**Option D -- put Linux on a spare disk or partition and test through KVM with
GVT-g or VFIO passthrough.**
*What changes:* the emulator can hand the real Intel chip to SlateOS, so i915
becomes testable without SlateOS having to boot a real PC first.
- *For:* far smaller than C, keeps the crash-safety of a virtual machine, and
  yields a *repeatable automated* test rather than a manual one -- which is what
  the rest of this project's testing depends on. GVT-g explicitly covers this
  chip's generation.
- *Against:* a second OS to install and maintain, and the boot-test scripts
  currently assume Windows paths, so they would need a Linux path. Intel has also
  deprecated GVT-g in recent kernels, so this may mean pinning an older kernel;
  plain VFIO passthrough of an iGPU works too but is fiddly to set up.

**Recommendation: A for now -- same as Q49.** If you do want real graphics
support at some point, I would pick **D over C**: it costs no money, keeps
testing automated instead of manual, and it is the same setup that would let us
test a modern AMD card if you ever fit one -- so it answers Q49 and this question
with one piece of work. C is worth doing eventually for its own sake (real
hardware support is a goal in the plan), but as a way to *test a driver* it is a
lot of prerequisite for the purpose.

**If this is never answered:** nothing breaks and nothing worsens. The emulator
keeps displaying through virtio-gpu and real machines through the firmware
framebuffer. The practical consequence is that roadmap §3.1 never closes -- both
of its two remaining items are these two GPU questions -- and lane A works on
other things, of which there is no shortage. No time pressure, no drift.

**Why this is filed rather than decided:** it is the same class of call as Q49 --
spend money, hardware or setup effort, versus narrowing what we claim to support.
That is the operator's decision, not mine. Related: Q49 (modern AMD),
`design-decisions.md` §217 (why the old-AMD driver was written first).


---

## B-Q3 — [B] Passwords set before today cannot be checked, because the three programs that store them each scrambled them differently. They now fail closed — an administrator must reset them. Accept that, or let those users in one last time? — Status: OPEN

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


## B-Q4 — [B] The system has two separate lists of who its users are, and nothing keeps them in step. A user created in one is invisible to the other. Which one is the real one? — Status: OPEN

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


## B-Q5 — [B] 70 compiled programs are stored in git, and they go out of date without git noticing. Keep storing them, or rebuild them on demand? — Status: OPEN

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

---

## Q51 — [A] The thing that blocked 3D graphics for a month has quietly been available all along; it was one wrong command-line flag. Start the 3D work now, or leave it parked? — Status: OPEN

**In short:** Our OS can draw a desktop but cannot do 3D — no games, no
hardware-accelerated video, no 3D modelling. A month ago you decided to build
the plumbing for 3D but *not* the graphics engine itself, because we had no way
to test 3D: the emulator we develop against appeared not to offer it. That turns
out to have been wrong. It offers it fine; we were starting the emulator with a
flag that says "no screen at all", and no screen means no 3D. There is another
flag that means "no window, but 3D still works". I measured it today and our own
code now sees the 3D capability being offered. **The reason you deferred the
work has evaporated.** The question is whether to pick it up now — it is a large
job, roughly the size of the biggest thing this project has attempted — or leave
it parked and spend the time elsewhere.

**One honest caveat up front:** I proved the *emulator* offers 3D to us. I did
not prove any 3D actually renders — nothing in our code yet asks for it. And I
found that simply switching the flag today would **break the 2D display we
already have** (details below). So this is "the blocker is gone", not "it works
now".

**Terms, glossed on first use.** *virgl* — the emulator's 3D feature; it takes
3D commands from inside our OS and runs them on the real graphics card of this
PC. *Mesa* — the large open-source library that turns an application's 3D calls
into those commands; it is the piece we would have to port, and it is external
code, not ours. *Headless* — running with no visible window, which is how all
our automated tests run. *Scanout* — the act of pointing the display at a chunk
of memory so it appears on screen.

### What changed

`design-decisions.md` §59 (your decision, 2026-07-14) says the 3D work waits
"until a virgl test environment exists", and cites the evidence: the emulator
offered our driver the feature mask `0x30000002`, which contains no 3D bit.

That number is real, but it came from starting the emulator with `-display none`
("no screen"). Running the **same kernel image** and changing only the graphics
device and the screen setting:

| Emulator flags | Feature mask offered to our driver | 3D bit? |
|---|---|---|
| `-device virtio-gpu-pci -display none` *(what we use today)* | `0x30000002` | no |
| `-device virtio-gpu-gl-pci -display egl-headless` | `0x30000013` | **yes** |

`egl-headless` is the "no window, but 3D still works" mode. It has been in our
emulator the whole time.

**What is *not* proven:** nothing negotiated the 3D feature, created a 3D
context, or drew a triangle. And with the 3D device our current 2D setup
regresses — the display fails to attach (`SET_SCANOUT: resp=0x1203`), and the
graphics subsystem goes from 2 devices to 1 with no primary display. Cause: the
emulator's 3D device routes everything through its 3D engine, which rejects the
simple 2D memory buffer we hand it. That is a real, fixable piece of work, and
it is **lane A's** (kernel-side), separate from the Mesa port.

### The options

**A. Do the kernel-side half now; leave Mesa parked.** Fix the 2D-under-3D
regression and make our driver negotiate and report the 3D feature honestly, so
the harness *can* run the 3D device. Stop before Mesa.
*What changes:* nothing a user sees. Internally, the automated test can run
against a 3D-capable emulator without losing its display, and the "no test
environment" excuse is gone for good.

**B. Do A, then start the Mesa port.** The full path to actual 3D.
*What changes:* eventually, 3D applications run — the first time anything in
this OS renders a 3D frame. Cost: Mesa is a large external port, and by this
project's own measured rates that is days-to-weeks of active work, the largest
single item attempted so far. It is also **lane C's zone** (`gui/**`), so it
competes with the compositor and desktop work, not with kernel work.

**C. Leave it parked; just correct the record.** Update §59 to say the
prerequisite exists but we are choosing not to spend the time.
*What changes:* nothing, except that the entry stops asserting something false.
The risk this guards against is real and has bitten before (§305's audit
finding: a decision resting on a missing prerequisite went 25 days unchecked
after the prerequisite arrived, and ~1,100 commits landed on a dead premise).

**My recommendation: A now, and treat B as a separate decision you make later.**
A is small, is unambiguously lane A's, removes a regression that would otherwise
ambush whoever *does* flip the harness, and makes the honest-reporting code in
`virtgpu_uapi.rs` testable against a device that actually offers the bit. It
commits you to nothing about Mesa. I am not recommending B without your call —
it is the largest item on the board and it belongs to a lane with its own queue.

### If this is never answered

**Nothing breaks and nothing gets worse.** 3D stays unsupported, which is what
the code already reports honestly (`3D_FEATURES = 0`, no capsets — those stay
correct and are *not* being changed on my own initiative). The only ongoing cost
is that §59 keeps stating a reason for the deferral that is no longer true, and
the next person to read it will re-derive today's measurement from scratch. The
§59 entry has been annotated with the finding, so that cost is already capped
whatever you decide.

**Where it bites:** `kernel/src/drm/virtgpu_uapi.rs:503` (`param_value`, the
honest zeros), `kernel/src/virtio/gpu.rs:284` (`negotiate(0)` — requests no
features), `scripts/boot-test.sh:2262,2276` (`-device virtio-gpu-pci`,
`-display none`).

---


## Q52 — [A] Should a benchmark-grading gate keep failing on noise it cannot tell from a real fault? — Status: OPEN

**In short:** we have a tool that watches for the machine getting busy while
benchmarks run, so a slow benchmark isn't mistaken for a real slowdown. To check
that the tool works, we deliberately load the machine over a known stretch of
benchmarks and see whether it points at the right ones. Part of that check fails
the tool if it points *anywhere else at all* — zero tolerance. We have now
measured that the machine hiccups on its own in 2 runs out of 5 with nothing
running, and the tool correctly reports those hiccups, which the check then
counts against it. So the check can fail a perfectly good tool on a coin flip.
The question is whether to loosen it, and if so how.

**Glossary, in case this is read cold:** *canary* — a tiny fixed piece of work
timed repeatedly during the run, so a change in its cost reveals the machine got
busy. *False positive* — the tool pointing at a benchmark nothing was actually
done to. *Tolerance 0* — any single one of those fails the check outright.

#### The evidence

RESULT P24 (in `known-issues.md`): five benchmark runs, nothing else running on
the machine. Two of the five contained a real one-off 12.6% jump in the canary's
cost, and each caused about 3 benchmarks to be reported. That is the same
signature that failed the check in RESULT P23. The two cases are not
distinguishable from the output.

Worth stating plainly: the tool is not malfunctioning in any of this. Both
experiments found it reports exactly what its input says. The disagreement is
about what the *grading rule* should count as a mistake.

#### Options

| | *What changes:* |
|---|---|
| **A. Leave it at zero** | Nothing. The check keeps returning `FAILED (misplaced)` roughly 2 runs in 5 no matter how good the tool is, and each result needs a human to read the note and discount it. |
| **B. Allow a small isolated cluster** (~3 benchmarks, the measured size of one hiccup) | Runs like P23 read `PASSED` instead of `FAILED`. A tool that genuinely pointed at a *few* wrong benchmarks would now slip through. |
| **C. Fail only on a shifted *band*, not on isolated spikes** | Distinguishes "reported a stray hiccup somewhere" from "found the window but in the wrong place" — the fault the check actually exists to catch. More code, and the band/spike rule needs its own justification. |
| **D. Measure the noise properly first** (20+ idle runs, then set the rule from the distribution) | Nothing changes for now; the decision gets made on 20 samples instead of 5. Costs about 45 minutes of machine time, none of it mine to spend badly. |

**My recommendation: D, then C.** Five runs is enough to prove the assumption is
wrong but too few to calibrate anything — picking a number from 5 samples is how
the original zero got there. C is the option that matches what the check is for,
but it should be built on a real distribution rather than on two observations.

#### Why I did not just decide this

The rule I would be relaxing is the one that returned `FAILED` on my own
experiment three hours ago. Changing a gate immediately after it fails you is
indistinguishable from moving the goalposts, whatever the reasoning, so this is
worth a second opinion even though the technical argument seems clear to me.

I have already made the part that carries no such hazard: the grader now prints
the measured idle-host rate next to the count, so the number is readable in
context. **No verdict changed.**

#### If this is never answered

Safe, and it does not get worse. The check keeps running and keeps being
slightly too harsh in a documented, annotated way; nothing is blocked and no
recorded benchmark number is affected (`design-decisions.md` S229 means nothing
is auto-corrected on this tool's say-so). The cost is only that future runs of
this experiment need the note read alongside the verdict.

## Q53 — [A] `CLAUDE.md` says to investigate any benchmark that slows by more than 10%. We have now measured that 71% of our benchmarks can slow by more than that from nothing at all. Change the rule? — Status: OPEN

**In short:** `CLAUDE.md` tells every lane that if a change makes a benchmark
more than 10% slower, stop and investigate before merging. I have now measured
what our benchmarks do when *nothing is changed at all* — same source code,
rebuilt so the machine code sits at slightly different addresses in memory. 61
of our 86 benchmarks move by more than 10% on that alone. So for most of the
suite the "10%" rule is asking people to investigate an effect smaller than the
one the measurement makes up by itself. The number in `CLAUDE.md` is the
operator's to change, not mine, which is why this is here.

**Glossary, in case this is read cold:** *relink* — rebuilding the program so
its pieces land at different memory addresses; happens on essentially every
commit, and changes no behaviour. *QEMU* — the emulator our benchmarks run
inside; it happens to run a tight loop noticeably slower when the loop
straddles a particular memory boundary, so an address change alone can change a
timing. *Band* — the measured range a benchmark moves across several such
rebuilds; our stand-in for "how much of this number is meaningless".

#### The evidence

Six builds of the identical commit `b36a244bb`, differing only in a deliberate
padding that shifts everything in memory, benchmarked back-to-back on an idle
machine (4128 s). Full table in `known-issues.md`.

| benchmarks that move by ≥ | count of 86 | share |
|---|---|---|
| 5% | 74 | 86% |
| **10%** — the `CLAUDE.md` threshold | **61** | **71%** |
| 20% | 51 | 59% |
| 50% | 26 | 30% |
| 100% | 10 | 12% |

Median 26.0%. Worst 182.0%. And the benchmarks `CLAUDE.md` itself singles out
as performance-critical are among the *worst*, not the best: `pick_next` 132%,
`page_alloc_free` 92%, `page_fault` 85%, `ipc_channel` 83%, `syscall_dispatch`
42%. That is not bad luck — they are tight hot loops, which is precisely what
the emulator's penalty acts on.

Two things this is **not**: it is not ordinary run-to-run noise (the
measurement medians repeats within each build and then corrects each build
against the others for machine drift before comparing), and it is not a
complaint that the tooling ignores this. The harness already withdraws a
movement that falls inside a measured band. The problem is only the sentence in
`CLAUDE.md`, which a reader will apply to numbers no band has been measured
for — and today that is every benchmark except these 86, on this one host, in
this one build profile.

#### Options

| | *What changes:* |
|---|---|
| **A. Leave the 10% as written** | Nothing. The document keeps stating a threshold that, for 71% of benchmarks, is below what a no-op rebuild produces. Anyone who trusts it investigates phantoms, or — worse — reads a real 15% regression as "probably layout" without checking. |
| **B. Restate it as "larger than that benchmark's measured band, or 10% if no band has been measured"** | The rule matches what the harness already does. A benchmark with a 132% band needs a >132% movement to be worth investigating; an unswept one keeps today's 10%. Honest, but it makes the guarantee for hot paths very weak — and says so out loud. |
| **C. Raise the flat number** (e.g. to 30%, just above the median band) | One number, still simple. Wrong in both directions at once: too loose for the 25 quiet benchmarks, still far too tight for the 26 that move ≥50%. |
| **D. Stop grading these benchmarks on QEMU** — treat emulator timings as smoke tests only, and get real regression numbers from hardware | The 10% rule becomes meaningful again, because the effect it is fighting is largely a TCG artefact. Costs a way to run the kernel on real hardware, which we do not currently have as routine infrastructure. |
| **E. B now, D as the real fix** | Document says something true today; the hardware path is booked as the thing that makes the threshold trustworthy rather than merely honest. |

**My recommendation: E.** B alone is the only option that makes the document
true, and it costs one sentence. But B is an admission, not a solution — it
concedes that we cannot detect a 100% regression in `pick_next`, which is not
an acceptable end state for a scheduler hot path. D is the only option that
actually restores the guarantee. I have not started D because "run the
benchmark suite on real hardware" is a piece of infrastructure with its own
prerequisites, and because whether it is worth building depends on how much you
want these numbers to mean.

Related but separate: **Q46** asks whether the non-bench boot test should also
build release. This sweep is release-profile only; the `debug` profile has no
band measured at all, and the majority of historical benchmark records are
`opt-level = 0`.

**Update 2026-08-19 — option D may be much cheaper than stated above.** D was
written as "get real numbers off hardware", i.e. infrastructure we do not have.
But the penalty producing these bands is a *TCG* artefact — QEMU's software
interpreter bounds a translation block at the guest page, so a hot loop whose
backward branch straddles one costs ~1.7× per iteration. QEMU's hardware-
accelerated mode (WHPX) has no translation blocks at all, and this host supports
it. If the bands collapse under WHPX, the 10% threshold becomes meaningful
without any new hardware. That has a cost of its own — WHPX silently disables
SMEP/SMAP/UMIP here — and is asked as **Q54**, which also commits to measuring
the WHPX bands so this stops being speculation.

#### Why I did not just decide this

The threshold lives in `CLAUDE.md`, which lane A may edit only on an explicit
instruction from the operator. Beyond the rule, this one deserves asking on its
merits: it is a gate that would be *loosened* on the strength of my own
measurement, and loosening a gate using evidence you produced yourself is worth
a second pair of eyes regardless of how good the evidence looks.

#### If this is never answered

Safe today, and it degrades slowly rather than suddenly. Nothing is blocked;
the harness's own behaviour is already correct and is not affected by the
wording. The cost is that the written rule and the measured reality disagree,
so every future reader has to rediscover this on their own — and the failure
mode is the quiet one, where a genuine regression in a hot path is waved
through as "within the band" by someone who never checked whether a band was
ever measured for it.

#### Update 2026-08-19 (later) — option D has been measured, and the answer is "mostly yes, but you cannot yet trust the remainder"

> **CORRECTED 2026-08-19 (later still).** The TCG column of the table below
> was wrong, and wrong in the direction that flattered the conclusion. It has
> been replaced with numbers `bench-history.py --layout-bands` now reproduces
> on demand, and the sixth WHPX arm is included. See "How the first version of
> this table came to be wrong" at the end — it is the same defect this whole
> question is about, committed by the analysis rather than by the harness.

**In short:** the guess that most of this noise is an artifact of the emulator
(rather than something real about our code) has now been tested rather than
argued, by building the *same* kernel at six deliberately different code
placements and running the suite on the faster emulator mode. The noise does
fall a long way — the typical benchmark's placement-only spread goes from
**26.0% to 5.4%**, and the number of benchmarks too noisy to grade nearly
halves. But it does not collapse to nothing; fourteen benchmarks get *worse*;
and the instrument that would tell us whether the remainder is placement or
merely a busy PC is broken on that emulator mode. So this changes the
recommendation without settling the question.

**The measurement.** Six arms, identical source, `.text` padded by
0/1024/1536/2048/2560/3072 bytes, all under `-accel whpx`, compared against
the six-arm TCG sweep at `b36a244bb` — the same six arms Q53's head table is
drawn from, so the two agree by construction:

| | TCG | WHPX |
|---|---|---|
| median placement band | **26.0%** | **5.4%** |
| mean | 40.6% | 12.8% |
| worst benchmark | 182.0% (`heap_raw_alloc_free_4096`) | 94.1% (`firewall_check`) |
| benchmarks whose band exceeds the 10% regression threshold | **61 of 86** | **35 of 86** |
| benchmarks whose band exceeds 25% | 45 | 13 |

WHPX is narrower on **72 of 86** benchmarks, and the page-straddle explanation
survives the correction: the benchmarks that were worst under TCG are still
the ones that collapse, and they are tight hot loops, which is what the
translation-block penalty acts on.

```
benchmark                            TCG      WHPX
heap_raw_alloc_free_4096          182.0%     30.8%
vfs_throughput_16k_read           148.3%      3.9%
vfs_stat_breakdown_resolve        142.8%     18.5%
pick_next                         132.0%     13.3%
ipc_eventfd                       126.1%     15.4%
http_gzip_8KiB                    101.0%      3.0%
```

`pick_next` is the one to look at: it is the scheduler hot path `CLAUDE.md`
names as performance-critical, it was the headline embarrassment of the
original Q53 evidence at 132%, and under WHPX it is 13.3%. That is the single
strongest argument on this page for moving off TCG.

**Why this does not settle it.** Three reasons, in increasing order of how
much they should bother you:

1. **35 of 86 benchmarks still have a placement band wider than the 10%
   threshold this question is about.** A 4.8x improvement in the median is not
   the same as the problem going away. On those 35, a movement under ~10-30%
   still cannot be read as a regression.

2. **Fourteen benchmarks are *wider* under WHPX**, and the identity of three
   of them is a warning rather than a curiosity:

   | benchmark | TCG | WHPX |
   |---|---|---|
   | `firewall_check` | 52.3% | **94.1%** |
   | `isr_latency` | 25.4% | **84.4%** |
   | `ipc_channel_roundtrip_64k` | 4.2% | **50.6%** |

   `isr_latency` is the control benchmark from the original WHPX comparison —
   the one that barely changed (x0.862) and thereby proved the other speedups
   were real. A benchmark that does *not* speed up under hardware
   virtualisation has no translation blocks to lose, so removing them cannot
   make it *more* placement-sensitive. `ipc_channel_roundtrip_64k` is worse
   still: it goes from *quiet* (4.2%, comfortably inside the 10% rule) to 50.6%,
   a twelvefold widening with no placement mechanism that could explain it.
   Something other than placement is in these numbers, and the correction made
   this worse rather than better — the first version of this table reported
   eight wideners, not fourteen, and did not include this one.

3. **We cannot tell how much of the residue is placement and how much is a
   busy PC, because the detector for that was broken on WHPX when these arms
   ran.** Every one of them recorded `canary_verdict: broken` — see
   known-issues.md `B-A-THE-CONTAMINATION-CANARY-IS-A-TCG-ONLY-INSTRUMENT`.
   The canary's resolution floor was 100x tighter than its own derivation,
   which TCG clears by 26x and WHPX missed by 4.6x, so on WHPX it refused
   every sample. And the loss bites hardest exactly here: host interference
   matters *more* the faster the guest runs (a fixed interruption is a larger
   fraction of a 3.5x shorter run), so the accelerator with the narrowest true
   placement band is also the one whose measurements are most exposed to the
   noise we can no longer detect.

   **That bug is now fixed** (`bf565ae6a` + `26c139a81`), but the fix is not
   retroactive and must not be: a record whose canary refused every sample
   does not become trustworthy because the next kernel's canary works. These
   six arms stay `broken`, and the band above keeps its caveat until the sweep
   is re-run on a kernel that carries the fix. That re-run is now the single
   thing standing between this question and an answer.

   Worse, nothing in the tooling says so. `layout_arm_rejection()` screens arms
   on `host_load`, which is a **hand-supplied label** (`unknown` on every one
   of these runs), and never consults the **measured** `canary_verdict` sitting
   in the same record. So the band above was computed from six runs of
   explicitly unknown contamination. `describe_layout_band` now prints the
   warning that `--layout-bands` emits above the table
   (`WARNING: 6/6 arms could not measure host load`), so it is at least no
   longer silent — but it is reported, not screened, deliberately: see
   design-decisions.md §239 on why a broken canary is evidence that nobody
   knows rather than evidence the band is wrong.

**What this does to the options.** Option D ("run the benchmarks somewhere
faster than TCG") is still the best-supported option on the table — 4.8x on
the median, and 132% -> 13.3% on `pick_next`, is not a marginal gain — but it
is *gated on a re-run*, not merely improved by one. The canary fix has landed;
the arms above predate it. Without a working contamination check the WHPX band
cannot be distinguished from WHPX host noise, and adopting a threshold derived
from it would bake that confusion into the gate. The order is therefore:
re-run the sweep on a kernel carrying the canary fix, and only then set a
threshold from the result.

Note that the correction to this table **weakened** the case for D without
overturning it: the median improvement drops from a claimed 7.4x to a measured
4.8x, the worst-benchmark collapse from a claimed 26x to a measured 1.9x, and
the count of benchmarks that get *worse* rises from eight to fourteen. If the
recommendation had rested on the headline ratios it would have moved. It rests
on `pick_next` and on 61 -> 35 benchmarks crossing the threshold, and both
survive.

**A note on how this was obtained**, because it affects how much to trust it:
the six arms were recorded under six different commits (documentation commits
landed while the sweep ran), so `bench-history.py` filed them as six unrelated
one-arm experiments and would have reported no band at all. The fix — grouping
arms by a digest of the build-relevant tree, which is verified identical across
all six commits — has now landed, so the numbers above are no longer a
one-off analysis: `python scripts/bench-history.py --layout-bands --profile
release` prints them. See known-issues.md
`B-A-A-LAYOUT-SWEEP-IS-VOIDED-BY-ANY-COMMIT-MADE-WHILE-IT-RUNS`.

#### How the first version of this table came to be wrong

Worth reading, because the mistake is the one this whole question is about,
made by the analysis instead of by the harness.

The TCG column as first published claimed a median band of **36.5%**, a mean
of **104.9%**, and a worst benchmark of **2466%** (`hpet_read`). None of those
is reproducible from the six TCG arms, which give **26.0% / 40.6% / 182.0%** —
and the correct figures were sitting a few hundred lines up this same page, in
Q53's own head table, disagreeing with it.

`hpet_read`'s actual TCG placement band is **6.2%**. A four-hundred-fold error
does not come from an arithmetic slip; in this dataset a figure of that size
arises for exactly one reason, which is comparing a WHPX timing against a TCG
one (`hpet_read` is 446 ns under TCG and 13,680 ns under WHPX — an HPET read
costs a VM exit under hardware virtualisation and is emulated inline under
TCG). Adding the unpadded 16:15 WHPX probe of the same day to the TCG arm set
and taking a raw peak-to-peak reproduces all three of the old table's top rows
at the same rank and within a factor of 1.2 (2969% / 1714% / 1624% against the
claimed 2466% / 1389% / 1333%). The lower rows of the old table are close to
the clean TCG numbers, so it was a *mixture*, and I cannot fully reconstruct
how it was produced — only what contaminated it.

That probe is the subject of known-issues.md
`B-A-AN-ORDINARY-RUN-NEARLY-JOINED-A-LAYOUT-BAND-AS-A-SEVENTH-ARM`: it matches
a genuine sweep arm on every field the banding code compares — same host, same
profile, unloaded, `text_pad: 0` truthfully, `accel` absent truthfully, and the
identical kernel tree — and is separated *only* by not carrying the layout
sweep's tag. `bench-history.py` rejects it and always did. A hand-written
analysis that selected arms itself did not, and so made exactly the merge the
tag exists to prevent, in the exact direction the tag's comment warns about.

Three things follow, and the third is the one that changes practice:

- The near-miss documented in that entry is no longer only a near-miss. The
  merge happened, in an ad-hoc script, and its output reached this page — a
  document whose entire purpose is to inform an operator decision.
- It failed in the predicted direction. Inflating TCG's noise makes the case
  for moving off TCG look stronger than it is, which is the answer the analysis
  was hoping for. That is what "fails silently and plausibly" looks like.
- **Numbers in this file must come from a command the reader can re-run.**
  Every figure in the corrected table is printed by
  `bench-history.py --layout-bands`, which routes through
  `layout_arm_rejection` and therefore cannot pick up an untagged run. That is
  now the rule for evidence quoted here, and it is cheap: the reason the old
  table could be checked at all is that the raw records were still on disk.

#### Update 2026-08-19 (final) — the blocking measurement exists now, and it does *not* rescue the 10% rule

Point 3 above said the WHPX band could not be trusted because the contamination
canary refused every sample on both WHPX runs, and called re-running it "the
single thing standing between this question and an answer". That re-run has now
happened, cleanly: six arms, one source digest, one accelerator, and every arm
reporting `canary_verdict: clean` with 13 valid samples, 0 invalid, 0 below the
resolution floor.

Reproduce with:

```bash
python scripts/bench-history.py --layout-bands --profile release
```

| band | median | mean | worst | over 10% | over 25% |
|---|---|---|---|---|---|
| **TCG** (today's default) | 26.0% | 40.6% | 182.0% | **61 of 86 (71%)** | 45 of 86 |
| WHPX, *broken* canary (the caveated figures in point 3) | 5.4% | 12.8% | 94.1% | 35 of 86 | 13 of 86 |
| **WHPX, clean canary** (this run) | **6.4%** | **11.1%** | **86.0%** | **29 of 86 (34%)** | 10 of 86 |

**The headline answer: WHPX halves the problem and does not solve it.** The
share of benchmarks that can move more than 10% from a rebuild that changes
nothing falls from **71% to 34%** — a large improvement, and still a third of
the suite. The worst case is 86.0% (`vfs_stat_breakdown_ns`). So the hope
embedded in this question's framing — that Q53 might be answered by answering
**Q54** instead, i.e. that switching accelerator would make `CLAUDE.md`'s 10%
rule workable — **is now measured and false.** A 10% threshold is unusable on
either accelerator. Q53 has to be decided on its own merits.

**Point 3's caveat is discharged, and the caution it carried was right.**
`design-decisions.md` §239 decided to *report* the broken-canary band with a
warning rather than void it, on the grounds that a broken canary means "nobody
knows", not "the band is wrong". That call is now testable, and it holds: the
clean-canary band lands within a few points of the caveated one at every summary
statistic (median 5.4 → 6.4, mean 12.8 → 11.1, worst 94.1 → 86.0). Voiding those
figures would have discarded an accurate result.

**But one thing the aggregate hides, and it matters for how a band may be
used.** The two WHPX sweeps agree well in *distribution* and much less well
*per benchmark*: median absolute difference 2.8 points, but the 90th percentile
is 27.1 points, the maximum is 63.6, and the two sweeps **agree on the "is this
benchmark over 10%?" verdict for only 60 of 86**. Some of that is real — the
sweeps are of different source, and placement sensitivity is a property of a
particular layout of particular code — but the consequence stands either way:

> A layout band is trustworthy as a statement about *the suite* ("about a third
> of these benchmarks can move >10% for free"). It is much weaker as a licence
> to dismiss *one specific* benchmark's movement, because that benchmark's own
> band is not stable across sweeps.

That distinction was not visible before there were two comparable sweeps to put
side by side, and it argues against any option that silently subtracts a
per-benchmark band from a per-benchmark result.

**What this does to the options.** Nothing here selects an option for you, but
it removes one line of reasoning and sharpens another:

- *Removed:* "wait for Q54; switching accelerator may make this moot." Measured
  and false. 34% is not a workable false-positive rate for a rule that says
  investigate every one.
- *Sharpened:* any option built on per-benchmark bands needs to survive the
  60-of-86 reproducibility above. An option that raises the threshold uniformly,
  or that grades against the band as a distribution rather than per benchmark,
  is not exposed to it.

## Q54 — [A] The emulator we test in has a second mode that is 3.5× faster and roughly halves our benchmark-noise problem — but it silently switches off three of the CPU security features the kernel is built around. Switch, split, or stay? — Status: OPEN

**In short:** Every test we run happens inside QEMU, a program that pretends to
be a PC. It can do that two ways: by interpreting each machine instruction in
software (what we use today), or by handing them to the real CPU's built-in
virtualization hardware. The hardware way is 3.5× faster and roughly halves
the measurement problem in **Q53** — but does not remove it: the share of
benchmarks that move by more than 10% from a rebuild that changes nothing falls
from 71% to 34%, measured. (An earlier version of this paragraph said it "would
very likely fix" that problem. That was a prediction; it has now been run, and
it was too optimistic. See Q53's final update.) But on this machine the hardware
way also silently drops three CPU security features that the kernel relies on,
so 47 places where the kernel emits protective instructions would stop being
exercised. I have verified there is no setting that gives both. So: keep
testing the kernel we ship and keep the bad numbers, or get good numbers by
testing a kernel that is missing part of its armour.

**Glossary, in case this is read cold:** *TCG* — QEMU's software interpreter;
today's default; slow, but emulates whatever CPU we ask for. *WHPX* — Windows
Hypervisor Platform, QEMU using the real CPU's virtualization hardware; fast,
but only offers features the real CPU and the hypervisor both support. *SMEP /
SMAP / UMIP* — three CPU switches that make the kernel fault instead of
proceeding if it is tricked into running or reading user memory at the wrong
moment; defence against a large family of exploits. *`stac`/`clac`* — the two
instructions the kernel must wrap around a deliberate access to user memory to
temporarily lift SMAP; we insert them in 47 places at boot, but only if the CPU
says it has SMAP. *VM exit* — when the guest touches an emulated device, the
hardware has to stop and hand control to the host; costs ~13.5 µs.

#### The evidence

One byte-identical kernel (`kernel_sha 7a17cf6be2a10a26`), release profile, 86
benchmarks, back to back on this host. Full analysis in `design-decisions.md`
§237; the feature table in `known-issues.md` under
`ENV-WHPX-CPU-HOST-FIRMWARE-GP`.

| | TCG (today) | WHPX |
|---|---|---|
| Typical benchmark | — | **×3.53 faster** (82 of 86 faster) |
| Best case | — | `ipc_channel_roundtrip_64k` ×10.36 |
| Device-touching benchmarks | — | **~30× slower** — `hpet_read` 453 ns → 13534 ns |
| SMEP / SMAP / UMIP | yes | **no** |
| `stac`/`clac` sites patched in | 47 of 47 | **0 of 47** |
| Write-combining measurable | no | yes (51.74× vs 1.02×) |

Two things worth pulling out of that table.

**It is not a speed dial, it is a different shape.** Everything CPU-bound gets
much faster; everything that touches an emulated device gets ~30× *slower*,
because each access now traps to the hypervisor instead of being computed
inline. `hpet_read`, `net_arp_lookup` and `net_ns_arp_lookup` all collapse to
the same ~13.5 µs regardless of what they were before — the cost of one VM exit.
So switching does not just move the numbers, it changes which benchmarks are
even meaningful.

**The security-feature loss is not WHPX being unable to do it.** It is that our
command line asks for `-cpu qemu64,+smep,+smap,+umip` and WHPX silently ignores
the three `+` additions. The obvious repair — `-cpu host`, ask for the real
CPU's features — **does not boot at all**: the firmware takes a #GP in early
platform init, because `-cpu host` advertises VMX and WHPX does not implement
the register the firmware then reads. That is recorded, with the crash log, as
`ENV-WHPX-CPU-HOST-FIRMWARE-GP`. I tried it rather than assuming, so the
either/or below is a measured fact about this host, not an inference:

| | TCG | WHPX + `qemu64` | WHPX + `host` |
|---|---|---|---|
| Boots | yes | yes | **no — firmware #GP** |
| SMEP/SMAP/UMIP | yes | **no** | n/a |

**And the sharpest point against switching, which is easy to miss:** a benchmark
measures the thing you run it on. Under WHPX we would be timing a kernel with
the 47 `stac`/`clac` pairs *absent* — a build we do not ship. The numbers would
be precise, reproducible, free of the layout artefact, and about a slightly
different kernel. That is not fatal (the effect is bounded: `alternatives.rs`
has exactly one `Feature` variant, `Smap`, so the whole difference is those two
instructions on user-memory paths, and the ×10 results are on paths that never
touch user memory) — but it should be said out loud before choosing, not
discovered later.

#### Options

| | *What changes:* |
|---|---|
| **A. Stay on TCG** | Nothing. Benchmarks stay 3.5× slower and keep the layout noise that makes Q53 unanswerable; the hardening stays exercised on every boot. |
| **B. Switch everything to WHPX** | Boot tests finish in about half the time, benchmarks become far less noisy — and SMEP/SMAP/UMIP stop being exercised anywhere, so a bug in the `stac`/`clac` insertion would no longer be caught by any test we run. |
| **C. Split by purpose: boot/correctness on TCG, benchmarks on WHPX** | The boot gate still runs the shipped kernel with all its protections; the benchmark suite gets numbers that mean something. Costs a second QEMU run for a full release gate, and restarts the benchmark history — the `accel` key added in §237 correctly refuses to compare across accelerators, so today's baselines do not carry over. The device-bound benchmarks would need to stay on TCG or have their targets rewritten. |
| **D. Run both, for everything** | Maximum coverage: a regression that only appears under one accelerator is caught. Roughly doubles the wall-clock cost of the gate, which is already ~6 min for a debug boot. |
| **E. Sweep intermediate `-cpu` models first, then decide** | Possibly finds a model that boots under WHPX *and* carries SMEP/SMAP — which would collapse this whole question. Cheap to test (the answer is in the first 80 lines of serial output, one boot per model), but the augmentation-dropping above suggests WHPX ignores `+feature` for any base model, so the likely outcome is "no such model exists". |

#### Update 2026-08-19, from the first sweep arm: switching also disables the contamination check

Found while running the probe promised below, and it is the strongest argument
against switching that has turned up so far — strong enough that it belongs in
the question rather than in a footnote.

Every benchmark run takes a small reference measurement a dozen times during the
suite, so that a run polluted by other work on this PC is labelled rather than
believed. **Under WHPX it fails on all 13 samples and the suite reports
`canary_verdict: "broken"`** — reproduced on both WHPX runs. So a WHPX run today
has *no* contamination detection.

Two things make this worse than a missing feature.

- **The loss is largest exactly where the risk is.** Contamination matters more
  the faster the guest runs: under TCG the emulator's own overhead dominates, so
  a fixed interruption from Windows is a small slice of a long run; under WHPX
  the same interruption lands on a run 3.5× shorter and is 3.5× the slice. WHPX
  needs the canary more and currently supplies it less.
- **It would trade a noise source we can measure for one we cannot.** The layout
  artefact this switch is meant to cure is *deterministic* — that is what makes
  a band measurable, and the harness already withdraws a movement inside one.
  Host contamination is random and unbounded. Swapping the first for the second
  is not obviously a gain even though the numbers would look tidier.

**This does not by itself settle the question, because it looks fixable.** The
cause is not WHPX: the canary refuses any measurement finer than 4 cycles per
access, and a store costs 0.85 cycles on the real CPU against 5.16 under
emulation. Worse, that threshold is wrong on its own terms — its stated
derivation gives 4 *hundredths* of a cycle at the precision the code has used
since 2026-08-14, so it is 100× too strict, and TCG itself was clearing it by
only 29%. Full arithmetic in `known-issues.md`
(`B-A-THE-CONTAMINATION-CANARY-IS-A-TCG-ONLY-INSTRUMENT`). I will fix that
regardless of how this question is answered, since it is a defect under TCG too.

What that leaves genuinely open, and what the operator should weigh: fixing the
floor restores the *measurement*, not automatically the *verdict*. The 25%
tolerance would then apply to a 0.85-cycle quantity whose real variation has
never been observed on this accelerator, and a second check in the same family
(the scattered-access scale test) asserts something that is simply **false on
real hardware** — that a per-access cost cannot depend on how many pages the
loop walks, which ignores caches. So option C's true cost is not "one more QEMU
run"; it is *re-establishing the benchmark suite's self-validation on a platform
where its assumptions do not hold*. That is real work, it is not hard to
justify, but it should be chosen knowingly rather than discovered afterwards.

The same bill arrives with **Q53 option D** (real hardware), and larger — every
assumption that breaks under WHPX breaks on bare metal too, plus the accelerator
would be recorded as absent there (`TD-A-A-BARE-METAL-RUN-RECORDS-ITS-ACCELERATOR-AS-ABSENT`).

**My recommendation: E first — it is one boot per candidate and it might make the
question disappear — then C.** C is right if E finds nothing, because the two
accelerators' weaknesses are disjoint *by purpose*: losing SMAP coverage matters
for the correctness gate, where a missing `stac` shows up as a fault; the layout
artefact matters only for benchmarks, where it is currently making 71% of the
suite unreadable. Splitting puts each weakness where it does not bite. The
infrastructure for it already exists — §237 made the accelerator part of the
comparison key precisely so two series can coexist without contaminating each
other. I am not recommending B: giving up the only place SMEP/SMAP/UMIP are ever
exercised, to make a benchmark faster, trades a security property for a
convenience.

**This partly unblocks Q53.** Q53's only option that actually restores the 10%
regression threshold was **D — get real numbers off real hardware**, which I
flagged as needing infrastructure we do not have. WHPX is a much cheaper
candidate for the same job: the page-straddle penalty behind the layout noise is
a *TCG translation-block artefact*, and WHPX has no translation blocks. Whether
that is true is measurable in about an hour — see below.

#### What I can do without an answer, and will unless told otherwise

Run the layout sweep under WHPX. Same six padded builds as the Q53 sweep, same
host, `QEMU_EXTRA="-accel whpx"`; `layout-sweep.py` already propagates the
setting to every arm, and the `accel` key means the WHPX arms automatically form
their own band group rather than mixing with the TCG ones. That converts
"WHPX would probably fix the noise" into a measured band, and it changes no
default and no committed configuration — it is a probe, and it will be recorded
as one. If the bands stay wide under WHPX, option C loses most of its value and
this question gets much easier to answer.

#### Update 2026-08-19 (later) — this table was re-checked against the tool, and the headline holds; one row does not

`design-decisions.md` §240 adopted a rule after a table on **Q53** was found to
be wrong: evidence quoted here has to be reproducible by a command the reader
can re-run, because the Q53 figures had been produced by a one-off analysis that
picked its own runs. This table has the same provenance — it was built by hand
in §237, and there is no `bench-history.py` mode that regenerates it — so it was
re-derived from `bench/history.jsonl` before being left in front of you.

**The arithmetic is exact.** Comparing the two records the table names
(`kernel_sha 7a17cf6be2a10a26`, release, TCG at `text_pad 0` vs the WHPX probe)
reproduces ×3.53, 82 of 86 faster, best ×10.36, and `hpet_read` 453 → 13534 ns,
to the digit. Nothing here is fabricated.

**But the TCG side is a single layout arm, and Q53 exists because a single
layout arm is not a reliable number.** The comparison used the `text_pad 0` arm
of a six-arm sweep. Re-running it against each of the other five:

| | as published (pad 0) | across all six TCG arms |
|---|---|---|
| Typical speedup | ×3.53 | **×3.53 – ×3.97** |
| Benchmarks faster | 82 of 86 | 82–83 of 86 |
| Best case | `ipc_channel_roundtrip_64k` ×10.36 | **×10.34 – ×16.71, and a different benchmark each time** |
| `hpet_read` | 453 → 13534 ns | 441–453 → 13534 ns |

Three conclusions, and they differ by row:

- **The headline is safe, and understates itself.** ×3.53 is the *lowest* of the
  six; the true typical figure is ×3.5–4.0. Unlike the Q53 case, the arm that
  happened to be picked was the one least favourable to the argument being made,
  so this error — such as it is — runs against the conclusion rather than toward
  it.
- **The device-bound row is solid.** `hpet_read` sits in 441–453 ns across every
  arm and collapses to one VM-exit cost regardless, so "~30× slower, and it is a
  different *shape* rather than a speed dial" is unaffected. That was always the
  most important claim in the table and it is the best-supported one.
- **The "best case" row should not be read as naming a benchmark.** Which
  benchmark wins depends on which TCG arm you compare against —
  `ipc_channel_roundtrip_64k` at pads 0 and 1536, `io_ring_nop` at 2048 and
  3072, `http_gzip_8KiB` at 1024, `vfs_throughput_16k_read` at 2560 — and the
  value ranges to ×16.71. The honest statement is "the best cases are ×10–17",
  with no name attached. Individual per-benchmark speedups swing by up to 2.9×
  with arm choice (`heap_raw_alloc_free_4096` is ×1.72–×5.00).

**What this does to the options: nothing.** Every option above turns on the
typical speedup, the shape change, and the security-feature loss, and all three
survive. The row that moved was the one no option depends on. This is recorded
because a table that was checked and found sound is worth as much as one that
was checked and found wrong — and because the check was cheap and the last one
like it changed an answer.

#### Why I did not just decide this

Switching the accelerator would stop exercising a security feature on every
test we run. That is a user-visible security posture change, not a tooling
preference, and it is exactly the kind of thing that should not be traded for a
3.5× speedup by the person who wants the speedup. It also touches
`scripts/boot-test.sh`'s defaults, which every lane depends on.

#### If this is never answered

Safe, and stable — nothing degrades over time. TCG remains the default and the
kernel keeps being tested with its protections on, which is the conservative
side to be stuck on. The standing cost is that the benchmark suite stays 3.5×
slower and stays noisier than it needs to be. It does **not** block **Q53**
any more: that question was measured on 2026-08-19 and a 10% rule is unusable on
*either* accelerator (71% of benchmarks exceed it under TCG, 34% under WHPX), so
Q53 must be decided on its own and answering this one would not settle it. An
earlier version of this paragraph said Q53 "stays stuck too" and named this
question as its cheapest fix; that was a prediction and it did not survive the
measurement.


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

## Resolved — lane A

*(none yet)*

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

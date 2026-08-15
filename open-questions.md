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

Earlier deferred operator decisions (Q1–Q38) have been
resolved — see the "Recently resolved" list below and `design-decisions.md` for
full rationale. New decisions are appended just above the `---` separator that
precedes the "Recently resolved" list, numbered with your lane's prefix
(`A-Q<n>`, `B-Q<n>`, `C-Q<n>`) — the unprefixed `Q<n>` numbers are pre-split and
are not to be extended.

## Q40 — Should osh reproduce bash's *null array element*, which looks like an upstream defect? — Status: **RESOLVED 2026-08-15 → design-decisions.md §309**

**Answer: B — do not reproduce it; waive it in the corpus.** osh keeps `Str`
array elements and the array reads normally. The write-up stays in
`known-issues.md` →
`TD-OILS-A-DECLARATION-WITH-NOTHING-TO-DO-BINDS-A-NULL-THROUGH-THE-REFERENCE`
so the call is reversible if a real script is ever found that depends on it.

Decided by the operator; Claude recommended this option.

**The part that outlives the bug:** byte-fidelity with bash now has an
**"unless it is a defect" clause**. A measured behaviour may be waived when it
is unreachable except through a construct built to reach it, inconsistent with
bash's own observable model, and expensive to reproduce in a way that degrades
osh's value model — and every future waiver must be argued against those three
tests in `known-issues.md` rather than taken silently. See §309; this does not
loosen §305's frozen fidelity scope.

## Q41 — Should bash be cross-compiled instead of osh reimplemented? — Status: **RESOLVED 2026-08-14 → design-decisions.md §305**

**Answer: the hybrid, with osh's fidelity scope frozen.** osh remains the shell;
the cross-compiled GNU bash 5.2 (which boots and runs on SlateOS) ships beside
it as the escape hatch and future on-device differential oracle; and
byte-for-byte bash parity stops being an open-ended goal — **§305 carries the
binding stopping criterion, and every `TD-OILS-*` entry and new corpus case is
now gated by it.**

Decided by the operator, who also raised the question. Claude recommended this
option.

**Do not re-open this as a feasibility question.** Feasibility was settled by
measurement on 2026-08-12 (`scripts/bash-spike/`): bash cross-compiles with
`zig cc`, links against our own `toolchain/sysroot/lib/libc.a` with zero
undefined symbols and no shims, and runs — `kernel/src/proc/spawn.rs::self_test_bash_on_slateos_libc`
proves it on every boot. The full spike results, the day-by-day history of how
§72's expiry condition fired on 2026-07-22 and went unchecked for 25 days, and
the general rule that failure established, are all in **§305**.

## Q42 — Two crates are not rustfmt-clean, which makes `cargo fmt` a trap. Do a one-shot repo-wide reformat, or keep formatting only touched files? — Status: **RESOLVED 2026-08-15 → design-decisions.md §310**

**Answer: A — one-shot repo-wide reformat, with a `.git-blame-ignore-revs` file
committed alongside.** Decided by the operator; Claude recommended this option.

Three constraints on how it lands, from §310:

- `cargo fmt --all` **does not run here** (`os error 206`, the Windows
  command-line length limit, hit by the number of workspace members). Iterate
  crates one at a time.
- It is **two commits in two lanes**: `posix/` is Lane B's, `kernel/` is Lane
  A's (16 911 hunks). A single cross-lane reformat commit is exactly the
  clobbering the lane split exists to prevent. Both hashes go into
  `.git-blame-ignore-revs`. Lane A's half is requested in
  `requests/b-a-rustfmt-repo-wide-reformat.md`.
- Each reformat commit must contain **nothing but** formatting, so
  `--ignore-rev` is safe to apply wholesale.

## Q43 — The compiler-KASAN kernel is ~20× slower to boot, so the B-KNULLJUMP soak it was built for would take over a week. How should the hunt be made viable? — Status: **ANSWERED 2026-08-15 — Lane A to record in `design-decisions.md`**

> **Operator's answer (2026-08-15, given to Lane B): "e, then a if necessary."**
>
> That is Claude's revised recommendation: run **E** — soak the ordinary,
> uninstrumented kernel carrying the `B-NO-CLD-ON-INTERRUPT-ENTRY` fix and see
> whether B-KNULLJUMP stops — and fall back to **A** (build the instrumented
> kernel `--release` and soak that) only if E fails to settle it. Note what "if
> necessary" covers: E catching a B-KNULLJUMP is a *falsification* of the DF
> hypothesis and is exactly the case that hands A a well-motivated job. E coming
> back clean is suggestive, not proof — it cannot distinguish "fixed" from "got
> lucky" — so a clean E is a reason to stop, not a reason to escalate.
>
> **A still carries the caveat that made it operator-worthy:** no release kernel
> has ever been booted in this project, so the cheap gate stands — build
> `--release`, run `kasan-check-preshadow.py`, attempt **one** boot (~30 min)
> before committing to a soak. And treat a clean release soak as weaker evidence
> than a debug one, since optimization perturbs the instruction timing a rare
> race depends on.
>
> **This is Lane A's item.** The answer was delivered in a Lane B session, so it
> is recorded here rather than in `design-decisions.md` §200–299 — Lane A owns
> that range. See `requests/b-a-operator-answered-q43.md`. Lane A: record it,
> then delete this section.

The original analysis follows, unchanged, because it is what E and A have to be
executed against.


**Raised by Claude** (2026-08-12), on measuring the profile the operator
approved in §107.

**The finding.** The instrumented kernel from `scripts/kasan-build.sh` now
survives everything that used to kill it (§118's pre-shadow triple fault,
§119's user-pointer `#GP`) and boots normally — but it is far slower than the
"roughly doubled" figure §119 was written with. A plain debug boot reaches
`BOOT_OK` in **~283–318 s** (`soak-20260723-190300`, 100/100 iterations). The
instrumented one spent **975 s to reach 17 %** of that log, with 48 of the 66
ring-3 spawn tests and 178 MB of the 217 MB of test ELF still ahead; line-rate
and remaining-ELF extrapolations agree on **5500–8500 s per boot**, i.e. ~20×.

**Why it matters.** For the one validating boot this is only tedious. For the
hunt it is disqualifying. B-KNULLJUMP fires at ~1 boot in 120, so an
even-odds soak needs ~80 boots: **over a week of wall-clock** here, versus ~7 h
for the plain-build soak that has already been run several times. The
escalation to compiler instrumentation was justified precisely because the
passive tooling could not localize the bug (§107, and the Path-A exhaustion note
in `known-issues.md`) — so the profile working but being unaffordable to soak
leaves the bug exactly where it was.

**Options.**
- **A — build the instrumented kernel optimized (`kasan-build.sh --release`).**
  *Pro:* most of the 20× is almost certainly `-O0` codegen and the outlined call
  per access, not the shadow check itself; `-O2` would fold most redundant
  checks and could plausibly land within 2–3× of a plain debug boot, which makes
  the soak affordable at close to its usual cadence. The flag already exists and
  the build gate (`kasan-check-preshadow.py`) validates the release binary the
  same way, so the invariants stay mechanically proven.
  *Con:* two real risks. (1) **No release kernel has ever been booted in this
  project** — every boot test to date is the debug profile, so `--release` may
  surface latent UB that debug codegen hides, and debugging *that* in the middle
  of a corruption hunt is the worst possible time. (2) Optimization perturbs
  instruction timing and layout, which is exactly what a rare race depends on;
  the 1-in-120 rate is a *debug-build* measurement and may not carry over. A
  release soak that comes back clean would be much weaker evidence than a debug
  one.
- **B — soak the debug instrumented build anyway, accepting >1 week.** *Pro:*
  changes nothing about the population being sampled, so a catch is
  unambiguous and a clean run is comparable to the existing 100-iter baseline.
  *Con:* a week of the machine doing one thing, and it is only even odds at the
  end of it.
- **C — cut the instrumented boot's workload rather than its cost per
  instruction** (e.g. a cmdline that runs only the Path-Z spawn tests
  B-KNULLJUMP has been seen near, skipping the rest). *Pro:* potentially a large
  constant-factor win with debug codegen retained, so timing is perturbed much
  less than by (A). *Con:* the bug has never been localized to a specific test —
  that is the whole problem — so trimming the workload may trim away the
  trigger, and a clean soak would then prove nothing. Also needs new
  test-selection plumbing that does not exist.
- **D — leave the instrumented profile as a validated tool, do not soak it now,
  and spend the time on roadmap work instead.** *Pro:* the profile is finished
  and committed either way, ready for the moment a *reproducible* trigger is
  found; B-KNULLJUMP does not block other work and never has. *Con:* the bug
  stays open with the escalation built but unused.

**Claude's recommendation.** **A, gated on a cheap experiment**: build
`--release`, run the checker, and attempt one boot. That costs ~30 min and
answers both unknowns at once (does a release kernel boot at all; what is the
real per-boot cost). If it boots green and fast, soak with it and treat a clean
result as suggestive rather than exonerating — the §119 update already records
that timing caveat. If it does not boot, fall back to **D** rather than **B**:
a week of machine time for even odds is a bad trade while roadmap work is
unblocked. I have not started (A) because "boot an optimized kernel for the
first time" is a change of profile for the whole project, which reads as
operator-decision-worthy rather than mine to take unilaterally.

**Where it bites:** `scripts/kasan-build.sh`, `scripts/wedge-soak.sh` (which now
catches `[kasan] CRITICAL:` and documents the raised `SOAK_TIMEOUT`),
`design-decisions.md` §107/§119, and `known-issues.md` → `B-KNULLJUMP-SIGNAL`.

### UPDATE 2026-08-12 — a concrete suspect appeared, which adds a much cheaper option E

Since this question was written, a specific candidate root cause for
B-KNULLJUMP was found and fixed: `known-issues.md` →
`B-NO-CLD-ON-INTERRUPT-ENTRY`. No IDT stub cleared the direction flag, so the
kernel ran with whatever DF ring 3 left set, and every `rep`-string
operation — including compiler-emitted `memset`/`memcpy` — walked backwards,
writing *before* each intended destination.

The precondition is confirmed rather than assumed: the exact `libc.so.6` staged
into `rootfs.ext4` contains `std; rep movsb; cld` in `__memmove_erms`'s
overlapping-backward path, so ring 3 demonstrably holds DF = 1 across an
interruptible window whose width is proportional to the copy length.

**This changes the economics of the question.** The original framing was "we
have no lead, so we must sample blindly, and instrumentation is the only way to
localize a catch." There is now a *falsifiable hypothesis*, and testing a
hypothesis is far cheaper than searching without one:

- **E — soak the ordinary (uninstrumented) fixed kernel and see whether
  B-KNULLJUMP stops.** *Pro:* no instrumentation cost at all, so it runs at the
  ~283–318 s per-boot rate the existing 100-iteration baseline was measured
  at — directly comparable, same codegen, same timing, no release-build gamble.
  ~250 boots (≈2× the 1-in-120 base rate, ~21 h unattended) coming back clean
  would be strong evidence the fix landed; a single catch immediately falsifies
  the hypothesis and hands the instrumented profile a much better-motivated job.
  Either outcome is informative, which is not true of options A–D.
  *Con:* a clean result is statistical, not proof of causation — it cannot
  distinguish "fixed" from "got lucky", and the confidence is only as good as
  the 1-in-120 estimate.

**Claude's revised recommendation: E first, then re-ask this question.** The
instrumented profile stays exactly where option D leaves it — built, gated and
ready — but there is no longer a reason to spend a week of machine time on a
blind search *before* spending a night on a targeted one. If E comes back
clean, this question may be moot; if it catches, A–D are all still available and
better aimed. I have started E, since it commits nothing and reverses freely;
A (booting an optimized kernel for the first time) remains yours to call.

#### UPDATE 2026-08-13 — the first attempt at E would have been worthless; E is now genuinely running

The E soak described above was launched once and **aborted before it could
produce a misleading answer.** B-KNULLJUMP has only ever been observed inside
the tcc-driven Path-Z rungs, and `/bin/tcc` had silently fallen out of
`rootfs.ext4`, so all 26 of those rungs (Parts 35–60) were no-opping on every
boot while `boot-test.sh` still reported PASSED. The sampled population did not
contain the trigger: 250 clean boots would have looked like strong evidence and
meant nothing. See `known-issues.md` → `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT`,
now fixed so a rung that skips says so and the harness reprints it.

The image was rebuilt with tinycc present and a full boot verified that all 26
tcc rungs run and pass, ending in `Path-Z prerequisites: complete — 0 rungs
skipped`. **The real E soak is now running** against that frozen, verified image
(`MAX_ITERS=250 HUNT=0 SOAK_TIMEOUT=600 STALL_SECS=150`, `--no-stage`), watching
for the `RIP=0x0` / `error=0x10` signature.

Two caveats for whoever reads the result:

- It samples a **SMAP-enabled** kernel (`qemu64,+smep,+smap,+umip`), which the
  1-in-120 base rate was not measured on. Treat that rate as order-of-magnitude.
- Per-boot wall time is now ~355 s, not the ~283–318 s the ~21 h figure above
  was built on, so budget ~24 h for the full 250.


## Q44 — libc reports "all Linux capabilities held" to every process because nothing maps our `(ResourceType, Rights)` handles onto `CAP_*` bits. Which mapping do you want? — Status: **RESOLVED 2026-08-15 → design-decisions.md §312**

**Answer: A — conservative projection.** Decided by the operator; Claude
recommended this option.

Each Linux `CAP_*` bit is derived from a specific `(ResourceType, Rights)`
predicate over the capabilities the process actually holds, and reports **not
held** whenever no rule matches — the default is *deny*, so an unmapped `CAP_*`
is false, never true. `CAP_SYS_RAWIO` ⇐ a `PortIo` handle with `READ|WRITE`;
`CAP_KILL` ⇐ a `Process` handle with `SIGNAL`; `CAP_SYS_PTRACE` ⇐ `Process`
with `DEBUG`; `CAP_SYS_NICE` ⇐ `Thread` with `IO_REALTIME`.

`CAP_SYS_ADMIN` is the deliberate exception: an **explicit hand-maintained
union** of what its 20 gate sites actually need, one commented member each. It
is Linux's junk drawer and has no natural preimage in a per-object model, so a
derived rule would be either permanently false (breaking 20 sites) or broad
enough to re-grant everything, which is the bug being fixed.

Rejected: **B** (`ResourceType::PosixCapability`) is ambient authority wearing a
capability costume — process-wide authority tied to no object, spelled as a
handle — and it was rejected even though it is the option that would have made
`CAP_SYS_ADMIN` easy. **C** (stay optimistic, document `capget()` as "the
ceiling, not the grant") leaves the silent-wrong-answer trap open, which is why
the entry was logged. **D** (`capget()` fails) trades one silent wrong answer
for loud breakage in every port that calls it informationally.

**How it lands, in order** (from §312):

1. **The enumerating query syscall first.** `SYS_CAP_QUERY` (400) returns only a
   *count*; nothing can be seeded from it. That handler is **Lane A's tree** —
   filed as `requests/b-a-cap-enumerating-query-syscall.md`.
2. libc seeds its three words from that query rather than from `CAPS_DEFAULT`.
3. **The libc gates stay advisory until the fixtures hold real capabilities.**
   `services/ctest-jobctl`, `self_test_cctty` and `self_test_cpgroup` all spawn
   with `capabilities: &[]` and pass today only because every bit is set. The
   flip from advisory to enforcing is boot-test-visible and lands with QEMU
   free.

## Q45 — Text clipped by `max_width` is cut mid-glyph with no ellipsis. Should `RenderCommand::Text` carry an overflow policy? — Status: **ANSWERED 2026-08-15 — Lane C to record in `design-decisions.md`**

### ANSWER 2026-08-15 — option **A**: `RenderCommand::Text` gets an `overflow` field

The operator answered in a Lane B session. Verbatim: **"q45: a."** That is
Claude's own recommendation, so nothing about the plan changes — but it is a
decision now, not a proposal.

**What was chosen.** Add an `overflow: TextOverflow` field (`Clip` | `Ellipsis`)
to `RenderCommand::Text`, and let the **compositor** draw the ellipsis, because
it is the only party that knows exactly where the glyphs ran out. One
measurement instead of two, and the policy is visible at every call site.

**The cost that was accepted, so it is on the record.** Rust has no default for
a struct-variant field, so this edits **every construction of `Text` in the
tree** — several hundred sites across `gui/**` and `apps/**`. The question said
so, and the answer is still A. Two consequences follow:

- **Land it as its own commit with nothing else in flight.** A several-hundred-
  site mechanical diff conflicts with anything else touching rendering, and this
  is the same trap `§310` (the rustfmt reformat) was about — a wide mechanical
  change entangled with real work cannot be separated afterwards.
- **`gui/**` and `apps/**` are Lane C's tree.** Lane B is recording the answer,
  not implementing it. Filed as
  `requests/b-c-operator-answered-q45-and-c-q1.md`.

**Recording:** `design-decisions.md` under Lane C's §400–499 range. Lane B has
deliberately not written it there — inventing a section number from another
lane's range is how two lanes collide after a merge. Also close
`known-issues.md` → `TD-GUI-CLIPPED-TEXT-IS-NOT-MARKED` when it lands.

**Raised by Claude** (2026-08-14), falling out of the pass that closed
`known-issues.md` → `TD-GUI-TEXT-COMMAND-DOES-NOT-WRAP`. Logged there as
`TD-GUI-CLIPPED-TEXT-IS-NOT-MARKED`.

**The situation.** `max_width` on a `Text` command clips: the compositor walks
glyphs and stops before the first one that would cross the limit, drawing no
mark. A label that does not fit therefore ends mid-word and ends *plausibly* —
"Gateway 192.168.1.1 res" looks like a complete string to a reader who cannot
see the field it was cut from. A caller that wants the cut marked must call
`text::elide` first, which measures the string a second time to answer a
question the compositor is about to answer again while drawing it. Well over a
hundred single-line labels across `gui/**` and `apps/**` pass `max_width`
without eliding; most are safe only because their values are short and
app-authored, and the ones that bite carry user or network data — file names,
SSIDs, error strings, host names.

**Why this needs you rather than me.** Every option is a different tax on the
same several-hundred call sites, and the cheapest-to-write one is the one that
does not actually stop the mistake recurring. That is a taste call about the
API's shape, and it lands across three lanes' in-flight work.

- **A — add an `overflow: TextOverflow` field to `RenderCommand::Text`**
  (`Clip` | `Ellipsis`), and let the compositor draw the mark, since it is the
  only party that knows exactly where the glyphs ran out. *Pro:* one
  calculation, right by construction, and the policy is visible at every call
  site. *Con:* Rust has no default for a struct-variant field, so this edits
  every construction of `Text` in the tree — several hundred, mechanical but
  wide, and it conflicts with anything else in flight that touches rendering.
- **B — a second variant** (`TextClipped` / `TextElided`). *Pro:* no churn at
  existing call sites. *Con:* splits the match arms in every renderer and every
  test that walks a command list, forever, to encode one boolean.
- **C — a constructor/builder** (`RenderCommand::text(..).elided()`), leaving
  the struct literal as it is. *Pro:* no churn; opt in where it matters.
  *Con:* the literal form stays available and stays wrong, so it prevents
  nothing — it is documentation with a return type.
- **D — sweep `text::elide` across the data-bearing call sites** and leave the
  command alone. *Pro:* smallest diff, fixes the sites that actually bite.
  *Con:* keeps the double measurement, and the next label someone adds has the
  bug again.

**Claude's recommendation.** **A**, done as its own commit with nothing else in
flight, because it is the only option that makes the mistake unrepresentable —
and the churn is mechanical, which is the cheap kind. **D** is the sensible
answer if you would rather not spend a wide diff on this now; in that case it
should be scoped to labels carrying user or network data rather than swept
blindly.

**Where it bites:** `gui/toolkit/src/render.rs` (`RenderCommand::Text`),
`gui/compositor/src/main.rs` (`draw_text`, the `break` at the limit),
`gui/toolkit/src/text.rs` (`elide` / `elide_start`), and every `max_width:
Some(..)` in `gui/**` and `apps/**`.

## A-Q1 — Install the GNAT/SPARK and LLVM toolchains? — Status: **A ANSWERED 2026-08-15 (Lane A to record in `design-decisions.md`) — B STILL OPEN**

**In short:** this entry asked about **two unrelated compiler installs** in one
question, and only the first was answered.

| | What it is | Status |
|---|---|---|
| **A** | **Ada/SPARK** — a second programming language whose toolchain can mathematically *prove* driver code has no buffer overflows and no bad state transitions. `design.txt` wants it for safety-critical drivers. | ✅ **Answered: install it, with the prover (`gnatprove`).** |
| **B** | **clang + lld** — an alternative C compiler and linker. Installing them is what would let us switch on **CFI** (Control-Flow Integrity: a compiler feature that stops an attacker redirecting a function call to code of their choosing). | ❓ **Still open — nothing has been said about it.** |

**What B is actually asking you for:** one word, install or don't. It is a
small, standard, uncontroversial install. The only reason it isn't obvious is
that the payoff is currently near zero — we use C only for *ported* code, and
the one piece of C we compile today (`scripts/create-ext4-rootfs.sh`) is built
with gcc, so turning CFI on would change Lane B's build for a benefit that only
arrives when the big C ports (ext4, Mesa) land. It also pulls in LTO
(whole-program optimization at link time), which slows every build it touches.
Saying "not yet" costs nothing; nothing is blocked either way.

*(Two smaller follow-ups inside A are also unsettled — which GNAT distribution
to install, and which cut-down Ada runtime to use — but those are Lane A's calls
to make, not yours. They are spelled out at the end of the answer below.)*

### ANSWER 2026-08-15 — option **A**, including `gnatprove`. Option **B** was not answered.

The operator answered in a Lane B session. Verbatim:

> q44: a, including gratprove.

The `q44` label is a typo for this question — it arrived in the same message as
the real Q44 answer, immediately after it, and Q44 has no option "including
gnatprove". Read as **A-Q1: A**.

**What was decided: install GNAT/SPARK *with the prover*.** The "including
gnatprove" is the load-bearing half, and it closes the correction in the
`UPDATE 2026-08-15` block below — the prover is part of the definition of done,
not an optional extra, because Ada-without-SPARK is just another systems
language and we already have a memory-safe one.

**Consequences that follow directly from "including gnatprove":**

- **The install route cannot be MSYS2.** `mingw-w64-x86_64-gcc-ada` ships
  `gnat` and `gprbuild` and no `gnatprove`, and MSYS2 has no such package.
  Taking the easy route would buy the entire cost of the feature and none of its
  justification. The route must be **Alire** (`alr toolchain --select`, then the
  `gnatprove` crate) or **AdaCore's own download**.
- **The prover stack is a further install:** Why3 + Alt-Ergo, optionally Z3 and
  CVC5. `gnatprove` without a solver proves nothing.
- **GPL is not a problem here.** The toolchain is a tool we *run*, not something
  we link; it does not reach our output.

**Two sub-decisions this answer does not settle.** Both are Lane A's to make or
to escalate:

- **Which GNAT distribution.** The FSF-via-Alire route now looks clearly
  preferable to GNAT Pro precisely because it carries `gnatprove`, but nobody
  has said so as a decision.
- **The restricted runtime: ZFP vs light.** A freestanding kernel cannot use the
  full Ada runtime, which wants an OS underneath it. This is configuration work
  with real content, not part of the install.

**Option B (clang + lld, for LLVM CFI) was not answered and is still open.**
This question says out loud that A and B "are separable, so please answer them
independently", and only A came back. B remains as written: cheap and
uncontroversial to install, but its payoff is currently small (C is used only
for ports, and the C in `scripts/create-ext4-rootfs.sh` is built with gcc and is
Lane B's tree), and CFI wants LTO, which changes build times and link behaviour
everywhere it reaches.

**Recording:** the decision belongs in `design-decisions.md` under Lane A's
§200–299 range. Lane B has deliberately **not** written it there — inventing a
§2xx number from another lane is how two lanes end up with the same section
number after a merge. See `requests/b-a-operator-answered-a-q1.md`.


*(Renumbered from `Q40` on 2026-08-15 by Lane B. It collided with the pre-split `Q40` above, and the operator's one-word answer "q40: b" was genuinely ambiguous between the two. Lane-prefixed per `roadmap.md`'s convention, as `B-Q1`/`C-Q1` already are.)*

**Question.** The Lane A roadmap backlog has five items. Three are either
"Later" (NTFS/Btrfs/ZFS/F2FS), lane-C-driven (TCP/IP stack), or a very large
port that wants its own go-ahead (AMDGPU / i915-xe). The remaining two are the
natural next increments — and **both are blocked on a compiler that is not
installed on this machine**, not on any design or code question:

| Roadmap item | Needs | Probe result |
|---|---|---|
| `[A]` Ada/SPARK FFI bridge for kernel-space drivers | `gnat`, `gprbuild`, `gnatprove` | all missing |
| `[A]` Enable LLVM CFI as default for C/C++ compilation | `clang`, `lld` | both missing |

(Probed 2026-08-14 via `command -v`. The `*ada*` directories under
`userspace/` are coincidental CLI names — `ada-cli`, `cutadapt-cli` — not an
Ada toolchain. A stray `ld.lld` exists under an Embarcadero install but is not
a usable LLVM toolchain.)

Per the global tooling rule I install missing tools myself when that is safe
and self-contained, and pause to ask when it is heavyweight, system-wide, or
has licensing implications. Both of these are in the "ask" category, and they
are separable, so please answer them independently.

**Option A — install GNAT/SPARK (unblocks the Ada FFI bridge).**

- *Pro:* it is the only thing standing between the roadmap and a design-spec
  feature. `design.txt` (lines 84-95) is specific about what it buys: SPARK
  *proves* driver logic has no buffer overflows, no integer overflows and no
  invalid state transitions, and the layering it names — Rust kernel → FFI →
  SPARK driver in kernel space → IOMMU-constrained DMA — is a real
  defence-in-depth story rather than a nice-to-have.
- *Con:* it is the heaviest of the three asks. Beyond the ~1-2 GB toolchain
  there is a **licensing fork** worth your call (FSF GNAT via MSYS2/MinGW vs
  AdaCore's GNAT Pro; GNAT Community is discontinued), and a real technical
  cost: a freestanding kernel needs a **restricted Ada runtime** (ZFP or
  light), because the full runtime wants an OS underneath it. That is
  configuration work, not just an install.
- *Con:* `gnatprove` is what makes this "SPARK" rather than "Ada". If we
  install a toolchain without the prover, we get FFI plumbing and none of the
  proof — i.e. the entire justification.

**Option B — install clang + lld (unblocks LLVM CFI).**

- *Pro:* much lighter and less contentious; clang/lld are a standard,
  well-understood install with no licensing question.
- *Con:* the payoff is presently small. The rule is that C is used *only* when
  porting existing C code (ext4, Mesa, Chromium), so "CFI as default for C/C++"
  currently governs a very small amount of compilation — the C in
  `scripts/create-ext4-rootfs.sh` is built with **gcc**, and that script is
  Lane B's. Enabling CFI as a default would therefore reach across a lane
  boundary to change Lane B's build for a benefit that only materialises once
  the big C ports land.
- *Con:* CFI wants LTO, which changes build times and link behaviour for
  everything it touches.

**Option C — install neither now; defer both.**

- *Pro:* neither item is on the critical path today, and there is unblocked
  Lane A work (see below), so the cost of waiting is zero.
- *Con:* it leaves the Lane A roadmap backlog effectively down to
  "Later"/large-port items, so the *next* time Lane A needs a task the same
  question comes back.

**Claude's recommendation:** **B now, A when you want the driver-safety story
started, and tell me which GNAT.** clang/lld is cheap, uncontroversial, and I
can install it without further input if you say go. GNAT/SPARK I would rather
not choose for you: the FSF-vs-AdaCore call and the restricted-runtime
decision are both yours, and installing the wrong one wastes the larger
download. I would also want to sequence A *after* the IOMMU work it pairs with
in `design.txt`, so it is not urgent.

**What I am doing in the meantime — this question blocks nothing.** I have
moved to `TD-KSHELL-LINE-EDITOR-IS-UTF8`, which is unblocked, in-lane, pure
Rust, and a genuine correctness item rather than polish: `CLAUDE.md`'s rule 7
says OS-boundary data is bytes and must never be forced through UTF-8, and the
kshell line editor currently holds the command line as a `String`, so a
filename containing a non-UTF-8 byte can be listed but neither typed nor
tab-completed. Please do **not** treat this question as a reason to expect me
idle.

**Where it bites:** `scripts/kasan-build.sh`-style toolchain probing generally;
for A, a new `drivers/spark/` tree plus a `build.rs` FFI shim and
`toolchain/`-side runtime configuration; for B, `.cargo/config.toml` C flags
and `scripts/create-ext4-rootfs.sh` (Lane B — would need a `requests/` entry).
Roadmap lines ~297-298 (`roadmap.md` Lane A backlog) and `design.txt` lines
84-95.

---

### UPDATE 2026-08-15 — the operator asked why we would not install `gnatprove`; the con was overstated

**The operator's question:** *"why wouldn't we install gnatprove?"* Fair — the
bullet as written implies a reason to skip it, and there isn't one. Corrected:

**`gnatprove` is freely available, including on this machine's platform.** SPARK
is open source, AdaCore publishes binaries for Windows x86-64 / Linux x86-64 /
macOS x86-64, there is an Alire crate (`alr with gnatprove`), and the
alire-project `GNAT-FSF-builds` repository ships FSF builds. No licence blocks
it and no money is involved.

**So the real content of that bullet is a route warning, not a veto.** The
obvious way to get Ada on this box — MSYS2's `mingw-w64-x86_64-gcc-ada` — gives
`gnat` and `gprbuild` and **no** `gnatprove`; MSYS2 has no such package. Stop
there and we would have paid the whole cost of the feature (a second language
and toolchain in the build, an FFI bridge, a restricted ZFP/light runtime for a
freestanding kernel) and collected none of the benefit, because
**Ada-without-SPARK is just another systems language and we already have a
memory-safe one.** `design.txt` lines 84-95 justify this on the *proof*
specifically — no prover, no justification.

**Therefore, if A is chosen, the prover is part of the definition of done**, and
the install route must be one that can actually deliver it: Alire
(`alr toolchain --select`, then the `gnatprove` crate) or AdaCore's own
download, not MSYS2 alone. Two things worth knowing before committing:
`gnatprove` needs its prover stack (Why3 + Alt-Ergo, optionally Z3/CVC5)
installed too, and the toolchain is GPL — it is a tool we *run*, not something
we link, so it does not reach our output.

**What is still open, and still yours.** Nothing above chooses for you. The
remaining calls are (i) go/no-go on **B** (clang + lld — cheap, uncontroversial,
I can do it on a one-word yes), (ii) go/no-go on **A**, and (iii) if A, which
GNAT distribution — the FSF-via-Alire route now looks clearly preferable to
GNAT Pro given that it carries `gnatprove`, but the restricted-runtime
(ZFP vs light) configuration is still real work and still a decision. Claude's
recommendation is unchanged: **B now, A when you want the driver-safety story
started, sequenced after the IOMMU work it pairs with in `design.txt`.**

*Sources for the availability claim: AdaCore SPARK User's Guide §3
"Installation of GNATprove"; alire.ada.dev crate `gnatprove`; alire-project
`GNAT-FSF-builds`.*

## B-Q1 — The zoneinfo reader is done and nothing on disk to read: which tzdata do we ship, from where, and how is it updated? — Status: **RESOLVED 2026-08-15 → design-decisions.md §311**

**Answer: A1 + B1 + C1.** Decided by the operator; Claude recommended this
combination.

- **A1 — full tzdata**, backward links included (`US/Eastern`,
  `Asia/Calcutta`). ~450 KiB, ~1 800 files in the base image.
- **B1 — vendor the prebuilt TZif binaries** from IANA, checked in and
  version-pinned. No `zic` port, no home-grown TZif generator: the failure mode
  of getting that subtly wrong is a wrong clock nobody notices for months.
- **C1 — ship it as a `pkg/` package** and update it there. C3's dedicated fast
  channel is the escalation if C1 proves too slow, not the starting point.

**Residual risk accepted:** a user who never runs `pkg update` drifts into a
stale tzdata, silently.

**Execution note:** the reader, the libc paths and osh are Lane B; **`pkg/` is
Lane C's tree**, so the packaging half goes via `requests/b-c-tzdata-package.md`.
The two tests asserting the current UTC fallback
(`test_zoneinfo_names_resolve_to_utc_until_tzdata_is_shipped`,
`printf_time_falls_back_to_utc_for_a_zone_it_cannot_resolve`) **must start
failing the day the data lands** — that is the signal it worked, not a
regression.

## C-Q1 — Should normalization consult font coverage? The last 339 sweep disagreements are all this one question — Status: **ANSWERED 2026-08-15 — Lane C to record in `design-decisions.md`**

### ANSWER 2026-08-15 — option **C**: keep `nfc` pure, let `fit_to_face` decompose what it cannot draw

The operator answered in a Lane B session. Verbatim: **"c-q1: c."** That is
Lane C's own recommendation, so the plan is unchanged — but it is now a decision
and needs recording.

**What was chosen.** The layering principle in `norm.rs`'s module doc **stands**:
`nfc` answers a question about *text* and never looks at a font; `fit_to_face`
answers a question about the *font*. The narrow fallback goes in the second
stage — when `fit_to_face` meets a composed character the face cannot draw, and
the *pieces* are drawable, it decomposes. `split_undrawable` already exists and
already has this shape, which is why C was the recommendation.

Result for the 339 residual sweep disagreements: they should move to `agree`
without `nfc` ever taking a face as input.

**The cost that was accepted.** Two mechanisms where HarfBuzz has one — we agree
with HarfBuzz on output while diverging on structure. The concrete risk the
question named is **mark reordering after a late decomposition**, which HarfBuzz
gets right by construction and we would not. Treat that as the thing to test
rather than assume: the sweep is the instrument, and any ordering case it
surfaces is this decision's bill, not a surprise.

**Not chosen, and why it matters later:** **B** (adopt HarfBuzz's font-aware
recomposition) was refused because it makes normalization a function of
`(text, face)` — no longer hoistable, no longer cacheable per string, not
reasonable about without a font in hand. If a future case cannot be fixed inside
`fit_to_face`, that is the argument that has to be beaten, not re-litigated from
scratch.

**Recording:** `design-decisions.md` under Lane C's §400–499 range — Lane C's own
question, Lane C's own range, so Lane B has recorded the answer here only. Filed
as `requests/b-c-operator-answered-q45-and-c-q1.md`.

**Raised by Claude (Lane C)** (2026-08-15), on finishing `known-issues.md` →
`TD-FONT-HAS-A-HANGUL-SHAPER-NOTHING-CALLS`. That fix took the HarfBuzz
differential sweep from 892 disagreements to 339, and the 339 that remain are
**one question asked 339 times**, not a scatter of unrelated bugs: `\u1e09`
(ḉ — c with cedilla and acute) 255 cases, `\u212b` (Å angstrom sign) 57,
`été` 10, and a short tail.

**Question.** `norm.rs` is layered on a deliberate principle, written into its
module doc: **`nfc` answers a question about *text* and knows nothing about
fonts; `fit_to_face` answers a question about the *font* and does not
renormalize.** Unicode composition is a property of the string, so it is
decided before any face is consulted. HarfBuzz does the opposite — it
decomposes to NFD, then *recomposes only where the face has a glyph*, so the
same string normalizes differently in two fonts. Which layering do we want?

Concretely, for `\u1e09` in a face that has `c`, the cedilla and the acute but
no precomposed ḉ: we emit one missing-glyph box, HarfBuzz emits three glyphs
that stack into the right-looking character.

**Options.**

- **A — keep the current layering** (`nfc` is pure Unicode; font coverage is
  `fit_to_face`'s problem). *Pro:* each stage has one job and one input, which
  is why the module reads clearly and why the Hangul work above was four small
  edits rather than a rewrite; normalization is reproducible without a font in
  hand, so it can be tested, cached, and shared across faces. *Con:* we draw a
  box where HarfBuzz draws correct text, on real strings, in real fonts. The
  user does not care which stage was principled.
- **B — adopt HarfBuzz's font-aware recomposition.** *Pro:* matches the
  reference implementation and every other shaper, closes the sweep's residue
  to near zero, and is strictly better output on faces with partial coverage.
  *Con:* normalization becomes a function of `(text, face)`, so it can no
  longer be hoisted, cached per string, or reasoned about without a font;
  `norm.rs`'s layering claim becomes false and its doc has to be rewritten to
  say the opposite.
- **C — a narrow fallback: keep `nfc` pure, but let `fit_to_face` decompose a
  composed character it cannot draw when the parts *are* drawable.** *Pro:*
  gets B's user-visible outcome for exactly the failing case while keeping
  A's layering, because the decomposition happens in the stage that already
  owns "what can this face draw" — `split_undrawable` is already that
  function and already takes this shape. *Con:* it is two mechanisms where
  HarfBuzz has one, so we would agree with HarfBuzz on output while diverging
  on structure, and the sweep may surface ordering cases (mark reordering
  after a late decomposition) that HarfBuzz gets right by construction.

**My recommendation: C**, and I have *not* implemented it. It is the only
option that does not require choosing between correct pixels and a coherent
module boundary, and `split_undrawable` already exists as the hook. But it is
a user-visible rendering-policy change on a design principle that was written
down deliberately, so it is yours rather than mine. Meanwhile the behaviour is
A (unchanged) and the residue is documented, not silently tolerated.

**Where it bites:** `gui/font/src/norm.rs` (`nfc`, `normalize`,
`decompose_once`, `split_undrawable`, `fit_to_face`, and the module doc's
layering paragraph), `gui/font/src/scaled.rs::shape` (call order), and
`gui/font/tools/harfbuzz_sweep.py` (the 339 would move to `agree`). Reference:
HarfBuzz `src/hb-ot-shape-normalize.cc`,
`HB_OT_SHAPE_NORMALIZATION_MODE_COMPOSED_DIACRITICS_NO_SHORT_CIRCUIT`.

---

Recently resolved (see `design-decisions.md` for the full rationale):

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

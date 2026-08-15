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

Format for each entry:

- **Question** — the decision to be made.
- **Options** — each with its pros and cons.
- **Claude's recommendation** — if there is a defensible default (and what
  Claude is doing in the meantime).
- **Where it bites** — files/symbols affected, so the resolution can be applied.
- **Status** — `OPEN` until the operator decides.

Earlier deferred operator decisions (Q1–Q38) have been
resolved — see the "Recently resolved" list below and `design-decisions.md` for
full rationale. New decisions should be appended as `## Q40 …` just above the
`---` separator that precedes the "Recently resolved" list.

## Q39 — Where should the *shipping default* point once a fastpy utility clears both bars? — Status: OPEN

**Question.** §108 settled that fastpy utilities stay additive for now and that
the trajectory is for them to become real implementations, per command, once
each has (a) a parity test suite and (b) measured performance that is faster,
equal, or not significantly slower than the canonical implementation — with the
user able to opt in. What it deliberately left open is which way the **default**
points in a stock install: does SlateOS prefer the fastpy implementation
wherever one has cleared both bars, or prefer the canonical one and make fastpy
the thing you switch on?

This is not the same as "may fastpy ever replace a Rust coreutil" — that is
answered (yes, per command, gated). This is about what a user who never touches
the setting gets.

**Options.**

- **A — canonical by default, fastpy opt-in.** *Pro:* a stock install is always
  the most-exercised code path, so bug reports and performance numbers describe
  what almost everyone runs; switching is a deliberate act with a deliberate
  owner. *Con:* the fastpy implementations stay lightly exercised in the field
  precisely because they are off, which is the same "perpetual demo" trap §108
  was trying to leave — just one bar higher.
- **B — fastpy by default wherever it has cleared both bars, canonical opt-out.**
  *Pro:* the bars are the whole point; if a fastpy utility is genuinely at parity
  and not slower, defaulting to it is what makes the two bars mean something, and
  it gets real-world exercise. *Con:* the bars are measured, not proven — a
  parity suite is not the same as years of field use, and the failure mode is
  user-visible behaviour changing under people who never asked for it.
- **C — per-command, decided at promotion time.** Each utility's swap carries its
  own default, argued on its own evidence. *Pro:* no blanket rule to be wrong
  about; a `cat` and a package manager are not the same risk. *Con:* no coherent
  story for a user to hold ("which of my tools are which?"), and it defers the
  question forever by construction.

**Claude's recommendation.** None yet, on purpose. Answering this before a
single fastpy utility has cleared both bars would be answering it without
evidence — the honest input is *how close to parity the first one actually gets
and what it measures*, and that does not exist yet. Ask again then.

**Not blocking.** §108 part 1 (additive-only promotion into the test rootfs) is
the current behaviour and needs no answer here. This question only becomes live
at the first real swap.

**Where it bites.** `scripts/create-ext4-rootfs.sh` (the `PROMOTED` map, and
whatever assembles the production rootfs `/bin`), `kernel/src/proc/spawn.rs`
(`resolve_command` / `COMMAND_PATH`), and wherever the opt-in switch ends up
living — most likely the settings surface rather than a build flag, since §108
makes it a user choice.

## Q40 — Should osh reproduce bash's *null array element*, which looks like an upstream defect? — Status: OPEN

**Question.** osh (`userspace/oils`) is held to byte-fidelity with bash 5.2.37.
One measured bash behaviour is reachable only through a nameref and appears to
be a bug rather than a design: a valueless `declare` on a nameref that points at
an array *element* stores a **null pointer** in that element, and every later
reader of the array trips over it.

```text
$ n=(a b c); declare -n q='n[1]'; declare q
$ declare -p n                      # bash: declare -a n
$ echo "${#n[@]} [${!n[@]}] [${n[@]}]"   # bash: 0 [] []
$ echo "${n[1]-UNSET}"              # bash: UNSET
$ n[5]=z; declare -p n              # bash: declare -a n=([0]="a" [1]= [2]="c" [5]="z")
```

The array reads as empty while its elements are demonstrably still there; one
store past the end makes the readers able to walk it again. The path is
`bind_variable("q", NULL, ASS_FORCE)` → `bind_variable_internal` →
`assign_array_element("n[1]", NULL, …)` → `make_array_variable_value` returns
`NULL` → `array_insert(…, NULL)`. The same happens to an associative base, to a
scalar base, and — because the bind carries `ASS_FORCE` — to a **readonly** one,
with no `readonly variable` reported.

**Options.**

- **A — reproduce it.** osh's array element type becomes `Option<Str>`, and every
  reader (`declare -p` listing, `${!a[@]}`, `${#a[@]}`, `${a[@]}`, `${a[i]-D}`,
  iteration, `unset`, …) learns to stop at the first null. *Pro:* byte-fidelity
  is the project's stated bar, and this is the only place the bar is knowingly
  not met for a *measured* behaviour; whatever a real script does that lands here
  keeps working the same way. *Con:* a large, invasive change to the core value
  model — every array reader in `interp.rs` — bought entirely to preserve a state
  no bash-level operation can otherwise produce or explain. It would make the
  value model permanently harder to reason about for the sake of a defect, and if
  bash ever fixes it the change becomes dead weight that must be unwound.
- **B — do not reproduce it; waive it in the corpus.** osh keeps `Str` elements
  and the array reads normally (`declare -a n=([0]="a" [1]="b" [2]="c")`). *Pro:*
  no cost, and the divergence is confined to a construct that is hard to reach on
  purpose. *Con:* a knowing, documented deviation from measured bash — the first
  of its kind in oils, which so far has treated "the measurement wins" as
  absolute. Once one exists, "is this one worth reproducing?" becomes a judgement
  call on every future oddity rather than a settled rule.
- **C — reproduce only the *visible* half.** Make the element read as unset
  without a nullable element type (e.g. an out-of-band "poisoned index" marker
  the readers stop at). *Pro:* much smaller than A. *Con:* it is a second,
  parallel representation of emptiness that exists for one construct, and it has
  to be threaded through the same readers anyway — most of A's cost for a less
  honest model.

**Claude's recommendation.** **B** — do not reproduce it, waive it in the corpus,
and keep the full write-up in `known-issues.md` so the decision is reversible if
a real script is ever found that depends on it. The behaviour is not documented,
not otherwise reachable, and leaves the array in a state bash itself cannot
describe; paying a core-value-model refactor for it inverts the usual
cost/benefit. But this is the operator's call precisely because it sets the
precedent for *whether byte-fidelity has an "unless it's a bug" clause at all* —
and that is a policy, not a bug fix.

**Meanwhile.** osh does the sane thing (the array keeps its elements). Nothing is
blocked; the corpus case
`a-declaration-with-nothing-to-do-evaluates-the-subscript-the-reference-carries.sh`
covers the *evaluated-subscript* half, which osh does match, and stops short of
the store.

**Where it bites.** `userspace/oils/src/interp.rs` —
`Shell::declare_ref_bind_read` (the read with no store), `Shell::arrays` /
`Shell::assoc` (element type `Str`), and every array reader named under option A.
Full write-up:
`known-issues.md` → `TD-OILS-A-DECLARATION-WITH-NOTHING-TO-DO-BINDS-A-NULL-THROUGH-THE-REFERENCE`.


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

## Q42 — Two crates are not rustfmt-clean, which makes `cargo fmt` a trap. Do a one-shot repo-wide reformat, or keep formatting only touched files? — Status: OPEN

**Raised by Claude** (2026-08-12) after it cost a revert-and-redo cycle.

**The finding.** CLAUDE.md sets the convention as "`rustfmt` defaults. No manual
formatting overrides." Two crates comply and two do not (measured with
`cargo fmt -p <crate> -- --check`):

| Crate | Hunks needing reformat |
|---|---|
| `kernel` | 16 911 |
| `posix` | 389 (244 of 2 299 files, ~11%) |
| `net` | 0 |
| `fs` | 0 |

**Why this is more than cosmetic.** `cargo fmt` is package-scoped and has no file
filter, so in a drifted crate the ordinary act of formatting your own change
rewrites hundreds of files you never touched. Today, `cargo fmt -p posix` after a
~150-line edit produced a 1 403-insertion / 1 429-deletion diff across 173 files;
the two could not be separated afterwards, so the change had to be reverted and
re-applied by script. It also makes pre-existing oddities look like your own
damage — I lost time proving a strange `CapGuard` layout predated me. Every fmt
run in `kernel` or `posix` carries both costs.

**Options.**
- **A — one-shot repo-wide reformat, then it stays clean.** *Pro:* removes the
  trap permanently and makes the stated convention true; afterwards `cargo fmt`
  is safe and any drift is a real diff. Cheap to do (minutes of active work).
  *Con:* rewrites `git blame` for ~17 000 hunks of kernel code. Blame is the
  primary tool for "why is this line here?" in a codebase with no human
  reviewer and a 4 600-commit history — this is the one cost that cannot be
  undone. (`git blame --ignore-rev` + a `.git-blame-ignore-revs` file mitigates
  it for anyone who configures it, but not for GitHub's plain view or a casual
  `git log -S`.)
- **B — keep the current working rule: format only the files you edited**, via
  `rustfmt --edition 2024 <file>` rather than `cargo fmt -p`. *Pro:* zero
  history churn; already adopted and it works. *Con:* the convention stays
  aspirational; the trap stays armed for anyone who reaches for the obvious
  command; drift never shrinks except where files happen to be edited.
- **C — reformat `posix` only, leave `kernel` alone.** *Pro:* clears 11% drift in
  the crate under active daily work for ~250 files of blame churn, 1.5% of A's
  cost. *Con:* leaves the worst offender armed, and a half-applied convention is
  the state that caused this.

**Claude's recommendation.** **A**, with a `.git-blame-ignore-revs` file
committed alongside — the blame cost is real but one-time and partially
mitigable, whereas the trap is permanent and recurs on every edit. If the blame
history is considered untouchable, **C** is a reasonable middle. I have adopted
**B** in the meantime, so nothing is blocked either way.

**Note:** `cargo fmt --all` does not run in this workspace — it dies with
`The filename or extension is too long. (os error 206)` (Windows command-line
limit, hit by the number of workspace members). Any of A/C must iterate crates.

**Where it bites:** everywhere, but the recorded incident is
`known-issues.md` → `TD-REPO-IS-NOT-RUSTFMT-CLEAN-SO-RUNNING-CARGO-FMT-IS-A-TRAP`.


## Q43 — The compiler-KASAN kernel is ~20× slower to boot, so the B-KNULLJUMP soak it was built for would take over a week. How should the hunt be made viable? — Status: OPEN

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


## Q44 — libc reports "all Linux capabilities held" to every process because nothing maps our `(ResourceType, Rights)` handles onto `CAP_*` bits. Which mapping do you want? — Status: OPEN

**Raised by Claude** (2026-08-12), from the survey behind
`known-issues.md` → `TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S`.

**The situation.** `posix/src/sys_capability.rs` keeps Linux's three capability
words in libc's own memory and initialises them from `CAPS_DEFAULT` — *every*
bit set. Nothing ever asks the kernel what the process actually holds, so
`capget()` reports the full set to a process that was spawned with
`capabilities: &[]`, and every libc-side gate passes. It is safe today only by
accident: the kernel re-checks every privileged operation itself, so libc's
optimistic answer can never *grant* anything. The failure is silent, not loud —
a port that trusts `capget()` to decide what to attempt, or to drop privileges,
gets a confidently wrong answer with no error anywhere.

**Why this needs you rather than me.** The plumbing is easy; the *mapping* is a
policy decision. The two models are not the same shape and do not have an
obviously-correct correspondence:

- **Kernel:** 25 `ResourceType` variants (`Channel`, `Pipe`, `SharedMemory`,
  `EventFd`, `CompletionPort`, `Process`, `Thread`, `PortIo`, `DeviceIrq`,
  `File`, `Socket`, `Timer`, `IoScheduler`, `Service`, `Namespace`,
  `StreamSocket`, `MemFd`, `Epoll`, `SignalFd`, `Timerfd`, `Inotify`,
  `AlsaPcm`, `Drm`, `NetRaw`, `NetSocket`) × 12 `Rights` bits (`READ`, `WRITE`,
  `EXECUTE`, `CREATE`, `DELETE`, `METADATA`, `TRANSFER`, `DUPLICATE`, `WAIT`,
  `SIGNAL`, `IO_REALTIME`, `DEBUG`) — a *per-object* model with no ambient
  authority, which is the whole point of the design.
- **Linux:** 41 numbered, *ambient*, process-wide bits. Our libc currently
  gates on 22 distinct ones across **63 production sites**, all inside
  `posix/` (0 in `userspace/`, `services/`, `apps/`): `CAP_SYS_ADMIN` (20),
  `CAP_SYS_NICE` (6), `CAP_SYS_PTRACE` (5), `CAP_SYS_TIME`/`CAP_SYS_MODULE`/
  `CAP_SETGID`/`CAP_KILL`/`CAP_CHOWN` (3 each), then a long tail of 1–2.

Deciding which kernel rights *imply* `CAP_SYS_ADMIN` is deciding what a Linux
port is permitted to conclude about our capability model — that is a design
statement about the POSIX layer's honesty, not an implementation detail.

**A blocker the note did not know about.** The existing "proper fix" pointed at
`SYS_CAP_QUERY` (400) as the source of truth. It cannot serve: the handler
(`kernel/src/syscall/handlers.rs`, `sys_cap_query`) returns only a *count* of
the caller's capabilities, and its own doc comment says "a future extension
will support filling a user-space buffer with detailed capability entries."
Its sole consumer today is `userspace/strace`'s syscall name table. So **every**
option below needs an enumerating query syscall built first; that part is not
in dispute and I can do it under any answer.

**Options.**
- **A — conservative projection.** Derive each `CAP_*` from a specific
  `(ResourceType, Rights)` predicate, and report *not held* whenever no rule
  matches. E.g. `CAP_SYS_RAWIO` ⇐ any `PortIo` handle with `READ|WRITE`;
  `CAP_KILL` ⇐ a `Process` handle with `SIGNAL`; `CAP_SYS_PTRACE` ⇐ `Process`
  with `DEBUG`; `CAP_SYS_NICE` ⇐ `Thread` with `IO_REALTIME`.
  *Pro:* `capget()` becomes truthful, the gates start meaning something, and
  the mapping is auditable rule by rule. *Con:* `CAP_SYS_ADMIN` — 20 of the 63
  sites — has no natural preimage; it is Linux's junk drawer and would have to
  be either a hand-maintained union or permanently false. And every fixture
  spawned with `capabilities: &[]` starts failing gates that pass today (see
  blast radius).
- **B — capability-per-CAP.** Add `ResourceType::PosixCapability` and grant
  Linux bits explicitly at spawn, leaving the native model untouched.
  *Pro:* exact, no lossy projection, and the two models stay cleanly separated.
  *Con:* imports Linux's ambient-authority model into a design whose stated
  first principle is that there is none — the thing `design.txt` deliberately
  rejected.
- **C — keep libc optimistic, but stop pretending.** Leave the words as they
  are and make the dishonesty explicit: document `capget()` as "reports the
  ceiling, not the grant", and treat libc-side gates as advisory only, with the
  kernel as the sole authority. *Pro:* zero risk, matches how it already
  behaves, and no fixture breaks. *Con:* the silent-wrong-answer trap for
  future ports stays open, which is exactly why the entry was logged.
- **D — make `capget()` fail** (`ENOSYS`/`EOPNOTSUPP`) rather than answer
  wrongly. *Pro:* the most honest option; no caller can be silently misled.
  *Con:* Linux software calls `capget()` informationally and often does not
  expect failure, so this trades a silent wrong answer for loud breakage in
  ports — probably the worst outcome for a compatibility layer.

**Claude's recommendation.** **A**, with `CAP_SYS_ADMIN` as an explicit
hand-maintained union rather than a derived rule, and staged: build the
enumerating query syscall, seed the words from it, but keep the libc gates
advisory until the fixtures are given real capabilities. I lean against **B**
because it contradicts the no-ambient-authority principle for the benefit of
compatibility shims only, and against **D** because loud breakage in ports is
worse than the current documented-safe optimism. **C** is the honest do-nothing
and is a perfectly reasonable answer if you would rather this wait.

**Blast radius you should know about before answering A or B.** Making any gate
truthful breaks fixtures that currently rely on the permissive behaviour.
`services/ctest-jobctl`'s doc comment already says so out loud — "our libc's own
`CAP_KILL` gate reads the process capability words, which start out with every
capability held" — which is why it needs no capabilities to make a real
cross-process signal send. `self_test_cctty` and `self_test_cpgroup` spawn with
`capabilities: &[]` and would need real grants too. That is a boot-test-visible
change, so it should land with QEMU free.

**Where it bites:** `posix/src/sys_capability.rs` (`CAPS_DEFAULT` ~line 251),
the 63 gate sites led by `posix/src/process.rs` (13) and `posix/src/unistd.rs`
(10), `kernel/src/cap/mod.rs` + `kernel/src/cap/rights.rs` (the model being
projected), `kernel/src/syscall/handlers.rs` (`sys_cap_query`), and
`known-issues.md` → `TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S`.

## Q40 — Install the GNAT/SPARK and LLVM toolchains? Two Lane A roadmap items are blocked on them, and nothing else in Lane A is — Status: OPEN

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

## B-Q1 — The zoneinfo reader is done and nothing on disk to read: which tzdata do we ship, from where, and how is it updated? — Status: OPEN

*(First question filed under `roadmap.md`'s lane-prefix convention; the
unprefixed `Q1`–`Q44` above predate the three-lane split.)*

**Raised by Claude (Lane B)** (2026-08-13), on finishing
`known-issues.md` → `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`.

**The situation.** As of today both the libc and osh resolve `TZ` through real
binary zoneinfo files: `tzrules::TzFile` reads TZif v1/v2/v3 (RFC 8536) with no
allocator, `TZ=America/New_York` is looked up under `TZDIR` (default
`/usr/share/zoneinfo`), and an unset `TZ` follows `/etc/localtime` exactly as
glibc does. The reader is tested and the search order matches glibc's.

**Nothing is on disk.** So `TZ=America/New_York` still silently answers UTC —
the user gets UTC while believing they selected Eastern — and a fresh machine
still has no wall clock it can be honest about. Every piece of the fix is built
except the data, and shipping the data is a packaging decision rather than a
coding one, which is why it stops here instead of me picking.

**Why this needs you rather than me.** Three sub-decisions, none with an
obviously-correct answer, and all of them user-visible:

**(a) Which zones.** A full tzdata is ~450 KiB of binaries plus ~1 800 files.

- **A1 — full tzdata.** Everything, including backward-compatibility links
  (`US/Eastern`, `Asia/Calcutta`). Any ported program finds the name it expects.
  Costs ~450 KiB of every base image forever.
- **A2 — current zones only** (`zic -b slim`, no `backward` links). ~250 KiB,
  ~350 files. A script or a container image that says `TZ=US/Eastern` — a very
  common spelling — breaks, silently, back to UTC.
- **A3 — a minimal set at install, the rest as a `pkg/` package.** The
  installer ships the zone the user picks plus UTC; `pkg install tzdata` gets
  the rest. Smallest image; but a program that needs a zone the user never
  picked fails on a machine that looks fully installed.

**(b) Where the bytes come from.** We do not have `zic`, and I would rather not
write one — it is a real compiler for the tzdata source grammar, and getting it
subtly wrong means a wrong clock that nobody notices for months.

- **B1 — vendor the prebuilt binaries** from the IANA distribution into the
  repo (or into `pkg/`), checked in and version-pinned. Reproducible; no build
  dependency. Adds ~450 KiB of binary to git history per update.
- **B2 — port `zic`** (it is small, portable C) and compile tzdata from the
  text sources at image-build time. Keeps only text in git and makes the data
  auditable, at the cost of a C port on the critical path of the image build.
- **B3 — generate the TZif files with a small Rust tool of our own** reading
  the tzdata text sources. Same benefit as B2 with no C port, but it is the
  option most likely to be subtly wrong, for the reason above.

**(c) How it is updated.** tzdata changes several times a year, usually at
short notice, and a stale one is a wrong clock — the failure mode that started
this entry.

- **C1 — a `pkg/` package updated like anything else.** Fits the existing
  machinery; a user who never updates drifts.
- **C2 — updated with the OS image only.** Simple, but ties a timezone fix to a
  full release.
- **C3 — a dedicated fast channel** for tzdata (and only tzdata), so a zone
  change ships without a release.

**My recommendation: A1 + B1 + C1.** Full tzdata because ~450 KiB is nothing
against being wrong about `US/Eastern`, and because the whole reason to use TZif
rather than invent something is that ported programs expect what everyone else
ships. Vendored prebuilt binaries because the alternative is writing or porting
a compiler for a grammar whose bugs are invisible. A `pkg/` package because the
update cadence is exactly what `pkg/` exists for, and C3's dedicated channel is
infrastructure to build only once C1 has proven too slow in practice.

**Where it bites:** `pkg/` (the packaging decision), `posix/src/tz.rs`
(`TZDIR_DEFAULT`, `LOCALTIME_PATH`, `load_zoneinfo`),
`userspace/oils/src/interp.rs` (`TZDIR_DEFAULT`, `Shell::zoneinfo_dir`),
`tzrules/src/tzif.rs` (the reader, already done), the installer (which must
write `/etc/localtime`), and the two tests that assert the current
UTC fallback — `test_zoneinfo_names_resolve_to_utc_until_tzdata_is_shipped`
(libc) and `printf_time_falls_back_to_utc_for_a_zone_it_cannot_resolve` (oils),
both of which should start failing the day the data lands.

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

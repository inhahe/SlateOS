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

Earlier deferred operator decisions (Q1–Q33) have been
resolved — see the "Recently resolved" list below and `design-decisions.md` for
full rationale. New decisions should be appended as `## Q35 …` just above the
`---` separator that precedes the "Recently resolved" list.

## Q34 — Escalate to a full compiler-instrumented KASAN kernel to catch B-KNULLJUMP? — Status: OPEN

**Question.** We built the two *lighter* corruption detectors the Q32→A
decision called for: a lazily-mapped KASAN **shadow** (`mm/kasan.rs`) and a
slab **free-quarantine** (`mm/quarantine.rs`), both boot-green and self-tested.
Neither *passively* catches B-KNULLJUMP's actual failure mode — an arbitrary
wild **store** into a live scheduler BTree node — unless the corrupting write
happens to hit a parked/poisoned slot (quarantine) or unless we manually
`check_access` the exact suspect line (shadow). The *definitive* tool is
LLVM's `-Zsanitizer=kernel-address`, which auto-instruments **every** load/store
in the kernel and would flag the faulting instruction directly. Probing
confirms it **is supported** on our target
(`x86_64-unknown-none` → `supported-sanitizers: ['kcfi', 'kernel-address']`).
Should we invest in wiring up full compiler KASAN, or first exhaust the lighter
shadow + quarantine tools?

**Options.**

- **A — Try the lighter tools first (current path).** Run the Path-Z stress
  repro with quarantine (and targeted `kasan::check_access`) enabled; if the
  corruption vanishes under quarantine that confirms UAF/reuse and likely
  localizes the culprit free. *Pro:* cheap, low-risk, already built, doesn't
  touch the kernel build. *Con:* may not pinpoint the exact faulting store if
  the write lands outside a parked window; intermittent (~1-in-120) so needs
  many stress iterations.
- **B — Escalate to full compiler-instrumented KASAN now.** *Pro:* definitive —
  auto-catches the exact wild store with a backtrace, no guessing. *Con:* large,
  higher-risk bring-up and a genuine build fork: needs whole-kernel-VA shadow
  backing (not just heap; Linux uses a shared zero shadow page for untracked
  regions), in-kernel `__asan_*`/`__kasan_*` runtime callbacks, a fixed
  compile-time shadow offset matching our layout, `#[no_sanitize]` + careful
  ordering on all early-boot/shadow-setup paths, and almost certainly a
  separate debug build profile (whole-kernel instrumentation is a big perf hit).
  Risk of destabilizing boot if the shadow isn't perfectly ready before
  instrumented code runs.

**Claude's recommendation.** **A first, B as fallback.** Sequence the lighter
tools (done) → run the hunt → only escalate to compiler KASAN if quarantine +
targeted checks fail to localize it. Flagging B because committing the
kernel to a full instrumented build is a costly, hard-to-reverse fork the
operator may want to weigh in on — but it does **not** block A.

**UPDATE 2026-07-23 — Path A now effectively exhausted for this bug.** A full
100-iteration armed hunt campaign (`soak-20260723-190300`, `mm.corruption_hunt=1`,
KASAN shadow @64 GiB cover + slab free-quarantine) ran to completion with the
harness now false-positive-free: **100/100 boots PASSED, `[hunt] corruptions=0`
on every iter, zero wedges.** This is *inconclusive, not exonerating* — at the
~1-in-120 base rate, a clean 100-run is ~43% likely even if the bug is fully
present (see known-issues.md B-KNULLJUMP UPDATE (f)). The passive tools did not
catch the wild store, consistent with their known structural blind spot (they
only see the write if it lands in a parked/poisoned granule; B-KNULLJUMP stomps
a *live* BTree node). **Bottom line for the operator:** the cheap Path-A tooling
has been built, hardened, and run at scale without localizing B-KNULLJUMP, so
the remaining escalation is **Option B (compiler-instrumented KASAN)** — the one
tool that instruments *every* store and would flag the exact faulting
instruction. I am **not** starting B unilaterally (it's the costly build fork
this question is about) and B-KNULLJUMP does not block other roadmap work, so
I'm moving on to other tasks until you weigh in.

**Where it bites.** `kernel/src/mm/kasan.rs`, `kernel/src/mm/quarantine.rs`,
`kernel/src/mm/heap.rs` (alloc/free hooks); a compiler-KASAN escalation would
add `.cargo/config.toml` rustflags (`-Zsanitizer=kernel-address`,
`-Cllvm-args=-asan-mapping-offset/scale`), a new `__asan_*` runtime module, and
whole-VA shadow setup in early boot (`main.rs` mm init).

## Q35 — Should promoted fastpy coreutils ever *replace* the Rust coreutils in the shipping /bin, or stay a parallel demonstration track? — Status: OPEN

**Question.** The fastpy `/bin`-promotion (design-decisions.md §87 follow-on) is
underway: `cat`, `wc`, `head`, `tail` are now installed at `/bin/<cmd>` and run
as real commands resolved by name. These are **minimal** implementations (e.g.
`cat` is ~5 lines of Python) — proof-of-pipeline, not feature-complete. SlateOS
*already* ships 85 mature Rust coreutils (roadmap §2.7). At some point a single
shipping `/bin` must decide which `cat` (etc.) is *the* `cat`. Do the fastpy
utilities eventually replace the Rust ones, coexist under different names, or
remain a demo track that never lands in the real shipping image?

**Options.**

- **A — Demonstration track only (current, default).** Keep promoting fastpy
  commands additively into the *test* rootfs `/bin` to exercise the pipeline, but
  never let them shadow the Rust coreutils in a production image. *Pro:* zero
  regression risk to the mature Rust tools; purely additive/reversible. *Con:*
  the fastpy build pipeline never becomes the *actual* implementation of anything
  user-facing — it stays a perpetual demo.
- **B — Fastpy becomes the real implementation, per-command, as each reaches
  parity.** Grow each fastpy utility to feature parity, then have it *be* the
  shipping `/bin/<cmd>`, retiring the Rust one. *Pro:* realises the CLAUDE.md
  "prefer Python via fastpy for userspace tools" guidance; one implementation to
  maintain. *Con:* large per-command effort to reach parity + the maturity/perf
  of the Rust tools is thrown away; user-visible behaviour changes; needs a
  parity bar + test suite per command before any swap.
- **C — Coexist under distinct names** (e.g. `/bin/pycat`). *Pro:* both available,
  no collision. *Con:* clutters `/bin`, no clear "which is canonical" story.

**Claude's recommendation.** **A for now** — the current promotions are
explicitly proof-of-pipeline and I am keeping them additive (no Rust coreutil is
touched or shadowed). Do **not** silently swap any Rust coreutil for a fastpy one
— that's a user-visible policy change and belongs to the operator. Revisit
toward **B** only per-command, and only once a given fastpy utility has a real
parity test suite. Not blocking: more commands can be promoted additively (track
A) without resolving this.

**Where it bites.** `scripts/create-ext4-rootfs.sh` (`PROMOTED` map — currently
maps to the *test* rootfs `/bin`), `kernel/src/proc/spawn.rs`
(`resolve_command`/`COMMAND_PATH`), the fastpy `services/fastpy-*` sources, and
whatever eventually assembles the *production* rootfs `/bin` vs. the Rust
coreutils in `userspace/`.

---

## Q37 — How far should osh's bash parity go when the behavior being matched is an upstream bash *defect*? — Status: OPEN

**Question.** osh is driven toward byte-exact bash 5.2.37 parity, and until now
every divergence found has turned out to be *designed* bash behavior once its
source was read. This one is not. `declare -n q='n[1]'; declare q` — a valueless,
flagless declaration through a reference to an array element — makes bash bind a
**null value** into `n[1]`. The element list is untouched, but every reader of
`n` then stops at the null:

```text
$ n=(a b c); declare -n q='n[1]'; declare q
$ declare -p n;  echo "${#n[@]} [${!n[@]}] [${n[@]}]";  echo "${n[1]-UNSET}"
declare -a n
0 [] []
UNSET
$ n[5]=z; declare -p n
declare -a n=([0]="a" [1]= [2]="c" [5]="z")     # …and they are all still there
```

It ignores `readonly` (the bind carries `ASS_FORCE`), it turns a scalar base into
an *empty* array, and it empties an associative one the same way. No other
bash-level operation can produce a null element, and nothing in the manual or
the source comments suggests this state was intended — the chain is
`bind_variable(q, NULL, ASS_FORCE)` → `assign_array_element("n[1]", NULL, …)` →
`array_insert(a, 1, NULL)`, i.e. a NULL that was never checked for.

Full detail, probes and the reading of the bash source are in
`known-issues.md` under
`TD-OILS-A-DECLARATION-WITH-NOTHING-TO-DO-BINDS-A-NULL-THROUGH-THE-REFERENCE`.
The *read* half of this path (the subscript really is evaluated as arithmetic,
which is designed behavior) is already implemented and matched.

**Options.**

- **A — Waive it.** Mark the case `EXPECT-DIFF` in the corpus with the reasoning
  above, and treat "bash's own bugs" as outside the parity target from here on.
  *Pro:* costs nothing; keeps osh's value model honest (`Str`, never null);
  a script that relies on this is relying on a state bash cannot explain.
  *Con:* a divergence a real script could hit, however absurdly; and it sets a
  precedent that requires judging "bug vs. design" case by case.
- **B — Reproduce it.** Make the array element type nullable (`Option<Str>`) and
  teach every reader — listing, `${!a[@]}`, `${#a[@]}`, `${a[@]}`, `${a[i]-D}`,
  arithmetic reads, `unset`, iteration — to stop at the first null. *Pro:*
  byte-exact parity with no exceptions, which is the stated goal; the "stop at
  the first null" rule is at least uniform. *Con:* a large, invasive change to
  the core value model (`Shell::arrays` / `Shell::assoc` are threaded through
  most of `interp.rs`) purely to chase a defect; every future reader has to
  remember the rule; and if bash fixes it upstream the change becomes dead
  weight that has to be unwound.
- **C — Reproduce only the observable surface, not the model.** Keep `Str`
  elements and instead mark the *variable* "poisoned" with a flag that makes the
  readers report it as empty until the next store. *Pro:* far smaller than B;
  no change to the element type. *Con:* the flag is a fiction that will not
  survive the next edge case (bash's `n[5]=z` recovery already needs a rule of
  its own), i.e. exactly the band-aid CLAUDE.md forbids.

**Claude's recommendation.** **A.** The parity target is worth a great deal, but
not the core value model, and this is the first divergence where the thing being
matched is not a behavior at all — it is an unchecked NULL. If you want B I will
do it (it is a few focused hours, not a blocker), but I would rather spend that
on the ~20 genuine divergences still open in `known-issues.md`. **Not blocking:**
the read half is fixed and committed, and the corpus case that covers it omits
the store cases, so the sweep stays green either way.

**Where it bites.** `userspace/oils/src/interp.rs` —
`Shell::declare_ref_bind_read` (the read that would have to become a store),
`Shell::arrays` / `Shell::assoc` and every reader of them. Probes:
`/d/tmp/hh/bo.sh` (T-series), `/d/tmp/hh/bp.sh` (U-series).

## Q38 — Add antivirus exclusions so the osh corpus sweep is runnable again? — Status: OPEN

**Question.** Process creation on this machine currently costs **~390 ms per
spawn** through the MSYS runtime — roughly 20× normal, and stable across
back-to-back measurements. `bash -c 'for i in $(seq 1 100); do /usr/bin/true;
done'` takes 36–41 s. That makes `scripts/osh-bash-diff.py` unusable: the sweep
of 2026-08-06 05:58 produced seven failures that were all `status: bash=-1`
(the *reference* shell timing out with osh completing correctly), and the
failures cascade, because each timed-out case leaves its bash tree behind.
Individual cases that should take a second now take 13–53 s against a 20 s
budget. Full measurements are in `known-issues.md` under
`TD-OILS-CORPUS-SWEEP-IS-UNRUNNABLE-WHEN-PROCESS-SPAWN-LATENCY-SPIKES`.

Windows Defender real-time protection is on and its exclusion list cannot be
read or written without admin — hence this question rather than a fix.

**Options.**

- **A — Add Defender exclusions** for `C:\Program Files\Git\usr\bin\`, the
  repo's `target\` tree, and `osh.exe`. *Pro:* directly targets the most likely
  cause (real-time scanning of every short-lived MSYS process); restores the
  sweep as a trustworthy gate, which is the only cross-checking tool osh parity
  work has. *Con:* needs admin; narrows AV coverage over a build tree and a
  shell — a real, if small, security tradeoff, and one on paths that execute
  downloaded toolchain code.
- **B — Diagnose further before excluding anything.** The cause is not proven:
  Defender was equally on during the green 444-case sweep at 05:15 the same
  morning, so something *changed*. *Pro:* avoids weakening AV for a guess.
  *Con:* costs operator time, and the sweep stays unusable meanwhile.
- **C — Live with it.** Rely on the 1384-case unit suite plus targeted
  single-case corpus runs, and treat full sweeps as occasional. *Pro:* free.
  *Con:* the unit suite does not compare against real bash at all; single-case
  runs cannot catch a regression in a case you did not think to run.

**Claude's recommendation.** **A**, scoped as narrowly as it will go — ideally
a *process* exclusion for `bash.exe`/`osh.exe` rather than blanket path
exclusions, which keeps file scanning intact. If you would rather not touch
Defender at all, **C** is survivable and is what I am doing meanwhile.
**Not blocking:** I discriminate `bash=-1` timeouts from real regressions by
timing the case under bash alone, and there is plenty of unblocked parity work
(~42 open `TD-OILS-*` entries).

**Where it bites.** `scripts/osh-bash-diff.py` (`CASE_TIMEOUT = 20`, the
`# TIMEOUT: N` per-case override) and every `userspace/oils/tests/corpus/*.sh`
that spawns externals. Note the proper fix is *not* raising `CASE_TIMEOUT`:
that would make every genuine hang cost minutes instead of seconds.

## Q38 — Should osh be locale-aware, or UTF-8-only? — Status: OPEN

**Question.** bash decides *per locale* whether a string is a sequence of bytes
or of characters: every multibyte site is behind `HANDLE_MULTIBYTE` and calls
`mbrlen`/`mbstate`, so `${#s}` on `a…b` is 5 under `LC_ALL=C` and 3 under
`LC_ALL=C.UTF-8`. osh has no such switch — it always does UTF-8 character
semantics. Should osh grow one?

This surfaced because `scripts/osh-bash-diff.py:274` pins `LC_ALL=C` for both
shells (deliberately, for a reproducible environment). So today, on any
multibyte input, osh is compared against a bash doing byte semantics — a
baseline osh was never built for. No corpus case had exercised it until one
happened to put a `…` inside a `printf '%-46s'` label.

**Options.**

- **A — osh is UTF-8-only; move the harness to `LC_ALL=C.UTF-8`.**
  *Pros:* one line of harness change; osh's existing behaviour becomes correct
  by definition; UTF-8 is the only locale a modern desktop OS ships, and this OS
  targets exactly that; no new state on every string operation.
  *Cons:* a real bash under `LC_ALL=C` is then not reproducible by osh at all,
  so that whole axis of bash's behaviour goes untested and undocumented; scripts
  that set `LC_ALL=C` for speed or determinism — a common idiom — would get
  different answers from osh than from bash.

- **B — make osh locale-aware, as bash is.**
  *Pros:* actually matches bash, which is the project's stated goal; makes
  `LC_ALL` observable the way every other shell variable is; lets the corpus
  test both axes.
  *Cons:* touches every character-counting site (`${#v}`, `${v:off:len}`,
  `${v^^}`/`${v,,}`, `printf %q`, `\u`/`\U`, `select`'s `display_width`, and
  plausibly globbing and `[[ =~ ]]`); needs a locale notion threaded through
  `bytes.rs`, which is currently free functions with no state; and the C locale
  is the *easy* half — a non-UTF-8 multibyte locale would be far worse, so the
  honest scope is "C vs UTF-8", not "all locales".

**Claude's recommendation.** **A**, with the scope of B written down. The OS
this shell ships in is UTF-8 throughout, and B's cost is spread across the whole
string layer for an axis nothing in the OS will exercise. But A is a real
narrowing of the fidelity goal, which is the operator's call, not mine — so I
have changed nothing and am leaving the harness on `LC_ALL=C`.

**Not blocking.** In the meantime I keep multibyte strings out of
character-counting positions in corpus cases, which costs nothing. Note that
`printf`'s field width and `%c` are *not* part of this question: those are
byte-counted in every locale (bash hands them to C — `PF`, printf.def:124;
`getchr`, printf.def:1165), they were genuine osh bugs, and they are now fixed
and pinned by a corpus case verified identical under both locales.

**Where it bites.** `scripts/osh-bash-diff.py:274`;
`userspace/oils/src/bytes.rs` (`char_count`, `char_slice`, `char_at`) and its
callers. Tracked as
`TD-OILS-THE-CORPUS-HARNESS-RUNS-THE-REFERENCE-BASH-IN-THE-C-LOCALE` in
`known-issues.md`.

**Why this is a fork and not a bug list.** `printf %q` on a byte that is no
character shows it cleanly. bash's `ansic_shouldquote` sends any non-basic byte
to `ansic_wshouldquote`, which quotes when `mbstowcs` fails: under UTF-8 `a\xffb`
does not decode and bash writes `$'a\377b'`, but under C every byte is a
character and bash writes the raw `a\xffb`. osh writes the raw form — so osh is
*correct against the harness as configured today* and incorrect against a UTF-8
bash. There is no edit to osh that is right under both; only choosing A or B
makes one of them the answer. That is precisely why I have not touched it.

---

Recently resolved (see `design-decisions.md` for the full rationale):

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

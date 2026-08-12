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


## Q41 — §72's blocker expired on day 4 and was never re-checked: should bash be cross-compiled instead of osh reimplemented? — Status: OPEN

**Raised by the operator** (2026-08-12), who asked whether comparing osh against
bash and patching every difference means we should have cross-compiled bash from
the start. Auditing the decision showed the concern is better founded than §72
reads.

**The finding.** §72 rejected cross-compiling on one decisive fact: *"There is no
C/C++ → `x86_64-slateos` cross-toolchain in this repo."* That was **true when
written** (oils' first commit: 2026-07-18). It stopped being true almost
immediately:

| Date | Event |
|---|---|
| 2026-07-18 | oils begins; §72 rejects cross-compile as prerequisite-blocked |
| 2026-07-21 | `x86_64-slateos` C cross-compilation target added (fastpy, initiative F) |
| 2026-07-22 | `zig cc` wired in as the C cross-compiler; `toolchain/sysroot/lib/libc.a` |

§72 wrote its own reversal condition ("if a C++/slateos toolchain is later
built…"). The **C half fired within four days and nobody audited it.** Roughly
1,100 of the 1,181 `userspace/oils` commits landed *after* the stated blocker
ceased to exist. The original call was sound; the failure is that a decision with
a written expiry was never re-examined. bash is C, not C++ — so it needs strictly
less than the `oils-for-unix` cross-compile §72 was actually arguing against.

**What is genuinely still missing** (so this is not a slam dunk): `posix/src/signal.rs:572`
— *"We have no kernel suspend mechanism; report ENOSYS."* Bash's job control
(`SIGTSTP`/`SIGCONT`, Ctrl-Z, `fg`/`bg`) is built directly on it. Also unmeasured:
autotools cross-configure, readline, and how much of `libstubs.a` bash would hit.
Note this gap constrains **osh identically** — it is a kernel limitation, not an
argument for the reimplementation.

**Options.**
- **A — timeboxed spike, then decide.** Point `zig cc --target=x86_64-slateos` at
  bash against the existing sysroot; report how far it gets. *Pro:* converts the
  question from speculation to measurement for ~1–2 h of active work; both
  outcomes are valuable (bash runs → freeze osh's scope immediately; it walls →
  we learn exactly which libc/kernel pieces are missing, a far better roadmap item
  than "keep patching diffs"). *Con:* the spike is wasted if the operator would
  keep osh regardless.
- **B — keep osh, close this permanently.** *Pro:* osh is 138k lines, 642/642
  byte-exact vs bash, and works *today* on an OS with no dynamic linker; a real
  bash still could not do job control. *Con:* commits to an unbounded fidelity
  chase — bash has 40 years of edge cases and the corpus can grow forever.
- **C — switch to cross-compiling bash.** *Pro:* fidelity becomes free and exact.
  *Con:* discards 1,181 commits; blocked on kernel suspend for job control; osh
  would still be needed as the fallback shell meanwhile.

**Claude's recommendation.** **A**, then very likely **B** — but the spike first,
because the honest answer is that nobody has measured it and §72's factual basis
is now stale. The deeper question the operator is really raising is not
strategy but **scope**: byte-for-byte bash fidelity has no stopping criterion.
One case validated today asserts `OPTIND=4294967297` wraps to the first argument
because bash stores it in a C `int` — true of bash, and nothing on SlateOS will
ever depend on it. Worth pairing with Q40, which asks the same "does fidelity
have limits?" question from the other side.

**Meanwhile.** Nothing is blocked; osh work continues and is green (642/642).

**Spike results (2026-08-12, operator authorised option A).** Scripts live in
`scripts/bash-spike/` (see its README); artifacts land in the gitignored
`build/spike/`. Measured, not estimated — and the headline is that **it works**:

> **GNU bash 5.2 now boots and runs on SlateOS**, as a 5,349,720-byte static
> ELF linked against `toolchain/sysroot/lib/libc.a` — our own POSIX layer, not
> glibc, with **zero undefined symbols and no shims**. The kernel self-test
> `self_test_bash_on_slateos_libc` (`kernel/src/proc/spawn.rs`) runs a script
> using arrays, `${#a[@]}`, `${v,,}`, `$(( ** ))` and brace expansion — none of
> which dash has, so the result cannot be a `/bin/sh` fallback — with bash
> doing its own `{ …; } > file` redirection. Exit 0, 55 bytes byte-exact.
> Boot is green.

The detail:

- **bash 5.2 builds.** A native build succeeded first (4,501,576-byte binary),
  proving the source tree is sound. A cross-configure/cross-compile with
  `zig cc --target=x86_64-linux-musl` (`--without-bash-malloc --disable-nls
  --disable-readline --without-curses`) then compiled every translation unit
  clean. Note the first cross attempt died with `CROSS_CONFIGURE_EXIT=77`
  ("C compiler cannot create executables") purely because autotools
  word-splits `$CC` and this repo lives under `visual studio projects` — fixed
  with a wrapper script on a spaceless path, not a real toolchain problem.
- **The symbol surface is nearly covered already.** SlateOS `libc.a` defines
  2,900 symbols; bash references 2,030; **only 23 are unresolved**, and they
  decompose as: 9 termcap (`tgetent`/`tputs`/`tgoto`/`tgetflag`/`tgetnum`/
  `tgetstr`/`BC`/`PC`/`UP`, all dropped by `--disable-readline`), 8 glibc-ism
  artifacts of the native build (`__isoc23_strtol`, `__longjmp_chk`,
  `__fdelt_chk`, `__fpurge`, `__mbrlen`, `__mbsrtowcs_chk`, `__wcsrtombs_chk`,
  `__isoc23_strtoumax` — musl doesn't need these), 1 linker symbol
  (`_GLOBAL_OFFSET_TABLE_`), and **5 genuinely missing, all trivial**:
  `arc4random`, `eaccess`, `getservent`, `setservent`, `endservent`.
- **Stub quality is not the problem either.** Exactly **one** bash symbol was
  served only by `libstubs.a`: `killpg`. There are 1,299 `ENOSYS` sites in
  `posix/src`, but they cluster in aio/crypt/dirent/epoll — subsystems bash
  never calls.
- **Linking against our own `libc.a` left exactly three real gaps**, since
  closed for real in `posix/src` (not shimmed): `killpg` (`signal.rs`),
  `eaccess`/`euidaccess` (`file.rs`), `__fpurge` (`stdio.rs`).
- **The one real blocker is unchanged and is a kernel gap, not a libc gap:**
  `posix/src/signal.rs:572`, no kernel suspend ⇒ no `SIGTSTP`/`SIGCONT` ⇒ no
  Ctrl-Z / `fg` / `bg`; and no process groups, so `killpg` can only return
  `ENOSYS`. **This constrains osh identically** — it is not an argument for
  either side.

So §72's prerequisite objection is not merely stale, it is *comprehensively*
stale: C bash was three small libc functions from running on this OS, and now
does. **Feasibility is settled and is no longer an input to this decision.**

That does not make the answer C. What the spike changes is *which* arguments
are live. Still-valid reasons to keep osh (option B): 138k lines already
written and byte-exact at 642/642; it is ours to debug and extend, whereas bash
is 40-year-old C we would be maintaining a fork of; and a real bash still
cannot do job control here. Still-valid reasons to switch (option C): fidelity
stops being an unbounded chase with no stopping criterion — which is the actual
concern the operator raised — and every future corpus case is one we no longer
have to write. **A hybrid is now also on the table and was not before:** ship
osh as the shell and keep the cross-compiled bash as a differential oracle that
runs *on SlateOS itself*, which would remove the Linux-reference-bash
dependency from `scripts/osh-bash-diff.py` entirely.

**Still open, and now purely a scope/ownership call:** B, C, or the hybrid.

**Where it bites.** `design-decisions.md` §72 (its "How to reverse" clause and
the now-stale prerequisite claim), `userspace/oils/` (all of it),
`posix/src/signal.rs:572` (the suspend gap), `toolchain/sysroot/lib/libc.a`,
fastpy's `compiler/toolchain.py` (`SLATEOS_TARGET`, `_find_zig_cc`),
`scripts/bash-spike/` (the spike, kept reproducible),
`scripts/create-ext4-rootfs.sh` (stages `/bin/bash`, best-effort) and
`kernel/src/proc/spawn.rs::self_test_bash_on_slateos_libc`.


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

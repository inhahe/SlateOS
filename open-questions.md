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

## Q45 — Text clipped by `max_width` is cut mid-glyph with no ellipsis. Should `RenderCommand::Text` carry an overflow policy? — Status: **RESOLVED 2026-08-15 → design-decisions.md §427**

**Answer: A — `RenderCommand::Text` gets an `overflow: TextOverflow` field
(`Clip` | `Ellipsis`), and the compositor draws the ellipsis.** Decided by the
operator ("q45: a."); Claude raised the question and recommended this option.

A is the only option that makes the mistake *unrepresentable*: after it, a text
command cannot exist without having answered "and what if it doesn't fit?". The
operator was told the churn was several hundred call sites and chose it on that
ground anyway. Rejected: **B** (a second `RenderCommand` variant) splits a match
arm in every renderer and test forever to encode one boolean; **C** (a builder)
leaves the wrong struct-literal form available; **D** (sweep `text::elide`
across today's bad call sites) fixes today's hundred and not the
hundred-and-first.

Two things the entry in §427 carries that this summary does not: the site count
turned out to be **4517 across 208 files**, so the edit is scripted rather than
hand-made; and the sites that already set a `max_width` default to `Ellipsis`
rather than to the behaviour-preserving `Clip`, because today's behaviour *is*
the reported bug. That sub-decision is Claude's and is written down in §427 to
be overruled if the operator disagrees.

**Execution constraint:** its own commit, nothing else in flight — see §310.
Closes `known-issues.md` → `TD-GUI-CLIPPED-TEXT-IS-NOT-MARKED`.

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

## C-Q1 — Should normalization consult font coverage? The last 339 sweep disagreements are all this one question — Status: **RESOLVED 2026-08-15 → design-decisions.md §428**

**Answer: C — keep `nfc` font-blind; let `fit_to_face` decompose a composed
character the face cannot draw when the pieces are drawable.** Decided by the
operator ("c-q1: c."); Claude raised the question and recommended this option.

`norm.rs`'s layering principle stands: `nfc` answers a question about *text* and
never looks at a font, `fit_to_face` answers a question about the *font*. The
fallback goes in the second stage, where `split_undrawable` already lives and
already has that shape. The 339 residual HarfBuzz sweep disagreements — 255 of
them `\u1e09`, 57 `\u212b` — should move to `agree` without `nfc` ever taking a
face as input.

Rejected: **A** (change nothing) draws a missing-glyph box where HarfBuzz draws
correct text, and the user does not care which stage was principled. **B**
(adopt HarfBuzz's font-aware recomposition wholesale) makes normalization a
function of `(text, face)` — no longer hoistable, cacheable, or testable without
a font in hand. §428 keeps B's argument written out, because it is what has to
be beaten if some future case cannot be fixed inside `fit_to_face`.

**The bill to watch for:** mark reordering after a late decomposition, which
HarfBuzz gets right by construction and we would not. The sweep is the
instrument; an ordering case it surfaces is this decision's cost coming due, not
a surprise.

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

**In the meantime** Claude is implementing the `--bench` → release change and
the `profile` history field, which is common to all three options.

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

**If never answered:** the disk fills again — more slowly now, and it will
announce itself as a refused boot test rather than as a truncated source file.
Today's damage was one committed file and cost a minute; the same event during a
large uncommitted change loses that change outright. Note the floor protects the
*harness* only: a `cargo build` you run by hand, or an editor writing a file, is
still unguarded, so this reduces the blast radius without removing it.

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

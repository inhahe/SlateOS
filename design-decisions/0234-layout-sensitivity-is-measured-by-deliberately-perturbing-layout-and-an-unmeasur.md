## §234 — Layout sensitivity is measured by deliberately perturbing layout, and an unmeasured benchmark stays a regression

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The benchmark harness kept reporting confident, repeatable
"regressions" that were not code changes at all — they were the *addresses* the
code happened to land at. QEMU runs a loop noticeably slower when the loop
straddles a 4 KiB boundary in memory, and every commit relinks the kernel and so
re-rolls that dice for every function. The fix is to build the *same source*
several times at deliberately different offsets and measure how far each
benchmark moves with no source change at all; a later movement smaller than that
is not reported as a finding. The decision recorded here is what to do when a
benchmark has *not* been swept: it keeps its regression, rather than being
excused.

### The confound

Under QEMU's TCG a translation block is bounded by the guest 4 KiB page, so a
hot loop whose backward branch crosses a page boundary is retranslated far more
often — measured at ~1.7x per iteration. Whether it crosses is a property of the
loop's *address*, not of its code. Relinking, which any commit does, shifts
every function after the edited file.

What made this worse than ordinary noise is that it is *deterministic*. The
harness's strongest label, `REGRESSED (...every recorded run of this same kernel
image shows it)`, is awarded on replication — and a layout artifact replicates
perfectly, every time, forever. Replication asks "did the same binary produce
this twice?", and a fixed-address artifact answers yes. It is blind to layout by
construction, so the artifact arrives wearing the best label the harness has.

### The mechanism

`kernel/src/layout_pad.rs` emits a `#[used]` byte array into
`.text.slateos_layout_pad`, sized by `SLATEOS_TEXT_PAD` at build time and empty
by default. `kernel/linker.ld` places that section *first* in `.text`, so its
size shifts every other function. `scripts/layout-sweep.py` builds and benches
several pads; `bench-history.py` records the pad in each history record and
computes a per-benchmark band from the spread across arms.

Three implementation choices are worth recording because the obvious
alternatives are wrong in ways that do not show up as failures:

- **The kernel reads the pad from the linker symbols bracketing the section,
  never from the Rust constant.** Reading the constant let the compiler fold
  `if PAD_BYTES == 0` away in the unpadded build, which made that build's code
  ~256 bytes shorter — so the baseline arm differed from the padded arms *in
  code as well as in placement*, reintroducing the exact confound the sweep
  exists to isolate. This was caught by the sweep's own negative control, which
  saw shifts of 4096, 4336 *and* 4352 where a pure placement change must produce
  a single uniform number. After the fix: `+3072 .. +3072` and `+4096 .. +4096`
  across all 115,542 shared `.text` symbols, 0 unmoved.
- **A pad that is a multiple of 4096 is rejected as a sweep sample**, by a
  negative control in `--self-test`. It shifts everything by a whole page and
  therefore *preserves every straddle relationship* — a sample that looks like a
  sample, moves the whole image, and measures nothing.
- **The boot-time placement check is fatal.** It is unreachable in a normal
  build (pad 0 returns immediately), and in a sweep build a misplaced pad makes
  every arm a subset of every other, yielding a sensitivity underestimate of
  unknown size — numbers worse than none, because they would be used to dismiss
  real regressions.

### The decision: what "unmeasured" means

The band's job is to *dismiss* movements, so every uncertainty must be resolved
in the direction that dismisses fewer. Three consequences, all deliberate:

| Situation | Chosen behaviour | The tempting alternative, and why it is wrong |
|---|---|---|
| Benchmark never swept | Stays a regression, with a printed note that placement was **not** ruled out | Excusing it would silence the check in the ordinary case — nothing has been swept — which is the same failure as a check that cannot fire |
| Only two layouts sampled | No band at all | Two points define an interval containing both by construction: no residual, no way to be wrong, and it would be reported as if it were a measurement |
| One arm's host-drift factor uncomputable | The whole group is voided | Falling back to an uncorrected 1.0 lets host drift inflate the spread, widening the band — failing in the one direction that hides regressions |
| Record predates the `textpad=` banner | Excluded (absent ≠ 0) | Folding absent into 0 would enrol ~70 historical records into the unpadded arm of a sweep they were never part of, manufacturing a wide band out of months of unrelated code change |

The general principle, shared with `replication_verdict` and `MODE_UNDECIDED`:
**only a positively-evidenced verdict may excuse a finding.** An excuse granted
by absence is indistinguishable from having no check.

### Which sweep applies, when more than one exists

The most **recent** sweep wins; arm count only breaks ties. "Most arms wins" is
the tempting rule, because more sampled layouts genuinely is a better lower
bound of the true sensitivity. It is still wrong, for a categorical rather than
a statistical reason: a band is evidence about the hot loops that *exist*, and a
sweep of a commit whose code has since been rewritten is evidence about code
that no longer runs. Preferring it lets a wide, well-sampled, obsolete band
dismiss a real regression in today's code. The converse error — a
barely-above-floor recent sweep giving a band too narrow to excuse a genuine
artifact — fails the safe way: the movement stays a regression and a human looks
at it. No staleness cutoff is imposed on top, because any threshold would be
arbitrary and would silently flip the answer to "unmeasured" at some commit
count nobody chose; the report names the commit the band came from instead, so a
reader who recognises it as ancient can discount it.

### What this does not do

The band is a **lower bound**, and the report says so in as many words. Three or
four sampled layouts cannot contain the worst pair among all layouts, so a
movement just *outside* its band is not thereby cleared — it is merely not
*explained*. The alternative, presenting the band as exhaustive, would let a
near-miss be waved through by a number that was never entitled to clear it.

### Both paths to a build failure, not just the loud one

There are two independent ways a movement can fail `--fail-on-regression`: the
run-over-run comparison, and `level_shifts()` — a *sustained* shift off a
baseline older than the last three runs, which exists because a regression that
appears and then persists is invisible run-over-run by construction. The first
version of this work taught only the first path about layout.

That was a real hole, and the reasoning that hid it is written in
`level_shifts()`' own comment: *"host disturbance is random per run, while a
code regression is in every run after the commit."* True, and incomplete — **a
layout artifact is also in every run after the commit**, because the addresses
are a property of the image and every re-run of that image reproduces them
exactly. Persistence therefore separates {code *or* layout} from host noise and
cannot say which of the two it is holding.

`mode_structure()` does not cover it either. That check needs the benchmark to
have been *seen* at both of its modes across different binaries in the recorded
history, so the very first commit whose relink lands a hot loop across a page
boundary is `MODE_UNDECIDED` — which fails the build, correctly, on the evidence
it has. A sweep knows the same thing before the artifact has ever repeated,
which is the whole reason to run one.

So the band now applies to sustained shifts on the same terms as everywhere
else — a band measured for that exact benchmark, at least as large as the shift
— **plus** one extra precondition that is specific to this path:

> Placement can only explain a shift if placement could have *changed* across
> the runs the shift is drawn from.

`placement_is_constant()` answers that, and answers `True` **only on proof**: if
every run in the shift's reference and corroboration windows, and the run being
judged, are provably one kernel image, then the addresses are identical
throughout and layout is ruled out by arithmetic rather than weighed against the
numbers. Such a series is a host-level change (thermal, background load) that
persisted; reporting it is right, calling it code placement would be a false
statement. This is the same mistake as filing an A/A movement under "explained
by code placement", in a different costume — *a filter that is correct for the
ordinary case, applied to the one case whose premise it violates*.

Ignorance is deliberately **not** proof. A run with no `kernel_sha`, or a dirty
tree with no clean commit, makes the predicate `False`, because "we do not know"
is not "they are the same". That direction is the safe one here precisely
because this predicate only ever *blocks* an excuse: answering `False` on
ignorance leaves the band to be judged on its own positive evidence, exactly as
it is everywhere else, whereas answering `True` would let a missing hash veto a
correct excuse.

One structural note, because it is the kind of thing that decays: the window a
shift is drawn from is now computed once, by `level_shift_window()`, and read by
both the finding and its veto. A veto computed over a different window than the
finding it vetoes is a check that appears to fire on the evidence and does not —
this project's signature failure, and it would be invisible.

### What the first sweep measured (added 2026-08-19, after the fact)

Everything above was written before a sweep had ever completed, so the
confound's *size* was an argument from mechanism — a ~1.7× per-iteration
penalty on a straddling loop — not a measurement. It has now been measured, and
the honest note to record is that **the mechanism argument understated it
badly.**

Six arms at pads 0/1024/1536/2048/2560/3072, one commit (`b36a244bb`), release,
Logoplex3, 4128 s. Bands for 86 benchmarks: median **26.0%**, max **182.0%**,
and **61 of 86 (71%) at or above 10%**. Full distribution in
`known-issues.md`.

Three things follow that the design as written did not anticipate:

- **The suite is not split into a sensitive minority and a stable majority.**
  The tacit expectation behind "a movement smaller than the band is not a
  finding" was that most benchmarks would have a small band and the exception
  would be loud. The real shape is the opposite: 86% of the suite moves ≥5% on
  placement alone, and the *median* benchmark can be made to move a quarter of
  its own value by relinking. There is no quiet majority to fall back on.
- **Performance-critical paths are among the worst, not the best.**
  `pick_next` 132.0%, `page_alloc_free` 91.9%, `page_fault` 84.9%,
  `ipc_channel` 82.8%, `io_ring_nop` 74.5%, `syscall_dispatch` 41.9% — the
  exact benchmarks `CLAUDE.md` names as the ones that must not regress. That
  is not a coincidence: these are tight hot loops, which is precisely what a
  page-straddle penalty acts on. The benchmarks worth guarding are the
  benchmarks least able to be judged without a band.
- **The "unmeasured stays a regression" rule is now the expensive one, and it
  is still right.** With bands this wide, refusing to excuse an unswept
  benchmark means a lot of movements keep a regression they may not deserve.
  That remains the correct direction — an excuse granted by absence is
  indistinguishable from no check — but it converts the sweep from a nicety
  into a prerequisite for reading release numbers at all. The `debug` profile
  still has no sweep.

The one thing this does **not** license is treating a wide band as licence to
ignore the benchmark. A 182% band means placement can manufacture 182%; it does
not mean a 182% code regression is acceptable. It means this emulator cannot
distinguish the two, and something other than a single relink-to-relink
comparison has to.

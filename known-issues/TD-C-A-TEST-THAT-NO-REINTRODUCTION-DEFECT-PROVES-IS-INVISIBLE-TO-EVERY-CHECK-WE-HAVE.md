### TD-C-A-TEST-THAT-NO-REINTRODUCTION-DEFECT-PROVES-IS-INVISIBLE-TO-EVERY-CHECK-WE-HAVE

**Status:** OPEN
**Found:** 2026-08-24, while repairing the 56 defects the control-module
refactor stranded (design-decisions.md §535).

**What it is.** `scripts/reintro-palette.py` answers "is this test real?" by
putting a bug back and checking the test complains. It answers that question
only for tests some defect *names*. A test that no defect names is never asked
anything, and nothing anywhere reports that fact — not `--check`, which only
verifies that existing patterns still match, and not the orphan diff added in
§535, which only compares a defect list against its own previous version.

The gap is not hypothetical. `mouse_settings::the_knob_is_legible_on_both_pills`
was added by the switch work and had **zero** provers until it was noticed by
accident, during the orphan diff, because a *different* test in the same file
happened to lose its last prover. Had the switch work not also stranded 56
defects, nobody would have looked.

**Why it matters.** This is the harness's own failure mode, one level up. The
entire argument for the harness is that a test which passes tells you nothing
until you have seen it fail for the right reason. A test with no defect is in
exactly that unproven state, and it is worse than an untested one because the
suite's green result now includes it and reads as if it were checked.

**Where it lives.** `scripts/reintro-palette.py` — the `DEFECTS` list and the
`check()` preflight (around line 22471). The information needed is already in
the tree: every `#[test] fn` name under `gui/`, minus every name appearing in
any defect's expected-failures list.

**How to reproduce.** `python scripts/reintro-palette.py --coverage`. It reads
the tree and writes nothing, so it is safe to run at any time, including while
a sweep is in flight.

**MEASURED 2026-08-24.** The reporting half is now built (`--coverage`, added
the same day this entry was written), so the size is no longer unknown:

```
2922 tests in the swept packages, 2479 unproved (15.2% proved),
0 dangling, 77 single-prover
```

Read that carefully before reacting to it. **It does not mean 85% of the suite
is worthless.** A large share of those 2479 cannot be reached by a
source-level find/replace at all — a test of a pure arithmetic helper has no
"defect" short of rewriting the helper, and inventing one would be ceremony
rather than proof. What the number *is* is the first honest measurement of how
much of the suite has been examined, against a defect list that (as of the same
change) is 0 stale, 0 ambiguous, 0 no-op and therefore means what it says.

The two encouraging figures:

- **0 dangling.** No defect names a test that does not exist, so no defect can
  currently report a spurious `MISSING` after an hours-long run.
- **77 single-prover.** These are the fragile ones — one refactor from becoming
  unproved, exactly as `touchpad::the_panel_draws_nothing_that_is_immediately_erased`
  was. They are the cheapest thing to harden and the list is short enough to
  work through deliberately.

**What remains.** The report exists; the number now has to be driven down. That
is deliberate ongoing work, not a single task: pick a module, read its unproved
tests, and either write a defect that makes each one fire or satisfy yourself
that it is genuinely unreachable by patching. The report is explicitly **not** a
gate — it exits non-zero only on dangling declarations, which are unambiguously
a mistake — because a gate on a number with a legitimate floor just gets
silenced.

**Why the report could not be written earlier.** The 56 stranded defects had to
be resolved first. A coverage report computed against a list containing 56
entries that no longer apply would have overstated the proved set by exactly
those 56, and been wrong in a way that is very hard to see.

**RE-MEASURED 2026-08-25, and three faults in the harness itself found on the
way.** The figures moved because the dead-key work added tests, but the run
that produced them only became trustworthy after fixing three bugs in
`reintro-palette.py` — every one of which made the harness *understate* a
problem, which is the direction that matters:

```
2935 tests in the swept packages, 2473 unproved (15.7% proved),
0 dangling, 84 single-prover
```

- **The dangling universe was the wrong set.** `COVERED_DIRS` was doing double
  duty: it is the denominator for *unproved*, which is a deliberately narrow
  set of directories, and it was also being used to answer "does this declared
  test name exist?" — a strictly wider question, because a defect may patch a
  crate that is not swept. Fifteen live tests in `gui/toolkit` were reported as
  dangling on that basis. Fixed by splitting `tests_in_tree()` (the sweep set)
  from a new `names_in_tree()` (the existence set), the latter **derived from
  the `DEFECTS` list** so the next crate a defect reaches widens it by itself
  rather than reintroducing the same false alarm.
- **Patterns were never normalised for line endings.** Git here runs
  `core.autocrlf=input`, which normalises on commit but never on checkout, so a
  file once written by something CRLF-aware stays CRLF in the working tree
  while its committed bytes are LF. Every pattern in `DEFECTS` is spelled with
  `\n`, so in such a file *every multi-line pattern silently stops matching*
  and reports as `PATTERN NOT FOUND` — indistinguishable from the genuine
  rename this script exists to catch. Three of the four stale defects were
  purely this — `Y`, `PPPP…` and `QQQQ…`, all three in
  `gui/desktop/src/run_dialog.rs`. Note *how selectively* it strikes:
  `gui/desktop/src/lib.rs` is CRLF as well and was completely unaffected,
  because its defects happen to be single-line patterns. The bug hides only
  the multi-line defects, and only in files that happen to be CRLF — which is
  why it survived so long. `reintro-keylayout.py` had already learned the
  lesson; the fix here is a shared `source()` helper used by *both* the
  preflight and `apply_to`, because those two had duplicate copies of the
  matching logic and fixing only one changed nothing.
- **A stale defect was tallied twice.** The preflight `break`s on the first
  missing pattern, which leaves the text untouched — which is exactly what a
  self-cancelling defect looks like, so the no-op check fired as well. One
  fault arrived as two, and the misleading half was the no-op line, which
  claims the edits undo each other when in fact none of them ran. This is why
  the stale and no-op counts moved in lockstep (4/4, then 1/1) and looked like
  a correlation rather than a duplicate.

Only the fourth stale defect was real: the tray's layout label had gone from
`.unwrap_or("??")` to `self.active_layout().map_or("??", |l| l.short_label)`
when the accessor became fallible. It is retargeted. The list is once again
**1722 defects, 0 stale, 0 ambiguous, 0 no-op** (1737 after module 82 below).

**All four were then re-run for real, not left at preflight-green** — which is
the whole point of this harness applied to itself. A defect that merely
*matches* again has proved nothing; it has to go in, make the right tests fail,
and come back out. All four now do:

| defect | verdict |
|---|---|
| `Y` (run dialog focus border) | caught by 1 |
| `PPPP…` (arrows step by string) | caught by 2 |
| `QQQQ…` (caret slices on byte offset) | caught by 2 |
| `BBBB…` (tray label fallback) | caught by 2 |

`4 caught, 0 escaped, 0 never asked, 0 under-caught, 0 under-declared`, each
run ending in `restored: all files match their recorded SHA-256` — which also
confirms the newline round-trip is lossless, since `run_dialog.rs` is the CRLF
file and its restore is byte-compared, not text-compared.

The shape of all three is the same and worth naming, because it will recur:
each was a case of the harness answering a *slightly different question* from
the one asked — a narrower directory set, an LF file that is really CRLF, a
skipped loop that looks like a cancelled one — and reporting the answer as if
it were to the original. A harness that exists to stop tests being trusted on
faith is the last place that should be trusted on faith itself; every one of
these fixes is therefore a permanent check rather than a repair to one entry.

**MODULE 82, 2026-08-25 — the first tranche aimed at *logic* rather than
colour, and it found three genuinely broken tests.** Target chosen by
measurement rather than taste: `gui/desktop/src/calendar.rs` was the largest
single hole in the tree — **102 of its 113 tests unproved**. The reason turned
out to be structural and worth recording, because it holds for every file
converted in the colour campaign: calendar.rs already *had* 49 defects, and
every one of them was a palette defect. The colour half of the file was
thoroughly proved and the logic half — recurrence expansion, timestamp
arithmetic, range queries, search, sorting — had never been asked a single
question. **A file is not "covered" because it appears in the defect list; it
is covered in the dimension the defects were written along.**

Fifteen defects (`A`…`O`, labelled with 82-character runs per the
run-length-is-the-module-number convention). Final sweep, 708 s:

```
15 defects: 15 caught, 0 escaped, 0 never asked, 0 under-caught,
0 under-declared
restored: all files match their recorded SHA-256
```

That clean line is the *second* run. The first exposed four problems, three of
which were faults in the production tests rather than in the defects, and all
three were fixed in the tests rather than by weakening or dropping the defect:

- **Lesson 26: a count-only assertion cannot tell a monthly series from a
  30-day one.** `recurring_monthly` asserted `events.len() == 6` and nothing
  else, so defect `B` (monthly recurrence steps a flat 30 days) reported
  `[MISSING: recurring_monthly]` — stepping 30 days also produces six hits
  across six months. It just walks them off the 15th, one or two days further
  each time. Landing on the same day of each month is the *entire point* of
  anchoring a recurrence to the original date instead of repeatedly adding to
  the previous occurrence, so that is what the test now asserts: every
  occurrence is `(2024, i+1, 15, 09:00)`. The general form — **a length check
  proves a series has the right number of elements, never that they are the
  right elements** — applies to every `assert_eq!(x.len(), n)` in the suite.
- **Lesson 27: a case-insensitivity test whose every query is lower-case
  constrains only half the rule.** Defect `K` deleted `query.to_lowercase()`
  from `EventStore::search` and escaped outright. `search_case_insensitive`
  passed only lower-case queries in, and the haystack is lower-cased too — so
  it tested the haystack half and was blind to the query half. Three
  assertions were added with capitals in the query (`"MEETING"`, `"Team"`,
  `"ENGINEERING"`), and note the ordinary path to them is a user typing a
  proper noun, not an exotic input. A test named for a symmetric property must
  exercise **both** sides of the symmetry or it has tested one.
- **Lesson 28: a sort with no interleaved fixture has no test at all.** Defect
  `O` deleted `result.sort_by_key(|e| e.start_timestamp)` from
  `events_for_range` and the entire 2936-test suite stayed green. Every other
  caller either stores its events already in order or gets exactly one back, so
  the sort was load-bearing in production and unobserved in test. Order is not
  cosmetic here: the day cells, the detail card and the `N more` overflow all
  render the list front-to-back, so an unsorted result puts a 5pm meeting above
  a 9am one and hides the *later* event behind the overflow.
  `events_come_back_in_time_order_whatever_order_they_were_stored_in` now
  stores four events latest-first and interleaved across two days — so neither
  insertion order nor a per-day grouping would pass — and states the
  non-decreasing invariant as well as the exact list, so a future event added
  to the fixture cannot quietly weaken it.
- **Lesson 29: a `match` arm can be the type-pinning site, so a defect that
  adds a method call there fails to compile instead of proving anything.**
  Defect `D` was first written as `Recurrence::Daily =>
  anchor.add_days(step.saturating_mul(2))` and produced two
  `error[E0689]: can't call method saturating_mul on ambiguous numeric type
  {integer}`. The Daily arm is where `step`'s integer type gets fixed for the
  *whole* `match`; calling a method there makes every arm's literal ambiguous
  at once. Retargeted to `anchor.add_days(step + 1)` — "a daily event skips its
  own first day" — which needs no method resolution. The general rule: a
  reintroduced defect must be spelled in operations that do not depend on
  inference the defect itself removes, or `DID NOT COMPILE` replaces the
  verdict you were after.

Two further defects were wrong about *where* the behaviour lives, which is the
ordinary cost of writing declarations before seeing them fire:

- `C` (yearly recurrence) escaped when written as `add_years` → `add_months`,
  because a monthly step still hits the anniversary on the way past and never
  lands on the day after. Retargeted to a flat 365-day step, which drifts to
  2024-03-13 because 2024 is a leap year — the failure exists only because the
  fixture happens to cross a leap year, which is the sort of thing to choose
  deliberately rather than notice afterwards.
- `E` (the range guard `occ_end > range_start` opened to `>=`) was declared
  against `events_for_date_spanning_midnight`, but the guard lives inside
  `expand_recurrence`, which a *non-recurring* event never reaches. Re-declared
  against `recurring_yearly`, where an all-day occurrence ends exactly where
  the queried day starts.

Two defects declare unusually long catcher lists on purpose: `I` (the minute
component read off the day's remainder rather than divided by 60) declares
**12** and `N` (the day window is an hour long) declares **8**. Those lists are
not padding — they *measure* how central `timestamp_parts` and the day window
are to the module, and a future refactor that shrinks either list is telling
you something about the file.

Net effect on the campaign: unproved **2473 → 2442**, proved **15.7 % →
16.8 %**, `0 dangling`, single-prover 84 → 113, on `1737 defects, 0 stale, 0
ambiguous, 0 no-op`. The single-prover rise is expected and is not a
regression: fifteen new defects against a module with one view of each
behaviour produce mostly-sole catchers, exactly as `blur.rs` did.

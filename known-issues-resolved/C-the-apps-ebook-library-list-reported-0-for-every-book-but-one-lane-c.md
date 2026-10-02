## The `apps/ebook` library list reported 0% for every book but one (lane C)

**Status: FIXED 2026-08-16** (lane C). Verified by reverting; the test then
reports `a book read to its last page reports 0, not near 1.0`.

The shelf showed `… | 12 min | 0%` against every book. Progress was
`current_page / total_pages`, and **a pagination exists only for the book that
is currently open** — every other row divided a page number that was never
updated by a total that was zero. A reader who had finished nine of ten books
saw a shelf of untouched ones.

Now `EbookApp::book_progress(index)`, measured in bytes of the text, which is
the only measure available for a book that is not open. Deliberately a separate
method from `reading_progress`, not a generalisation of it: that one answers
"which page of how many", which is the right thing for the status bar of an
open book and is unavailable everywhere else. Bytes rather than characters is
fine here and the doc comment says why — it is a ratio of two lengths of the
same text, rounded to a whole percent.

Extracting the method was itself part of the fix rather than tidying. The
computation had lived inline in the render loop, so the only way to test it was
to recompute the ratio in the test — which would have proved that the test and
the fix agreed with each other and nothing more.

### Negative result: the wrapping-cursor rule does not want a shared crate

Recorded so the analysis is not repeated. The `if !v.is_empty() { i = (i + 1) %
v.len() }` shape appears about **59 times** tree-wide, and unifying it into a
shared helper looked like the obvious follow-up to `filediff`'s `wrap_next` /
`wrap_prev`. A scan (`build/scratch/wrapscan.py`) flagged 32 as unguarded;
reading all 32 found **none that can actually divide by zero**. Nearly all take
the modulus of a `const` array's length, which cannot be zero, or derive the
index from a `position()` call, which implies the collection is non-empty. The
two that looked genuinely dangerous — `gui/toolkit/src/svg.rs:2279` and
`apps/crossword/src/main.rs:866` — are both guarded, the second by an
`is_empty` early return sitting exactly twelve lines up, which is what the
scanner's window missed.

So the shape is common but the *bug* is not, and a shared crate would be
churn across sixteen files to prevent nothing. Keep `wrap_next`/`wrap_prev`
local to `filediff`, where they earned their place by collapsing three copies
that were genuinely reachable. The general point: **a repeated shape is a
reason to look, not a reason to refactor** — the refactor needs its own
evidence.

### Sweep progress: `filediff` 23 → 0, all lint classes (2026-08-16)

Ninth crate, fourth to reach **zero warnings of every class** across
`--all-targets`. Tests 79 → 97. The lint work itself was small; what the crate
actually had was **a feature that was fully computed and never drawn**, which
no lint could have named — the warnings were the reason to read the file, not
the finding.

Three structural changes carried the warnings, and each of the three is a
repeat of a shape already recorded in this sweep:

| Was | Is | Shape |
|---|---|---|
| the wrapping-cursor rule (`if !v.is_empty() { i = (i + 1) % v.len() }`) written out for the search matches, the change list and the merge hunks | `wrap_next` / `wrap_prev`, where `checked_rem(len) == None` **is** the emptiness test | *the same bound written out N times* |
| the viewport rule (`scroll as usize`, `(height / LINE_HEIGHT) as usize`, `.min(len)`) written out in four render paths, three of them with a bare `+ 2` | one `visible_range` and a named `OVERSCAN_ROWS` | *the same bound written out N times* |
| `Vec<SideBySidePair>` returned alone, with "row N is edit N" as an unwritten convention that is **false** for this view | `SideBySideRows { pairs, row_of_edit }`, both filled by the one loop that knows | *a `Vec` plus an index is an invariant expressed as a convention* |

The third of those is the interesting one, and it is recorded as a defect
below: side-by-side is the only view whose row count differs from its edit
count, and two separate places had assumed otherwise.

Also worth keeping: the directory list was the *fourth* copy of the viewport
rule and the only one that added no overscan, so it dropped its bottom row
while scrolling — a real if minor rendering bug that existed purely because
the rule was written four times instead of once. Unifying the four fixed it
without anyone diagnosing it.

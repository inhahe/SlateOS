## Four defects in `apps/pdfviewer`'s print range, and one in its annotations

**Status: all FIXED 2026-08-16** (lane C). Each verified by reverting it alone
and watching the specific tests fail.

`PrintSettings::resolve_pages` turns a page range into the list of pages to
print, and `parse_page_range` turns what the user typed into that range. Both
clamped against the page count — *the same bound written in two places*, the
shape this sweep keeps finding — and the two copies disagreed:

- **A range naming no pages printed the whole document.** `parse_page_range`
  dropped the parts it could not use, found itself holding an empty list, and
  fell back to `All`. So typing `50-60` into a ten-page document — a plain typo
  — printed all ten pages. Now `Custom(vec![])`, which prints nothing. Blank
  input and the word `all` are handled before that point, so reaching it means
  the user named specific pages; if none of them exist, the honest answer is
  "none". Of the two ways to be wrong about a range nobody asked for, printing
  everything is the expensive one.
- **A range beyond the end was dragged onto the last page.** The parser clamped
  the range's *start* as well as its end, so `50-60` of a ten-page document
  became `(9, 9)` — "print page 10". Now the start is a validity test (drop the
  range) and only the end is clamped, at print time, by the one function that
  knows how many pages the document has *now* rather than when it was typed.
- **`Custom` returned `[0]` for a document with no pages.** The end clamp is
  `page_count - 1`, saturating to `0`, so every range collapsed to `start..=0`
  and one starting at zero yielded page zero — an index into an empty document,
  handed to a caller with every reason to trust it. The `All` and `CurrentPage`
  arms both return nothing there; this arm being the odd one out is the tell.
  For a document that *has* pages the bug was masked, because `49..=9` is an
  empty `RangeInclusive` — and it is exactly that accidental reasoning which
  stopped holding at zero pages, so the precondition is now stated outright
  rather than left to emerge from the range type.
- **Overlapping ranges were deduplicated in quadratic time**, by a `contains`
  scan per page, on a list that was sorted on the way out anyway. Now
  `sort_unstable` + `dedup`.

And separately, in the annotation layer:

- **A failed annotation consumed an id.** `add_highlight`, `add_note` and
  `add_freehand` were three copies of one routine, and each took an id from the
  counter *before* looking for a page to put the annotation on — the guard
  running downstream of the operation it guards, another shape already in this
  sweep. Every call on a tab with no document returned `None` having burnt an
  id. The ids stayed unique so nothing broke visibly; they simply grew gaps,
  which is the kind of thing noticed only by whoever later assumes they are
  dense. Now one private `add_annotation` that allocates after the page is in
  hand, with the three public methods differing only in the `AnnotationType`
  they build.

Both id counters (`next_annotation_id` and `IdGenerator::next_id`) also moved
from `+= 1` to `saturating_add`. The reasoning in the comments is deliberately
not about overflow being reachable — it is that *wrapping* is the one failure
mode that would be silent and wrong, because a wrapped id collides with a live
object and makes `remove_annotation(id)` delete something else. Saturating
turns that into a stuck feature rather than a corrupted document.

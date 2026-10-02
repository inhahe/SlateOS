## An edit index used as a display row in `apps/filediff` (lane C)

**Status: FIXED 2026-08-16** (lane C). Verified by reverting; the test then
reports `row 39 is not on screen: scrolled to 56 showing 4 rows`.

Side-by-side folds a delete and the insert immediately after it into **one**
row — that is what a modified line is — so its row count is smaller than its
edit count, by one per modified line. Unified and inline draw one row per
edit, so for those two the row *is* the edit. Two places used the edit index
as a row anyway:

- `scroll_to_current_change` scrolled to edit N when the change was drawn on
  row N−k. On a file with twenty modified lines the last change scrolled to
  row 56 of a 40-row list: **"jump to last change" showed a blank panel.**
- `max_scroll` returned `diff.edits.len() − visible`, letting the view scroll
  past the bottom by one row per modified line.

Fixed structurally rather than by correcting the two arithmetic sites:
`build_side_by_side_pairs` now returns `SideBySideRows { pairs, row_of_edit }`
with the map filled *by the loop that builds the rows* — the only code that
knows an edit was folded into the row before it — and the app asks
`display_row_count()` / `display_row_of_edit()`, which answer per view mode.
A second pass over the output would have had to reconstruct that decision, and
could have reconstructed it differently. Pinned by
`jumping_to_a_change_scrolls_to_the_row_it_is_drawn_on`,
`the_scroll_limit_counts_rows_not_edits` and
`a_paired_modification_puts_both_its_edits_on_one_row`.

**Note on the test's file shape**, because the first attempt at it was wrong
in an instructive way: two files with *no* lines in common diff as one delete
block followed by one insert block, so the pairing rule fires exactly once, at
the seam. Twenty changed lines with no context are 39 rows, not 20. The
divergence only appears when the changes are separated by unchanged lines,
which is what real files look like and what the test now builds.

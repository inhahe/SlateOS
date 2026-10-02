## Find-in-diff in `apps/filediff` was computed but never drawn (lane C)

**Status: FIXED 2026-08-16** (lane C). Verified by disabling the highlighter
and watching five tests fail, then restoring.

`SearchState` scanned every edit, recorded byte offsets and match lengths,
recorded an equal line's hit once per side-by-side panel, and the status bar
counted them — "4/17". Enter and F3 advanced the counter. **Nothing was ever
rendered, and the view never moved.** On any file longer than one screen the
entire find feature was a number that changed while the display stayed still.

The giveaway that this had been true for a while: an earlier pass had
carefully corrected the offsets to be offsets into `edit.text` rather than
into a `to_lowercase()` copy — the `İ` (U+0130) two-bytes-folds-to-three case —
*for the benefit of a highlighter that did not exist*. Correctness work had
been done on the output of a function whose only consumer was a counter.

What was added: `render_search_highlights`, a `SearchOverlay` carrying the
three things a line needs (its matches, which panel it is, which match is
focused) as one value, `SearchState::matches_on_edit` (a binary search — the
list is non-decreasing in `edit_index` by construction, because `search` walks
the edits in order — since the highlighter asks once per visible row per
frame), `canonical_panel` so the single-column views draw an equal line's
doubly-recorded hit once rather than twice, and `scroll_to_current_match`
wired into Enter, F3, Shift-F3 and every edit of the query.

Two details in the highlighter are the recurring hazards of this sweep:

- **The box is measured in cells, not bytes.** `columns(before)`, not
  `before.len()`. Byte-vs-character confusion is now this sweep's single most
  common defect — this is the fifth — and here it would have put the box some
  columns right of the word on any line holding a non-ASCII character. Pinned
  by `a_highlight_is_placed_in_cells_not_bytes`, which searches `éééNEEDLE`.
- **`text.get(..)`, not `&text[..]`.** A match list that has outlived the diff
  it was computed against should draw nothing, not take the window down.

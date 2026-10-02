## Two smaller `apps/filediff` findings from the same read (lane C)

**Status: both FIXED 2026-08-16** (lane C).

- **The whole document was cloned every frame.** `build_side_by_side_pairs`
  and `build_inline_rows` ran inside `render()`, and both clone every line of
  both files. At 60 fps on a hundred-thousand-line comparison that is
  megabytes of `String` allocation per frame against a two-millisecond frame
  budget. Both are now cached on the app and rebuilt in `recompute_diff`,
  which is also what made `row_of_edit` reachable between frames at all — it
  had been a local variable inside `render`.
- **Stale matches survived a diff recompute.** `recompute_diff` re-ran the
  search only `if self.search.visible`, but Escape hides the search *bar*, not
  the highlights. Toggling "ignore whitespace" with hidden-but-live matches
  left them naming edits that no longer existed. Harmless while nothing drew
  them; not harmless once something did. Now unconditional, and it clears the
  list outright when there is no diff. Pinned by
  `recomputing_the_diff_does_not_leave_stale_matches` — which had to be
  rewritten once, because the first version's file shrank by too few edits for
  the stale index to fall off the end, so it passed against the bug.

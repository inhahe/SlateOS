## A byte-measured "character" window in `apps/rssreader` (lane C)

**Status: FIXED 2026-08-16** (lane C), commit `59b097746`. `extract_snippet`
takes a `context_chars` argument and its doc described a window in characters,
but it computed the window with `saturating_sub`/`+` on **byte** offsets. For
a non-ASCII article the snippet was therefore both the wrong length (up to 4×
short) and cut at an arbitrary byte, which the subsequent `&text[head..tail]`
slice panicked on whenever the cut landed inside a character. Rewritten to
walk `char_indices` outwards from the match, so the window is in the unit it
claims. Pinned by `a_snippet_window_lands_on_character_boundaries`.

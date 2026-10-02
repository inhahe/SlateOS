## A hang in `apps/spreadsheet`'s find-and-replace (lane C)

**Status: FIXED 2026-08-16** (lane C), commit `59b097746`. Found by the same
sweep. `case_insensitive_replace` looped `while let Some(pos) = hay[at..]
.find(needle)`, and an **empty** needle matches at every position without
consuming anything, so `at` never advanced and the loop appended the
replacement string to a `String` until the process ran out of memory. Reaching
it took one keystroke: open Replace, leave Find empty, type a replacement,
press the button. `textfind::matches` yields nothing for an empty needle, so
the rewritten loop terminates immediately; pinned by
`an_empty_needle_replaces_nothing`.

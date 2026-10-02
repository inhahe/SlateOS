## The reading position and bookmarks in `apps/ebook` were page numbers (lane C)

**Status: FIXED 2026-08-16** (lane C). Verified by reverting each half
separately: restoring the page-preserving font change fails
`changing_the_font_size_keeps_the_reader_where_they_were` and
`shrinking_the_font_keeps_the_reader_where_they_were`; restoring page-number
bookmarks fails three, including two that predate this work.

Reproduced before fixing, on sample book 3: at page 2 the reader is at
`"True determinism would mean we're living in a sim…"`; press `+`; the same
page number now shows `"2. Calibration"`. The book got longer in pages, the
stored number did not move, so the reader was thrown backwards. `-` threw them
forwards by the same mechanism. Bookmarks had it too — a bookmark set at one
font size marked different words at another — and because bookmarks persist,
that one is silent and permanent rather than merely startling.

The two tests are written to be independent rather than as a round trip. The
first draft *was* a round trip (`increase` then `decrease`, assert the offset
came back), and it **passed with the bug reverted**: both directions repaginate
by the same rule, so two page-preserving errors cancel exactly. The
replacement grows the font *before* measuring, leaving the shrink as the only
operation under test. Same lesson as `filediff`'s stale-match test, in a
different disguise: a test that exercises an operation and its inverse together
cannot see a bug that is symmetric in them.

The bookmark test carries a `assert_ne!` premise for the same reason —
comparing the stored bookmark list before and after a font change proves
nothing, because nothing writes to it. The test only says something once the
bookmarked text is known to have landed on a *different page number*.

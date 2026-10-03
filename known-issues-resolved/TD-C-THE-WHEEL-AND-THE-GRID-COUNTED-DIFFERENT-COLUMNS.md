## `TD-C-THE-WHEEL-AND-THE-GRID-COUNTED-DIFFERENT-COLUMNS` -- **FIXED 2026-09-18** (lane C)

**In short:** In `apps/explorer`'s icon view, with the preview panel open, the
grid was drawn four columns wide and the scroll wheel moved seven entries per
row. After any scroll the two were out of step, so clicking the top-left icon
selected **a different file from the one drawn there** -- and then opened it.
Silently, with the right name under the wrong picture.

**Two computations of one fact.** `icon_columns()` measured the whole file
pane; `render_file_list` handed the grid the *list rect*, which with the
preview open is the left half of that pane. At 900x700 the difference is seven
columns against four.

**The comment was already right.** `icon_columns`'s own doc said it is "shared
by the renderer and the wheel, because a wheel stepping by a different column
count than the grid is laid out in would move by a fraction of a row and feel
stuck". That is the exact defect, written down, above the function that had it.
**A doc comment stating an invariant is not an invariant** -- it is a claim
about code somewhere else, and this codebase has the same lesson recorded one
file over, where a comment claimed the picker was drawn above code that did not
draw it.

**How it surfaced, which is the part worth keeping.** A full workspace run went
red on `a_scrolled_icon_cell_names_the_file_that_is_drawn_in_it`. It passed
when run alone. It passed with `--test-threads=1`. It failed in parallel, every
time. That triple is what says *concurrency*, not ordering, and it narrows the
search to shared state.

The shared state was not what the test touched. `preview_open` is read from
**persisted preferences** at construction, and another test toggling the panel
writes them -- so whether this test constructed a four-column or a seven-column
grid depended on what some other test had saved, in another thread, moments
earlier. The intermittency was the *preferences*; the bug was the arithmetic,
and it was there in every run.

**The fix is one place for each fact.** `list_rect()` is the only thing that
knows whether the panel is open, and `columns_for(w)` is the only place the
division is written; the renderer and the wheel both go through them.

**The test now runs both panel states** rather than inheriting whichever one
another test persisted, and asserts that the panel really does change the count
before relying on it -- a test that silently checked the same grid twice would
pass while covering half of what it names. Mutation-checked against the old
behaviour: it fails on both states, deterministically, single-threaded.

**The general form:** an intermittent failure is usually two bugs -- a real one
that is always present, and a source of variation that decides whether you see
it. Fixing the variation makes the test green and leaves the defect shipping.
The question to ask of a flake is not "what makes this unstable" but "what is
it unstable *about*".

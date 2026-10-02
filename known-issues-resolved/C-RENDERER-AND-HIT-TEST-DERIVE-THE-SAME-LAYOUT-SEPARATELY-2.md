### C-RENDERER-AND-HIT-TEST-DERIVE-THE-SAME-LAYOUT-SEPARATELY — benchmark struck off — 2026-08-26 — LANE C

`apps/benchmark` was on the list above and is no longer: it now derives its
fixed chrome once, in a `Layout` built from the live window size, and records a
hit box for every control **as it draws it**, through `guitk::frame::Frame`.
There is no second expression left to disagree with the first, so the class of
bug is gone rather than currently-correct.

The `debug_assert!` that used to police the History tab's two derivations —
comparing the `y` the renderer had stacked against the one `history_rows_top`
computed — has been deleted, and deleting it is the point. **An assertion that
two copies agree is the shape you use when you cannot merge them.** When you
can, the assertion becomes noise that documents a hazard no longer present.
The renderer now *calls* `history_rows_top_at` to place its rows, so the sum is
made once and read by whoever needs it, including the tests.

Two things worth carrying to the remaining apps on the list:

**The clip is what makes off-screen rows unclickable, for free.** Pushing the
content rectangle with `Frame::clip` rather than `RenderCommand::PushClip`
trims every hit box recorded inside it, and drops the ones with nothing left.
A row scrolled up under the tab bar therefore records no box at all — no
`if y < content_top { return None }` guard anywhere, and no possibility of that
guard going stale. `a_row_clipped_off_the_bottom_of_the_pane_cannot_be_clicked`
passes without the handler containing a bounds check.

**A disabled control must still record its box.** benchmark's Run button stays
drawn while a benchmark runs — greyed, minus its outline — and still records
`Target::Run`; the click handler returns `Ignored`. Dropping the hit box
instead would have been the smaller change and is wrong: the click would fall
through to whatever is behind, which is not what a disabled button does.
`a_running_benchmark_refuses_a_second_start_without_disappearing` pins both
halves.

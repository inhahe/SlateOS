## C-DOTS-TWO-HIT-TESTS-THAT-DID-NOT-AGREE (lane C, 2026-08-20) — FIXED

**In short:** dots-and-boxes carried *two* functions for working out which line
a click landed on, and they did not implement the same rule. The one the app
actually ran measured how far the click was from the line itself, so any point
along a line worked. The other measured how far the click was from the line's
*middle*, so only a small disc in the centre of each line answered and both
ends were dead. Three of the tests were aimed at that second one — which
nothing in the app called. The tests were green about code that does not ship,
and the rule they were checking was not the rule the player got. The midpoint
version is gone. Fixing the geometry underneath it also turned up a second,
visible fault: the board hung 12 pixels up and to the left of centre in its
own window.

### Two rules, not two spellings

```rust
fn hit_test_line(&self, mx: f32, my: f32) -> Option<LineId> {
    let threshold = 12.0;
    …distance from (mx, my) to the segment's MIDPOINT…
}

fn hit_test_line_precise(&self, mx: f32, my: f32) -> Option<LineId> {
    let threshold = 10.0;
    …point_to_segment_distance(mx, my, x1, y1, x2, y2)…
}
```

`handle_mouse` called `hit_test_line_precise`. At `DOT_SPACING` 70 the midpoint
version made roughly the outer two-thirds of every line unclickable — so had
the app ever been switched to the shorter name, or had someone "deduplicated"
the two by keeping the wrong one, the game would have become nearly unplayable
with a full test suite still passing. This is `design-decisions.md` §486's
fault in its sharpest form: the twin was not merely *unused*, it *behaved
differently*, so the suite was not just uninformative but actively misleading.

The tell was the `_precise` suffix. A function whose name ends in `_precise`,
`_clean`, `_v2`, `_new` or `_impl`, next to one that already exists, is two
answers to one question with no statement of which is authoritative.

### The board was not centred in its window

Collapsing the geometry surfaced this. `dot_pos` put the first dot's *centre*
at `PADDING`, so its left half sat inside the padding; but `window_width`
reserved `PADDING` *plus* a whole dot's width past the last column. The painted
board therefore sat 14px from the left edge and 26px from the right, and the
same 14/26 split vertically. Nothing caught it because the only window test
asked that the board be *inside* the window, which a lopsided frame satisfies.
`dot_pos` now offsets by `DOT_RADIUS`, so the picture — discs included — sits
exactly `PADDING` from all four edges, and
`the_painted_dots_are_a_square_even_lattice_centred_in_the_window` requires the
opposing margins to be equal rather than merely positive. This is the same
survivor that nonogram's sweep produced a commit earlier, from the same cause:
"inside the window" is not a constraint.

`test_dot_pos_origin` had asserted `x == PADDING` — a restatement of the very
arithmetic that was wrong, so it passed throughout. It now asserts that the
dot's *disc* begins at `PADDING`, which is a claim about the picture.

### The duplication underneath

A line's two endpoints were derived at six sites: twice in each hit test, twice
in `render_lines`, twice in `render_cursor` — which is what let the hit test and
the picture of the same line disagree in the first place. There is now one
`// ── Board geometry ──` section: `CLICK_THRESHOLD`, `dot_pos`,
`line_endpoints`, `all_lines`, `board_pixel_span`, `window_width`,
`window_height`, `hit_test_line`. `render_lines` lost its two per-orientation
loops and is now a single pass over `all_lines`; `render_cursor` lost its
`match` on orientation entirely.

### The survivor: tests that restate the number instead of the requirement

The first mutation sweep came back 27/28. The survivor cut `CLICK_THRESHOLD`
from 10px to 1px — which would have made the game close to unusable — and no
test objected, because every slop test measured *against
`DotsAndBoxes::CLICK_THRESHOLD` itself*. All of them moved with it, so none of
them could disagree with it. That is the same fault as `test_dot_pos_origin`
asserting `x == PADDING`, and it is what `design-decisions.md` §487 is about:
a test that names the implementation's number tests only that the number equals
itself.

`the_clickable_band_covers_the_painted_line_but_not_its_neighbour` states the
two bounds in terms a player would recognise instead. Lower: a click on the
painted line — within `LINE_HOVER_THICKNESS / 2` of its centre — must find it,
or clicks that visibly land on target miss. Upper: a click in the dead centre
between two parallel lines, `DOT_SPACING / 2` from each, must find neither, or
the bands overlap and "nearest line" becomes a coin flip in empty space.
Neither bound mentions the threshold. 28/28 on the re-run.

### Outcome

156 tests green, up from 148 — three duplicate `_precise` tests deleted, eleven
written to §485's checklist, including one that walks 39 points along *every*
line on every board size (the test the midpoint rule would have failed), one
that a click exactly on a dot still lands on a line meeting there rather than
in a dead spot, and one that the renderer paints each of the `2·n·(n−1)` lines
exactly once. Mutation sweep 27/28, then 28/28 once the threshold was pinned by
something other than itself. rustfmt drift 173 → 0 (committed separately),
binary clippy 95 → 78, test-module warnings 7 → 0.

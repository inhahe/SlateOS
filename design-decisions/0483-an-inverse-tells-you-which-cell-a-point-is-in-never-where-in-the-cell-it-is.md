## §483 — An inverse tells you which cell a point is in, never where in the cell it is

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** chess drew its board and hit-tested clicks from the same
arithmetic written out eleven times, so the two could drift apart the way
§475-§482 keep drifting apart. Collapsing them into `square_origin` /
`square_at` is the same fix as always. What is new is a hole the *tests* had:
checking a dot by clicking it — feeding its position back through the hit
test and seeing which square comes out — cannot detect a dot drawn in the
corner of the right square, because the hit test answers "that square" for
every point in it. A mutation that made `square_center` return the corner
survived a full round-trip suite.

### The collapse

Eleven sites spelled out the same mapping: the square's top-left corner five
times, the board's far edge four, the centre twice. They are now one place:

```rust
fn square_origin(pos: Pos) -> (f32, f32);   // paint
fn square_center(pos: Pos) -> (f32, f32);   // labels, dots, glyphs
fn square_at(x: f32, y: f32) -> Option<Pos>; // clicks
```

Nothing user-visible was wrong beforehand — unlike checkers (§482), this board
was oriented correctly and its bounds guard was intact. This is the *preventive*
half of the family: the arithmetic agreed today, and nothing made it agree
tomorrow. Two byproducts of doing it: `PANEL_X` and the window height now derive
from a `BOARD_SIZE` constant instead of restating `SQUARE_SIZE * 8.0`, and the
lone test that hand-wrote the mapping a fourth time (`test_mouse_click_on_board`,
which computed e2's pixel position in a comment and then in code) now reads its
coordinates off the painted board.

### The hole in the round-trip

§481 established that collapsing paint and hit test *removes the disagreement
the tests were looking for*, and that the remedy is an assertion whose expected
value comes from outside the mapping. Chess's dot test looked like it obeyed
that rule: it took each painted dot, put its centre through `square_at`, and
required the answer to be a legal destination. But:

**An inverse is a function onto cells. Every point in a cell maps to the same
answer, so a round-trip through it is blind to sub-cell position by
construction.** A dot painted at its square's top-left corner — visibly wrong,
overhanging into the neighbour by a radius — round-trips perfectly.

This is not the same failure as §481's. There the test and the code shared an
origin; here they do not, and the test still cannot see the fault, because the
*question* discards the information. Round-tripping proves membership. If you
care about placement, membership is not enough, and no amount of round-tripping
will become enough.

### What was asserted instead, and the tradeoff

| | Approach | Verdict |
|---|---|---|
| Compare the dot to `square_center(dest)` | one line | rejected — that is the function under test supplying its own expected value; the mutation moves both |
| Compare the dot to the painted square containing it | ~10 lines | **chosen** |
| Assert the dot's rectangle lies wholly within some painted square | similar | rejected — passes for any dot that fits, and a dot is much smaller than a square |

Chosen: find the `SQUARE_SIZE` fill whose rectangle *contains* the dot centre,
and require the dot centre to be that rectangle's centre to within 0.01. The
expected value comes from the render commands — a second, independently emitted
artefact — so it holds regardless of what `square_center` computes. The
containment search is the part that costs the extra lines, and it is also the
part that makes the assertion mean something: it identifies the square by
geometry rather than by asking the mapping.

Generalising for the next one of these: **a test of a positioned element should
name the element's position relative to something else that was drawn, not
relative to the formula that drew it.** Round-trips remain worth having — they
caught six of the fifteen mutations here — but they are a membership check
wearing a placement check's clothes.

### Result

15 mutations, 15 caught (14 before the dot assertion was strengthened). 121
tests green, up from 113. Clippy on the binary fell 78 → 76 warnings as the
duplicated arithmetic went away, and the test module gained the
`unwrap_used`/`expect_used`/`indexing_slicing`/`arithmetic_side_effects` allow
that `CLAUDE.md` prescribes for `#[cfg(test)]`, clearing its remaining three.

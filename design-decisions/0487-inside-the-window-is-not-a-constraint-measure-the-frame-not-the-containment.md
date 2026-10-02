## §487 — "Inside the window" is not a constraint: measure the frame, not the containment

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** when a test checks that a game board is drawn *inside* its window,
it is checking almost nothing — a window twice the size it should be passes just
as happily as a correct one, and so does a board shoved into one corner. Two
mutation sweeps in a row (nonogram, then dots) produced a survivor for exactly
this reason, and in dots the fault was not hypothetical: the board really was
hanging 12 pixels up and to the left of centre, and had been all along, with a
green suite. The rule adopted here is to assert on the *margins* — the gaps
between the board and each of the four window edges — and to require the
opposing pair to be equal, which is a claim that has only one right answer.

### What went wrong twice

Both boards derived their window size from their content, which is correct, and
both were then tested with a containment assertion:

```rust
assert!(right <= win_w, "the grid runs off the right of the window");
```

That is satisfiable by an infinite family of wrong answers. In **nonogram**,
`grid_pixel_span` counted a trailing gap after the last cell that the last cell
does not have, so every window was 2px too wide; nothing could see it. In
**dots**, `dot_pos` placed the first dot's *centre* at `PADDING` — putting its
left half inside the padding — while `window_width` reserved `PADDING` *plus* a
whole dot's width past the last column. Result: 14px of margin on the left and
top, 26px on the right and bottom. A player would have seen the board sitting
visibly off-centre; the suite saw a board inside its window.

The shape of the mistake is the same in both: **the window and the board were
each computed correctly with respect to a different idea of where the board's
edge is**, and containment is too weak to notice a disagreement that goes the
"safe" way. Containment only fails when the board overflows — i.e. it catches
the *less* likely of the two errors.

### The rule

For any board painted inside a window, assert on the four margins, derived
**from the painted geometry** (the render commands), not from the constants:

| Assertion | Catches |
|---|---|
| every margin > 0 | the board overflowing, or being drawn under the header/footer |
| left margin == right margin | any horizontal size or origin disagreement |
| top margin == bottom margin | the same vertically |

The equality is the load-bearing half, and it is what a containment check can
never give you: two numbers that must agree pin the origin and the span
*together*, so a change to either one that is not matched by the other fails.
It also costs nothing to write — the margins are already being computed to do
the containment check.

The margins must be measured to the **painted** extent, not the logical one.
In dots, the board's logical edge is the last dot's *centre* but its painted
edge is half a disc further out; measuring the wrong one is precisely how the
bug got in. So the test reads the dot rectangles out of the render command list
and adds back `DOT_RADIUS`, rather than calling `dot_pos`.

### The alternative that was rejected

The obvious cheaper fix is to assert the window size against a formula:

```rust
assert_eq!(app.window_width(), PADDING * 2.0 + (gs - 1) as f32 * DOT_SPACING + DOT_RADIUS * 2.0);
```

*What changes:* the test names a number instead of a relationship.

This is the anti-pattern §486 is about, in miniature: the test restates the
implementation's arithmetic, so it passes whenever the two copies match and
tells you nothing about whether either is right. `test_dot_pos_origin` was
already written that way — `assert_eq!(x, PADDING)` — and it passed throughout
the entire period the board was off-centre, because it was asserting the very
expression that was wrong. It now asserts `x - DOT_RADIUS == PADDING`, which is
a statement about the picture rather than about the source.

### The general form: a test may not name the constant it is testing

The same sweep produced a second survivor of the same shape, and it is the
clearer statement of the rule. Cutting dots' `CLICK_THRESHOLD` from 10px to 1px
— which would have made the game nearly unclickable — passed every test,
because all six slop assertions were written as
`app.hit_test_line(mid, y - DotsAndBoxes::CLICK_THRESHOLD + eps)`. They move
with the constant, so they can never disagree with it. They were testing that
10 equals 10.

**A test that mentions the constant under test is measuring the constant
against itself.** The fix is not to hard-code a rival number — that is just the
formula anti-pattern again — but to state the two bounds the constant has to
sit between, in terms of *other* quantities that mean something to a user:

| Bound | Written as | Fails when |
|---|---|---|
| lower | a click within `LINE_HOVER_THICKNESS / 2` of the line finds it | the threshold is too small to cover the line the player can see |
| upper | a click `DOT_SPACING / 2` from two parallel lines finds neither | the threshold is wide enough that the bands overlap |

Neither mentions `CLICK_THRESHOLD`, so both can object to it. This generalises:
a padding is bounded below by what it has to clear and above by what it must
not collide with; a font size by legibility and by its slot. Where those bounds
genuinely cannot be named, the constant is arbitrary and should be documented
as arbitrary rather than pinned by a test that cannot fail.

### Where this goes

Item 1 of §485's board-geometry checklist ("the lattice is measured against the
window") should be read as requiring the margin form. Item 6 ("frame margins
positive and equal") already said so; the two are one item in practice and the
mistake both times was doing 1 and skipping 6. The checklist is unchanged, but
the two items are now always written as a single test —
`the_painted_grid_is_square_evenly_spaced_and_fits_the_window` in nonogram,
`the_painted_dots_are_a_square_even_lattice_centred_in_the_window` in dots — so
that it is not possible to do one without the other.

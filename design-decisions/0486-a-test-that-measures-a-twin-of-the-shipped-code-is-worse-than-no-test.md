## §486 — A test that measures a twin of the shipped code is worse than no test

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** sudoku is the fifth board game in `apps/` whose screen geometry
was written out at a dozen scattered sites, and the collapse went the way §485
predicted. What was new is *why* the existing tests had never noticed. The app
had two functions that computed the same thing — `cell_pixel_pos`, which the
renderer called, and `cell_pixel_pos_clean`, which every test called. The suite
was green, and it was green about code that never ran. Nothing in it could have
failed if the grid on screen had moved.

### The fault: a twin, not a gap

A missing test is a hole you can see. This is worse, because it looks like
coverage:

```rust
// what the renderer called
fn cell_pixel_pos(row: usize, col: usize) -> (f32, f32) { … }
// what all five geometry tests called
fn cell_pixel_pos_clean(row: usize, col: usize) -> (f32, f32) { … }
```

The two bodies agreed, so the tests passed. They were separate functions, so
nothing kept them agreeing. Change either one — a spacing tweak, a new gap rule,
an off-by-one — and the suite reports on the other. Five tests, eighty-one
cells' worth of assertions, and a total of zero statements about the picture.

The name is the tell. A `_clean`, `_v2`, `_new` or `_impl` suffix on a function
that already exists means someone wanted the honest version *without* editing
the one in use — which is the moment the split happens. There is no legitimate
reason for a test to prefer a duplicate over the function the product calls: if
the product's version is too tangled to test, that is the bug, and untangling it
is the fix.

### The same fault wearing a different hat

The collapse produced `cell_origin`, `cell_center`, `axis_offset`, `axis_index`
and `cell_at`, and the tests were rewritten against them — so the twin was gone.
Then the mutation sweep came back **20 of 22**, and both survivors were
mutations of `cell_center`: make it return the cell's corner, or put it half a
cell out vertically, and every one of 106 tests still passed.

The reason is the same fault in the other direction. `cell_center` was called by
tests only. The renderer, drawing a digit, had spelled the arithmetic out for
itself:

```rust
x: px + CELL_SIZE / 2.0 - 7.0,
y: py + CELL_SIZE / 2.0 - 10.0,
```

A helper nothing ships is a twin of the shipped code with the roles swapped, and
it fails the same way: the tests are right about a function, and the function is
not what runs. The fix is not another test — it is to make the renderer call
`cell_center`, and to name the two bare literals (`DIGIT_HALF_WIDTH`,
`DIGIT_HALF_HEIGHT`) so the subtraction reads as "half a glyph up and left of
the middle" rather than as arithmetic.

### Why the sweep found it and the review had not

Both instances are invisible to reading, because nothing about either is *wrong*
— the duplicated bodies agree, and the unused helper is correct. They are only
visible to the question mutation testing asks: *change this, does anything
notice?* A surviving mutation has exactly two causes, and both are bugs:

| Nothing failed because… | What it means | Fix |
|---|---|---|
| no test exercises the function | a genuine coverage hole | write the test |
| **nothing in the product calls it** | the tests measure a twin | make the product call it |

The second row is the one worth remembering. It reads as a coverage report at a
glance and is a much bigger problem: coverage can be added later, whereas a suite
aimed at the wrong function will stay green through any change at all. So a
mutation sweep is not just a check on the tests here — it is the only mechanical
way to ask whether the tests and the product are looking at the same code.

### Decision: the painted lines between cells select nothing

A sudoku grid has visible gaps — 2px inside a box, 4px between boxes — and a
click can land in one. The three options:

| Option | *What changes:* |
|---|---|
| **Chosen: the gap selects nothing** | a click exactly on a printed line leaves the selection where it was |
| Round to the nearer cell | a click on a line moves the selection to whichever cell is closer |
| Grow each cell to own the gap after it | a click on a line selects the cell above/left of it |

Chosen because the gaps *are* the grid's printed lines, and a click on the line
between two cells has not said which of the two was meant; guessing produces a
selection the user did not ask for and, on the 4px box boundaries, a visibly
arbitrary one. The gaps are 2–4px against a 52px cell, so the cost is a rare
no-op rather than a mis-selection, and a no-op is the failure the user can see
and correct. Against it: on a touchscreen or at a coarse pointer scale a 4px
dead band is a real miss, and the other two options never miss. If that ever
matters, `axis_index` is the one place to change, and
`a_click_on_the_line_between_two_cells_selects_neither` is the test that will
fail and name the policy.

### Outcome

`apps/sudoku/src/main.rs`: `axis_offset`, `grid_total_size`, `box_pixel_span`,
`cell_origin`, `cell_center`, `axis_index` and `cell_at` in one
`// ── Grid geometry ──` section; `cell_pixel_pos`, `cell_pixel_pos_clean`,
`inner_gaps_before`, `pixel_to_grid_coord` and `cell_pos` deleted. `handle_mouse`
went from twelve lines to three. All five pre-existing geometry tests were
instances of anti-patterns already named in §481–§483 and were replaced by nine
written to §485's checklist. 106 tests green, up from 102 net of the five
deletions; 23 of 23 mutations caught once the `cell_center` survivors were
closed (the twenty-third seeds the fault those survivors had exposed — digits
pinned to their cells' corners); rustfmt drift 368 → 0 (committed separately);
binary clippy unchanged at 95.

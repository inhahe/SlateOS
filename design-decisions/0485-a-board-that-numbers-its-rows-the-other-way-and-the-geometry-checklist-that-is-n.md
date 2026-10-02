## §485 — A board that numbers its rows the other way, and the geometry checklist that is now reusable

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** reversi is the fourth board game in `apps/` to have its screen
geometry written out at a dozen scattered sites, and the first where the
collapse caught every seeded fault on the first attempt — because §481–§484
had already worked out what such a test suite has to check. It also turned up a
convention that looks like a bug and is not: Othello numbers its rows *downward*
from the top, the opposite of chess and go. The code had it right, nothing said
so, and the natural "fix" would have broken it. It now says so, pinned to the
published rules rather than to a comment.

### Othello counts down; chess and go count up

`a1` is the **upper-left** square of an Othello board and `h8` the lower-right.
The notation is Goro Hasegawa's and has been standard since the game was
published. Chess's rank 1 is the bottom row and a go board's line 1 likewise, so
three neighbouring apps in this directory now disagree with each other on
purpose:

| app | row 1 is | screen row 0 is |
|---|---|---|
| `apps/chess` | the bottom | row 8 — `square_origin` flips |
| `apps/gomoku` | the bottom | row 15 — the labels flip |
| `apps/reversi` | **the top** | **row 1 — nothing flips** |

Reversi is the odd one out, and it is the odd one out *by being simpler*: no
flip appears anywhere in `cell_origin`, which reads exactly like a bug of
omission. The guard is
`the_board_is_lettered_and_numbered_the_othello_way`, which requires the painted
label "1" to sit **above** the painted label "8". Its expected value is the
published rule, not the code — the §482 discipline. Without it the collapse
would have made the convention *easier* to break, since after collapsing there
is one place to "correct" instead of a dozen places that would each have to be
changed in step.

### The checklist

Four collapses in, the questions a board-geometry suite has to answer are
settled enough to write down. Reversi's seventeen seeded mutations were all
caught on the first pass by applying them mechanically:

1. **Is the lattice right, measured against the window?** Count the painted
   cells, check the spacing between them, check the board is inside the window
   and clear of any side panel. Never against the placement function. (§481)
2. **Does every point of a cell hit that cell — not just its centre?** Probe a
   grid of points across each cell. A mapping shifted by less than half a cell
   still answers correctly dead centre. (§484)
3. **Do all four edges agree?** Sweep outward past each edge and inward from
   each edge. Two of the four apps had a hit test that was correct on two sides
   only. (§484)
4. **Does anything far outside resolve to a cell?** Sweep well past the board:
   wraps, clamps and saturating casts live out there, not just outside. (§481)
5. **Is each drawn element in the right *place within* its cell?** Measure it
   against the painted cell that contains it, never against the centring
   function. (§483)
6. **Does the frame agree with the cells about how big the board is?** Check the
   margins on all four sides are positive and equal. A board sized one cell
   wrong shifts nothing else, so this is its only symptom. (§484)
7. **Does the notation match the published rules?** Labels compared to each
   other's painted positions, with the expected ordering taken from the game,
   not the code. (§482)

### A trap in writing (3): a half-open box has no mirror

The first draft of the edge sweep asserted that `left - d` and `right - 1 + d`
gave the same answer, and failed immediately. The board occupies the half-open
box `[left, right) × [top, bottom)`: `left` is on the board and `right` is
not, so the two edges are not reflections of each other and no offset makes
them so. The fix is not a better mirror but to stop mirroring — state the
interval directly, `left - d` and `right + d` both off, `left + d` and
`right - d` both on. That reads as what it means, and it does not silently
encode an off-by-one of its own.

The general form: **a symmetry assertion needs the two things it compares to be
genuinely symmetric.** When they are not, the test's own arithmetic becomes a
second place the geometry is written down — which is the very fault the
collapse exists to remove.

### Alternatives considered

| Option | Verdict |
|---|---|
| Give reversi a `flip` for consistency with chess and gomoku | Rejected. It would make the three apps' code look alike and make one of them wrong. Consistency between apps is worth nothing against the rules of the game each implements. |
| Leave the numbering undefended, since it is currently correct | Rejected. Correct-and-unstated is exactly the condition §482 was written about; the collapse concentrates the convention into one line, which makes a well-meaning edit cheaper, not dearer. |
| Pin it with a comment rather than a test (**partly**) | The comment is there and names the test. On its own it is not enough — a comment does not fail. |

### Outcome

`apps/reversi/src/main.rs`: `cell_origin`, `cell_center`, `cell_at` and
`BOARD_PIXEL_SIZE` in one `// ── Board geometry ──` section, read by all the
former copy sites. 90 tests green, up from 81; 17 of 17 mutations caught on the
first pass; rustfmt drift 26 → 0 (committed separately); binary clippy unchanged
at 40 and the test module's 5 warnings cleared by the `#[cfg(test)]` allow.
`test_app_mouse_click_on_board` had spelled the mapping out a second time and
then asserted only that *some* move had been played; it now clicks
`cell_center(Pos::new(2, 3))` and asserts which square was played.

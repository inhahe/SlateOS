## §482 — A convention the code invents for itself is a convention nothing can contradict

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the checkers board was built mirror-image — the bottom-left
square was light and out of play, where a real board has it dark and playable.
The game was legal and every one of its 103 tests passed, because a checkers
board is symmetric under that mirror, so the app and its tests agreed with each
other perfectly while both disagreed with the world. Fixing it meant deciding
how to move a hundred coordinates without changing what any test meant, and
what kind of assertion could have caught this in the first place.

### Why the tests could not see it

`Pos::is_dark` said `(row + col) % 2 == 1`. Everything downstream took its
answer: the opening position, the click filter, the square colours. Nothing in
the program had an independent opinion about which squares are dark, so nothing
could disagree. The tests were worse than silent — they were *confirmatory*:
`test_pos_dark_squares` asserted `Pos::new(0, 1).is_dark()`, which is the
implementation restated in the test file.

This is the same shape as §480's "a test that only ever uses the input that
works" and §481's "collapsing paint and hit test means a wrong function moves
them together", arriving now at the level of a whole subsystem:

**A test whose expected value is derived from the code under test measures
consistency, not correctness.** The remedy is the same each time — one
assertion sourced from outside — but the *outside* is different here. For a
layout function it was the window's own dimensions. For a rule of a game it is
the rule as published: a dark square in each player's lower left, and the double
corner on the right. Those are facts about checkers, not facts about this
program, so the test can fail.

Two of them are asserted, not one, and deliberately: either alone is a single
bit that a mirrored board has a fifty-fifty chance of satisfying by accident.
Together they cannot both hold on a mirrored board.

The second, complementary check is `the_board_is_painted_the_colour_it_says
_each_square_is`, which reads the colour back off the render commands rather
than asking `is_dark`. It found the bottom-left painted square by geometry —
smallest `x`, largest `y` — and required it to be dark. It alone caught three of
the nine mutations, including two that had nothing to do with parity (a board
drawn a column to the right of where clicks land, and a board drawn without its
vertical flip). **A test that reaches the subject by a different route than the
code does tends to catch faults nobody was aiming at.**

### How a hundred coordinates were moved

Flipping the parity invalidates every square named in the tests. Three options:

| | Approach | Cost |
|---|---|---|
| Hand-edit each test | 183 lines, each re-reasoned | slow, and each edit is a fresh chance to encode the same mistake |
| Relax the tests to not name squares | fewer coordinates | throws away the specificity that makes them useful |
| Mechanical `col → 7 - col` mirror | one transform, applied uniformly | must be justified as meaning-preserving |

Chosen: **the mirror**, because it is provably meaning-preserving rather than
merely plausible. Checkers is symmetric under reflection in the vertical axis:
rows are untouched, so the direction each side advances and the rank on which a
man crowns are unchanged; diagonals map to diagonals; and old-dark squares map
to new-dark ones (`row + (7 - col) ≡ row + col + 1`). A test that said "this
piece has two moves against the edge" still says exactly that afterwards. The
transform also had to rewrite the expected move-notation strings, which is a
good sign it was applied at the right level of abstraction.

**Two tests were excluded from the mirror, and the reason is the interesting
part.** `test_cursor_bounds` and `test_cursor_upper_bounds` set the cursor to a
corner and press an arrow key; they are about the clamp at 0 and 7, not about a
square. Mirroring them silently inverted what they tested — the "upper bounds"
test began at column 0 — and they failed, loudly, which is the outcome you want
from a blind mechanical edit. Generalising: **a coordinate in a test is either a
position on the board or a limit of the coordinate system, and a transform of
the board must not touch the second kind.** They now carry a comment saying so,
because nothing else distinguishes them from the outside.

### The parity was written three times

`is_dark` and both of `Board::new`'s placement loops each spelled out
`(row + col) % 2 == 1` independently — the §475–§481 fault again, in a game
rule rather than a layout. The loops now ask `is_dark()`. The correction was
one line instead of three, and the pieces cannot end up on squares the board
paints light.

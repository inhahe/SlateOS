## `C-NO-CALENDAR-CLOCK-FOR-APPS` (lane C, 2026-08-27) -- **open**, missing kernel/service data

**What it is.** An application running inside a window cannot find out what day
it is. `oswindow::app::App` gives a program `tick_interval` and `Event::Tick`,
which is a *metronome* -- it says "another interval has passed", not "it is now
the 27th". There is no call anywhere in `gui/**` that returns a wall-clock time,
a date, or a timezone, and nothing in `apps/**` reads one.

**Where it bites.** Any feature whose behaviour is supposed to depend on the
calendar rather than on elapsed time. Concretely, today:

| Program | What it wanted | What it does instead |
|---|---|---|
| `apps/dictionary` | A **word of the day** -- one entry chosen by the date, changing at midnight. | A **featured word**: the same card, stepped by the reader with `Left`/`Right` or the two arrow buttons, and wrapping. |

The dictionary's original code named the field `word_of_day_index`, initialised
it to `0` with the comment "First word as word of the day", and never assigned
it again -- so it was permanently the first entry. Renaming the feature was the
honest fix: a "word of the day" that is not tied to a day is a label making a
claim the program cannot keep, and the operator would have no way to tell the
difference between "the date is stuck" and "the feature is fake".

**Why it is not simply a missing function.** A calendar clock is not one API but
a chain: a hardware time source, a persisted timezone and DST database, and a
service that owns "what time is it" so that every program agrees. Bolting
`std::time::SystemTime` into the toolkit would give apps a number without any of
that, and would then have to be taken away again when the real service arrives
-- with every caller already written against the wrong shape.

**What the proper fix looks like.** A time service reachable over a channel, and
a thin `guitk` accessor over it, so an app asks for a *civil* date (year, month,
day, weekday, in the user's zone) rather than for a count of seconds. Apps that
want a metronome keep using `Event::Tick`; apps that want the calendar ask the
service. Until that exists, an app must not pretend to know the date.

**Trigger to revisit.** When a time service exists and is reachable from
userspace: rename `Screen::Featured` back to a word of the day, seed the index
from the civil date, and keep the arrows as a way to browse.

### Lesson 51: a guard standing behind a duplicate of itself is a guard no test can reach (lane C, 2026-08-27)

**In short:** the sound mixer checked two of its invariants twice — once where
the value was produced and again where it was used. The second check never
fired, because the first one had already made it true. That sounds harmless,
and it is worse than harmless: it makes the *first* check untestable. Delete
the first one and the program behaves identically, because the second quietly
does its job. A 111-mutation sweep found both, and in each case the "surviving
mutation" was not a hole in the tests at all — it was a piece of the program
that could not be observed to matter.

Both, in `apps/mixer/src/main.rs`:

| Where | The redundant second guard | Why it could never fire |
|---|---|---|
| `percent_of` | after `level.clamp(0.0, 1.0) * 100.0` and `.round()`, a second `.clamp(0.0, 100.0)` | The first clamp already bounds the result to `0..=100`. |
| `Action::ChooseDevice` | `if self.picker_row >= self.picker_len() { return; }` | All four ways of setting `picker_row` already bound it: `SelectPickerRow` refuses an index past the end, `MovePickerRow` clamps, opening a sheet takes the row from the chosen device, closing one resets it. |

**Why the second clamp in `percent_of` was actively wrong, not merely
redundant.** Its own doc comment read: "the clamp is before the cast rather
than after it, because a cast out of range in Rust saturates silently and a
level that arrived out of range is a bug worth not hiding behind a number that
happens to look reasonable." The second clamp *is* that silent saturation,
written out longhand. The comment and the code two lines below it said
opposite things, and the code won.

**The rule.** Defence in depth is two checks of two *different* invariants at
two trust boundaries. Two checks of the *same* invariant in sequence is one
check plus one hiding place — and the hiding place is upstream, where the
check that people actually read lives. When a mutation sweep reports a guard's
removal as harmless, the finding is usually not "write a better test": it is
"one of these two guards is dead, and dead code in a safety position is worse
than no code, because it reads as protection."

**How to tell the two apart.** Ask what state the second check exists to
catch, and then try to reach that state from the public API. If nothing can
produce it, the check is dead. If something can — as with `value_at`, which is
`pub` and whose caller need not be the pointer handler that happens to pass it
a matching rectangle — the check is live, and it wants a test that reaches it
directly rather than through the caller that makes it unnecessary.

### Lesson 52: a test built out of the thing it is testing cannot fail (lane C, 2026-08-27)

**In short:** two shapes of test look thorough, pass forever, and are blind by
construction. An **agreement test** checks that two views of the program say
the same thing — but if both views are computed through the same function,
breaking that function breaks both views identically and they go on agreeing.
A **symmetry test** checks that two operations undo each other — but two
operations that have been *swapped* undo each other exactly as well as two that
have not. In both cases the fault is inside the part the test cancels out, so
no version of the fault can make it red.

Both showed up in one 111-mutation sweep of `apps/mixer`:

| The test | What it says | The fault it cannot see |
|---|---|---|
| `a_column_index_means_the_same_stream_to_the_screen_and_to_the_keyboard` | the name drawn on column *i* is the name of `stream_at(i)` | `stream_at` returning `streams[i]` instead of the *i*th column in draw order. Both sides call `stream_at`, so both moved together. |
| `left_and_right_undo_each_other_from_every_column` | Left then Right returns you where you started | Left and Right swapped. Swapped keys still undo each other. |

**What does catch them.** Not a better agreement test — a test that names an
outside fact. For the column mapping: assert against the order the columns are
*drawn* in ("column 0 is Discord, because playing streams sort first"), which
is a claim about the sort, not about `stream_at`. For the arrow keys: assert
which *way* a keystroke moves ("Right from Master selects column 0"), which is
a claim about direction, not about invertibility.

**The rule for writing them.** Before trusting a test of the form "X equals Y"
or "f then g is identity", ask: *what does each side go through?* If the two
sides share a step, the test is silent about that step, and you need a third
statement that comes from outside the program — a literal, a hand-computed
value, a fact from the design document. A test whose every term is defined by
the code under test is a tautology with assertions in it.

**Corollary for mutation sweeps.** When a sweep reports a mutation caught by
tests *other than* the one you predicted would catch it, that is not a
bookkeeping detail to fix by editing the expectation. It is the sweep telling
you the test you named is blind — and the reason is usually one of these two
shapes. Both entries above were found exactly that way: the harness reported
`WRONG TESTS`, and the honest reading was not "my expectation was off" but "the
test I wrote for this cannot see this."

### Lesson 53: an `else` is a two-way choice, and some choices have three ways (lane C, 2026-08-27)

**In short:** `if a > b { up } else { down }` reads as "go up, or else go
down". It is really "go up, or else go down *including when there is nowhere
to go*". Whenever the two branches are directions and the quantity can also be
**equal**, the `else` silently annexes the third case and runs it backwards.
The word search shipped exactly this and it made most of the game unplayable.

**The fault.** `apps/wordsearch`'s `walk(start, end, i)` answers "where is the
`i`th cell of the line from `start` to `end`, along one axis". It was:

```rust
fn walk(start: usize, end: usize, i: usize) -> usize {
    if end > start { start.saturating_add(i) } else { start.saturating_sub(i) }
}
```

A horizontal line does not move on the row axis at all: for the line from
`(2, 3)` to `(2, 7)`, `walk` is called with `start == end == 2`. `2 > 2` is
false, so it took the "count down" branch and produced rows `2, 1, 0, 0, 0`.
Every horizontal mark below row 0 previewed as a staircase running off the top
of the board, and — because the marked cells then matched no placed word — **no
word lying along any row or column but the first could be marked at all.** The
same for vertical words and the column axis. The `saturating_sub` is what made
it look plausible rather than crash: the wrong answer was clamped into a legal
coordinate.

**Why review does not catch it.** The bug is invisible at the call site
(`walk(sr, er, i)` is unremarkable) and invisible in the function (both
branches are correct code). It lives in the *boundary between* them, which is
the one place an `if/else` does not name. The `else` keyword is an
unconditional catch-all wearing the appearance of a complement.

**What catches it.** Two things, and the first is cheaper:

1. **Write the comparison as a `match` on `Ordering`.** `match end.cmp(&start)`
   forces all three arms to be spelled out; the compiler will not let the
   `Equal` case be forgotten. (Clippy's `comparison_chain` pushes the same way
   from the other side, by objecting to `if a > b … else if a < b …` chains.)
   In this file that turned a bug into three lines that state their intent.
2. **A test that exercises the still axis specifically.** The old suite tested
   `cells_between` on diagonals, where both axes move and the fault is absent,
   and on a horizontal line *on row 0*, where `saturating_sub` clamps to the
   right answer by luck. The general rule: when a function takes a pair of
   values that can be `<`, `==` or `>`, the `==` case is a case, and a suite
   that omits it has tested two thirds of a three-way branch.

**Where else to look.** Any `if x > y { .. } else { .. }` whose branches are
*opposite actions* rather than *a thing and its absence*: direction of travel,
sort comparators, scroll/clamp arithmetic, "grow or shrink". Where the branches
are "do it" / "don't", `else` is genuinely the complement and this lesson does
not apply.

### Lesson 54: a fixture in which two different quantities happen to be equal cannot tell them apart (lane C, 2026-08-27)

**In short:** a test proves a program right by watching the program get an
answer. If the *numbers you chose for the test* make the right answer and the
wrong answer come out the same, the test watches the program get the right
answer for the wrong reason and reports success. This is not a weak assertion —
the assertions can be exact and exhaustive — it is a weak **fixture**, and it
is invisible in the test's own text, because nothing in the test says "and
these two numbers are different". Minesweeper shipped two of these, found by
mutation, in code whose tests looked thorough.

**The two faults.**

1. **A square board cannot tell a row count from a column count.** The flat
   index of a cell is `row * cols + col`. Beginner minesweeper is 9 × 9 and
   Intermediate is 16 × 16, so on either board `rows()` and `cols()` are *the
   same number*; a version that multiplied by `rows()` passed every assertion
   in a test that walked all 81 cells and checked each index round-tripped.
   Expert (30 columns, 16 rows) is the only board on which the test means
   anything.
2. **Three ticks of 250 ms, 250 ms and 2500 ms add up to 3000 ms — and so do
   three ticks of a flat 1000 ms.** The clock test existed precisely to pin
   down "count the time that passed, not the number of times you were woken",
   which is a bug the program had once shipped. It asserted the exact final
   reading, `3_000`. The rule it was written to forbid produces `1000 + 1000 +
   1000`, which is *also* `3_000`. The test could not fail.
3. **A cell on the diagonal is its own mirror image.** (Found a day later, in
   the same suite, by the same sweep — which is the argument for reading every
   verdict rather than only the survivors.) `hit_test(event.x, event.y)` was
   mutated to `hit_test(event.y, event.x)`, which reflects every click across
   the diagonal. Sixty-odd tests failed. The one test whose entire subject is
   *where a click lands* — it clicked one cell, row 5 column 3, and checked
   that cell opened — passed, because on that board the board's left margin
   happens to exceed its top margin by about two cells, which puts cell 5,3 on
   the line the reflection fixes: the swapped point landed back inside the very
   cell it started in. A one-point fixture cannot see a transformation that has
   fixed points, and every reflection, rotation and swap has them. Click four
   cells spread about, and no single reflection fixes all four.

**Why review does not catch it.** All three tests read as strong: they assert
exact values, and (1) asserts them over every cell on the board. The defect is a
relationship between two constants that the test never mentions — `9 == 9`,
`250 + 250 + 2500 == 3 * 1000` — and there is nothing at the point of reading
to prompt the question. A reviewer checks that the assertion follows from the
rule; they do not usually check that it *fails* to follow from the plausible
wrong rules.

**What catches it.**

- **Mutation.** This is the class of blindness mutation testing exists for, and
  essentially the only one that finds it reliably. All three were caught by a
  one-line substitution (`cols()` → `rows()`, `elapsed_ms` → `1_000`,
  `hit_test(x, y)` → `hit_test(y, x)`) and by nothing else in the test that
  owned the rule.
- **Choose fixtures whose quantities are pairwise distinct, deliberately, and
  say so in the test.** Prefer the non-square board, the non-uniform interval,
  the list whose length differs from its element values, the index that differs
  from the value stored at it. Where the distinctness is the point, assert it:
  `assert_ne!(a.rows(), a.cols(), "a square board cannot tell them apart")`
  turns an invisible property of the fixture into a line that fails loudly if
  someone later "simplifies" the test onto Beginner.
- **Ask of every constant in a fixture: what wrong rule also produces this
  number?** For a sum, a uniform interval whose product matches is the usual
  culprit; for an index, equal dimensions; for a scale factor, 1.0; for an
  offset, 0.
- **For anything that maps a point to a thing — a hit test, a projection, a
  transform — use more than one point, and put them where a symmetry cannot
  hold them still.** A single sample cannot distinguish a mapping from any of
  its symmetries, and the fixed points of a swap, a reflection or a rotation
  are exactly where a tidy fixture (the middle, the diagonal, the origin) tends
  to land.

**Where else to look.** Any test using a square grid, a power-of-two size that
also appears as a count, a duration equal to the tick period, a scale of 1, an
origin of 0, a single sample point, or a collection whose length coincides with
one of its values.
Also any test whose fixture was chosen for tidiness — round numbers and equal
dimensions are exactly the choices that make wrong rules agree with right ones.

### Lesson 55: an unbounded loop in a test helper turns every fault it depends on into the same silence (lane C, 2026-08-27)

**In short:** test helpers often *drive* the program to a starting position —
"press Right until the cursor is on cell 5,3" — before the test asserts
anything. Written as a `while` with no step limit, such a helper never returns
if the program has stopped moving the way it says it does. The test does not
fail; it hangs. The runner kills it on a timeout, and every distinct fault that
touches the helper produces the identical, contentless verdict: *timed out*.
The suite has told you nothing about which rule broke, and it charged you the
full timeout to say it.

**What happened.** `apps/minesweeper`'s tests walked the cursor with

```rust
while a.cursor() != (r, c) {
    if cr < r { a.apply(Action::Move(Dir::Down)); } else if …
}
```

in three places. Five separate mutations of the movement code — a step that
stays put, an `Up` that goes down, a `Left` that goes up, a move that reports
it moved without moving, and a move that uncovers as well as moving — all
produced exactly one outcome: a 240-second hang. Five different faults,
one verdict, twenty minutes of wall-clock to learn nothing. `apps/maze` had
already hit this once, in a helper written as `while g.moves() < moves`, where
a program that stops counting moves spins forever; that finding was written up
in the roadmap entry and **not** promoted to a lesson, which is part of why the
same shape appeared again in a suite written afterwards.

**Why it is worse than it looks.**

- **A hang is scored as a catch, and it is a real one** — a program that hangs
  is a program that is wrong. So the sweep stays green and nothing prompts you
  to look. The loss is invisible in the summary line.
- **It destroys the one thing a mutation sweep is for.** The sweep's product is
  the *mapping* from fault to the test that owns it. A hang maps every fault to
  the same non-answer, so the mutations become a smoke test.
- **It is expensive in the currency that matters.** Each hang costs the full
  runner timeout. Sweeps are already the slowest thing in the loop.
- **It can mask a survivor.** If a mutation would otherwise have survived, and
  a helper it touches hangs first, the survivor is reported as caught.

**The rule.** *Every loop in a test that waits on the program to reach a state
gets a step bound, and the bound's failure message names the fault.* A bound is
one line and it converts a timeout into a sentence:

```rust
let bound = a.rows() + a.cols() + 8;
for _ in 0..bound { … }
panic!("the cursor did not reach {row},{col} in {bound} moves — it is at {:?}, \
        so a move is not moving where it says it does", a.cursor());
```

Pick the bound from the geometry, not from taste: it should be comfortably
above the longest legitimate route and far below "forever". Where two tests
need the same walk driven differently — one through `apply`, one through the
keyboard, because *that* test's subject is the keyboard — take the step as a
closure rather than writing the loop twice; two copies of a loop are two places
for the bound to be forgotten.

**Where else to look.** Any test containing `while`, `loop`, or a `retry`
helper whose exit condition is a fact about the program under test rather than
a counter: "until it settles", "until the queue is empty", "until the animation
finishes", "until the search finds it", "until the generator deals a solvable
board". Also every polling helper in an integration test. The safe shapes are a
bounded `for` with a named failure at the end, or an exit condition that
depends only on the test's own counter.

### Lesson 56: a fixture one move from finished tests the finish, not the move (lane C, 2026-08-27)

**In short:** most tests of a game start from a fixture — a board in some
prepared position — and then make a move. If the fixture is *one move away from
being over*, the first move a test makes ends the game, and every assertion
after that point is about a **finished** game rather than about the rule the
test was written for. The test still passes. It passes because the program
refused the move for a reason the test never mentions. Sudoku shipped six of
these at once, from a single fixture, and they all went green.

**What happened.** `apps/sudoku`'s tests were built on

```rust
fn board(holes: &[usize]) -> SudokuApp   // the solved grid, with `holes` emptied
```

and almost every test called `board(&[idx(1, 7)])` — the finished board with
exactly one square rubbed out. The solution's digit at `(1, 7)` is a `4`, so:

- `a.apply(Intent::Digit(4))` filled the last hole, `check_completion` fired,
  and the status became `Won`;
- `apply` refuses `Select`, `Digit`, `Erase`, `Hint`, `Undo`, `Redo` and
  `ToggleNotes` once the status is not `Playing`;
- so every later intent in that test returned `Ignored` — which is very often
  exactly what the test was asserting.

`a_hint_for_a_square_that_is_already_right_is_refused` is the clearest case. It
wrote the right digit into the hole, then asked for a hint, then asserted
`Ignored`. The rule it is named for — *a hint is not spent on a square that is
already correct* — was never exercised. The hint was refused because the game
was over. Deleting the `cell.value == self.solution_at(row, col)` clause from
the production guard would not have failed it.

Six tests were passing for this wrong reason. The fix was one line — a fixture
with **three** holes instead of one —

```rust
/// A game with room to play in.
///
/// Three holes, not one. A fixture one digit short of finished turns every
/// test that writes a digit into a test about a *won* game.
fn playground() -> SudokuApp {
    board(&[idx(1, 7), idx(7, 1), idx(4, 0)])
}
```

— and it turned ten tests red at once, which is what those tests should have
been saying all along.

**Why review does not catch it.** Nothing in the test's text mentions the
number of holes; the fixture is a helper call two lines up, and the count is a
property of the *board data*, not of the test. The assertions are exact and the
test name is honest about its intent. The defect is that the program has a
second, unrelated reason to produce the asserted answer, and that reason was
switched on by the fixture. It is lesson 54's shape — a fixture in which two
things coincide — but the coinciding things here are not two numbers, they are
**a rule and a state**: "refused because the square is right" and "refused
because the game is over" are indistinguishable from the outside.

**The rule.** *A fixture must leave room for every move the test will make, and
then some.* Concretely:

- **Count the moves the test makes, and give the fixture strictly more slack
  than that.** One hole plus one digit is zero slack. Three holes and one digit
  leaves two.
- **When a program has a terminal state, no fixture should be one step from
  it** unless reaching the terminal state is the test's subject — and then the
  test should be named for it (`the_last_digit_wins_…`, `a_redo_can_win_…`) and
  should be the *only* thing it does.
- **Assert the precondition the test depends on, in the test.** One line —
  `assert_eq!(a.status(), GameStatus::Playing, "the fixture was already over")`
  after the move — converts an invisible property of the board into a failure
  with a sentence on it. The same trick as lesson 54's `assert_ne!`.
- **Two fixtures, named for what they are.** `playground()` (room to play) and
  `almost_done()` (one square left) both exist here now. A single fixture used
  for both purposes is a fixture that is wrong for one of them.

**How it was found.** Not by review, and not by the mutation sweep either —
by *raising the hole count and running the suite*. Twelve tests went red;
ten of them were this. That is the cheap general move: when a fixture encodes a
number, change the number and see how much of the suite was standing on it. A
suite that does not notice is a suite that was not testing the number; a suite
that collapses was testing something else than it claimed.

**Where else to look.** Any test suite for something with a terminal state —
won/lost/complete/full/exhausted/closed/end-of-file — whose fixture sits on the
last step before it. Boards with one empty square, buffers with one byte of
room, counters one below their limit, a quota with one use left, a queue with
one slot, an undo history one entry from its cap. Also the mirror image: a
fixture already *in* the terminal state, which makes every "is refused" test
pass for free.

### Lesson 57: "is it drawn?" is not "is it drawn *there*?" (lane C, 2026-08-28)

**In short:** a layout test that asks only whether each part of the window
survived, and never where it landed, will pass with the window upside down.
Sudoku's band test named its subject exactly — "the bands go in the order they
are named" — and then checked only that each band was *shown*. The mutation
sweep drew the footer across the top of the window and slid the keypad
underneath it, and every assertion in that test still held, because a band in
the wrong place is still a band that is there.

**The fault.** The layout drops bands from the bottom up as the window shrinks,
so the interesting property is a ladder: at 400 px tall everything fits, at 120
the footer is gone, at 80 the keypad is gone too. The test walked that ladder
carefully at four heights and asserted twelve `shows(...)` predicates. Not one
of them mentions a coordinate. Two mutations survived it:

| Mutation | What it did | Why the test held |
|---|---|---|
| `Rect::new(0.0, (h - foot_h - pad_h)…)` to `Rect::new(0.0, (h - pad_h)…)` | put the keypad below the footer | the keypad is still non-empty, so it is still "shown" |
| `Rect::new(0.0, (h - foot_h)…)` to `Rect::new(0.0, 0.0, w, foot_h)` | drew the footer across the top of the window | same |

**The rule.** *When a test's name contains a spatial word — order, above,
below, beside, inside, first, last, left, right — at least one assertion must
compare two coordinates.* Visibility and placement are two different
properties, and a predicate over one says nothing about the other.

The replacement, `the_bands_stack_down_the_window_in_the_order_they_are_named`,
asserts four things at one size: the first band starts at the top, the last ends
at the bottom, every band lies inside the window horizontally, and each band
begins at or below the end of the band named before it. That last one is the
whole content of the word "order", written down. Note that the bands are padded
apart and so do *not* abut — the first draft asserted `a.bottom() == b.y` and
failed honestly on a six-pixel gap, which is the assertion telling you what the
layout really promises rather than what you assumed.

**Where else to look.** Anything that answers "which of these exist" as a proxy
for "how are these arranged": z-order tests that check a thing was drawn rather
than that it was drawn *last*; tab-order tests that check every control is
reachable rather than that Tab reaches them in order; menu tests that check
every item is present; sort tests that check the output is a permutation of the
input. In each, the sets match and the order is unexamined.

### Lesson 58: a witness that more than one rule explains tests only the strongest of them (lane C, 2026-08-28)

**In short:** to test that a program enforces rule A you give it something that
rule A forbids and check it says no. If the thing you gave it is *also*
forbidden by rule B, the program can say no with rule A deleted, and your test
cannot tell. Sudoku's candidate test struck out a digit in the row, a digit in
the column and a digit in the box, and asserted each was struck — and all three
of its witnesses sat inside the same 3x3 box, so the box scan alone did all the
work. Deleting the row scan and deleting the column scan both survived the
sweep, and the messages `"the row was ignored"` and `"the column was ignored"`
had never once been about the row or the column.

**The fault.** The witnesses were `(0, 1)`, `(1, 0)` and `(2, 2)`, chosen for
cell `(0, 0)`. They read as "one along the row, one down the column, one in the
box" — and `(0, 1)` and `(1, 0)` genuinely are in the row and in the column,
which is exactly what makes the mistake easy to make and hard to see. They are
also both inside cell `(0, 0)`'s own box, which spans rows 0-2 and columns 0-2.
Sudoku's three rules overlap by construction near the corner of a box, and the
fixture sat in the overlap. Moving the witnesses to `(0, 5)` and `(5, 0)` — same
row, same column, different box — took thirty seconds and turned two survivors
into two catches.

**The rule.** *For each rule you mean to test, pick a witness that no other rule
explains.* State it as a question about the fixture: "if the code under test
were deleted, would anything else still produce this answer?" If yes, the
fixture is wrong however sharp the assertion is.

Two habits that make it cheap:

- **Write the exclusion into the test as a comment naming the other rules,** so
  the next person to edit the coordinates knows what the coordinates are for. A
  witness chosen for a reason that is not written down will be moved.
- **When rules overlap by construction, put the witness as far from the overlap
  as the domain allows.** In a 9x9 grid of 3x3 boxes, a row witness belongs in
  a different box band, not in the next column.

**Where else to look.** Any validator with several independent rules that can
each reject the same input: a password checker tested with `""` (too short —
and also no digit, no capital, no symbol); an access check tested as a user who
is both unauthenticated *and* unauthorised; a parser tested with input that is
both malformed and too long; a firewall rule tested with a packet the default
policy would drop anyway. The tell is a run of assertions that all use the
*same* fixture value with only the expected message changing.

### Lesson 59: a fixture with no asymmetry cannot notice that two things were swapped (lane C, 2026-08-28)

**In short:** transposing rows and columns, reversing a cycle, and taking from
the wrong end of a list are all changes that leave the *shape* of the answer
intact and move only which item is where. A fixture in which every item is
alike, or whose arrangement is symmetric, comes out identical either way — so
the test passes and the swap ships. Sudoku's sweep found three of these in one
suite, and the fixture was the cause in all three.

**The three faults.**

1. **Nine marks in nine places is symmetric under transposition.** The pencil
   marks of a square are laid out 3x3, `nrow = slot / 3`, `ncol = slot % 3`.
   The test set *all nine* marks and asserted that nine texts were drawn in nine
   distinct positions. Swapping those two lines still draws nine marks in nine
   distinct positions — the very same nine positions, with the digits permuted.
   The all-nine fixture hid a second mutation too: with every slot occupied,
   drawing the marks that were *not* set changed nothing either. Three marks —
   1, 2 and 4 — answer both questions, because "2 is to the right of 1" and "4
   is below 1" are statements a transposition breaks, and "only three texts
   were drawn" is a statement an unfiltered loop breaks.
2. **A cycle that closes and visits everything also does so backwards.** The
   difficulty chip steps Easy, Medium, Hard, Easy. Its test asserted that three
   steps return to the start and that three distinct labels are seen. Both hold
   of Easy, Hard, Medium, Easy. Naming the three steps
   (`assert_eq!(Difficulty::Easy.next(), Difficulty::Medium)`) is what a test of
   an *order* has to say.
3. **Five hundred identical entries have no oldest and no newest.** The undo
   history is capped at 500 and drops from the front. The test made 502 changes
   by toggling one mark on and off, and asserted the depth was 500. Dropping
   from the *back* instead also leaves 500. Two changes fix it: make the count
   odd, so the board's final state records how many toggles survived, and then
   undo the whole history and look — the first toggle fell off the end, so the
   mark is still set, whereas a history that dropped its newest would rewind
   all the way to bare.

**The rule.** *Ask what your fixture is symmetric under, and break that
symmetry.* A test of a mapping needs elements that are distinguishable along
the axis being mapped: distinct values, an odd count, an asymmetric shape, a
non-square grid. This is lesson 54 ("two quantities that happen to be equal")
one level up — there the *numbers* coincided, here the *arrangement* does.

**Where else to look.** Square matrices in a transpose test; a full set where a
subset would do (all flags set, all bits on, every field populated); an even
count where parity is the signal; palindromic or sorted-and-uniform test data;
round-trip tests over a value that is its own inverse; ring buffers filled with
one repeated byte. Any time a test's data is "all of them" or "the same one
many times", it is probably symmetric under the very swap you are trying to
forbid.

### Lesson 60: a layout that shrinks type to fit assumes the renderer will honour the size it asked for (lane C, 2026-08-28)

**In short:** every app in this campaign scales its text with the window, so
that a small window gets small type. It does that by computing a font size as a
fraction of some band's height and passing it to the drawing pass. But the
renderer does not draw at the size you ask for — `gui/font/src/system.rs`'s
`round_px` does `px.clamp(1.0, 512.0).round()`, so a request below one pixel is
drawn *one whole pixel* high. Below that floor the layout's arithmetic and the
screen part company: the layout believes it laid out a tenth-of-a-pixel line,
the renderer draws it thirteen times taller than that, and the text spills out
of the band, over its neighbours, and off the window. 2048's help sheet did
exactly this at a 4x4 window — the rows were sized 0.0849 and drawn about
1.32 pixels high, stacked on top of each other and outside the sheet entirely.

**Why it is easy to miss.** The fault is invisible at every ordinary window
size and only appears when a fraction-of-a-band computation goes sub-pixel,
which needs a window small enough that nobody thinks to test it. It is also
*the opposite* of the failure you brace for: you guard against text being
clipped or unreadably small, and the actual bug is text coming out too
**large**. A `size > 0.0` check — the obvious guard, and the one 2048 had —
does not catch it, because 0.0849 is greater than zero.

**The fix, and where it goes.** Name the renderer's floor as a constant with
the reason attached (`const MIN_DRAWN_FONT: f32 = 1.0;`) and refuse to draw
below it, in *one* place — the single function every string in the program
passes through on its way to the frame. 2048 has `label`, which `centred` and
every button caption call in turn, so one guard covers the program. Putting the
check at each call site instead would be lesson 51's shape: a guard repeated in
front of a guard is a guard no mutation can remove and no test can own.
Refusing is the right answer rather than clamping the size up, because a line
drawn at a size its band cannot hold is not a smaller line, it is a line in the
wrong place; leaving it out is the only outcome the layout's own arithmetic
still describes.

**Where else to look.** Any `(band.h * f).min(cap)` or `font * k` that is not
floored; any layout that keeps shrinking rather than dropping a band when the
window runs out; and anything that treats "the size I asked for" as "the size
that was drawn" when measuring — `text::measure(body, size, weight)` in a test
computes the *asked* size's extent, so a test that measures cannot see this
fault either. The window-size fixture is the thing that catches it: keep a
window in the list small enough to drive at least one computed size under a
pixel (2048's `WINDOWS` has `(4, 4)` for exactly this), and assert that no text
is drawn outside the window it belongs to.

### Lesson 61: the exception you carve out of a rule is the one place a comment does the work of a test (lane C, 2026-08-28)

**In short:** when a sweep establishes a rule — "none of these five drawing
passes needs a `did this band fit?` guard, because every call inside already
refuses an empty box" — the tempting thing is to keep *one* guard and write a
paragraph explaining why that one is different. Connect Four did this twice in
the same afternoon, and both exceptions were wrong: the prose that justified
them was reasoning about a case that could not arise, and prose cannot be run.
An exception argued for in a comment is an assertion nobody checks.

**The two instances.**

- **A guard against a window the layout cannot produce.** Four band guards came
  out of `draw_board`'s siblings under lesson 51. `draw_board`'s stayed, with a
  comment: unlike the others it pushes a `RenderCommand::Line` between the ends
  of the winning four *unconditionally*, so on a board with no room that would
  be a zero-length stroke in the corner of the window. The premise is false.
  The board is the one band the drop ladder cannot take: the padding is capped
  at a quarter of the smaller side, so `width - 2 * pad` is never zero, and the
  ladder empties all four *other* bands before it would eat into the board's
  `BOARD_SHARE`, so the height left over is always positive. There is no window
  size, down to 1x1, at which `l.board.is_empty()`. The comment had been
  standing in for a test for as long as it had existed.
- **An exemption that landed where the rule landed.** The help sheet is modal:
  any intent while it is up shuts it and is swallowed. Two intents — "toggle
  the sheet" and "close the sheet" — were excused from that rule and left to
  fall through to their own match arms, on the reasoning that shutting the
  sheet was what they were going to do anyway. That reasoning is exactly why
  the exemption bought nothing: both arms ended where the guard ends, so no key
  could tell the two paths apart, and all the exemption actually produced was
  two branches below that could never be taken with the sheet up. Deleting the
  exemption turned `ToggleHelp` into an unconditional "open it" and `CloseHelp`
  into an unconditional "there is nothing to close" — three lines shorter and,
  for the first time, reachable by a test.

**How the two smell the same.** In both, the justification has the form *"this
one is different because of what happens over there"* — a claim about another
part of the program, held in a comment, checked by nobody. Mutation testing
finds them from the other end: the guard, or the exemption, is a line whose
removal changes no test's verdict, which is lesson 51's signature. What is new
here is the *shape of the trap*: lesson 51's guards were obvious duplicates
sitting one statement from the check they repeated, whereas these two were
defended, at length, by someone who had already thought about it.

**What to do instead.** If you believe an exception is needed, write the belief
as a test before you write the guard. "The board is drawn in every window
however small" is one sweep over a grid of sizes and it either passes — in
which case the guard is dead and goes — or it fails, in which case you have
found a real layout bug and the guard was never the fix. `connect4`'s
`the_board_is_drawn_in_every_window_however_small` is that test; it is also the
only thing now licensing `draw_board` to go without a guard, which is the
correct place for the licence to live. And prefer a *grid* of sizes to the
project's `WINDOWS` list for this: `WINDOWS` holds sizes each chosen because it
makes some other rule bind, and a claim about every size has to be asked about
sizes nobody picked deliberately.

### Lesson 62: a witness the code under test produced moves with the fault (lane C, 2026-08-28)

**In short:** to check that a program did the right thing you need a fact about
it that came from somewhere else. If your only witness is something the program
itself reported, a fault upstream of the report corrupts the answer *and* the
evidence together, and they go on agreeing with each other. Connect Four's
`ai_turn` chose a column, dropped a piece into it, and returned it; the test
checked that the returned column was the column in the move list. Both come out
of the same two lines. Shifting the played column one to the right between
choosing and dropping failed nothing at all.

**The fault, exactly.**

```rust
let col = ai_best_move(&mut self.board, self.ai_player, self.ai_depth)?;
if !self.drop_at(col) { return None; }
Some(col)                          // <- the test's only witness
```

The mutation inserted `let col = col.saturating_add(1) % COLS;` between the two
lines. Because the new binding shadows the old, the drop and the return both
use the shifted column: the piece lands in the wrong place, `ai_turn` reports
the wrong place, and the two match. Every assertion in the test still held. The
search's actual choice — the one fact that would have disagreed — was never
asked for.

**Why it is easy to write.** The test looks thorough. It asserts the move list
grew by exactly one, that the piece is the AI's colour, that the turn came
back, and that the reported column is the one that was played. Four assertions,
and the *last* one reads like the strong one. It is the weak one: it relates
two outputs of the same computation to each other, which no fault between them
can break.

**The rule.** *Every test needs at least one fact that did not come through the
code path under test.* Ask of each assertion: where did the right-hand side
come from? If the answer is "the function I am testing told me", it is not a
witness, it is an echo. The fix is usually one line — compute the expected
answer independently, before the call:

```rust
let mut board = app.board.clone();
let want = ai_best_move(&mut board, app.ai_player, app.ai_depth).expect("...");
let col = app.ai_turn().expect("...");
assert_eq!(col, want, "the AI played a column the search did not pick");
```

**Where else to look.** Anywhere a function both *acts* and *reports what it
did*: a writer that returns the byte count it wrote (asserted against itself
rather than against the file's length); a scheduler that returns the task it
picked; a parser that returns both a value and the offset it consumed; an
allocator that returns a pointer and a size. The tell is an `assert_eq!` whose
two sides can be traced back to a single call. Related to lesson 58 but not the
same shape: there, two independent *rules* could each explain one witness; here
there is only ever one rule, and the witness is downstream of it.

### Lesson 63: a rule kept only by copying is a rule that will be dropped (lane C, 2026-08-28)

**In short:** a keyboard on this system reports a key twice — once when it goes
down and once when it comes back up — and an application is expected to act only
on the first. Ninety-two applications did. One did not, and it was the one
written most recently, by the author who had just put the same check into six
others in a row. Nothing complained: it compiled, and every test that pressed a
key passed, because no test thought to let one go. The fix is not another
correct copy; it is `scripts/check-key-release-wiring.py`, a gate that fails the
build when a windowed program routes keys and never reads the flag.

**The fault.** `guitk::event::KeyEvent` carries `pressed`, and
`gui/compositor/src/lib.rs` sends one event with `pressed: true` and a second
with `pressed: false` for every keystroke. `apps/connect4`'s `key_intent`
matched on `ev.key` and never looked at `ev.pressed`, so a single press of `1`
dropped the player's piece *and then dropped a second piece into column one on
the release* — which, the turn having passed, was played on the AI's behalf. The
board advanced two moves for one keystroke, and the game shown was not the game
being played. `apps/life` had stepped the generation twice per press for the
same reason, and `apps/maze` had moved the walker two cells.

**Why it is easy to write.** The guard is one `if` at the top of a function
whose *interesting* content is a twenty-arm match, so it reads as boilerplate
and gets skipped by anyone writing the handler from memory rather than copying
the file next door. And it is invisible to everything that would normally catch
a mistake:

* the compiler is happy — `pressed` is a field nobody is obliged to read;
* `clippy` is happy for the same reason;
* the whole test suite is happy, because a test helper called `press` builds
  `pressed: true` and no test builds the other one. A suite of two hundred
  keyboard tests can have complete coverage of *which key does what* and zero
  coverage of *how many times*.

**The rule.** When the same one-line guard has to be written into every member
of a family, stop writing it and write the gate instead. The tell is the count:
if you can say "ninety-two files have this line", the line is a convention, and
a convention with ninety-two instances has no mechanism holding it up — only the
memory of whoever edits next. Conventions at that scale fail silently and fail
on the newest file, because the newest file is the one written without the
others open. A gate turns "everybody remembers" into "nobody has to", and its
baseline should be zero from the day it is written if the population is already
clean; a ratchet is for a mess you inherited, not for one you are still making.

**Where else to look.** Any flag on an event or a message that a handler is free
to ignore: `pressed` here; a `repeat` flag when key auto-repeat lands (the same
fault one step along — an action that runs thirty times while a key is held);
`MouseEventKind::Release` for a handler that only wants presses; a
`compact_boundary` or `is_replay` marker on a stream a client re-reads. In every
case the shape is the same — the field is *available*, ignoring it type-checks,
and the consequence is a doubled action rather than a crash.

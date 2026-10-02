## B-READLINKAT-CANNOT-READ-A-SYMLINK-FD-BECAUSE-O_PATH-IS-UNIMPLEMENTED (lane B, 2026-08-30)

**Status: half fixed, 2026-08-30.** The *observable* half — that an `O_PATH`
descriptor refuses every operation on the file — is implemented and tested
(`posix/src/file.rs` → `mod tests::o_path`, and see `design-decisions.md`
§733 for why only half). What remains is the half that needs the kernel: the
descriptor is still obtained by really opening the file, so it still asks for
read authority the flag is meant to avoid, and a symlink still cannot be named
at all. The original text follows, corrected.

**In short:** `open` accepts the `O_PATH` flag and then ignores it — it used to
ignore it entirely, and now honours it everywhere except where the kernel would
have to help. The flag's whole purpose is to get a descriptor for a *name*
without opening what the name refers to. We now refuse to read, write, seek or
map through such a descriptor, which is what a program can see; but underneath
we did open the file, which is what a program can trip over. One `readlinkat`
branch is still unreachable as a result.

**Where:** `posix/src/file.rs` — `open` (the flag is stored in the fd's status
flags but never reaches `translate_open_flags`, which maps only
READ/WRITE/CREATE/TRUNCATE/APPEND/DIRECTORY/EXCL/NOFOLLOW), and `readlinkat`,
whose comment points here. `O_PATH` itself is defined, correctly, in
`posix/src/fcntl.rs` (0o10_000_000).

**What diverges.** The Linux column is measured on 6.6; the "ours" column is
read off `translate_open_flags`, which is where the flag is still dropped.

| call | Linux | ours |
|---|---|---|
| `read`/`write`/`mmap`/… on an `O_PATH` fd | `EBADF` | `EBADF` ✓ *(fixed 2026-08-30)* |
| `fstat`/`dup`/`close`/`fcntl(F_GETFD)` on one | succeed | succeed ✓ |
| `open(link, O_PATH\|O_NOFOLLOW)` | fd naming the *symlink* | `ELOOP` — `O_NOFOLLOW` is the only one of the two bits that survives, and on its own it means "refuse a symlink" |
| `readlinkat(that fd, "", buf, n)` | the link body | unreachable: the fd above cannot be obtained |
| `readlinkat(fd_to_a_regular_file, "", buf, n)` | `ENOENT` | `ENOENT` ✓ |
| `open(p, O_PATH)` on a path the caller may traverse but not read | succeeds | fails as an ordinary read-open would |

The fifth row is why `readlinkat` is *correct today* rather than merely
untested: no descriptor a caller can obtain from this libc refers to a symlink,
so `ENOENT` is the right answer for every fd that can actually be passed. The
gap is latent, not live — but it becomes live the moment the *kernel* half
lands, and then `readlinkat`'s empty-path branch needs the other half written.

**Why it matters beyond `readlinkat`.** `O_PATH` is the standard way to pin a
directory for the `*at` family without holding it open for I/O — which is
exactly the TOCTOU work in
`B-THE-AT-FAMILY-IS-ONLY-PINNED-FOR-SINGLE-COMPONENT-UNLINK` above. A hardened
`rm -r` or `chmod -R` opens each directory `O_PATH|O_NOFOLLOW|O_DIRECTORY` and
walks with `*at` calls; ours opens them for read instead, which works but asks
for more authority than the operation needs, and fails outright on a directory
the caller may traverse but not read. That last case is the one remaining
*failure* rather than mere over-reach, and it is the reason this entry stays
open.

**What is left**, in order:

1. A kernel-side concept: a handle that names a path without an open file
   description behind it, obtainable without read permission and obtainable for
   a symlink. That is lane A's, and wants a request filed against
   `kernel/src/fs`. The userspace side would then carry it as a distinct
   `HandleKind::Path` rather than as a status-flag bit, which would also let
   `read` refuse it structurally instead of by flag test.
2. Only then: `readlinkat`'s empty-path branch grows the symlink case, and
   `openat`'s `O_NOFOLLOW` stops being the only way to decline to follow.

**Superseded recommendation.** This entry originally proposed, as step 1, that
`open` should return `EOPNOTSUPP` for `O_PATH` — on the precedent of
`O_TMPFILE`, "turning a silent divergence into a loud one". That was wrong and
is withdrawn. The precedent does not transfer: ignoring `O_TMPFILE` does
something actively destructive (it opens the *directory* the caller named,
which is not what was asked for by any reading), whereas ignoring `O_PATH`
merely opens a file that the caller did ask for, with more authority than
needed. And three uses of the flag already work end to end here — as a `dirfd`
for the `*at` family, as an argument to `fstat`, and as an argument to
`fexecve`. `EOPNOTSUPP` would break all three to make the fourth loud.

Until (1), the `*at` pinning work uses `SYS_FS_*_PINNED` handles from ordinary
`open`, which is what §730 describes and is not blocked by this.
### Lesson 93: a box sized by measuring one string and filled with another is a box that fits nothing (lane C, 2026-08-31)

**In short:** `chess`'s layout decided whether an information panel was worth
drawing by measuring the widest line it would hold. The widest line it
measured was `"Arrows/Enter: Navigate"`. The widest line it drew was
`"Arrows/Enter: Move"`. Six characters of column that no text ever occupied,
and -- had the two been the other way round -- a hint clipped in exactly the
narrow window the measurement exists to protect. Nothing failed. The panel
looked right at every size anyone tried, because the error is in the *slack*,
and slack is invisible until it runs out.

**The shape of it:**

```rust
// in Layout::solve
let panel_w_min = text::measure("Arrows/Enter: Navigate", label, Regular)
    .max(...)
    + pad * 2.0;
```

```rust
// in draw_panel, forty lines away
for hint in ["Ctrl+N: New game", "Arrows/Enter: Move", "Esc: Deselect"] {
    text_at(f, hint, ...);
}
```

The two lists were written months apart by the same author. The drawn one was
edited when the keyboard handling changed; the measured one was not, because
nothing points from one to the other. A literal in a `measure` call and a
literal in a `text_at` call are two unrelated facts as far as the compiler,
the tests and the reader are concerned.

**Why the tests did not catch it.** The obvious test -- "the panel is wide
enough for the lines it holds" -- is the one that would have, and it existed
in spirit: the wiring campaign writes exactly that test for every app with
chrome. But writing it against `Layout::solve`'s own constant reproduces the
bug in the test, and writing it against the drawn strings requires the drawn
strings to be *reachable from the test*, which they were not: they were
literals inside a `for` loop in a private drawing method.

**The repair is to make the two lists one list, at module scope:**

```rust
/// The key hints printed at the foot of the panel.
///
/// Named, because the layout has to measure them to decide whether a panel is
/// worth drawing at all, and a string measured in one place and drawn in
/// another is a column sized for a line that is not the line it holds.
const CONTROLS: [&str; 3] = ["Ctrl+N: New game", "Arrows/Enter: Move", "Esc: Deselect"];
```

`Layout::solve` folds `text::measure` over `CONTROLS`; `draw_panel` iterates
`CONTROLS`; the test iterates `CONTROLS`. Editing a hint now moves all three
together, and the test is no longer a restatement of the layout's arithmetic
but a genuine question put to it.

**Test it at every width, not at the default one.** The first version of the
test asked the question of `ChessApp::SIZE` alone and a mutation that replaced
the whole measurement with `let panel_w_min = 20.0;` survived it -- at 900 px
the panel is 28% of the window and comfortably wide either way. The
measurement only decides anything in the windows near the limit, which are
precisely the windows a single-size test does not visit. The test now sweeps
280..=1600 in steps of 20 at three heights, skips the sizes that drop the
panel, and asserts it checked more than fifty of them. That last assertion
matters: without it a layout change that dropped the panel everywhere would
turn the test into a loop that runs zero times and passes.

**Where else to look.** Any `text::measure` whose argument is a literal. The
question to ask is not "is this the right width" but "is this literal the
same object as the one that gets drawn". Sites where the answer is "there is
only one literal, and the layout reads it" are fine; sites where the answer
requires comparing two files are the fault. A grep for `measure("` across
`gui/` and `apps/` finds them; run on 2026-08-31 it returns about twenty
production sites, and the first one checked was already wrong:

`gomoku` (`apps/gomoku/src/main.rs:135`) sizes its panel from

```rust
let panel_w_min =
    text::measure("Draws: 88", font, FontWeightHint::Regular).max(font * 6.0) + pad * 2.0;
```

and the panel draws `"\u{25CF} Black: {n}"`, `"\u{25CB} White: {n}"`,
`"Draws: {n}"`, `"New game (N)"` and `"Undo (Z)"`. `"New game (N)"` is wider
than `"Draws: 88"`, so the line that decides the width is not the line that
was measured; the `.max(font * 6.0)` is a fudge factor standing in for the
measurement that was not taken. `"Draws: 88"` also caps the draw count at two
digits. Same repair as above: one named list, measured and drawn from it,
tested across widths.

Filed as its own task -- **audit the `measure("` sites in every wired app** --
rather than fixed in passing, because the fix is per-app and each one wants
its own width-swept test. The rest of the audit is written up in `todo.txt`,
and it sharpens the rule worth carrying forward: **the fault appears when the
`measure` and the `text_at` are in different functions.** Where the two
literals sit a few lines apart in one body -- `life`, `defrag`, `sokoban`,
`yahtzee`, `speedtest`, `renamer` -- a reader edits them together and every
such pair in the tree agrees today. Where they are a layout function and a
drawing function, as in `chess` and `gomoku`, they drift. `snake` and
`stickynotes` are in the second shape and merely happen to be right.

**Postscript (same day): the fudge factor beside the wrong string was what
made the wrong string harmless, and that is not a reason to leave either.**
All four sites are repaired now -- `gomoku` has `PANEL_HEADINGS`,
`PANEL_LINES` and `PANEL_BUTTONS` measured at the size *and weight* each is
drawn at, `snake` has `STATS_HEADING`, `stickynotes` has `PIN_PREFIX` -- and
`gomoku`'s repair taught something `chess`'s did not. Its shipped line was
`measure("Draws: 88", font, Regular).max(font * 6.0)`, and a mutation that
restores the whole of it *survives* the new width-swept test. Measured: the
fudge is `6.00 * font`, the widest line the panel really draws is
`5.31 * font`, and `"Draws: 88"` is `4.27 * font`. So the `.max` won at every
window and the panel was never actually too narrow -- one wrong number was
covering for another. The mutation that *is* caught is that measurement with
the fudge removed, which leaves the widest line 17 px short of the column at
280x400 and at 400x640.

Two things follow. First, a site like this cannot be found by testing, only
by reading -- which is why the grep is the tool here and the suite is not.
Second, and this is the part worth arguing with: "the fudge happens to be big
enough" is no defence of leaving it, because *nothing holds it there*. It is
a number tuned against a set of strings, sitting forty lines from those
strings, with no test able to notice when they change and no reader able to
see that it should. That is the same fault as the wrong literal, one level
up: a measurement replaced by a constant that agreed with it once. The repair
is the same either way -- measure the strings you draw -- and the surviving
row stays in the mutation table carrying its reason, rather than being
deleted (lesson 94).

### Lesson 94: a branch the current constant makes unreachable is a landmine, not dead code (lane C, 2026-08-31)

**In short:** `chess`'s minimax scored a leaf position from White's point of
view when it was White's turn to choose and from Black's when it was Black's
-- while the checkmate scores twelve lines below it were White-relative in
both cases. Two different scales, compared against each other. The program
nonetheless played correct chess for its whole life, because the only caller
searches an even number of plies from a maximising root, so every leaf it
reaches is a maximising one and the wrong arm is never taken. Change
`AI_DEPTH` from 3 to 4 and the engine starts preferring blunders.

**The code:**

```rust
fn minimax(board: &Board, depth: i32, ..., maximizing: bool) -> i32 {
    if depth <= 0 {
        let eval = evaluate(board);          // always White-relative
        return if maximizing { eval } else { eval.saturating_neg() };
    }
    ...
            return if maximizing {           // and these are not negated
                KING_VALUE.saturating_add(depth).saturating_neg()
            } else {
                KING_VALUE.saturating_add(depth)
            };
```

This is half a negamax bolted onto a minimax. Either convention is fine; the
two together are not, and the tell is that the two `if maximizing` blocks in
one function disagree about what the returned number means.

**How it surfaced.** Only through mutation. A row that deleted the negation
entirely -- `return eval;` -- survived the full suite, which is the sweep's
way of saying *no input to this program can tell the two apart*. That is the
same signal as lesson 92's duplicated condition and it deserves the same
response: do not shrug and delete the row. Ask why the code cannot be reached,
and the answer here was a parity relationship between a constant and a root
flag, holding by luck across two functions that do not mention each other.

**The repair is to remove the negation, not to test it.** The leaf now returns
`evaluate(board)` unconditionally, which is what the mate scores already
assumed, and the doc comment says so in the first line: *"Every score this
returns is from White's point of view; `maximizing` says which side is
choosing at this node, not which side the number is about."* A test asserts
`minimax(board, 0, .., true) == minimax(board, 0, .., false) == evaluate(board)`
so the two conventions cannot drift apart again.

**Where else to look.** Search-like code where a flag and a depth constant
interact: anything of the form `if maximizing`/`if color`/`side_sign` inside a
function whose recursion alternates the flag. The question is whether *every*
value of the flag is reachable at *every* return site, and the cheapest way to
find out is to mutate each arm and see whether anything notices. More
generally: when a mutation survives, the finding is never "the sweep row was
bad" until you have established that the branch is reachable at all.

### Lesson 95: a scrolling list whose limit counts one kind of row and whose drawing emits another cannot reach its end (lane C, 2026-08-31)

**In short:** `crossword`'s clue panel scrolls by rows. Its scroll limit was
`clues.len() - rows_that_fit`, and its drawing pass emitted, among the clues,
two direction headings -- "Across" and "Down" -- that the limit had never
counted. So the list was two rows longer than anything scrolled for, and at
the bottom of the scroll the last clue of every puzzle was pushed off the
panel with no scroll position left that would bring it back. A player could
see nine of ten clues and had no way to reach the tenth.

**The code:**

```rust
fn max_scroll(&self) -> usize {
    self.clues.len().saturating_sub(self.layout().clue_rows_visible())
}

fn draw_panel(&self, ..) {
    let first = self.clue_scroll.min(self.max_scroll());
    // a heading before the loop ...
    if let Some(clue) = self.clues.get(first) { text_at(.., clue.direction.label(), ..); }
    y += row_h;
    for (i, clue) in self.clues.iter().enumerate().skip(first).take(visible) {
        if heading != Some(clue.direction) {
            // ... and a second copy of it inside, spending a row nobody budgeted
            text_at(f, l.panel.x, y, clue.direction.label(), ..);
            y += row_h;
        }
        ...
    }
}
```

Two separate faults, both of the same family. The heading-drawing code exists
**twice** -- once before the loop for the sticky heading and once inside it for
the transition -- which is lesson 92 (a rule written twice), and the copy
inside the loop is the one that spends an uncounted row. And the *unit* of the
scroll differs between the two functions that have to agree about it: one
counts clues, the other draws rows.

**Why nothing caught it for the length of the wiring turn.** The default
window, 860x580, has a panel tall enough for all ten clues plus both headings,
so `max_scroll()` is zero and the panel is never scrolled at all. Every
scrolling test written against `Crossword::SIZE` therefore passed against a
program whose wheel did nothing, because there was nothing for the wheel to do
-- lesson 90 exactly, four tests at once. The bug only appeared once the tests
were pointed at a deliberately short window (`SHORT = (900.0, 200.0)`) with a
guard test, `the_short_window_really_is_too_short_for_the_clue_list`, asserting
the fixture is in the regime the rule governs.

**The repair is to make the drawn row the unit of the arithmetic.** A
`PanelRow` enum -- `Heading(Direction)` or `Clue(usize)` -- and one
`panel_rows()` that produces the whole list in draw order. `max_scroll` counts
that list; `draw_panel` iterates it and does no heading bookkeeping of its own;
`show_clue` converts a clue index to a row index before scrolling. The
pre-loop copy is gone, so there is one place a heading is drawn and it is a row
like any other.

Two tests hold it: `a_heading_is_a_row_of_the_list_it_heads` (the model) and
`the_panel_draws_every_row_it_has_room_for_and_no_more` (the drawing agrees
with the model at every scroll position). `every_clue_can_be_scrolled_onto_the_panel`
is the one that found it.

**A visible consequence worth keeping.** Once headings scroll, the heading that
says which half of the list a row belongs to is frequently not on the screen
with it -- and 7 Across and 7 Down are both "7". The row label carries the
direction now: `7A.`, `7D.`. A scrolling list whose group headings can leave
the viewport needs its rows to be self-identifying, or the sticky-heading
machinery that was the original (duplicated) intent has to be done properly.
Carrying it in the label is the cheaper half and does not need a second copy of
anything.

**Where else to look.** Any pair of the form "a `max_scroll`/`page_count`
computed from a collection's length" and "a drawing loop that emits rows the
collection does not contain." Grep for `.len().saturating_sub(visible)` and
then read the drawing pass for anything that advances `y` outside the per-item
step: separators, group headings, sticky rows, spacers, "load more" rows,
wrapped items that take two lines. The question to ask is *what is the unit of
`clue_scroll`* -- and if the answer is "an item" in one function and "a row" in
another, the list has an unreachable end. The cheap structural fix is always
the same: materialise the row list, and let both functions read it.

More generally, this is a third instance of the same shape as lessons 91 and
93: a quantity computed by one piece of code and consumed by another, where the
two disagree not about the *value* but about the *unit*. A frame that says a
needle twice, a box measured with one string and filled with another, a list
counted in items and drawn in rows.

### Lesson 96: a whole-frame text search is not a test of the widget you meant, when two widgets draw the same string (lane C, 2026-08-31)

**In short:** `crossword` draws the clue for the word under the cursor twice --
once in the banner across the top, once as a row of the scrolling clue panel.
`a_clue_with_an_accent_in_it_is_drawn_rather_than_aborting` put an accented
clue in the puzzle and asserted that the frame contained `piñata` *somewhere*.
Mutating the panel to cut its clue at a byte offset -- the exact fault the test
was written to prevent, and one that aborts the process on a real cut -- left
the test green, because the banner was still drawing the whole string.

**The code:**

```rust
let drawn = painted_text(&app, (w, h));   // every Text command in the frame
assert!(
    drawn.iter().any(|(s, _)| s.contains("piñata"))
        || Layout::solve(w, h, app.width, app.height).panel.is_empty(),
    "the accented clue has to reach the renderer whole at {w}x{h}"
);
```

The `||` names the panel, so the author knew perfectly well which widget the
claim was about. The assertion just never restricted itself to that widget:
`any(|s| s.contains(..))` searches every string in the frame, and one of the
other strings in the frame is the same clue drawn by a different code path.

**A second test in the same file had the mirror-image weakness.**
`a_clue_is_handed_to_the_renderer_whole_and_bounded_by_width` built each clue's
expected text, looked it up with `.find(..)`, and `continue`d when it was not
there -- so a clue the panel had cut passed the test *by being absent*. A
lookup that skips what it cannot find asserts nothing about the missing case,
which is the only case the test exists for.

**The repair is to assert over the set of strings the widget emits, not the
presence of one of them.** A panel row is exactly `<number><A|D>. <text>`, and
the banner's format is `"{} {} ({}): {}"`, so the two are distinguishable by
shape. Both tests now enumerate every painted string of the row shape and
require each to be some clue of this puzzle *whole* -- with a guard that at
least one such row was drawn, since an enumeration over nothing is vacuous.

**Where else to look.** Any test whose assertion is `contains`,
`any(|s| s.contains(..))`, `find(..).is_some()`, or that `continue`s past items
it could not find. Two questions: *which widget is this claim about*, and *does
anything else in the frame draw the same text?* In a GUI the answer to the
second is very often yes, because showing the same fact in two places is a
feature -- a banner and a list, a title bar and a tab, a status line and the
thing whose status it reports, a tooltip and its label. A search finds the copy
the *other* widget drew. An enumeration of one widget's output cannot.

This is adjacent to lesson 90 but distinct from it. There the fixture never
enters the regime the rule governs; here the fixture is in exactly the right
regime, and the assertion is satisfied by evidence from somewhere else in the
picture.

### Lesson 97: a layout invariant sampled at a handful of window sizes is not tested; the sizes that break it are exactly the ones nobody would list (lane C, 2026-08-31)

**In short:** every wired app carries a `SIZES` array -- half a dozen window
sizes a test loops over -- and the layout claims loop over it: the panes stack,
the widgets stay inside their panes, nothing runs off an edge. `hearts` had six
such sizes and 84 tests. Its mutation sweep then broke four separate layout
guards and *none* of the four was caught, because a layout rule is a property
of `Layout::solve` at **every** size, and the sizes that violate one are the
awkward ones -- which is precisely why they are not in a hand-written list of
plausible windows.

**The four guards, and the size that reaches each:**

| Guard removed | Reaches it | What the six sampled sizes saw |
|---|---|---|
| `card_w` capped at `MAX_CARD_W` | 1200x900 | nothing: the card is capped only when the window is *both* wide and tall, and a bigger card still passes every containment claim |
| `hand_h` pays for the footer first | 400x57 | nothing: the guard binds only when `free_h < 25.6`, a window about 57 pixels tall |
| the fourth button is dropped when it will not fit | 60 wide | nothing: four buttons need about 110 pixels and the narrowest sampled window was 360 |
| a seat's label is clamped onto the felt | 20 wide | nothing: algebra says the clamp binds only when `pad > 2w/11`, hence only below about 22 pixels of width |

Every one of those numbers came from *solving the guard's own condition*, not
from guessing. That is the tell: if you can write down the inequality under
which a clamp binds, you can write down a window that satisfies it, and if no
size in `SIZES` satisfies it then the clamp is untested no matter how many
assertions loop over `SIZES`.

**The repair is to sweep, not to sample.** A claim about `Layout::solve` costs
microseconds per size, so there is no reason to check six:

```rust
const GRID_W: [f32; 7] = [0.0, 20.0, 60.0, 120.0, 360.0, 900.0, 1400.0];
const GRID_H: [f32; 6] = [0.0, 57.0, 120.0, 280.0, 620.0, 900.0];
```

Forty-two sizes, chosen so that each degenerate regime is represented: zero,
smaller than one widget, smaller than the row of widgets, ordinary, and larger
than anything the layout wants. All four mutations are caught now, and the
grid's doc comment names which size exists for which fault, so nobody deletes
`57.0` for looking arbitrary.

**One of the four was a real fault, not a coverage gap.** At 20 pixels wide the
clamp that keeps a seat label on the felt puts the label on top of the very
card it names -- the clamp cannot satisfy both constraints, and silently chose
the wrong one. The fix is the answer the scoreboard and the footer in the same
program already gave: a thing that cannot be drawn properly is **left out**,
not squeezed. Sweeping the grid is what turned an unreachable-looking defensive
clamp into a visible bug.

**Where else to look.** Every app in the wiring campaign with a `SIZES`
constant -- which is all of them. Two questions per layout guard: *under what
inequality does this clamp/`min`/`max`/`break` actually bind*, and *is there a
size in the list that satisfies it?* A guard whose condition no sampled size
reaches is in the same position as lesson 94's unreachable branch: it looks
like defensive code, it is never executed by the suite, and nobody finds out
which way it fails until a user resizes a window. Prefer a swept grid for any
claim that is a pure function of the window; keep `SIZES` for claims that need
a *game* as well as a window, where building the fixture is the expensive part.

### Lesson 98: a text-bounding fix applied site by site is not a fix; the claim belongs to the frame, not to the call site (lane C, 2026-08-31)

**In short:** `spades` was wired with fault (7) of its roadmap entry -- every
string drawn with `max_width: None`, so on a narrow window the text simply kept
going past the edge -- and the rewrite fixed it by putting each string through a
`bounded()` helper. The grid sweep then failed on `"Spades"` running off a 20x57
window at `x 4 + 43.018066`. The fix had been applied at the sites the author
was thinking about, and a frame has forty of them; the two the author was not
thinking about were the header title and the card corners.

**Why site-by-site loses.** `max_width: None` is the *default* shape of a text
command, so every call site starts unbounded and must be individually converted.
That makes the property "no text leaves the window" a conjunction over forty
independent edits, and a conjunction over forty edits is not a property you can
hold -- it is a property you can only re-audit, once per new string, for ever.
Worse, the audit has no failing test to anchor it: the app's status-line test
(`the_status_line_is_bounded_and_never_runs_off_the_edge`) passed the whole
time, because it looked at the status line.

**Two repairs, and both are needed.**

*Make the claim whole-frame.* The test now walks **every** `RenderCommand::Text`
in the frame, at all 42 grid sizes, and fails if any run's measured width takes
it past the window edge. It was renamed to say so --
`no_text_runs_off_the_window_it_is_drawn_in` -- and a narrower
`the_status_line_is_elided_rather_than_cut_off` was kept beside it, so the
whole-frame claim does not quietly become the only owner of the status line's
own behaviour (lesson 96).

*Make the helpers incapable of producing an unbounded run.* `centred` used to
compute `x + (w - measured) / 2.0` and pass the string on with the caller's
width. When `measured > w` that offset is **negative**, so an over-long run was
centred by hanging off *both* edges at once -- a bound on the right does nothing
about the left. It now clamps the offset at zero and bounds to the width it
centres in, so a run too wide to centre starts at the left edge and is elided at
the right one:

```rust
let measured = text::measure(s, size, weight);
bounded(f, (x + ((w - measured) / 2.0).max(0.0), y), w, s, color, size, weight);
```

That converts "no text leaves the window" from forty facts into two: every
string goes through `bounded` or `centred`, and both of those are correct.

**Where else to look.** Every app in the wiring campaign. Two greps and one
test: `max_width: None` and `RenderCommand::Text {` outside the helpers find the
sites that bypassed the helper; the whole-frame assertion above is cheap to copy
and is the only thing that keeps them from coming back. Note that the generic
form of this lesson is not about text -- it is that a defect described as "every
X is wrong" must be repaired by a change that makes a *wrong X unrepresentable*
or by a test that quantifies over all X. Fixing the instances you can find is
fixing the symptom; the roadmap entry for `spades` lists the same defect twice,
as fault (7) and fault (15), for exactly that reason.

### Lesson 99: a whole-frame invariant is only as wide as the states you draw; one fixture is a sample, not a sweep (lane C, 2026-08-31)

**In short:** lesson 97 says a layout claim checked at six window sizes is
untested, and the repair is to sweep a grid of sizes. Lesson 98 says a
per-call-site fix is untested, and the repair is to quantify the claim over the
whole frame. `spades` did both -- 42 sizes, every `RenderCommand::Text` in the
frame -- and the mutation sweep still walked a broken `centred` straight past
it, because the test drew **one game**. A playing hand draws no centred run at
all: the three that exist are on the bid pad, the help card and the message
across an empty felt, and the fixture was in none of those states.

**The two faults that were hiding behind the single fixture.** Both are in the
one helper the whole picture funnels through, so neither is obscure:

| Fault | What it did | Why the frame sweep could not see it |
|---|---|---|
| the bound was the whole box, measured from the *centred* start | `centred(pad.x, pad.w, ...)` emitted `x = 144.6, max_width = 300.9` in a 360-pixel window -- a claim reaching 85 pixels past the pad's own right edge | no `centred` call happens in a playing hand |
| the offset was not floored at zero | a run too wide to centre got a negative offset and hung off **both** edges | same, plus the right-edge check alone cannot see it: `x + w` lands level with the box either way |

**Two repairs, and the second is the general one.**

*Draw more than one state.* The test now builds four -- playing with an
over-long status, bidding with the pad up, playing with the help card open, and
a round played out so the felt carries a message -- and asserts over all 42
sizes for each. That is 168 frames and it costs under two seconds.

*Give the helper its own test.* A helper that every drawing site funnels
through is the highest-leverage thing in the file and the easiest to test
directly: `a_centred_run_stays_inside_the_box_it_is_centred_in` calls `centred`
into a bare `Frame`, once with a run that fits and once with a run that cannot,
and reads the command back. It owns both halves of the fault. A whole-frame
assertion can only see a helper through whatever the application happens to
ask it to draw; a direct test sees all of it.

**The state dimension is exactly as adversarial as the size dimension.** Lesson
97's rule was "solve the guard's own condition, then pick a window that
satisfies it." The same question works here: *which application state reaches
this code at all?* A drawing routine guarded by `if self.show_help`, by a phase
match, or by `if trick.cards.is_empty()` is unreached by a fixture that is not
in that state, and no amount of sweeping the window size will enter it.

**The same sweep also found a clamp no window could reach.** `hand_h` was
`(card_h * HAND_STRIP_SLACK).min(strip_h)` while `card_w` was already capped at
`strip_h / (CARD_ASPECT * HAND_STRIP_SLACK)` -- so the `.min` was arithmetically
unreachable, and striking it out changed nothing anywhere. It is lesson 94's
unreachable branch wearing a `min`: a clamp that cannot bind is not a
safeguard, it is a line that takes the credit for the cap that is actually
holding the invariant, and it hides which one that is. It has been removed and
the row now cuts the real guard.

**Where else to look.** Every app in the wiring campaign whose whole-frame
assertions use a single fixture -- which is most of them, because the fixture
helper (`playing_game()`, `open_document()`, and so on) was written to build the
*ordinary* state. Two questions per app: *which of my drawing routines are
behind a state guard*, and *does any fixture enter it?* Prefer a list of
fixtures over one, and prefer a direct test of a shared helper over inferring
its behaviour from the pictures its callers happen to produce.

### Lesson 100: when a true invariant fires on a legitimate case, split it -- do not dilute it, or you trade a false alarm for a silent hole (lane C, 2026-08-31)

**In short:** `automator` had a test saying *a click at the centre of a hit box
reaches the control that box belongs to*. That is the property you actually
want. It also fails honestly when the help card is open, because the card is
painted over the controls and is *supposed* to swallow the click. I repaired it
by restating the claim in terms of paint order -- the topmost box containing the
point wins -- which made the help card legal and made the test true. It was
true because `Frame::hit_test` returns the last-painted match **by
construction**: the reformulated test could no longer fail. Four mutations that
move a hit box away from the rectangle its ink is drawn at walked straight
through it.

**The shape of the mistake.** A test asserted `A and B` where `A` (a hit box
coincides with its ink) is the thing being tested and `B` (nothing legitimate
is painted on top) is an assumption. `B` broke. The cheap repair weakens the
conjunction until it passes; what survives is `B`-shaped and `A` is gone, and
nothing in the suite says so, because the file still contains a test with `A`'s
name on it. A weakened assertion is worse than a deleted one -- a deleted test
leaves a gap somebody can see.

**The tell: could this assertion fail at all?** Ask it of every invariant
phrased in terms the implementation guarantees. "The topmost hit box wins the
hit test" is a restatement of `hit_test`'s own loop; "the click reaches the
control whose *ink* is under the point" is a claim about two independent things
(where `Frame::hit` was called and where `FillRect` was emitted) and can be
false. If the mutation you can imagine cannot break the assertion, the
assertion is not the one you meant to write.

**The repair is two tests, not one.** Split the conjunction along its seam:

| Test | Claim | What it catches |
|---|---|---|
| `every_hit_box_has_ink_painted_at_exactly_that_rectangle` | every `frame.hits()` entry has a `FillRect` matching all four fields to 0.01 | a box translated, resized or emitted for something never drawn |
| `every_hit_box_lies_in_the_band_of_the_pane_that_owns_it` | a `Macro` box is inside the sidebar body, an `Action` box inside the list body, a `Speed`/`Repeat` pad inside the pads strip, and so on | a box in the right *place* but belonging to the wrong panel -- which the ink test alone cannot see |

Neither mentions occlusion, so the help card is not a special case in either;
the case that broke the original claim simply is not part of what they say.

**Where else to look.** Any test in the campaign whose name promises a
behaviour but whose body was later loosened to accommodate a state that
legitimately violates it -- overlays, modals, disabled controls, empty lists.
Two questions: *what conjunct did the loosening drop*, and *is anything else in
the suite still asserting it?* If the answer to the second is no, the loosening
was a deletion in disguise.

### Lesson 101: a clamp is only as strong as the directions it corrects and the paths that call it (lane C, 2026-08-31)

**In short:** `automator`'s `clamp_scrolls` put a scroll offset back in range and
pulled it up so the selection was not above the window. Two mutation rows found
that it did not pull the offset **down** when the selection walked off the
bottom, and that a **click** never called it at all. Both are the same fault
seen twice: the code that restores an invariant was written against the cases
the tests happened to walk, not against the invariant.

| Fault | What a user saw | Why the suite was quiet |
|---|---|---|
| the offset followed the selection up but never down | holding <kbd>Down</kbd> walked the selection past the last visible row while the list sat still | the arrow-key test only ever walked the selection *upwards*, so the down half of the clamp was never asked for |
| `click` did not re-clamp | clicking a 40-action macro, scrolling to the bottom, then clicking a 1-action macro left the list scrolled 30 rows past its only row -- blank | the button and key paths both clamped; the click path was the one way in that did not, and no test clicked *after* scrolling |

**Two rules, one for each half.**

*State the clamp as a two-sided containment and implement both sides.* "The
selection is visible" is `offset <= i < offset + rows`. Code that only ever
lowers `offset` implements the left inequality. The right one needs
`offset = i + 1 - rows`, and the test needs to move the selection in the
direction that reaches it -- which means a scrolling test that only walks one
way is testing one inequality.

*Give the invariant one funnel and route every entry point through it.* The
repair was not "add a clamp call to the click arm"; it was to rename the body
`click_inner` and make `click` a wrapper that calls it and then clamps. A new
target added next month cannot forget. The general form: when an invariant must
hold after *any* of N handlers, the handlers should not each be responsible for
it -- one of them will be added without it, and that one will not be the one you
test.

**Where else to look.** Every app in the campaign with a scroll offset (this is
most of the list-shaped ones: `filemanager`, `logviewer`, `procexplorer`,
`notes`, ...). Three questions: *does the clamp move the offset both ways*,
*does the scrolling test walk the selection both ways*, and *is there a path in
-- a click, a tick, an external state change -- that mutates the list without
passing through the clamp?* The third is the one that bites, because the state
change and the clamp are usually in different files.

### Lesson 102: test the entry point the platform calls, not the one underneath it (lane C, 2026-08-31)

**In short:** the `terminal` had a test saying *a tick is what reads the child*
-- the thing that makes a shell's prompt appear. It passed, and the terminal was
still write-only under a real window. The test called `handle_event`, the
terminal's own dispatcher. The compositor calls `App::on_event`, which is a
layer above it, and that layer answered a tick itself and returned before
`handle_event` was ever reached.

The reason `on_event` did that was not laziness. It has to return a `Response`
-- `Redraw` or `Idle` -- and a tick that changed nothing must not ask for a
frame, or the terminal redraws twenty-five times a second at an idle prompt. So
it needed the answer `tick` returns. Reaching for it directly was the obvious
way to get it, and it silently bypassed everything else the tick did.

| Layer | Who calls it | What the suite called |
|---|---|---|
| `TerminalState::handle_event` | the app's own `on_event`, and every test | ✓ eleven tests |
| `App::on_event` | the compositor, and nothing else | ✗ nothing, until this one |

**The rule.** *For each trait a type implements for a platform, at least one
test must enter through the trait.* An `impl App` is not documentation; it is
the only code the compositor runs. A suite that tests the inherent methods and
trusts the impl to forward to them is testing a program the user never runs. The
same holds for `impl Probe`, for `Iterator`, for `Drop` -- any impl whose caller
is not in your own crate.

**The shape of the bug, generally: a wrapper that needs one fact from the body,
and takes it by calling a part of the body directly.** The fix is to let the
body run in full and have it *report* the fact -- here `on_tick` does the ageing
and the read and leaves the answer in `tick_changed`, which `on_event` then
reads. The wrapper is no longer allowed to choose which half of the body runs.

**Where else to look.** Every app in the wiring campaign: each has an `impl App`
with `on_event` and `render`, and the suites overwhelmingly call the inherent
`click`/`key`/`frame` beneath them. Two questions per app: *does any test call
`on_event` or `render` by those names*, and *does `on_event` have an early
return above the call to the real dispatcher?* An early return for
`CloseRequested` is fine -- there is nothing below it to skip. An early return
that computes something first is the fault.

A second instance of the same family was in the terminal's other direction, and
is worth recording because it looks nothing like the first: `output_buffer` had
**two writers and one dead end**. Keystrokes were written straight to the PTY by
`to_child`, while the parser's own replies -- the cursor position report, the
device attributes answer -- were appended to `output_buffer`, which nothing ever
drained. A full-screen program that asked the terminal where its cursor was
waited for an answer sitting in a `Vec`. Two ways *out* of a subsystem, one of
which was never finished, is the same mistake as two ways *in*, one of which is
never tested. The repair is lesson 101's funnel: `to_child` only queues, and
`flush_to_child` is the one route out, reached from every arm of the dispatch.

---

### Lesson 103: a test that asks the predicate cannot see which argument the drawing pass gave it (lane C, 2026-08-31)

**In short:** the terminal highlights the text you drag over, and paints its
cursor only in the lit half of the blink. Two tests said so, both passed, and
the mutation sweep broke each feature outright without either test noticing.
Both asked the thing that *decides* -- `is_selected(...)`, and the `blink_on`
field -- instead of looking at the picture. The drawing pass was left free to
call that decider with the wrong row, or never call it at all, and every
assertion stayed true.

| What the user sees | What the test asked | What it could not see |
|---|---|---|
| the highlight is on the line you dragged over | `term.is_selected(buffer_row, 0)` | which row `draw_cells` passes it -- the screen row or the buffer row |
| the cursor blinks | `term.blink_on` after a tick | whether `draw_cursor` consults `blink_on` at all |

Both faults were in the *call*, not in the function: `is_selected(screen_row,
col)` where the buffer row was meant, and a `draw_cursor` that painted
regardless. A predicate test pins the predicate, which is worth having -- but
the call site is where the argument is chosen, and no amount of predicate
testing reaches it.

**The rule.** *For any state that reaches the user only through the picture, at
least one test must read the frame.* Concretely: scan
`frame(w, h).commands()` for the ink -- the selection-coloured `FillRect`, the
cursor-coloured one -- and assert where it is, or that it is absent.

**How to tell which kind of test you are writing.** Ask what would still be true
if the drawing pass never called this function at all. If the answer is
"everything this test asserts", the test is about a helper, not about a feature.

**A third survivor in the same sweep is this blindness in a different costume,**
and is worth naming because it looks like a *whole-frame* test, which is the
strong kind: `no_glyph_runs_off_the_window_it_is_drawn_in` bounded every `Text`
command by `max_width.unwrap_or_else(|| text::measure(glyph, ..))`. Deleting the
bound from the production code left the test computing a plausible one on its
behalf -- and one character measured is about one cell wide by definition, so
an unbounded glyph looked perfectly bounded. When an invariant is about what a
command *declares*, a computed stand-in for the missing declaration is not a
convenience, it is the hole (lesson 100's dilution, arriving through a
`unwrap_or_else` instead of through a weakened assertion). The fix was to fail
on the `None`.

**Where else to look.** Every app in the wiring campaign: any `is_*`/`should_*`
predicate the drawing pass consults per item (selected, highlighted, hovered,
disabled, checked, filtered, expanded) and any field whose only effect is
whether something is painted (blink, flash, focus ring, unread badge). Grep the
suite for the predicate's name: if every use is `assert!(x.is_selected(..))` and
none is a scan of `frame(..).commands()`, the picture is untested. And grep the
whole-frame tests for `unwrap_or`/`unwrap_or_else`/`unwrap_or_default` on a
field read out of a `RenderCommand` -- each one is a declaration the test has
agreed to supply for the code.

---

### Lesson 104: finding a control by the code's own label and then clicking it proves nothing (lane C, 2026-08-31)

**In short:** the camera's device panel lists the resolutions a webcam
supports, one per row, and clicking a row selects it. The test said so, and it
passed over a version of the panel where **every row chose the setting named on
the row above it**. The test asked the drawing pass "where is the 1920x1080
row?", clicked wherever it was told, and then asked whether 1920x1080 had been
chosen. Both halves went through the same map, so the map's error cancelled
itself out.

This is not lesson 103 -- the test *did* read the picture. It read the wrong
part of it. `probe::rect_of(&app, Target::Resolution(2))` searches the hit
boxes the pass recorded, and the fault under test was in *what payload the pass
recorded*, not in where. Relabelling every row consistently is invisible to any
test whose only handle on a row is the label it is checking:

| | what the test did | what the mutant did | verdict |
|---|---|---|---|
| find | ask for the box of `Resolution(2)` | recorded row 3's box under `Resolution(2)` | got row 3 |
| act | click that box | dispatched it as `Resolution(2)` | chose 1920x1080 |
| assert | is the resolution 1920x1080? | yes | **passes** |

The user, meanwhile, clicks the row that reads `1920x1080` and gets `1280x720`.

**The rule.** *When the point of the test is **which** control a click lands
on, find the control by something the code did not choose for the purpose --
the words drawn in it.* Scan `frame(w, h).commands()` for the `Text` whose
content is the label, click at a point inside it, and assert on the label. The
`Target` payload is the very thing in question; a test may not use it as both
the question and the answer.

**The shape to look for is circularity, not weakness.** These tests are not
loose -- `the_device_panel_chooses_the_resolution_and_rate_it_names` asserts an
exact equality on an exact setting. They are *closed loops*: every step of the
test is derived from the same expression in the production code, so the whole
test is invariant under a change to that expression. A mutation sweep is the
only cheap way to find one, because a closed loop reads exactly like a strict
test.

**Two smaller consequences, both worth copying.**

- **Locate by text, and require the text to be unique.** The camera's helper
  asserts that at most one run of text reads the wanted words, because the same
  string appearing twice in a frame would silently hand the caller whichever
  came first -- a second closed loop hiding inside the fix for the first.
- **A mutation that survives is not automatically a hole.** The same sweep's
  other survivor widened a clip from the strip to the whole window, which
  cannot alter one pixel: the tile count is a floor of whole steps, so nothing
  ever reaches the clip. That row was deleted with the reason written into the
  table rather than given an owning test. Inventing an owner for a mutation
  that cannot change the output teaches the table to claim coverage it does not
  have, which is worse than the missing row.

**Where else to look.** Every app in the wiring campaign, and mechanically:
grep the suites for `probe::click(&mut app, Target::X(i))` (or `rect_of`
followed by a click) where `i` also appears in the assertion. Each one is a
closed loop unless something outside the pass's own bookkeeping -- the drawn
label, a fixed coordinate, a count -- breaks it. The panels most at risk are
the ones drawn from a list in a loop: filter lists, resolution and rate lists,
tab strips, column headers, palette swatches, and any `Target` variant carrying
an index or an enum payload.

---

### Lesson 105: a probe placed at a symmetry of the thing under test cannot see the symmetry break (lane C, 2026-08-31)

**In short:** the compass turns when you press its rose — press up and to the
right, and the needle points north-east. The test pressed at exactly 45°,
checked the compass read 45, and passed over a rose that measured its angle
**from east instead of from north**. 45° is precisely the bearing where those
two conventions agree: `atan2(dx, dy)` and `atan2(dy, dx)` return the same
number when `dx == dy`. The test had picked the one point on the circle that
cannot tell the two apart.

The mutation was `dx.atan2(dy)` → `dy.atan2(dx)`, which turns every bearing
into its complement: 20° reads 70°, 10° reads 80°, 120° reads −30°. The whole
face is wrong. One press, at one angle, saw none of it.

**The rule.** *When a test probes a function at a single input, ask what
symmetries that input sits on — and move off them.* The convenient input is
usually the symmetric one: it is the round number, the diagonal, the middle,
the square. Those are exactly the inputs at which a swapped pair of arguments,
a transposed pair of axes, or a flipped sign is invisible.

The catalogue is short and worth memorising:

| Probe | Blind to |
|---|---|
| 45° on a circle, or any point where `x == y` | swapping the two axes |
| the centre of anything | a sign flip on either axis |
| a **square** window (`600×600`) | width used where height was meant |
| `0` or `1` as a factor | a multiply that should be an add, or vice versa |
| an empty list, or a list of one | an off-by-one in an index or a count |
| a palindrome, or a string of one repeated character | a reversal |
| the identity element of any operation | that operation being replaced |

Two of these were already load-bearing here and are the reason the fault was
narrow rather than total: the window-size grid is deliberately made of
*non-square* shapes (`1600×300` and `480×900` sit next to `900×720`), and the
state list carries a list of one, a full list and an empty one. The rose test
was the one place a single convenient point had been used, and it was the one
place the sweep found a hole.

**The fix is not a better assertion, it is a second point.** The 45° press
stays — it is the clearest statement of what the control does, and it is the
one that proves declination is taken off the pressed bearing. A press at 20° is
added beside it, chosen because it is not a multiple of 30 (so the rose's own
degree labels cannot supply the number the readout is checked against) and
because sin 20 ≠ cos 20 (so a rose with swapped axes reads 70 and is caught).

**Where else to look.** Any test that presses "the middle" of a control and
asserts a coordinate came back; any layout test run at a single size; any
geometry helper tested at 0°, 45°, 90° and nowhere else. Mechanically: grep the
app suites for `0.5` used to derive a probe point from a pair of edges, and for
size constants whose width equals their height.

---

### Lesson 106: when a mutation moves both sides of a comparison, the comparison is blind (lane C, 2026-08-31)

**In short:** the waypoint list draws as many rows as fit and records a hit box
for each. A test asserted the honest thing — *the rows that are drawn are
exactly the rows that can be clicked* — and it passed over a list that counted
a **half-fitting row as a row** (`floor` changed to `ceil`), which paints a row
with its lower half cut off and lets the user click the visible sliver.

It passed because the mutation adds the part row to *both* sides. It is drawn,
so it is in the "drawn" set; it is hit-boxed, so it is in the "clickable" set.
The two sets stayed equal while both were wrong.

**The rule.** *A test that compares two quantities derived from the same
expression proves only that the expression is used consistently — never that it
is right.* This is the same family as lesson 104's closed loop, but the loop is
between two **assertions** rather than between a lookup and a click, so it
survives the lesson-104 fix (both sets here are already read from the picture).
To break it, at least one side must come from somewhere the mutation cannot
reach.

**What broke it here was shape, not membership.** Every row is recorded at the
same pitch, and `Frame::hit` trims a hit box to the clip in force — so the row
that only half fits answers over a box measurably shorter than its neighbours'.
The replacement test asserts that *every waypoint hit box has the same height*,
which is a property of the picture that the row count cannot fake: to pass it,
the last row must genuinely fit.

The general move is to find a second, independent consequence of the fault:

| the fault | the blind comparison | the independent consequence |
|---|---|---|
| a part row is counted | drawn set vs. clickable set | the hit box is short |
| every row shifted by one | label found vs. label asserted (lesson 104) | the words drawn in the row |
| a pane sized from the wrong axis | pane vs. its own contents | the pane vs. the *window* |
| an off-by-one scroll offset | first visible vs. first drawn | the selected row is on screen |

**Where else to look.** Any assertion of the form `assert_eq!(a(), b())` where
`a` and `b` are both computed from the frame the code just drew. In the wiring
campaign specifically: every test that pairs "is it drawn?" with "is it
clickable?", and every one that compares a count against a count. Neither is
wrong to have — they catch a real class of fault, where one side is updated and
the other is not — but neither can stand alone as the owner of a mutation to
the shared expression underneath them.

---

### Lesson 107: a clip hides a drawing pass's overrun from every test but one (lane C, 2026-09-01)

**In short:** the contacts detail panel drew a contact's fields down a column
and never checked that it still had column left. A contact with six phone
numbers and a paragraph of notes pushed the group chips **below the box they
were being drawn in**, where they landed on top of the Edit / Star / Delete
buttons. The edit form did the same to Save and Cancel. Both faults were
present from the first commit, under a suite of 230 tests, and not one of them
could see either.

They could not see it because the clip that was in force hides the overrun from
**two of the three kinds of thing a frame contains, but not the third**:

| what is emitted | what the clip does to it | can a test see the overrun? |
|---|---|---|
| a run of text | `put_text` asks `Frame::is_visible` first and emits nothing | **no** — the run is simply absent |
| a hit box | `Frame::hit` trims to the clip and drops an empty result | **no** — the control is simply absent |
| a filled box | pushed to the display list exactly as asked | **yes** — the rect is in the frame |

So every test phrased over text ("is the label inside the window?", "is the
column bounded?") and every test phrased over controls ("does every hit box lie
inside the window?", "is the panel clear of the status line?") passes over a
pass that has run hundreds of pixels past its box. The picture on screen is
even *correct* — the compositor throws the overrun away. The fault is real all
the same: the moment anything is drawn **after** the offending pass under a
different clip, as the buttons are, the overrun lands on top of it.

**The rule.** *A clip makes a fault invisible, not absent — and it hides it
unevenly, so the test that owns "a pass stops at its box" must be phrased over
fills, never over text or hit boxes.* The general form: when a mechanism
suppresses output, enumerate which of your assertion surfaces it suppresses.
Whichever surface it leaves alone is the only one that can carry the assertion.

**The exemption is the rule, not a hole in it.** "No fill is painted entirely
outside the clip" is too strong as written: a two-pixel sliver of a contact row
at the bottom edge draws its 40-pixel avatar circle in full, forty pixels below
the cut, and that is correct — *the unit of "do not draw this" is the item, and
an item is drawn whole or not at all.* The honest assertion is therefore: a
fill wholly outside the clip is allowed exactly when some other fill **drawn
under the same clip** encloses it and is itself partly visible. That excuses
the avatar (its row's own background carries it) and excuses nothing else — a
row three hundred pixels below the list has no such parent, because the only
fill enclosing it is its own background, which is just as invisible as it is.
Note the "same clip" qualifier: without it, the window background excuses
everything.

**Where else to look.** Every app in the wiring campaign whose panels are
`f.clip(...)`-ed and whose contents are laid out with a running `y` cursor:
the list bodies, the form bodies, and any column of sections. Grep for a `for`
loop that increments a `y` inside a `clip`/`unclip` pair and does not test the
cursor against the bottom of the box. In this crate the list already had the
guard (`if cy >= l.list.bottom() { break; }`) and the two panels did not, which
is the usual shape: the scrolling thing is guarded because someone thought
about scrolling, and the "it always fits" thing is not, because at the default
window size it always does.

---

### Lesson 108: a pass held to its caller's box is not held to its own (lane C, 2026-09-01)

**In short:** dbviewer has a test that hands each drawing pass a box and checks
the pass paints nothing outside it. It listed eight passes, one of them
`draw_sidebar`. But `draw_sidebar` cuts its box in two — an object tree on top,
a filter builder underneath — and hands each half to a sub-pass. So a tree row
that overran the *tree's* box landed on the *builder*, which is still inside the
sidebar, and the test that exists precisely to catch overruns reported nothing
wrong. A mutation that deleted the tree's bottom-edge guard was caught only by
an unrelated test, incidentally, and only in the states where the builder
happened to be hidden.

**The rule.** *List every pass that is handed a box, not every pass that is
handed a **window** region.* A containment test's resolution is the size of the
smallest box it knows about; overruns finer than that are invisible to it by
construction, and the guard being tested almost always lives in the sub-pass,
because that is where the cursor walks.

The practical obstacle is worth naming, because it is what caused the omission:
the two sub-passes each take the `DbTab` they draw, so they do not fit the
`fn(&App, &mut Frame, Rect)` alias the pass list used, and were quietly left out
rather than the alias being widened. Making `Pass` a `Box<dyn Fn(..)>` costs one
allocation per pass per test and admits them. **A type alias that silently
excludes members of the set it is enumerating is a coverage hole with a
compile-time excuse.**

**Fills are a complete witness; runs and hit boxes are one-way ones — and the
distinction is not the same as Lesson 107's.** Widening the pass list *still*
did not catch the mutation, because a tree row pushes a fill only when it is the
selected table; every other row is two runs of text and a hit box. Lesson 107
concluded that the assertion must be phrased over fills, and that is right for a
test that draws a **window**, where a clip is in force. This test hands each pass
an **unclipped** frame — so nothing is suppressing the runs and hit boxes, and
they say what the fills cannot. The correct formulation is therefore:

| surface | witnesses an overrun? |
|---|---|
| a fill | always — pushed exactly as asked, clip or no clip |
| a run of text | only if no clip suppressed it: finding one proves an overrun, finding none proves nothing |
| a hit box | same |

So a one-way witness may always be *added*: it can only ever add catches, never
cause a false failure. What it must never do is *replace* the fill, because its
silence means nothing. Lesson 107 said "phrase it over fills"; the sharper
statement is **"you may only conclude *absence* of an overrun from fills"**.

Doing that here found a fourteenth production fault the moment it was added:
three one-line placeholders (`No query results`, `Select a table to view its
schema`, `No tables in database`) placed their line at a fixed inset from the
top of their pane and inked it whatever the pane measured, so a pane shorter
than the inset got a line below its own bottom edge.

**Where else to look.** Any per-pass containment sweep: check that its pass list
is closed under "is handed a sub-box". `alarmclock` was checked and is clean —
its three passes are whole tabs, and the one sub-pass with a smaller box, the
alarm editor, has its own containment test. The placeholder shape is worth its
own grep across the campaign: a `put_text` at a *constant* offset from a pane's
top edge, in an early-return arm for the empty case. Those arms are written
first, before anyone has thought about small windows, and they are the only
paint in the pass, so no sibling ever collides with them and reveals the fault.

**Result of that grep (2026-09-01).** It ran over all 76 wired apps and found
exactly one candidate outside dbviewer, in `apps/automator`. Following it up
turned into Lesson 109.

---

### Lesson 109: centring is not a bound, and a sub-pass's box is the test's to choose (lane C, 2026-09-01)

**In short:** Lesson 108's grep pointed at automator, which had no per-pass
containment test at all — only a window-level one. Adding one found *six*
faults, none of which any of automator's 157 existing tests could see. Five of
the six are the same mistake written five ways: a line of text placed by
*centring* it in a strip, with nothing checking that the strip is as tall as the
line. The sixth was found only after the test was taught to hand a sub-pass a
box the layout does not currently produce.

**Rule 1 — vertical centring is not a vertical bound.** `band.y + (band.h -
size) / 2.0` is *above* `band.y` the instant `band.h < size`, and hangs the same
distance below `band.bottom()`. Every heading strip, footer button, list row and
status bar in a resizable layout has a window size at which it is squeezed to
less than one line, so this is not a corner case; it is what every small window
does. The campaign already knew the horizontal form of this (`centred` clamps
its offset at zero — automator had that fix already, and a mutation row for it).
The vertical form went unnoticed because **both** window-level text tests in
this program bound a run's left and right edges and neither binds its top and
bottom — a run has no height in the command stream, so you have to supply
`font_size` as its height yourself before the question can even be asked.

The fix is one helper, `centre_line(band, size) -> Option<f32>`, returning
`None` when the band cannot hold the line, with all eighteen call sites going
through it. **Not** eighteen copies of the comparison — which is exactly how the
rule would come to hold in seventeen places and not the eighteenth.

Two variants of the same fault that the helper does not cover, and which are
worth looking for separately:

- **A fill of a literal size, centred.** The header's recording dot is
  `l.small * 0.9` square, centred in the header; in a header shorter than that
  it painted over the toolbar. Fix: `dot.min(head.h)` — shrink to the band
  rather than centre in it. Being a fill, this one *is* a complete witness, so
  a window-level fill test could have caught it — except the header is nowhere
  near the window's edge, so it never escaped the window.
- **A band written as an offset rather than as a rectangle.** `draw_pads` split
  its strip into quarters by writing `pads.y + (quarter - small) / 2.0` inline.
  There is no band there for a helper to be given. Fix: name the four quarters
  as `Rect`s, then centre in them.

**Rule 2 — a sub-pass's contract is "stay inside the box you are given", for
*any* box.** The script tab's error strip was hung off the bottom of its text
area instead of the bottom of its body; the two agree exactly until the text
area's height clamps at zero, which needs a body under about seven points tall.
No window in the size grid splits the list into a body that short — so the fault
sat in the tree with a containment sweep over it that could not reach it, and
its mutation row survived.

A sub-pass *takes its box as an argument*, which means the test can simply hand
it one. `squeezes(r)` yields `r` plus `r` with heights `[0, 1, 3, 6, 12]` and
widths `[1, 5, 30]`, and every Rect-taking pass is swept over all of them. That
found the strip immediately, and then two more faults on the next two runs: the
pads' headings above, and an action row's playing marker, a fill of a literal
three points' width — the one thing in a row that no other measurement bounds.

**Rule 3 — sample the sliver.** automator's height grid was
`[0, 18, 55, 140, 700, 1100]`. Zero finds nothing, because every pass returns
early on an empty box; eighteen finds nothing, because by then everything fits.
The band *between* them — a strip that exists but cannot show anything — is
where the recording dot's fault lived. A `6.0` was added. **Any size grid that
jumps from zero straight to a comfortable size is missing its most productive
sample.**

**Where else to look.** The other 46 unwired apps get their containment sweep
when they are wired, and it should be written with `squeezes` from the start.
Of the wired apps, the grep to run is for vertical centring that is not clamped:

```
(\w+)\.y \+ \(\1\.h - <size>\) / 2\.0
```

That grep, run over `apps/*/src/main.rs` on 2026-09-01, reports **109 sites in
42 apps** (automator itself still accounts for 8 of them — the *clamped* forms,
`dot.min(head.h)` and `l.button.min(bar.h)`, which are correct and match the
same shape). So it is a candidate list, not a fault list, and each site has to
be read: the question is whether anything guarantees the band is at least as
tall as what is being centred in it. Where nothing does, the fix is
`centre_line` for a run and `.min(band.h)` for a fill.

Also grep for a literal-sized fill centred in a band. Both shapes are cheap to
check and, on this evidence, usually present.

**Tracked as C-CENTRING-IS-NOT-A-BOUND below.**

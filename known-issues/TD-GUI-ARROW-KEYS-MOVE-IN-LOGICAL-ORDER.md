## TD-GUI-ARROW-KEYS-MOVE-IN-LOGICAL-ORDER -- FIXED, and this entry was three weeks stale

**Status: OPEN 2026-08-16, UNBLOCKED 2026-08-21** (lane C).

**The question this waited on has been answered.** C-Q2 was decided by the
operator on 2026-08-21 -- **visual**, the screen direction -- and written up as
`design-decisions.md` §541 on 2026-08-24. This entry went on saying "do not fix
this without an answer there" for another 23 days, which is how a decided
question keeps looking like one still being thought about. Found by lane B's
`scripts/check-stale-blockers.py`, which cross-references answered questions
against entries that claim to be waiting on them.

**What §541 obliges**, and how much of it is done:

| | state |
|---|---|
| `gui/font/src/shape.rs` -- `ShapedRun::caret_left` / `caret_right`, the primitive | **done**, with tests |
| `gui/toolkit/src/text.rs` -- `TextCursor` (carrying `affinity`) and `caret_left`/`caret_right` | **done** |
| `guitk::widget::TextInput`, `guitk::modal::InputDialog` -- the arrow-key handling | **done**, both calling the wrappers above |
| `apps/editor` | **deliberately out of scope**: §541 does not cover it either way, because it draws and scrolls its own caret |

**So the whole thing is done, and has been for some time.** The evidence is two
end-to-end tests, both passing:

* `guitk::widget::tests::the_arrows_move_by_the_screen_and_keep_the_side_they_are_on`
* `guitk::modal::tests::a_plain_input_dialog_moves_its_caret_by_the_screen`

Both walk `ab` + two Hebrew letters + `cd` in each direction and assert the
exact offset sequence -- `7, 6, 4, 6, 1, 0` leftwards and `1, 2, 4, 2, 7, 8`
rightwards. The repeated offset is the interesting part and is the regression
test for §541's measured trap: one byte offset is visited twice per walk,
because the two gaps where the directions meet sit at opposite ends of the
Hebrew on screen and each answers to *both* offsets. A widget that kept only
the byte cannot tell the second 6 from the first and skips the whole word in
one press -- worse than the logical motion this replaced.

Home and End stay logical, also as §541 says, and `Backspace` still deletes the
previous character *in the string*: deleting and moving are allowed to
disagree, because "the previous character" is what a reader of that script
means regardless of which side of the caret it is drawn on.

**Why this entry is being closed rather than worked.** It said "blocked on
C-Q2, do not fix this without an answer" for 23 days after the operator
answered, and the fix had in fact already landed. Two separate staleness
failures in one entry, and the first hid the second: a reader who believed the
blocker stopped reading before the code. Found by lane B's
`scripts/check-stale-blockers.py`, which cross-references answered questions
against entries claiming to wait on them -- a shape worth having a gate for
precisely because a question *sounds* like it is still being thought about.

**What.** Left/Right arrow keys move the caret by one position in *logical*
order -- the order the characters are stored and read -- in every text widget in
the tree. On a line that mixes left-to-right and right-to-left text ("I said
<HEBREW WORD> to him"), logical order is not screen order, so pressing Right can
move the caret visibly *leftwards*, and pressing it repeatedly makes the caret
jump back and forth across the width of the right-to-left word rather than
stepping across it.

This is not a bug in the sense that it produces a wrong result -- the caret is
always at a real, correct text position, and typing there inserts where the user
would expect *in the sentence*. It is a bug in the sense that the key is named
after a direction on the screen and does not always move in it. Windows edit
controls move visually; macOS, GTK and Qt move logically. Both ship.

**Where.** The arrow-key handlers, each of which does `cursor -= 1` /
`cursor += 1` over a byte or character index with no reference to direction:

* `gui/toolkit/src/widget.rs` -- `TextInput`
* `gui/toolkit/src/modal.rs` -- `InputDialog`
* `apps/editor/src/main.rs` -- `move_left` / `move_right`

**Reproduce.** Type or paste a line containing a Hebrew or Arabic word between
two English words into any of the three. Put the caret immediately before the
right-to-left word and press Right once: the caret jumps to the *far side* of
that word rather than moving one letter-width right, then walks back across it
on subsequent presses.

**What the proper fix looks like, if C-Q2 answers "visual". The code for it is
now written, tested and merged -- it is simply not wired to the arrow keys.**
Added 2026-08-17 (lane C):

* `gui/font/src/shape.rs` -- `ShapedRun::caret_left` / `caret_right`. These take
  a `Hit { offset, affinity }` and return the neighbouring caret slot in *draw*
  order. n drawn clusters give n+1 gaps; moving right from slot k lands on
  `clusters[k].right`, moving left from slot k lands on `clusters[k-1].left`,
  where a cluster's `left`/`right` are its leading/trailing hits swapped when it
  is RTL. The convention is to name a boundary by the character just *crossed*,
  so the caret pixel round-trips exactly while the byte offset may come back as
  the boundary's other name.
* `gui/toolkit/src/text.rs` -- `caret_left` / `caret_right` (and the `_in`
  variants taking a prepared font), wrapping the above in `TextCursor`.

Wiring it up is one line per site: replace the `cursor.byte() -+ len_utf8()`
step with a call to `text::caret_left` / `caret_right`. Each of the three sites
carries a comment naming that line and pointing at C-Q2.

**The affinity turned out to be load-bearing, which was not obvious in advance.**
A widget that stores only the byte offset and rebuilds the cursor on each
keypress does not merely land on the wrong side of a direction boundary -- it
*skips the entire reordered run in a single press*. On `ab` + two Hebrew letters
+ `cd`, an affinity-less rightward walk visits offsets 1, 2, 7, 8 and never
enters the Hebrew at all. This is pinned as a test
(`dropping_the_affinity_between_steps_skips_the_reordered_run` in `text.rs`)
which asserts the *broken* sequence, so the requirement cannot be quietly
regressed. It is also why the three widgets were converted from `cursor_pos:
usize` to `cursor: TextCursor` in the same change: without that conversion,
answering C-Q2 "visual" later would produce a subtly worse behaviour than the
status quo rather than a better one.

Two things must stay logical under either answer, and would be wrong to convert
along with the arrows: Home/End and word-motion (Ctrl+arrow). Those name
positions in the sentence, not directions on the screen.

If C-Q2 answers "logical", this entry closes with no code change: the comment at
each of the three sites already records that the behaviour is deliberate, and
the `caret_left`/`caret_right` primitives stay -- selection-by-mouse and any
future visual-order feature want them regardless.

**The editor is a separate problem and is not covered by C-Q2.** `apps/editor`
cannot move its caret visually even if the answer is "visual", because it draws
the caret at `measure(prefix_of_line)` and scrolls horizontally by byte-slicing
the line -- both of which assume screen order equals logical order. Converting
only its motion would move the caret to positions it is not drawn at, which is
worse than today, where drawing and motion at least agree with each other. See
`TD-EDITOR-IS-NOT-BIDIRECTIONAL`.

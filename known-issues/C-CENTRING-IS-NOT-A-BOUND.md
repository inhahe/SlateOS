## C-CENTRING-IS-NOT-A-BOUND (lane C)

**Status:** OPEN 2026-09-01

**In short:** In 42 of the campaign's apps, a line of text (or a fixed-size
square) is positioned by centring it in a strip, with nothing checking the strip
is as tall as the thing being centred. When the window is small enough that the
strip is squeezed below one line's height, the centred thing sits partly above
the strip and partly below it — painting on whatever is next door. automator had
six such faults and none of its 157 tests could see any of them; see Lesson 109
for why (no window-level test in the campaign binds a run's *top and bottom*
edges, because a run has no height in the command stream).

**Where.** `apps/*/src/main.rs`, the shape `x.y + (x.h - <size>) / 2.0`. The
2026-09-01 count is 109 sites in 42 apps, largest first: automator 8 (already
fixed — the 8 remaining are the clamped, correct form), taskscheduler 8, hangman
6, sokoban 6, emojipicker 5, snippets 5, asteroids/camera/magnifier/maze 4 each,
calculator/klotski/pipes/rush/sliding/snake 3 each, and 26 apps with 1–2.

**It is a candidate list, not a fault list.** A site is fine if something
guarantees the band is at least as tall as the line — a `l.button.min(bar.h)`
clamp, or a row height with a floor that exceeds the font. Each site has to be
read.

**The proper fix, per app:** add `centre_line(band, size) -> Option<f32>` (see
`apps/automator/src/main.rs`), route every unclamped site through it, shrink
literal-sized *fills* with `.min(band.h)` instead, name any band that is
currently written as a bare offset as a `Rect`, and — this is the part that
makes it stick — give the app a `no_pass_paints_outside_the_region_it_owns`
test with `squeezes()`, plus a sliver entry in its height grid. Without the
test the fix is unverified and the next edit reintroduces it.

**If never fixed:** nothing is corrupted and nothing crashes; a user who makes a
window very small sees text overlapping the panel next door. It does not get
worse with time, but every newly wired app adds sites at the current rate of
roughly two or three per app.

### Progress

**automator** (2026-09-01) — the app the lesson was found in. 6 faults, fixed.

**taskscheduler** (2026-09-01) — done: 131 tests, 30/30 mutation rows caught.
The scope was three times what the grep predicted, and the extra two thirds are
the part worth carrying to the next app:

* The grep only sees *vertical* centring. The same fault is there horizontally
  and the shape is different, so it does not show up in a search for
  `(h - size) / 2.0`: a constant inset (`PADDING`) is right of a band narrower
  than the padding, a constant `max_width` runs off the right edge, and
  `text::center_x(label, rect.centre().0, …)` is left of a button narrower than
  its own label. Add `span(band, x, want) -> Option<(f32, f32)>` alongside
  `centre_line` and route every cell through one `run_in`.
* Fixed-size *fills* are a third shape again: a heading strip written as a
  32-point rectangle at the area's corner, a `bottom() - 3.0` underline, a
  `bottom() - 1.0` separator. `bottom_strip(band, want)` for the ones on an
  edge; `.min(band.h)` for the rest. Never `centre_line` — a fill shrinks.
* The largest fault in the app was neither: both modal dialogs painted a
  constant 440x380 (and 360x160) panel — scrim, border, fields, buttons and
  hit boxes — over whatever lies beyond a smaller window.

**And three things the containment test cannot see on its own**, all found by
the mutation sweep rather than by reading the code — they are the whole of the
first sweep's eight survivors, one of shape 1, three of shape 2, four of shape
3. Budget for them:

1. **A bound that nothing can squeeze is not verified.** taskscheduler's list
   rows were bounded correctly *inside their loops*, but the only rectangle the
   loop ever produced was a whole 32-point row, so every bound in the row body
   could be deleted with the suite green. The fix is structural: extract the
   row into a function *of the row*, and add it to the `squeezes()` sweep next
   to the text field and the button. Only one survivor was this shape, but it
   is the one that hid a real bug — once a row could be handed a three-point
   box it turned out a row squeezed to nothing still painted its background and
   still recorded a click target for a task the user cannot see. Expect it in
   any app with a list.
2. **A clamp that a stronger guard upstream already dominates is unreachable,
   and only the sweep will tell you.** `CHECKBOX_SIZE.min(row.h)` sitting above
   an `.intersect(row)` produces a byte-identical rectangle at every height; a
   caret clamped to a field that `centre_line`/`span` has already refused to
   draw in cannot bind. Such a clamp reads as prudence and is really a claim
   the tests cannot check. **Delete it and re-point its mutation row at the
   bound that does the work.** Do not weaken the test to "reach" it, and do not
   leave it in on the grounds that it is harmless — an unreachable guard is
   what a later reader will trust instead of the real one.
3. **Containment can always be satisfied by drawing nothing**, so it must be
   paired with a reachability assertion. Every bound in this campaign is an
   intersection, and an intersection is equally happy to return an empty
   rectangle: a dialog whose buttons are placed by their nominal offset rather
   than by the dialog's cut-down height has them *deleted* by the cut, and the
   containment test is delighted. `a_dialog_with_room_still_shows_its_buttons`
   is the counterweight — when the box is big enough for the control, the
   control must be there. Also assert the converse the guards actually promise:
   a control given an empty box draws nothing at all, since a zero-sized fill
   is inside every region there is.

**hangman** (2026-09-01) — done: 160 tests, 48/48 mutation rows caught. The
grep said 9 sites; the app had four unclamped-centring faults and five more that
only the sweeps found. taskscheduler's three survivor shapes all recurred, so
treat them as the standing budget rather than that app's peculiarity. Four
things this app added to the recipe:

4. **The squeeze has to reach the pass, and a `Layout` field is how.** A band
   computed inline inside the pass that paints it cannot be handed a smaller
   box, so every bound below it is unverified — `draw_result`'s region was
   `Rect::new(gallows.x, gallows.y, gallows.w.max(word.w), word.bottom() -
   gallows.y)`, four lines into the drawing code, and its card-height clamp and
   its button-fit check both survived because nothing could squeeze it. Naming
   it `Layout::overlay` is what made both rows die. **Sort the passes into those
   whose band is an *input* and those whose band is *derived*, squeeze the
   first group and say in the test why the second is excluded** — hangman's
   keyboard band is computed from `key` and `key_gap`, so a keyboard band shrunk
   on its own is a band the keys were never sized for and the resulting overrun
   would be the test's fault, not the program's.
5. **Squeeze to fractions of the band, not only to absolute slivers.** A fixed
   12-point band is below every font size in the app and so only ever reaches
   the outermost refusal. The interesting failures live in the gap between "the
   type fits" and "the type *and what hangs below it* fits", which is a fraction
   of the band and not a constant: leaving `RULE_WIDTH / 2.0` out of a rule-fit
   check is wrong for one narrow range of heights and right on either side of
   it. `squeezes()` now yields sixteenths of `r.h` and `r.w` as well as
   `[0, 1, 3, 6, 12]`.
6. **Measure a stroke's thickness on both axes, or the check is blind to any
   pass made of strokes.** A vertical or zero-length line has no extent on one
   axis; an empty rectangle is inside everything; so hangman's gallows — four
   strokes and a twelve-sided head, no fills at all — was measured as painting
   nothing and could be drawn anywhere. Widen a line's bounding box by its
   thickness across the line, and on *both* axes when it runs along neither.
   Then expect a real finding to fall out, because a stroke straddles the line
   it is drawn on and thicknesses usually have a floor of one point that does
   not scale: the gallows had to be inset by a whole stroke width before the
   6-point band stopped drawing the beam above its own top edge.
7. **"Drew something" is too weak a converse.** Shape 3's reachability
   assertion has to name a run from the pass's *far end* — the alphabet's last
   key, the statistics column's list of wrong letters, the result card's second
   button — because a column that stops after its heading has drawn something.
   And the empty-band half should assert **no command at all**, not merely no
   ink and no hit box: a degenerate fill is how a pass says it never looked at
   the band it was given, and it is invisible to containment.

One more, which is really shape 2 wearing a comment: **a guard whose answer is
recomputed downstream is dominated even when the downstream copy is inside a
shared helper.** `draw_word` said "one `centre_line` for the row rather than one
per letter" while its loop called `run_in`, which asks `centre_line` again for
every cell — so the row's answer placed nothing but the rule, and replacing the
row's guard with the bare centring it exists to prevent changed no test's
answer. The fix was to make the code true rather than to delete either copy:
the letters are placed on the row's single baseline now, through `span` for the
horizontal half. If a comment claims a guard is the only one, mutate it.

**sokoban** (2026-09-01) -- done: 112 tests, 120/120 mutation rows caught. The
grep said 8 sites; the app had eleven faults. Three were found by reading (an
unnamed band, a fallback branch, and three guards standing in front of rules
that already held) and six by the containment sweep -- and five of those six are
one mistake, which is the main thing this app adds:

8. **A run's width limit must be measured from where the run starts, not from
   the box it sits in.** `label_centred` passed `r.w` and `label_right` passed
   `right - left`, which is what the box is worth to a run beginning at its left
   edge. A centred run begins half the slack in and a right-aligned one begins
   `right - w` in, so the box's full width tells the renderer it may paint that
   far *past* the edge the run was aligned to: a "Play" 27 points wide, centred
   in a 136-point button, was licensed to fill 136 points starting 54 past the
   button's left edge, and ran 54 points off the band. The limit is
   `box.right() - x` now. Expect this in every app with a `label_centred` or
   `label_right`, which is most of them -- and note that the horizontal
   containment check is what finds it, so shape 6's "measure the ink on both
   axes" is a prerequisite.
9. **Make "no limit" unspellable.** `push_text` took an `Option<f32>` and the
   file carried a `label()` convenience that passed `None`; one caller used it
   for the header's title, and "Sokoban" at 28 points ran 110 points wide out of
   a band 1 point wide. Deleting the helper is not enough while the parameter is
   still an `Option` -- the next edit re-adds it. Change the type to `f32`, so
   the only way to draw a run is to say how much room it has, and split any band
   two runs share into named columns (here a title column that takes what the
   counters leave) so there is a real number to pass.
10. **A clamp on an origin is not a bound on the thing drawn from it.** The
    victory confetti clamped each dot's top-left corner with
    `dy.clamp(0.0, (l.window.h - d).max(0.0))` and then drew `d` points anyway,
    so in a window shorter than one dot the `.max(0.0)` collapsed the range to
    a single legal origin and the dot was painted out of the window from it. The
    same shape as shape 6's floors: a size with a floor that does not scale
    (`(l.pad * 0.5).max(1.0)` for the menu cursor's stripe, 1.7 points wide in a
    row 1 point wide) is a bound on nothing. Both are fixed by refusing the case
    outright -- `if d <= window.w && d <= window.h`, and `.intersect(r)` for the
    stripe, which bounds all four sides and returns `None` for a row with no
    room, the same `Option` shape as `centre_line`.

**Then the sweep over the finished suite faulted six rows, and not one of them
was a missing bound.** Four were shape 2 again -- and the interesting part is
that the *fix* introduced them. Three passes opened with an
`if l.<band>.is_empty() { return; }` that only became unreachable once `fill`
learned to refuse a rectangle with no area and `centre_line` learned to refuse a
band that cannot hold its stack; `draw_list`'s new `centre_line` is dominated by
the floor on the row height. **Expect the sweep after the fix to find guards the
fix made redundant, and re-point their rows rather than deleting them** -- at
`fill`'s own refusal, at the floor that does the work, at `centre_line` returning
`None` for every band. One guard survived the audit and is worth knowing about:

11. **A pass that clips is not free to skip its band check.** `Frame::clip`
    pushes a `PushClip` command whether or not the rectangle has area, so a pass
    that clips before it has refused the band emits two commands for a band it
    was never given -- and, on an early return between the two, leaves the clip
    unbalanced. `draw_list` keeps its `l.body.is_empty()` bail for exactly this
    reason while its three neighbours lost theirs; `draw_footer` gets away
    without one only because its refusal is above its clip. When deleting a
    dominated band guard, check for a `clip` below it first.

And two of the six were places **containment is structurally blind**, both worth
checking for in every app that gets the treatment:

12. **Containment cannot see a run that overlaps another run.** A title handed
    the whole band instead of what the counters leave does not leave the band --
    it runs underneath them. Naming the columns is what makes the claim
    sayable; say it, pairwise over the pass's runs, or the named columns are
    decoration. (Pairwise and not "the title against the rest": a subtitle
    shares the title's column and sits on the line below it.)
13. **Containment cannot see a run given no room at all.** The natural `inked`
    helper takes `max_width` as the run's width, so a run told it may fill
    nought points measures as an empty rectangle -- and an empty rectangle is
    inside everything. The command is still there, asking the renderer to draw
    a string with `Ellipsis` overflow in no room. Assert separately that every
    `Text` command carries a positive limit. Related, and the reason shape 9
    needs this one: closing off `None` **guts every test that only asserted a
    limit existed**. `max.is_some()` was a real check until `push_text` took an
    `f32`, and then it was vacuous -- the sweep caught it as a `[??]`, the fault
    falling to the containment test instead of its named owner. Grep for
    `.is_some()` in the suite after the type change and make each one assert the
    limit's *value* against the band.

Also worth carrying: **`stroke` should have `fill`'s contract.** A stroke
straddles the line it is drawn on, so a 2-point border asked for a card's exact
rectangle paints a point outside it on every side -- the victory card's border
landed at y = -0.71. Inset the rectangle by half the line width inside `stroke`
itself rather than at each call site; hangman fixed the same fault one call site
at a time, and sokoban shows it belongs in the helper.

**klotski** (2026-09-01) — done: 90 tests, 80/80 mutation rows caught. The
prediction above was right that sokoban's fix transfers nearly verbatim — shapes
8, 9, 2 and 11 all recurred and were fixed the same way — and wrong about the
two faults that mattered, which no amount of transferring would have found. Both
are new shapes:

14. **A pass is bounded by the region it *paints*, not by the region the layout
    named after it — and a solve is the easiest place to get that wrong.**
    `Layout::board` is the grid; what `draw_board` paints first and outermost is
    a mat one `gap` wider than the grid on every side. The solve sized only the
    grid, so on whichever axis binds — `cell` is a `min` of the two, and on the
    winner the grid fills the band *exactly* — the shortfall a centring has to
    divide is nought and the mat's ring has nowhere to go but outside. At
    900x500, where height binds, the mat's top edge landed a whole gap above the
    band it was solved from. Note what does **not** help: centring cannot
    rescue a child bigger than its parent (it just splits the overhang in two),
    and no clamp on the grid can either, because the grid was never the thing
    that overflowed. The fix is in the solve — `per_w`/`per_h` count the well as
    well as the grid — and in the *naming*: `Layout::board_frame` is the region
    `draw_board` owns, for the same reason a picture's region is its frame and
    not its canvas. **The trap here is the test, not the code.** The obvious
    containment check measures `draw_board` against `l.board`, which is the one
    region in the program this fault is invisible in; a pass checked against the
    wrong region is a test that cannot fail. Before writing the check, ask what
    each pass paints *first* and *outermost* and name that — every app with a
    solved-then-decorated region (a mat, a well, a card behind a list, a focus
    ring around a canvas) has the same hole.
15. **A "visibility floor" is an unbound, and dangerous specifically as an
    outset.** `(derived * k).max(CONST)` is written to keep a decoration legible
    when the thing it decorates gets small, so by construction it stops asking
    how much room there is — which is fine growing *inward* and is a licence to
    paint outside growing *outward*. klotski's selection halo grew by
    `(l.gap * 0.8).max(1.0)` around the picked-up block: on a board whose gap is
    a fifth of a point the halo is five times the ring it was meant to sit in.
    It grows by `l.gap` now — into the mat's ring and no further, which is what
    reserving the ring in the solve bought. Audit the `.max(` floors in each app
    by *direction*: inward `ring`/`inset` uses (checkers, chess, reversi,
    yahtzee) are safe; `apps/rush/src/main.rs:1451` is this fault
    character-for-character.

And one about the suite rather than the program, which is the reason fault 8
survived a fix that had already been made once in sokoban:

16. **A test can hold a fault in place, and it will read as the strictest test
    in the file.** `label_centred` insets the run by half the slack and handed
    the renderer `r.w` — the box's own width — as the limit, so the "Next" label
    was licensed to reach 123.5 in a band 120 wide. That is shape 8 exactly, in
    the third app to hit it, and it was missed on the read-through because
    klotski already had `a_centred_label_is_given_the_width_of_the_box_it_sits_in`
    asserting `max_width == Some(rects[i].w)` — a test whose *name* says the
    property is checked and whose body pins the bug. Only the new containment
    grid could contradict it. When a bound moves, **grep the suite for the
    tests that assert the old bound and rewrite them to the new claim** — here,
    `a_centred_label_is_stopped_at_the_right_hand_edge_of_its_box`, asserting
    `x + max == box.right()`. A test that must be rewritten to make a fix pass
    is not a test that was protecting anything.

And five more that the **mutation sweep** found and the read-through could not.
Every one of them is a test that could not fail rather than a program that was
wrong, which is the whole argument for running the sweep before calling an app
done: eleven of the seventy-nine rows the table then held survived their first
run, and not one of them was fixed by weakening an assertion. The eightieth row
is shape 18's, added because the field shape 17 introduced needed pinning.

17. **A derived region cannot check the thing that derived it.** Shape 14 above
    fixed the solve *and* named the region — `board_frame` — the board pass is
    measured against. But `board_frame` is computed *from* the solve, so a solve
    that oversizes the mat oversizes the region by exactly as much, and
    containment stays green however far the mat has escaped. The three rows that
    break the solve all survived. The window and the chrome cannot see it
    either: `pad` separates the band from both, and the overrun is one `gap`,
    which is smaller. **The only rectangle that holds still while the solve
    moves is the room the solve was handed — and it was a local, so the solve's
    contract had no name and therefore no test.** The fix is one field,
    `Layout::board_band`, and one test,
    `the_board_fits_the_room_it_was_solved_from`. Any app whose layout solves a
    size and then decorates it has this hole; the tell is a `let band = …` in
    `Layout::new` that never reaches the struct literal.
18. **A check whose subject is itself a claim needs its subject pinned.** Once
    `board_band` is a field, a mutation that reports the *whole window* as the
    room passes `the_board_fits_the_room_it_was_solved_from` trivially — a
    bigger room is a weaker check, and the check cannot object to its own
    subject. That row survived too. What a room that has swallowed the chrome
    cannot pass is `the_board_never_overlaps_the_chrome`, so that test now asks
    about the *band* as well as the frame. Whenever you add a field for a test
    to hold, add the row that widens it and find the test that says no.
19. **A fit check feeding a second fit check does not spill; it blanks.**
    `two_lines` (header) and `shown` (footer) choose *how many* lines to stack,
    and `centre_line` then refuses a stack the band cannot hold. Forcing either
    to its two-line answer therefore makes the band draw **nothing** — and
    nothing is inside everything, so every containment test in the file passes
    on a program whose header has vanished. The converse test the campaign
    already prescribes for whole passes has to exist for *bands* too:
    `a_band_tall_enough_for_a_line_draws_one`, sweeping the band's height and
    asserting that a band with room for one line draws one. Note it sweeps
    height only: a band can be legitimately too *narrow* to draw in, because
    `split` can collapse onto `left`, so a width sweep here would assert the
    opposite of shape 20.
20. **A guard no input reaches is indistinguishable from its own absence.**
    `push_text`'s `limit <= 0.0` refusal — the bound that stops a run being
    ellipsised into nothing — was never entered on a real window. It only bites
    where the header's `split` collapses onto `left`, which only the *squeezed*
    sweep produces. Deleting it changed no test's answer until
    `no_run_is_pushed_into_a_box_with_no_room` swept the squeezes as well as the
    window list. A guard is verified by the input that reaches it, not by
    reading it.
21. **A shared edge makes a mutation degenerate rather than visible.** `split`
    is the title's right bound *and* the counters' left bound. Widening it to
    the band's far edge does not let the title cross the counters — it leaves
    the counters nought points of room, `push_text` refuses them, and two
    columns of which one was never drawn do not overlap. The column test cannot
    fail; what dies is every test that expects a counter. This is not a fault to
    fix but a fact to record in the row's expectation, and it generalises: when
    a single number bounds two things from opposite sides, breaking it starves
    one of them instead of overlapping them.

Two smaller carries. `push_text` and `label_right` picked up sokoban's shape 9
and shape 8 fixes unchanged, and the header needed the same column split (a
title column that takes what the "Moves:"/"Undo:" counters leave) to have a real
number to pass. And three bounds in this app are genuinely **bounded by
construction** — the board solve, whose `min` over both axes does the work; the
button row, whose height is `controls.h - pad` so the centring offset is a
constant `pad / 2.0`; and the win panel, which is a *fraction* of the window and
so has a shortfall that scales. Each got the proof written out as a comment
rather than a redundant clamp, because shape 2 says the clamp would be a claim
no test could check.

And the counts to date, remeasured 2026-09-01 after this app: **46 apps and 133
sites remain**, with only automator, taskscheduler, hangman, sokoban and klotski
carrying `no_pass_paints_outside_the_region_it_owns`. The largest remaining are
rush and magnifier at 7 each, then spades and snippets at 6. **rush is next, and
it is klotski's before-state character-for-character**: same `Layout`, same five
`draw_*` passes, `push_text` still taking an `Option<f32>` with a `label()` that
passes `None`, a `label_right` with no left bound, the halo floor at line 1451,
and a `label_centred` whose doc comment claims it is "**limited to `r`**" while
passing `Some(r.w)` from a run inset by half the slack — fault 16's twin, one
app on.

**rush** (2026-09-01) — done: 124 tests, 89/89 mutation rows caught. The
prediction was exact: every fault named above was there, in those words, and the
whole of klotski's fix transferred. Shapes 14 and 15 recurred — the board's
solve sized the grid and not the mat and exit strip drawn around it, and the
halo's `(gap * 0.8).max(1.0)` floor was the same outward-growing "visibility
floor" character-for-character — and shapes 17 and 18 with them, so `board_band`
is a field here too. That is the campaign working as intended: the second app
costs a fraction of the first, and the sweep is what proves it rather than the
resemblance.

Two things this app added that klotski could not have shown.

22. **A modal that scrims the window owns the window, so the containment test
    is structurally blind to the modal's own panel.** `PASSES` gives the sheet
    `window`, correctly — it darkens the whole screen — and therefore
    `no_pass_paints_outside_the_region_it_owns` has nothing whatever to say
    about the panel the sheet's contents belong to. A heading hanging off its
    panel onto the scrim is still inside the window. Nor can `centre_line`
    object: the heading's box is a *nominal* `pad` below the panel's top and
    exactly one line tall, and a one-line box always fits itself — a bound on a
    run is only as good as the box it is measured against, and the box with the
    claim on it is the panel. Both the heading's and the hint's boxes are cut to
    the panel with `Rect::intersect` now (which bounds all four edges and
    answers `None` for a panel with no room, the same shape `centre_line`
    answers in), and `every_run_the_sheet_draws_stays_inside_its_panel` is the
    test. It must sweep **squeezed** windows as well as real ones for the reason
    `SQUEEZABLE` exists at all: at every size in `WINDOWS` the panel is generous
    enough that the nominal offsets land inside it, so a bound only those sizes
    exercise is a bound that has not been exercised. **Every app with a modal,
    an overlay or a dialog has this hole**, and it does not show up in a
    read-through, because the containment test it hides behind is present and
    green.
23. **When a guard has two halves, check whether one of them already has a
    backstop — because the half that does is the half that hides the other.**
    The guard dropping a car's letter when it will not fit the car reads as one
    condition and is really two, and replacing the whole of it with `true`
    survived all 124 tests. The height half is not its own bound: `centre_line`
    refuses whenever the box is shorter than a line, which is that half word for
    word, so deleting the guard still draws nothing at a size where the cells
    are short — shape 19 again, the check that blanks rather than spills. The
    old test stood on one small window, which was exactly such a size. The width
    half has no backstop, since `label_centred` clamps a run to the box and
    ellipsises it rather than refusing, so it is the half that had to be
    reached — and reaching it needs a car *tall* enough for its letter and too
    *narrow* for it, which only a vertical car can be. A brute-force probe over
    the 40..900 square found 2580 such sizes; 40x55 is one. The lesson for the
    remaining apps: **a guard whose halves bind on different axes needs a size
    per axis**, found by probe rather than by argument (the analytic estimate
    here said the width half was unreachable, and it was wrong), and the
    contract asserted as a biconditional with a coverage assertion per half —
    "drawn only if it fits" alone is satisfied by a program that draws nothing.

**magnifier** (2026-09-01) — done: 105 tests (from 92), 106/106 mutation rows
caught. The first app in the campaign that is not a board game, and the first
whose fixes were driven as much by the *mutation sweep* as by the containment
test. Two faults fell out of the new suite on its first run, and both were
hidden by a clip: `draw_pane`'s half-pixel seam filler spends its half pixel
outside the pane on the last block of each row and column, and `draw_help` did
not merely overflow, it **panicked** — `f32::clamp` requires `min <= max`, and a
sheet narrower than its own two margins inverts them. Neither was visible to any
of the 92 tests already there. `Frame::clip` pushes its command unconditionally,
so a bound only the clip enforces is a bound no test reading the command stream
can see; the campaign has now met that three times and it should be assumed
present wherever a pass clips.

Four things this app added.

24. **A guard that no input satisfies is not a bound. It is a delete button on
    the feature behind it, and it reads exactly like a bound.** `draw_info`
    stacked the status line under the coordinate reading behind
    `line_h + status_h <= info.h`. That condition is never true: two rows want
    2.55× the font at this face, and `Layout` gives the info band at most 2.28×
    at *every* window height from 40 to 2160, coming closest at h=309 where it
    is still 1.41px short. So every "Picked #89B4FA", every "Zoom 4x", every
    "Ruler cleared" had never been drawn, at any size, since the code was
    written — a whole user-facing feature, dead, behind a line that looks like
    careful defensive arithmetic. **No containment test can ever see this**, and
    not by accident: a run that is never emitted is trivially inside every band
    it might have left, so the campaign's central test is *structurally* blind
    to it, exactly as it is blind to a modal's own panel (shape 22). What
    pointed at it was a **mutation that survived**. The generalisation is the
    valuable part: *a survivor does not always mean a missing test — sometimes
    it means the code you broke is unreachable*, and the two are told apart by
    probing the guard's reachability before writing a test for it. A second
    tell was sitting in the file the whole time: `pick_colour_at`'s doc comment
    says the unfiltered value is "in the status line **beside** it", and the
    code stacked it *below*. **A doc comment that disagrees with the code it
    sits on is evidence about which of the two is wrong**, and it is cheaper to
    read than a sweep. The fix was not to widen the band but to move the
    feature: the info band is a one-line strip by construction (16..=30px
    against a single font), so the room it has is horizontal, and the status now
    shares the reading's row. The mutation table carries a row that puts the
    dead guard *back* — a bug found once should not be findable twice.
25. **Two clamps that cancel mean the first one is in the wrong place.**
    `status_size` was `(size * 0.92).max(7.0).min(size)`. The `.max(7.0)`
    legibility floor cannot bite — it needs `size` under 7.6, and `shows`
    refusing any band under 11 tall floors `size` at 7.7 — so it is inert, and
    inert by a *tenth of a pixel*, which is a coincidence rather than a margin.
    The `.min(size)` existed solely to undo the floor's one real effect: a floor
    is the only shape that can make a status *taller* than the reading beside
    it, and centring a taller run inside a shorter one's line box lifts it out
    through the band's top — this campaign's own fault, arrived at from the
    opposite direction. With both gone, `status_size <= size` is arithmetic,
    `status_h <= line_h` follows by monotonicity, and the offset is non-negative
    with nothing left to prove about reachability. **Look for the cancelling
    pair whenever a clamp is followed by a second clamp on the same value.**
26. **A no-overlap assertion is not a separation assertion.** Two runs that abut
    exactly do not intersect, so `a.intersect(b).is_none()` passes on a layout
    that a reader sees as one word. Any test asserting that two runs sharing a
    row stay out of each other must assert the *gap* — otherwise the padding
    between them is free to fall to zero and no test objects.
27. **When a test sweeps only the sizes the layout hands out, it says nothing
    about a bound only a squeezed band reaches — and the axis to squeeze is not
    always the axis the campaign has been squeezing.** The header's
    two-independent-guesses split (a flat `w * 0.5` for the title against a flat
    `w * 0.45` for the reading) survived the sweep, and correctly: at every size
    in `WINDOWS` the reading is far too short to reach a flat half of the band,
    because "paused" is the longest thing the header ever says and the widest
    band is 1920. The size that reaches it is narrow but **tall** — the
    reading's font follows `header.h` and the window's `big`, and *neither
    shrinks when the width does* — so a 120px-wide header on a 900px-tall window
    sets a 19px reading against a 63px half and runs the two straight through
    each other. The containment tests in this campaign squeeze both axes, but
    the ordinary tests beside them mostly sweep `WINDOWS` alone; **wherever a
    size is derived from one axis and spent on another, the fault lives at a
    band squeezed on the axis it was not derived from.** As in shape 23, the
    no-overlap claim now carries a coverage assertion — some band must actually
    have been narrow enough to collide — so it cannot silently return to
    asserting nothing.

And the counts to date, measured 2026-09-01 with `scripts/count_centrings.py`:
**49 apps and 113 sites remain**, with automator, taskscheduler, hangman,
sokoban, klotski, rush and magnifier carrying
`no_pass_paints_outside_the_region_it_owns`. The largest remaining is
**crossword at 7**, then spades at 6, then emojipicker, hearts and snippets at
5, then asteroids, camera, gomoku, maze and pomodoro at 4.

**Every count above this line in this entry was quoted from an ad-hoc grep, and
they are not comparable with the ones below it.** That is the point of the
script and it is worth stating plainly. The greps did not agree with each other:
a pattern loose enough to catch the forms rustfmt had wrapped also caught
horizontal centrings and the already-fixed helper, and a pattern tight enough to
exclude those missed every wrapped site. A raw grep gave 189 hits in 55 files
against a recorded 126 in 45, with no way to reconcile them, because neither had
written down what it counted. Nor is this hypothetical: the first draft of *this
paragraph* said "44 apps and 122 sites" — a number carried forward by
subtracting magnifier from the previous figure — and running the script gave 49
and 113. Both digits were wrong, in opposite directions, and the apps count was
wrong because the old tally had been undercounting all along. **A number in a
tracking document that cannot be reproduced is worse than no number**, because
it is quoted, subtracted from, and carried forward exactly as if it could be.

So the script is now the one executable definition of a site: it matches across
line breaks, outside `#[cfg(test)]` and comments, excludes the
`centre_line`/`centred_in`/`label_centred` helper bodies, and detects finished
apps by the presence of `fn centre_line(` rather than from a hand-kept list that
can go stale. It also reports residual matches *inside* finished apps, which is
the interesting case rather than the boring one: each is either a site bounded
by construction — with the proof in a comment beside it, per shape 2 — or a site
the campaign missed. The seven finished apps carry 21 such residuals between
them (automator 6, sokoban 5, hangman 3, klotski 2, rush 2, taskscheduler 2,
magnifier 1), and re-reading those 21 was a task in its own right: they are the
campaign's own claim that a site is safe, and until now nothing had checked it.

**That audit is done, and it found one.** Twenty of the twenty-one are bounded,
and they come in three grades, which is worth naming because the grades are not
equally trustworthy:

| Grade | How the bound is enforced | Count |
|---|---|---|
| **A — bounded at the site, proof written** | `X.min(band.h)` / `(band.h - k).max(0.0)`, *and* a comment saying why | 13 |
| **B — bounded at the site, proof unwritten** | same shapes, no comment; the reader re-derives it | 7 |
| **C — bounded only by an argument about `Layout`** | the site itself constrains nothing | 1 |

Grade A is the campaign's intended end state. Grade B is fine but costs the next
reader the derivation each time. **Grade C is a bug waiting for a layout change**,
and its one instance is `apps/automator/src/main.rs:2274` — the macro-list
trigger dot, `let dot = l.small * 0.8;` centred in a row of height `l.row - 2.0`.
Nothing at the site relates the two. It is safe *today* only because
`Layout::solve` derives both from `font`: `dot = 0.8 * max(font - 2, 8)` against
`max(2.2 * font, 14) - 2`, which over `font ∈ [9, 15]` is 6.4–10.4 against
17.8–31, a margin of about 2.7×. Comfortable — but that margin is an accident of
two unrelated formulas, and any edit to `Layout::row` or `Layout::small` can
close it silently, with no test and no comment to object.

The damning detail is that the dot is **inconsistent with its own neighbours two
lines away**: the count and the name in the same loop body both go through
`centre_line(rect, …)`, which refuses when the row is too short. The dot is the
one thing in that row that cannot refuse. That is shape 28.

> **Shape 28 — an unrelated-derivation bound is not a bound, it is a coincidence
> with tenure.** A site whose safety rests on an arithmetic relationship between
> two `Layout` fields that were never written to have one is not bounded; it is
> unfalsified. Grade C differs from grade B in kind, not degree: grade B's proof
> is a re-derivation of something the site enforces, grade C's is a re-derivation
> of something nothing enforces. Prefer the fix that costs nothing — `.min(rect.h)`
> on the fill, or `centre_line` when a refusal is the right answer — over the
> comment that records the coincidence, and reach for the comment only when
> shrinking would be wrong. The tell that found this one is **local
> inconsistency**: when two sites in the same loop treat the same band
> differently, the one that cannot refuse is the suspect.

So the residual-match report is not noise to be tuned out of the script — it is
the only thing that would have surfaced this, and it should be read on every
run, not just when the number changes.

**crossword** (2026-09-01) — done: 77 tests (from 70), 7 sites → 0.

**The mutation sweep is incomplete, and this is the honest number: 16 of 107
rows ran, all 16 caught, and the remaining 91 were never executed.** The sweep
was killed part-way by a client restart, not by a failure. Two things follow.
First, the three survivors described below were found by the rows that *did*
run, so the shapes they taught are real; what is unverified is only whether the
other 91 mutants would also have been caught. Second, a killed harness leaves
the file it was mutating **mutated in place** — `apps/crossword/src/main.rs` was
recovered from the `main.rs.bak` the harness writes, and it had a live
`while let` → `if let` mutation in `offset()` at line 863 at the moment the
process died. Anyone resuming a sweep after an interruption must diff against
that backup before doing anything else; committing the tree as found would have
committed the mutant.

Re-running the remaining 91 rows is parked with the rest of the campaign
(below). It is ~9 minutes per row of exclusive cargo-lock time — around 14
hours — which is why it is not being finished ahead of the WiFi work the
operator asked for.

The read-ahead had predicted two faults and both were
there: sokoban's shape 9 (`text_at` pushed `max_width: None`, so no run it drew
was cut to anything) and the new one — **every centring in the file was computed
against `font_size` rather than `line_height`**. The renderer places a run by
its top-left — the compositor's `draw_text` computes `baseline = y + ascent`, so
a run occupies `[y, y + line_height)` — and a box centred on the font size is
therefore short by the face's leading, 33% here. Every centred run in the app
sat **low** by half of that, which is the direction worth stating: the slack
above the run grows and the slack below it shrinks, so the edge the fault
reaches first is always the band's *bottom*.

That second fault is what makes this app worth reading, because **the campaign's
central test cannot see it, and not for want of sizes**. It produced three
separate mutation survivors before the cause was understood, and the four shapes
below are the generalisation.

29. **Containment is strictly weaker than centring, and the gap is exactly the
    band's slack.** A clue row here is `small * 1.7` tall and a line of `small`
    is `small * 1.33`; a run centred on the font size instead of the line height
    is off by a sixth of a line and *never leaves the row*. No window size
    reaches it, no `squeezes()` entry reaches it, and no squeeze ever will —
    the fault is invisible to containment **by construction**, in every band
    with more than 1.33× slack, which is most bands in most apps. This is a
    second structural blind spot alongside shape 22's "drawing nothing is
    contained" and shape 24's "an unreachable feature is contained". The
    counterweight is cheap and general: **where a pass fills a box and then
    writes in it, the fill *is* the band**, so assert that the run and the fill
    share a centre line. `a_run_in_a_band_the_drawing_filled_is_centred_in_it_
    and_not_merely_inside_it` needs no fixture beyond the ones already there and
    caught all three survivors. Every app in this campaign should have it.

    **And the seven already-finished apps were checked for it, and three have
    it.** The test is a one-line grep — an app that calls `centre_line` but
    never calls `text::line_height` is passing font sizes to it, because there
    is nothing else it could be passing:

    | App | `centre_line` calls | `line_height` calls | Verdict |
    |---|---|---|---|
    | automator | 21 | **0** | every centring is on the font size |
    | taskscheduler | 8 | **0** | every centring is on the font size |
    | hangman | 11 | **0** | every centring is on the font size |
    | sokoban | 13 | 9 | mixed — read each site |
    | klotski | 13 | 14 | mixed — read each site |
    | rush | 13 | 20 | mixed — read each site |
    | magnifier | 15 | 16 | mixed — read each site |

    So the campaign's first three apps carry the fault it has only now learned
    to see, on every run they draw, and their containment suites cannot report
    it for the structural reason above. `centre_line(rect, l.font)` reads as
    correct and is the shape the campaign has been *recommending* — which is the
    uncomfortable part. **The helper's signature is what allows it**: `size` is
    an `f32` and a font size is an `f32`, so the wrong one is spellable and
    looks right. Each of the three needs a pass and the shape-29 test, and
    `centre_line`'s own doc comment must say that `size` is a *line height*
    and never a font size. This is logged as its own work item below rather
    than folded into the per-app progress, because it is a regression in
    finished work.
30. **Pair a run to its band by the run's centre point, not by containment.**
    The first draft of that test paired a run with a fill only when the run's box
    was *inside* the fill, and the grid pass then contributed exactly zero
    checked runs: a crossword cell's fill is the cell inset by one point while
    the letter is bounded to the whole cell, so the run is never inside the fill
    it belongs to. An inset does not move a centre, so pairing on the centre
    point pairs correctly and the comparison — of centre lines — is unaffected.
    A test that pairs by containment silently drops precisely the boxes that are
    drawn tight to their band, which are the ones worth checking.
31. **A coverage counter must be per pass.** That zero was invisible behind a
    single `checked > 40` total, which the panel and footer passes met on their
    own. It only surfaced when the count was broken out per pass and each
    required to be non-zero. And the reason the grid contributed nothing was a
    *fixture* fault, not a code fault — `playing(0)` is a fresh puzzle with no
    letters in it at all, so the pass had no runs to give. **A fixture that
    stops one pass producing output hides behind the passes that do**, and an
    aggregate assertion is what lets it hide. Break the counter out by whatever
    the test iterates over.
32. **A bound that refuses rather than clamps moves the failure to a different
    test — expect the mutation row's expectation to change with it.** Mutating
    the footer's button strip to be taller than the footer used to spill; with
    `centre_line` in front of it the strip is not drawn at all, so the
    containment tests are content and the *existence* tests are what fail. This
    read as a `WRONG TESTS` verdict and looks at first like a hole in the suite.
    It is the opposite: it is the fix working. Retarget the row and say so in a
    comment, because the next reader will make the same misdiagnosis.

**The new sliver sizes found a real bug that twelve sizes had not.** The height
grid gained `(900, 26)`, `(900, 18)` and `(900, 14)` — added only because two
mutations of the help card's and end card's row fences survived for want of a
window short enough to reach them — and at 900×18 the *menu* drew its
17.29-point heading at a fixed `l.pad`, four points down an eighteen-point
window. The heading had never been in a named band at all. This is rule 3
("sample the sliver") paying out a second time, and the mechanism is worth
noting: **the sizes were added to make a mutation reachable, and they found an
unmutated fault on the way.** A size grid extended for one reason routinely
pays for itself in another.

**Two survivors here were genuine equivalent mutants, and both were *proved*
rather than assumed.** `line_height(small, Bold).max(line_height(small, Regular))`
→ `.min(…)` survives because this font's Bold and Regular faces have identical
vertical metrics — measured with a throwaway probe test, not reasoned about: 8pt
→ 10.640625 and 23pt → 30.591797 for both weights. The `.max()` stays, because
the reason it is the taller of the two does not depend on today's metrics. (The
probe also showed the cache **quantises size**, so 12.6pt → 17.29, a ratio of
1.372 rather than the 1.330 the integer sizes give. **Never compute an expected
line height by multiplying — probe it.**) The second was `let mut y =
head.bottom();` → the literal offset it used to be: `head.h` is that expression
`.min(window.h)`, so the two differ only when the window is shorter than the
heading band, and in that case both produce a first row already past the fence.
Both rows now carry a `NOT A MUTATION` note with the proof, and the second was
replaced by a row that does bind — deleting the fence itself.

One more small thing, which is shape 22 in a new dress: `inked` gives a run with
a zero width limit a zero-width box, and `inside` treats an empty box as inside
anything, so **a run the layout has left no room for passes containment because
it has nowhere to go**. A direct scan of the command stream for
`max_width: Some(w)` with `w <= 0.0` is a two-line addition to
`check_containment` and catches it; every app in the campaign should carry it.

### Open follow-up: the finished apps centre on the font size

**Status:** OPEN 2026-09-01, raised by crossword's shape 29.

**In short:** Every line of text in automator, taskscheduler and hangman is
drawn about a sixth of a line lower in its strip than it should be. It is a
small, uniform misalignment rather than a break — nothing overlaps at ordinary
window sizes — but it is wrong everywhere in three apps this campaign has
already signed off, and none of their tests can see it.

**Why they cannot see it.** `centre_line(band, size)` divides the *band's* slack
around a thing `size` tall. The thing being placed is a run of text, and a run
is `text::line_height(size, weight)` tall, not `size` tall — the compositor
draws it with `baseline = y + ascent`, so it occupies `[y, y + line_height)`.
Passing the font size understates the run's height by the face's leading (33%
at this face), which pushes the run down by half of that. It stays inside the
band wherever the band has that much slack, so containment — the campaign's
central test — is structurally blind to it (shape 29).

**Where.** `apps/automator/src/main.rs` (21 sites), `apps/taskscheduler/src/main.rs`
(8), `apps/hangman/src/main.rs` (11). The four other finished apps mix the two
and need reading site by site rather than wholesale.

**The fix, per app:** pass `text::line_height(size, weight)` at every
`centre_line` call that places a run (a *fill* is still its own height), add
crossword's `a_run_in_a_band_the_drawing_filled_is_centred_in_it_and_not_merely_
inside_it`, and add a mutation row that puts the font size back.

**And fix the helper, not just the callers.** The reason three apps got this
wrong is that `centre_line(band, size: f32)` accepts a font size and a line
height equally, and the wrong one is the shorter thing to write. Either the doc
comment must say plainly that `size` is a line height and never a font size, or
— better — the run-placing callers should go through a `centre_run(band, size,
weight)` that computes the line height itself, so the mistake is unspellable.
That is shape 9's rule ("make the wrong call unspellable") applied to a helper
this campaign introduced.

**If never fixed:** nothing overlaps and nothing crashes at ordinary sizes; text
sits slightly low in its strip in three apps, and becomes a genuine overflow at
the band's bottom edge in any strip squeezed to near one line.

### Next

`scripts/count_centrings.py` on 2026-09-01, after crossword: **spades at 6**
(`apps/spades/src/main.rs:1835, 1851, 2067, 2120, 2159, 2171`), then emojipicker,
hearts and snippets at 5, then asteroids, camera, gomoku, maze and pomodoro at 4.

A read-ahead of spades predicts crossword's two faults exactly. `text_at` pushes
`max_width: None` — but unlike crossword's, spades already has a correct
`bounded` beside it with only **three** `text_at` callers, so shape 9's proper
form here is to *delete* `text_at` rather than give it a limit, which makes
"no limit" unspellable rather than merely discouraged. And all six sites are
`band.y + (band.h - l.title | l.small | l.font) / 2.0` — the font size again, so
shape 29's test is needed from the start rather than discovered by a survivor.
Its size grid is already a 7x6 cross-product, which is better than most, but
`GRID_H` jumps `0.0 -> 57.0`: it needs sliver entries near 14 and 26, the exact
gap that has now paid out twice.

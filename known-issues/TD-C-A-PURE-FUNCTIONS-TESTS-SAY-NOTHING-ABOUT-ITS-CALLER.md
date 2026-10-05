## TD-C-A-PURE-FUNCTIONS-TESTS-SAY-NOTHING-ABOUT-ITS-CALLER -- METHOD 2026-09-17

**In short:** the desktop wallpaper offered six ways to fit a picture to the
screen -- letterbox it, crop it, tile it, centre it -- and for as long as the
setting existed all six drew exactly the same thing. The maths was right and
had six passing tests. Its only real caller handed it the *screen's*
dimensions where it wanted the *picture's*, and under that one wrong argument
every mode collapses to the same answer.

**Date:** 2026-09-17. **Lane:** C.

**What happened.** `compute_image_rect(display_w, display_h, image_w, image_h,
fit)` in `gui/desktop/src/wallpaper.rs` is correct, and six unit tests call it
with sizes like 1920x1080 against 3000x1000 and check the letterboxing
arithmetic. The one production call site was:

    compute_image_rect(width, height, width, height, self.config.fit)

-- the display size passed twice. With `image_w == display_w` and
`image_h == display_h`, `Fill` and `Fit` both scale by a ratio of exactly 1,
`Center` and `Tile` both return the rectangle they were handed, and `Stretch`
and `Span` return it directly. All six produce `(0, 0, display_w, display_h)`.

The manager had no field for a picture's size and no way to learn one: it
allocates image ids and never opens a file. The decode happens in `session.rs`,
which had the width and height in hand and dropped them on the floor.

**Why every signal was green.** The six tests exercise the function, which was
never the broken part. Nothing tested the path from the setting to the screen,
so nothing could see the caller. The Settings page offered the six modes, the
YAML stored the choice, `ImageFit` had been deliberately consolidated into
`appearance` so there was "one model" -- and the roadmap recorded the feature
`[x]` done on 2026-09-14 on the strength of all that.

**The rule.** *A unit test of a pure function proves the function. It says
nothing whatever about whether anybody calls it correctly, and a function whose
arguments are all the same type will accept them in the wrong order in
silence.* When a setting is meant to change what appears on screen, the test
that earns the `[x]` is the one that sets it two different ways through the
real entry point and asserts the results differ. Here that is
`a_letterboxed_picture_and_a_cropped_one_are_not_the_same_picture`, which
compares `Fit` against `Fill` through `get_render_commands`; sabotaged back to
the old line, it reports `Fit must leave a bar above and below: (0.0, 0.0,
1920.0, 1080.0)`, which is the original defect stated exactly.

**Related.** design-decisions §856 -- *a settings page is built when something
obeys it, not when something stores it.* This is that rule met from a new
direction: a consumer did exist, and was reached, and still made the displayed
claim false, because it was fed an argument that erased the difference.

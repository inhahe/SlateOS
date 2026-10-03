## BUG-C-A-DIALOG-BUTTON-WAS-DRAWN-ONE-WIDTH-AND-CLICKED-AT-ANOTHER — fixed in `a07710749`

**In short:** In the toolkit's dialog box, the rectangle a button was *painted*
into and the rectangle a click was *tested against* were computed in two
different places from the same constant. They agreed only for as long as every
button in the system was exactly the same width. They were about to stop
agreeing, because buttons were being given their own labels — and once a `Cancel`
sits next to a `Delete Partition`, a click aimed at one of them lands on the
other.

**Where it lived:** `gui/toolkit/src/modal.rs` — `AlertDialog::compute_layout`
stored hit rectangles using `BUTTON_MIN_WIDTH`, and `AlertDialog::render_buttons`
independently re-derived the drawn rectangle from the same constant instead of
reading the stored one.

**Why it was latent rather than live:** with a closed four-variant
`DialogButton` enum, every button really was `BUTTON_MIN_WIDTH` wide, so the two
copies could not disagree. The bug was created by the constant being copied, and
would have been *revealed* by the very next feature. `BUTTON_PADDING_H` had been
sitting unused in the file the whole time — a constant that existed for exactly
the width computation nobody had written.

**The fix:** one function, two consumers. `DialogButton::width()` measures the
label and floors it at `BUTTON_MIN_WIDTH`; `compute_layout` walks the buttons
accumulating x-positions and stores the resulting rectangles; `render_buttons`
reads those stored rectangles rather than re-deriving anything. The regression
test — `the_button_a_click_lands_on_is_the_button_that_was_drawn` — renders a
dialog whose buttons have deliberately different label lengths, pulls each
button's `FillRect` out of the render tree, clicks its centre, and asserts the
result matches that button. It tests the drawn geometry, not the stored
geometry, which is the only version of the test that could have caught the
original.

**Related:** the same change made a too-wide button row widen the dialog rather
than overflow it. Buttons are right-aligned inside the box, so the overflow ran
off the **left** edge — the near side, where it reads as an intentional inset
rather than as a mistake.

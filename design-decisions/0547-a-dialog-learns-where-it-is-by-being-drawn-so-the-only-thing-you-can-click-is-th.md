## 547. A dialog learns where it is by being drawn, so the only thing you can click is the thing that was drawn

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Every button on every pop-up dialog in the toolkit — the alert's
OK, the rename box's Cancel, the progress bar's Cancel — was drawn on screen and
could not be clicked. The code that decides *what a click hit* did not know how
big the screen was, so it guessed: it assumed an 800×600 desktop. On a real
1920×1080 desktop the invisible clickable patches sat 560 pixels to the left and
240 pixels above the buttons the user could see. This entry records the fix —
drawing is now the only way a dialog finds out where it is, and clicking can
only consult what drawing recorded — and the smaller choices that came with it.

The guess was not carelessness so much as the shape of the code forcing a bad
answer. `render` was given the parent's width and height because that is what
centring a box needs; `handle_mouse` was given only the click. So the geometry
existed at draw time and had evaporated by click time, and the click handler
recomputed it from the only numbers available to it, which were made up. The
alert's own `render` had the same problem in miniature: it computed the button
row's position twice, once for drawing and once inside `compute_layout`, and
nothing kept the two sums equal.

**`render` takes `&mut self`, and recording where things landed is a side
effect of drawing them.**
The obvious alternative is a separate `set_bounds(parent_w, parent_h)` that a
caller invokes before events are delivered, leaving `render` immutable. It was
rejected because that call is *forgettable*, and a forgotten call is exactly the
bug being fixed — a dialog with hit areas that do not match what is on screen,
which compiles, renders correctly, and fails only under the mouse. The toolkit
already had such a call: `ModalOverlay::set_content_rect` existed, and outside
the tests nothing in the tree ever made it. Tying the record to `render` makes
"you can only click what was drawn" true by construction rather than by a
convention that one call site can quietly break.

The cost is real and is the reason this is a decision rather than an obvious
fix: `render(&self, …)` is the conventional signature for a draw method, and
`&mut self` on it means a dialog cannot be drawn from behind a shared reference,
nor drawn twice concurrently into two render trees. Neither is something the
toolkit does or plans to do — a widget is drawn once per frame by the one thread
that owns the window — and the alternative trades a compile-time guarantee for a
runtime convention. If a second drawing context ever appears, the fix is to hand
`render` an explicit output parameter for the placement rather than to restore
the guess.

Checked before committing to it: `grep` finds 105 uses of the three dialog types
inside `modal.rs`, one doc mention in `textedit.rs`, and no external call sites
at all. The signature change breaks nothing outside the file.

**"Not drawn yet" is `None`, not a zero-sized rectangle at the origin.**
`ModalOverlay` held its content rect as a plain `(f32, f32, f32, f32)` starting
at all zeroes, and "is this click outside the dialog?" was answered against it.
Before the first frame that rectangle is empty, so *every* click is outside —
and a dialog that dismisses on an outside click would dismiss itself if a click
arrived between `show()` and the first frame, before the user had seen it. The
rect is now `Option`, and an unknown position classifies no clicks at all rather
than classifying all of them as misses. The same reasoning is why the progress
dialog's `cancel_rect` is `Option`, and why "there is no Cancel button" and "the
dialog has not been drawn" are deliberately the same value: to the only reader,
both mean there is nowhere on screen the click could have been aimed at.

**The hit rectangle and the drawn rectangle are one expression, not two equal
ones.** `compute_layout` now returns the button rectangles and the button row's
top edge, and `render_buttons` draws from those rectangles instead of re-deriving
them. This is what the guess was hiding: two copies of the same arithmetic can
agree today and drift on the next change to the padding, and no test would fail,
because a test that renders and then clicks at the *computed* position exercises
the same sum twice. The tests added with this change click at the *drawn*
position — they read the rectangles back out of the render tree — which is the
only form that can tell the two apart.

**A click into a text field is measured from the glyphs, not from the field's
border.** The field's clickable box includes its padding, so a click in the
padding still focuses the field; but once it is known to be in the field, the
offset is taken from the left edge of the drawn text, which is what
`textedit::draw` was given. Recording both rectangles looks redundant and is
not: using the box for measuring puts every caret a padding's width of
characters early, and using the text strip for hit-testing makes the padding a
dead zone the user can click into with no effect.

**A click in an empty field does not consult what is drawn there.** An empty
field draws its placeholder, and the placeholder is not the value — it is longer
than the empty string it stands in for. Resolving a click against it yields an
offset past the end of the text the caret actually lives in, which then reaches
`String::insert` and panics on the next keystroke. So the empty case is answered
before the geometry is: there is exactly one place typing can go.

**Where this is proved.** `scripts/reintro-modal-geometry.py` — nineteen
one-line defects, each a way the record could be unwritten, unread, stale, or
recomputed from a guess again, and every one of them a defect that compiles.

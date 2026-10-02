## BUG-C-A-DIALOG-DREW-ITSELF-ONE-HEIGHT-AND-WAS-CLICKED-AT-ANOTHER — fixed in `c9c7902bd`

**In short:** The disk imager's confirmation box grew or shrank to fit its text,
but the code that decided which button a click had hit always assumed the box
was 200 pixels tall. Whenever the text made the box a different size — which was
most of the time — the Cancel and Write buttons were clickable at a place on
screen where nothing was drawn, and unclickable where they actually appeared.

**Where it lived:** `apps/diskimager/src/main.rs` — `handle_dialog_click` opened
with `let dialog_h = 200.0_f32;` and derived the button row from it, while
`render_confirm_dialog` measured its own wrapped message and warning and used
whatever height that came to.

**Why it never fired in practice:** the button was inert (the entry above), so
the dialog never opened outside a unit test, and the unit tests called
`ConfirmDialog` methods directly rather than clicking anything. Two latent bugs
covering for each other: the reachability bug meant the geometry bug had no way
to be observed, and the geometry bug meant fixing the reachability bug alone
would have shipped a dialog whose buttons miss.

**The fix:** the whole hand-rolled dialog is gone, replaced by
`guitk::modal::AlertDialog`, which stores the rectangles it drew and hit-tests
against those. `clicking_the_verb_starts_the_write` renders the dialog, asks it
where the button that means `Ok` landed, clicks the centre of that rectangle and
asserts the write started — a test that only passes if the drawn geometry and
the hit geometry are the same object.

**The general shape, seen three times now** (here, in the toolkit's button
widths, and in the Write button above): a rectangle computed twice agrees only
until one copy is edited, and nothing warns you when they part. The fix is
always the same — compute once, read twice — and the test is always the same
too: recover the rectangle from the render tree, not from the code that was
supposed to have drawn it.

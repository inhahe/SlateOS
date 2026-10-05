## BUG-C-A-FULL-DIALOG-DROPPED-THE-WARNING-AND-KEPT-THE-QUESTION — fixed in `c9c7902bd`

**In short:** The toolkit's alert box refuses to grow past 500 pixels tall, and
when its text did not fit it stopped drawing lines at the bottom. The warning
line sits *below* the message, so the bottom is where the warning is: a long
drive name would push "This action cannot be undone" off the box and the user
would be asked to confirm an irreversible write with the word "irreversible"
missing. The name that causes it comes from the device, not from us, so its
length was never ours to bound.

**Where it lived:** `gui/toolkit/src/modal.rs` — `AlertDialog::render` drew the
message and the detail in two hand-rolled loops, each with a `break` when the
next line would cross the button row.

**How to see it:** an `AlertDialog::destructive` whose message is a drive name
repeated enough times to wrap past ~7 lines at the dialog's width. The yellow
detail text is simply absent from the render tree — no ellipsis, no indication
that anything was cut.

**The fix:** `line_budget` decides both allowances up front, before either is
drawn. The detail is served first, capped at half the column so it cannot crowd
out the question; the message gets the rest, with a floor of one line, because a
confirmation showing only its consequence with the question missing is not a
question. Whichever is cut is elided with `…` by `text::Paragraph`, which
already knew how — the old loops were a second, worse copy of wrapping logic
that existed. Three tests: that an overlong message does not take the warning
with it, that a cut is marked, and that no text reaches the button row.

**The principle:** when a box is too small, cutting at the bottom cuts whatever
happens to be last, which is a layout accident rather than a decision about what
matters least. Decide what to keep before you decide where to stop.

## 573. A dialog button carries its own label: the closed four-variant enum became an open struct

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The toolkit has a ready-made dialog box (`AlertDialog`), fully
written and fully tested, and *nothing in the tree used it* — the two programs
that needed a "are you sure?" box each wrote their own from scratch, right next
to it. The reason turned out to be one small thing: the toolkit's dialog could
only put four fixed English words on its buttons — `OK`, `Cancel`, `Yes`, `No` —
and they could only be blue or grey. Both programs needed a **red** button that
says **"Delete"** or **"Write Image"**, and the toolkit had no way to say that.
The choice was whether to keep the fixed four (which is what platform style
guides ask for, because it keeps every dialog in the system looking and reading
the same, and makes translation a solved problem) or to let a caller write its
own label and pick a "this destroys something" colour. I let callers write their
own label.

### The fixed four, and why they are the conventional answer

A closed set of buttons is not a limitation someone forgot to lift. It buys
three real things:

- **Consistency.** Every dialog in the system says `Cancel` in the same place
  with the same word. A user learns the shape once.
- **Translation.** Four strings get translated once, centrally, and every dialog
  in every program is translated with them. A hand-written `"Erase Disk"` is a
  string sitting in one app's source that a translator will never see.
- **No room for a bad label.** A caller cannot ship a button reading `Proceed?`
  or `Do it` or `Yes, really` when the convention is `OK`.

### Against the fixed four

They did not survive contact with the two callers that actually exist.
`apps/partmanager` needed to confirm deleting a partition; `apps/diskimager`
needed to confirm overwriting a whole drive. Both wanted a red button naming the
verb. The usability literature is on their side here and against the style
guides: on a destructive confirmation, a button reading `OK` tells the reader
nothing about what they are agreeing to, whereas one reading `Delete Partition`
is legible even to someone who did not read the sentence above it. That is the
case where a fixed label is not merely inconvenient but actively worse.

And the evidence of what the closed enum cost is not hypothetical. It is
`modal.rs` — 4,630 lines, ~149 test functions, and **zero callers**, with two
hand-rolled reimplementations of the same dialog living beside it in `apps/`.
The library was not unused because nobody noticed it. It was unused because it
was **unusable** for the only two jobs anyone had for it. A shared component
that the sharers cannot use is not a shared component.

### Shape of it

`DialogButton` stopped being `enum { Ok, Cancel, Yes, No }` and became a struct
of the three facts the enum had fused into four fixed combinations:

| field | what it is |
|---|---|
| `label` | what the button says |
| `result` | what it means — `DialogResult::Ok` / `Cancel` / `Yes` / `No` |
| `role` | how it is drawn — `Primary` / `Secondary` / `Destructive` |

The four old spellings survive as constructors (`DialogButton::ok()`, `cancel()`,
…), so the conventional path is still the shortest one to type and the fixed
labels remain the default a caller gets by not thinking about it. What is new is
that a caller *can* think about it: `DialogButton::destructive("Delete
Partition")` is a red button with that verb that still reports a plain
`DialogResult::Ok`, so calling code branches on meaning rather than on wording.

Three consequences fell out of the change, each of which was a latent defect the
enum had been hiding:

- **Buttons now measure their own width.** They used to all be exactly
  `BUTTON_MIN_WIDTH`, so the drawn rectangle and the clicked rectangle could be
  two separate copies of one constant and still agree. The moment labels vary
  they stop agreeing, and a click lands on the wrong button. Both rectangles now
  come from `DialogButton::width()` via `DialogLayout::button_rects`, and a test
  clicks the centre of each *drawn* rectangle and asserts the matching result.
  (`BUTTON_PADDING_H` had been a dead constant this whole time — it existed for
  exactly the width computation that was never written.)
- **A wide button row widens the dialog.** Buttons are right-aligned inside the
  box, so a row too wide for it runs off the **left** edge, which is where it is
  least expected and easiest to mistake for a layout that is simply flush.
  `dialog_width()` now takes the button row as a floor.
- **Focus can start somewhere other than the left.** See below.

### The sub-decision: a destructive dialog starts focused on Cancel

`ButtonSet::destructive_cancel(label)` is the one button set whose default focus
is not index 0. **Enter is what gets hit reflexively when a dialog appears
unexpectedly, and it must not thereby erase a disk.** The destructive button is
still the one the dialog is asking about — that is what its colour and its verb
are for — it just is not the one a stray keystroke reaches. Tab still reaches
it: the goal is un-hittable *by accident*, not unreachable.

This is why `default_index` is a field on `ButtonSet` rather than a special case
inside `AlertDialog::new`. `show()` and `with_buttons()` both reset focus to it,
the latter because the index focus was sitting on belongs to the row that has
just been replaced — leaving it put is how a dialog ends up focusing a button
that no longer exists. `with_default()` clamps rather than rejects, so a set
that later loses a button cannot leave focus pointing past the end of the row.

**Where it lives:** `gui/toolkit/src/modal.rs` — `ButtonRole`, `DialogButton`,
`ButtonSet::{destructive_cancel, with_default, default_index}`,
`AlertDialog::{destructive, with_detail, hovered_button}`, `dialog_width`,
`buttons_row_width`, `text_block_height`.

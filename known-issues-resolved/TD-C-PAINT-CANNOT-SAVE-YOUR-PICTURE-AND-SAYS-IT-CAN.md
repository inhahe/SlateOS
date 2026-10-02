## `TD-C-PAINT-CANNOT-SAVE-YOUR-PICTURE-AND-SAYS-IT-CAN` (lane C, 2026-09-21) -- **FIXED 2026-09-21**

**In short:** you can draw in `apps/paint` and you cannot keep what you drew.
There is no Save, no Open, no menu item and no key that reaches one -- yet the
program carries a list of its own shortcuts saying `Ctrl+S` saves as BMP and
`Ctrl+O` opens one. Neither key does anything. The list is not drawn on screen
either, so the false claim is currently invisible, which is the only reason
nobody has hit it.

**The three parts, each verified rather than inferred.**

*1. The file capability exists and only tests can reach it.* `save_bmp(path)`,
`load_bmp(path)`, `encode_bmp` and the BMP decoder are all implemented and
tested. `grep` for callers of `save_bmp` outside its own definition returns
exactly one hit, `apps/paint/src/main.rs:4458`, and `#[cfg(test)] mod tests`
begins at line 4407 -- so the only caller is a test. There is no file picker,
no `Target::Save`, no `"Open"` string anywhere in the crate.

*2. Two keys are advertised for it and neither is handled.*
`PaintApp::shortcuts_list()` returns 33 rows including `("Ctrl+S", "Save as
BMP")` and `("Ctrl+O", "Open BMP")`. `handle_key_press`'s `ctrl` branch matches
`z y c x v n + = - 0 f` and then `return false`. `S` and `O` are not in it; as
*plain* keys they select the Selection and Ellipse tools, so the letters are
live and the chords are dead.

*3. The list reaches no screen.* `shortcuts_list()` has two references in the
whole crate: its definition, and `test_shortcuts_list`. Nothing draws it. It is
a 33-row promise kept alive by one test -- the same shape as `apps/rssreader`'s
overlay of twenty-one shortcuts of which four worked, except that this one is
not even visible enough to be noticed as wrong.

**A fourth, found in the same read.** `handle_text_char` -- the text tool's
typing path -- also has exactly two references: its definition and one test.
The live key path (`handle_key` -> `handle_key_press`) never calls it, and
`handle_key_press` maps bare letters to tools. So with the Text tool active,
typing `Hi` does not type `Hi`: `h` flips the canvas horizontally and `i`
switches to the eyedropper. The text tool cannot be typed into at all.

**Why this is one entry and not four.** All four are the same failure with
different endings: a capability is built, a claim about it is written, and
nothing joins either to a user. The tests pass throughout, because each test
calls the function directly -- which is exactly what makes a test unable to
notice that nothing else does.

**What the proper fix looks like.** `gui/toolkit/src/dialog.rs` already has a
`FilePicker` and `apps/markdowneditor` already drives one, so Save and Open are
a wiring job rather than a new subsystem: `Ctrl+S` and `Ctrl+O` raise the
picker, its answer goes to `save_bmp`/`load_bmp`. The text tool needs
`handle_key_press` to route a character to `handle_text_char` while the Text
tool is placing text, before the tool-letter match. The list should become the
`SHORTCUTS` const that design-decisions 863 describes, drawn by
`guitk::shortcut::render_card` behind `F1`, with
`every_advertised_key_does_something` guarding it -- that guard is what would
have caught `Ctrl+S` on the day it was written.

**Order matters here.** Add the guard *before* wiring the keys: it fails on
`Ctrl+S` and `Ctrl+O` and names them, which is the difference between fixing
two keys and believing there were only two to fix.

**If this is never done,** a user who draws something loses it when the window
closes, having been told there is a key that saves. This is the most damaging
entry currently open in this lane's list, because unlike a missing shortcut it
destroys work the user has already done.

### Fixed the same day, and the guard found five more

`Ctrl+S` and `Ctrl+O` now raise `guitk::dialog::FilePicker` and its answer goes
to `save_bmp`/`load_bmp`. The dialog takes the keyboard while it is up, because
a dialog that does not is not modal -- without that, typing `blue.bmp` would
also be typing tool shortcuts into the canvas behind it and the `b` would swap
to the pencil.

**The guard was added first, and that was the whole value of it.** Its first
run failed on `Ctrl+S`, as expected. Its next five runs failed on keys nobody
had suspected:

| key | what was wrong |
|---|---|
| `Ctrl++`, `Ctrl+-`, `Ctrl+0` | zoom chords: a chord produces no text, and the `Key`-to-`char` fallback listed only letters, so no character ever arrived |
| `Ctrl+F` | fit-to-window: `Key::F` was simply missing from that same list |
| `[`, `]` | brush size: not letters either |

The fallback listed 16 of the 26 letters and none of the punctuation. It now
maps all 26 and the five non-letter keys this program binds, so the class is
closed rather than the six instances. **Had the keys been wired without the
guard, five of the eight would still be dead** -- which is the argument for
writing the guard before the fix rather than after it.

`Enter`, `Delete` and `Escape` turned out to be *correct* refusals on a blank
canvas -- they finish a polygon, clear a selection, and cancel whichever is in
progress. So the guard takes a small set of states and asks whether any of them
answers the key, rather than demanding a blank canvas answer everything.

The text tool is routed too: a character typed while a text box is being placed
goes to `handle_text_char` before the tool-letter match.
`typing_with_the_text_tool_types_instead_of_firing_shortcuts` pins it, and was
confirmed to fail with the routing removed. It carries a control -- the same
letters with no text box still select their tools -- because a test of only the
typing case would pass equally well if the tool shortcuts had been deleted.

Two smaller things fixed in passing: `save_bmp` and `load_bmp` took `&str` and
now take `&Path` (both turned it straight back into a `Path`, and the `&str`
forced UTF-8 on a filename), which let a `to_string_lossy` be deleted from the
one test that called them.

The 33-row list is now a `SHORTCUTS` const drawn by `render_card` behind `F1`.
`?` is deliberately *not* a second way in: this app has a text tool, so a `?`
has somewhere to go, which is the `apps/spreadsheet` case design-decisions 863
carved out.

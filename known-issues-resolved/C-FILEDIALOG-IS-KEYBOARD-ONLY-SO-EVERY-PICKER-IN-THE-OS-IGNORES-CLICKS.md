## C-FILEDIALOG-IS-KEYBOARD-ONLY-SO-EVERY-PICKER-IN-THE-OS-IGNORES-CLICKS (lane C, 2026-09-02) -- **CLOSED; entry was stale**

**Closed 2026-09-07 (lane C), on finding it had already been fixed and never
marked.** `FileDialog::frame(width, height) -> Frame<DialogTarget>` and
`FileDialog::handle_mouse` both exist, and the conversion went further than
this entry asked for:

- Every control the entry listed is a hit target -- back/forward/up,
  sidebar shortcuts, sort headers, rows, the confirm and cancel pair.
- Both rules the entry said the conversion must not break are kept, and
  visibly: a double-click on a row activates where a single click selects
  (`MouseEventKind::DoubleClick` acts only on `Entry`, with a comment on why
  every other control must not act twice), and `DialogTarget::Chrome` exists
  precisely so a host can tell a click *on* the dialog from a click past its
  edge -- "the click a modal must not let through".
- Two controls are recorded but inert on purpose: `AddressBar` and
  `FilenameInput` swallow a click rather than letting it fall through to the
  list behind, which is what a user expects of a control they can see.
- The scrollbar got a draggable thumb and a wheel path with it.

24 click/mouse/drag tests in `dialog.rs`; 110 dialog tests pass.

**The dependents named in the entry have also moved on.**
`C-VPNMANAGER-IMPORT-EXPORT-HAVE-NO-FILE-PICKER` is marked fixed 2026-09-03,
and `apps/archivemanager` has a test called `the_pickers_rows_can_be_clicked`.
Its table of "three reasons `FileDialog` cannot serve as a picker" led with
"keyboard-only; it records no hit targets" -- that row is no longer true, and
is left in place as the record of why the fixed path was chosen at the time.

**Nothing was done to the code for this closure.** It is a documentation
correction: the entry described a defect that had been repaired, and an entry
that overstates what is broken costs the next reader the same investigation it
cost me.

Original entry follows.

---


**In short:** every Open / Save / Choose-folder dialog in SlateOS can only be
driven with the keyboard. The dialog draws a list of files, a sidebar of
shortcuts, a toolbar and OK/Cancel buttons — and clicking any of them does
nothing at all. Arrow keys, Enter, Backspace and Escape work; the mouse does
not. A user who clicks a filename and then clicks OK has selected nothing, and
the dialog will act on whatever row the keyboard cursor happened to be on.

**Where it lives:** `gui/toolkit/src/dialog.rs`. `FileDialog::render` returns a
flat `Vec<RenderCommand>` — pure ink, with no record of *where* anything was
drawn — and there is no `handle_click` on the type at all. The absence is not
hidden: `apps/archivemanager/src/main.rs` says so at its own call site, "it is
drawn straight into the tree rather than through the frame because it records no
hit boxes".

**What a user sees:** the dialog looks exactly like a normal file picker. Rows
highlight on the keyboard cursor, so it even looks live. Clicking a row does not
select it; clicking OK does not press it; clicking a sidebar shortcut does not
navigate. Nothing reports an error, because from the program's point of view
nothing was clicked — the host window swallows the click to stay modal and
throws it away.

**Who is affected right now:** `apps/archivemanager` has four pickers (Open,
Extract All, Extract Selected, and — since 2026-09-02 — New and Add, so six),
and every one of them is keyboard-only.
`C-VPNMANAGER-IMPORT-EXPORT-HAVE-NO-FILE-PICKER` is a caller that declined to
use the widget *because* of this, and writes to one fixed path instead. The
start menu's Run box wants the same widget (see the entry at the "What is
actually missing" section for it). Every future app with an Open button
inherits the bug.

**Why it is like this:** the widget was written before `guitk::frame` existed and
was never revisited — it had zero users tree-wide until 2026-08-25. The rest of
the toolkit has since settled on "rendering and hit-testing are the *same
walk*": a `Frame<T>` records a target for each rect as it is drawn, intersected
with the clip and translation in force, so ink and click target cannot end up in
different places. `FileDialog` is the last widget of any size that still draws
into a bare command list.

**What the proper fix looks like:** convert `render` into one walk that draws
into a `guitk::frame::Frame<DialogTarget>`, with `DialogTarget` naming the file
rows, sidebar shortcuts, toolbar buttons, sort headers, the filter control and
the OK/Cancel pair. `render(width, height) -> Vec<RenderCommand>` stays as a
thin wrapper over that walk so existing callers do not change. Add
`handle_click(&mut self, x, y, width, height) -> DialogAction`, which hit-tests
that frame and returns the same `DialogAction` the keyboard path already
returns — so a host only has to forward mouse events the way it already forwards
keys. Two rules the conversion must not break: a double-click on a directory
navigates (a single click selects), and a click outside the dialog must not
reach the window behind it, which is what makes it modal.

**What must not be done instead:** hit-testing by recomputing the row geometry in
the caller. That is a second copy of the layout, and the bug is then in whichever
copy you are not reading — exactly the failure
`C-NETMANAGER-CLICKED-ROWS-THAT-WERE-NOT-ON-SCREEN` documents, where two private
copies of `Frame` disagreed about clipping.

**Fixed 2026-09-03**, along the prescribed line and with the two rules intact:
a single click selects, a double-click on a directory navigates, and every
pointer event — including one landing past the dialog's own edges — is swallowed
so nothing reaches the window behind. `render` is now a thin wrapper over
`pub fn frame(width, height) -> Frame<DialogTarget>`, so no caller had to change
how it draws; `handle_mouse(&mut self, event, width, height) -> DialogAction`
hit-tests that same frame. `apps/archivemanager` (six pickers) and
`apps/diskimager` forward the pointer to it now.

Three things the fix turned out to require that the plan above did not name:

- **Scrolling, or the fix would have been half a fix.** The list culled every
  row past the bottom edge and had no offset at all, so the tail of a long
  directory was unreachable by *any* means — the keyboard did not scroll to
  follow its own selection either. Shipping a clickable list whose rows below
  the fold cannot be reached would have been a picker that still could not open
  most files. There is now a wheel, a keyboard reveal, a page step, and a
  scrollbar that can be *dragged* — a drawn-but-inert one would only reproduce
  `C-SPREADSHEET-SCROLLBARS-ARE-DRAWN-BUT-NOT-DRAGGABLE`. The thumb's track and
  rect are read back out of the frame that was just drawn (`Frame::rect_of`),
  not recomputed, for the reason the paragraph above gives.
- **Reveal has to be stateful, not re-imposed at render time.** The obvious
  design — "render always scrolls to show the selection" — makes the wheel
  useless, because every notch is undone by the next frame. So the scroll
  follows the selection only when a *keystroke moves* it; explicit scrolling is
  allowed to leave the selection off screen, which is what every file manager
  does. `the_wheel_may_scroll_away_from_the_selection` pins it.
- **The dialog stores no size of its own.** `render(&self, width, height)` is
  given its size and cannot write anything back, so a stored copy would be a
  second answer to "how big is this dialog" that can disagree with the
  renderer's — the same class of divergence as a recomputed hit box. Every
  method that has to move the scroll takes the size as an argument instead,
  which is why `handle_event` grew a `height` parameter and the three callers
  had to be touched.

Two pre-existing bugs surfaced while doing it, both invisible only because
nothing could reach them, both fixed here:

- `toggle_sort` set the sort field and never reordered `self.entries`. The
  moment the headers became clickable that would have been a column header
  which moves its own little arrow and nothing else. It now re-sorts, and
  follows the picked *entry by name* through the reordering — keeping the row
  *number* would let a click on "Size" change which file Open opens.
- Alt+Backspace decided whether to report `NavigatedTo` by asking whether any
  history remained *afterwards*. Going back to the first directory of the
  session — the one case that empties the history — therefore reported no move,
  and the host left the previous directory's files on screen under the new
  directory's name. All three navigations now compare the path before and
  after.

**Still open after this:**
`C-VPNMANAGER-IMPORT-EXPORT-HAVE-NO-FILE-PICKER` — the caller that declined to
use the widget because of this bug — is now unblocked but not yet done. The
start menu's Run box is likewise still waiting on a caller, not on the widget.
`TD-C-FILEDIALOG-PATHS-ARE-STRINGS-SO-A-NON-UTF-8-FILENAME-OPENS-THE-WRONG-FILE`
is untouched by this change and remains open.

## TD-C-THE-PDF-VIEWER-PRINTS-EVERY-PAGE-OR-NOTHING

**Date:** 2026-09-07. **Lane:** C.
**Where:** `apps/pdfviewer/src/main.rs` — `print_job`, `print_active`,
`Target::Print`.

**In short:** The PDF viewer has a Print button and no print dialog. There is
nowhere to say which pages, how many copies, double-sided or not, colour or
grey. Pressing Print sends the whole document, one copy, every time. All the
settings exist in the code and none of them can be reached.

**How this came to light.** Moving the page-range parser out into
`gui/printjob` left `PageRange` imported and unused in the viewer's own
source: nothing outside the tests ever *builds* a range. The parser has been
there for months, is the best one in the tree, and has never had a caller
that was not a test — which is the same "a feature whose only caller is its
own test does not exist" shape found twice before in this lane.

The four sibling fields are worse: `copies`, `duplex`, `color` and `scale`
had no reader *and* no writer. They are no longer dead in the same way, since
they are now fields of the message the printing service will receive rather
than of a private struct, but nothing sets them either.

**What a user sees.** Print a 400-page manual to read one page of it, and you
get 400 pages. There is no way to say otherwise short of editing the file.

**The proper fix.** A print dialog: page range box, copies spinner, and the
duplex/colour/scale controls, writing into `print_job`. The range box wants
`PageRange::parse`, which already accepts `1-3, 5, 7-9` and is tested. The
dialog is the whole of the work; the model behind it is finished.

**Why it is not done here.** A print dialog is a feature, and this change was
a refactor — folding one in would have hidden a behaviour change inside a
move. Also worth doing *after* the printing service exists (§540), since the
dialog should show the printers the service reports rather than a list the
viewer invents, and there is no service yet.

**Not blocking.** Printing works; it just always prints everything. Nothing
regressed here — this is a gap that was invisible until the parser moved out
and left an unused import pointing straight at it.

**Fixed 2026-09-13. There is a dialog.** Pages (all / current / a typed
range), copies, colour, sides and scale, committed to `print_job` on Print
and discarded on Cancel. Eleven tests, every one of them driven through
`probe::click` on the controls rather than by assigning to the job.

Three things the work turned up that this entry did not anticipate:

- **`PageRange::CurrentPage` has no spelling.** `parse("")` is `All` and no
  input produces `CurrentPage`, so a free-text box alone would have left a
  variant of the message format unreachable from the only program that
  sends one. Hence a cycling Pages button and not just the range box this
  entry asked for.
- **The range box keeps the user's text, not a rendering of the parse.**
  `PageRange` has no `Display` and should not grow one: `parse("1-3,5")` is
  `Custom([(0,2),(4,4)])`, and writing that back out is a second conversion
  that can disagree with the first — re-rendering `7-7` as `7` would
  quietly edit what someone typed.
- **Typing has to select the custom choice.** Otherwise a user types `2-4`,
  presses Print, and gets every page: the exact defect this entry describes,
  reintroduced one control along and invisible, because both the typed text
  and the output would look deliberate.

**A vacuous test, caught before it was committed.** The first version of
`a_miss_inside_the_dialog_does_not_page_the_document` used `probe::click`
on the dialog panel — which clicks the *centre* of the box it finds, and
the centre of the panel is the Copies stepper. It pressed "+" and then
asserted the page had not changed, which was true and proved nothing. It
now asserts `target_at` returns the panel at the point it is about to
click, before clicking there.

**Still absent: a printer chooser.** There is no printing service to ask
which printers exist (§540), so the dialog configures the job and does not
choose a destination. That half is unchanged and still waits on the
service; the half a user feels — printing one page of a four-hundred-page
manual — does not.

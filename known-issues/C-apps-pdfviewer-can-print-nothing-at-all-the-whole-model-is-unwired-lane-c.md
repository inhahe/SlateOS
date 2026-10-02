## `apps/pdfviewer` can print nothing at all — the whole model is unwired (lane C)

### Update 2026-09-07: the blocker named here is answered, and the seam now exists

**This entry says "*how* to wire it is `C-Q4`". C-Q4 was answered by the
operator on 2026-08-21 and written up as `design-decisions.md` §540:** printing
becomes a background service applications submit jobs to, chosen over a shared
library on the grounds that a print job is not part of the application's
lifetime. The architecture question this entry defers to is settled; what
remained was that nobody had built the thing §540 says an application depends
on -- *"a message format, not code"*.

**`gui/printjob` is that format**, added today. It is the union of the two
vocabularies this entry tabulates: the discontiguous page range from the
viewer, and paper/copies/duplex/quality/scale/collate from the desktop. It has
no printers, no queue, no I/O and no dependencies -- linking it does not let a
program print, it lets a program *say what it wants printed*. 15 tests.

**Deliberately nothing submits one.** §540: *"until the service exists, neither
half gains new callers"*, and wiring an application straight to the desktop's
printing code is the stop-gap it refused.

**There are now two page-range parsers, and that is temporary and tracked.**
§540 says the viewer's parser *"should be lifted into the job format, not
reimplemented"*; it has been lifted, and `apps/pdfviewer` has not yet been
moved onto the lifted copy. That is 71 references and a dozen tests, and the
two do not agree in three places, so it is a considered migration rather than
a rename:

| input | `pdfviewer` today | `printjob` |
|---|---|---|
| `9-7` (reversed) | dropped | read as 7-9 |
| `x-5` (bad start) | becomes 1-5, silently | dropped |
| `1-5, 3-7` (overlapping) | page 3 printed twice | printed once |

I think `printjob` is right on all three -- a reversed span plainly means
something, a defaulted-to-1 start invents a page the user did not type, and
paper spent twice on one page was not asked for -- but each is a behaviour
change to a shipped app and wants its own commit, not a silent swap at the end
of another one.

**Next:** move `pdfviewer` onto `printjob::PageRange`, then the service.

**Status: OPEN.** Recorded 2026-08-16 by lane C. *How* to wire it is `C-Q4` in
`open-questions.md`, because it is an architecture choice rather than a repair.

`PrintSettings`, `PrintPageRange`, `parse_page_range` and `resolve_pages` are
complete, and as of today correct and tested. **Nothing outside the test module
calls any of them.** `PdfViewerApp` holds a `print_settings` field written once
at construction and never read. There is no print dialog, no Ctrl+P handler,
and no path from the viewer to any output.

This is the `filediff` find-in-diff shape again — a feature fully modelled and
never connected — with a twist that makes it an architecture question rather
than a missing render pass: **`gui/desktop/src/print_manager.rs` already
contains a full printing stack**, 1409 lines of it, with `Printer`,
`PrinterCapabilities`, a `PrintManager` job queue offering submit/cancel/pause/
resume, a spooler flag, and a `PrintDialog`. It is used only by
`gui/desktop/src/main.rs`. No application in `apps/**` submits a print job.

So the tree has two print models that do not know about each other, and each is
richer than the other in a different dimension:

| | `pdfviewer` | `gui/desktop` |
|---|---|---|
| Page range | discontiguous list — `1-3, 5, 7-9` | one `(start, end)` pair |
| Copies, paper, duplex, quality, scale | absent | present, with capability validation |
| Job queue, spooler, cancel/pause | absent | present |
| Reachable from an app | no | no |

Neither is wrong; they were written for different halves of the problem. The
range belongs with the document, which is the only thing that knows its page
count and the reader's current page; everything else belongs with the system,
which is the only thing that knows what printers exist. What is missing is the
seam between them, and choosing it is the open question — an `apps/**` crate
depending on the desktop shell crate would be the cheap answer and the wrong
shape.

Until that is answered, the honest description is that **the PDF viewer has no
print command**, and the tested range logic is a component waiting for one.

### Sweep progress: `ebook` 20 → 0, all lint classes (2026-08-16)

Tenth crate, fifth to reach **zero warnings of every class** across
`--all-targets`. Tests 114 → 119. As with `filediff`, the lint work was small
and the finding was not a lint: **the reader's position in a book was stored as
a page number**, and a page number is a fact about a *pagination*, which
depends on the font size and the window size. Pressing `+` once moved the
reader from page 2 of four to page 2 of six — backwards through the book, by
about a fifth of it, on one keystroke.

The shape is one already recorded in this sweep — *an index into a derived
layout stored across a rebuild of that layout* — and this is its third
appearance (after `filediff`'s edit-index-as-display-row and the stale search
matches). The fix is always the same: **store the stable thing and derive the
volatile one.**

What makes this instance worth recording separately is that the codebase
already contained the right answer. `Chapter` stored a `byte_offset`, with a
comment saying why. The reading position and the bookmarks were simply the two
places that had never been given the same treatment — so this was not a design
that needed inventing, it was a design that had not been applied uniformly.
That is a cheaper class of bug to look for than it sounds: *find the type that
already got it right, then check its siblings.*

| Was | Is |
|---|---|
| `ReadingState { current_page: usize }` | `ReadingState { offset: usize }`, with `current_page()` derived from the live pagination |
| `bookmarks: Vec<usize>` of page numbers | `Vec<usize>` of byte offsets; `is_page_bookmarked` is a containment test against the page's range, not an equality test |
| `jump_to_chapter` converted the chapter's offset to a page and jumped to that | jumps to the offset directly, so it no longer rounds the reader to the top of whichever page happens to contain the chapter |
| `repaginate` needed a fix-up for the stored page | needs none, which is the point |

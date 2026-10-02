## `C-PDFVIEWER-HAS-NO-PDF-PARSER-AND-NO-PRINT-SPOOLER` (lane C, 2026-08-26) — **open**, missing backends

**In short:** The PDF viewer now opens as a real window and everything in it
works — tabs, zoom, rotation, continuous scrolling, the sidebar's thumbnail /
bookmark / annotation panels, find-in-page, the print dialog — but it cannot
open a PDF and it cannot print one. Nothing in SlateOS can yet turn a `.pdf`
file into pages, and there is no print spooler for anything to be sent to. So
the app opens on an empty tab, the recent-files list is inert, and the Print
button is not drawn. It is a complete, tested front end waiting for two back
ends.

**What is missing, precisely.** Two things, neither of which exists anywhere in
the tree:

1. **A PDF parser.** Everything the app draws is derived from `PdfDocument` —
   page sizes, the page count, the outline that fills the Bookmarks panel, the
   `/Title` metadata that names the tab, the text that find-in-page searches.
   `PdfDocument` is a plain data structure with no reader: there is no code that
   can produce one from a file. A real one is a substantial port (xref tables,
   object streams, Flate/LZW/DCT filters, font and CMap handling), not a
   weekend's parsing.
2. **A print spooler.** There is no print service, no queue, and no driver
   interface. The dialog collects a page range, a copy count and duplex/colour
   choices, and has nowhere to send them.

**How the gaps are held open rather than papered over.** Each is a function
pointer seam on `PdfViewerApp`, following the precedent set by defrag's
`ScanFn` (see `C-DEFRAG-HAS-NO-WAY-TO-SCAN-A-DRIVE`):

| Seam | Type | Gate |
|---|---|---|
| `open` | `OpenFn = fn(&Path) -> Option<PdfDocument>` | `can_open()` |
| `print` | `PrintFn = fn(&PdfDocument, &[usize]) -> bool` | `can_print()` |

Both are `None` in the shipping binary. Each gate governs the control's hit box
and its appearance off the *same* condition, so a control that cannot work is
never drawn as though it can: with no parser the recent-file rows are drawn as
plain text and record no hit box, and with no spooler the Print button goes
through `render_disabled_button`, which greys it and — the part that matters —
records nothing for `hit_test` to find. There are no dead buttons.

Every test that exercises either path installs its own backend (`a_document` /
`refuses` for the parser, `accepts` for the spooler; installed through
`set_opener` / `set_printer`), and each path is
tested from **both** sides: the control is absent with no backend, and present
and working with one. That makes the missing backends impossible to forget
while working on this file.

**Two alternatives were rejected**, for the same reasons as in defrag. Shipping
a fabricated document so the viewer has something to draw would put a lie on
screen. Shipping a Print button wired to a function that always fails would be a
control that looks live and does nothing — the exact failure this lane has spent
weeks removing from app after app.

**Proper fix.** When a PDF library exists, write the real `OpenFn` against it
and set `app.open = Some(...)` in `main`; likewise `app.print` when a spooler
exists. Nothing in the UI needs to change — the seams were chosen so that the
window is already correct on the day the data arrives.

**Where it bites:** `apps/pdfviewer/src/main.rs` — `OpenFn`, `PrintFn`,
`PdfViewerApp::open`, `PdfViewerApp::print`, `PdfViewerApp::can_open`,
`PdfViewerApp::can_print`, `render_disabled_button`, and `main`.

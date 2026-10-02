### [E] The ebook reader could open no book but five invented ones -- 2026-09-27
**Status:** FIXED (lane E, 2026-09-27).

**In short:** the ebook reader opened on "The Clockwork Garden" by "Eleanor
Voss" and four more books that do not exist, said "cannot open your own files
yet", and had no way to open one. Where you were in a book and your bookmarks
were forgotten when the window closed. It now opens plain-text books from
disk and keeps them: the library, and each book's place, bookmarks and type
size, are the same the next time.

**What it does now.** Ctrl+O, or *Open a book* in the library's toolbar, puts
up the Open dialog (which takes the keyboard while it is up, so a key meant
for a file name cannot turn a page). A book is read by what its bytes are:
UTF-8 (a byte-order mark dropped), UTF-16 with a byte-order mark, and anything
else as Windows-1252 -- the encoding of most older plain-text books -- with
the row saying "Read as Windows-1252", never a lossy decode. Its title and
author are the ones a Gutenberg-style header gives, or the file's name and no
author rather than an invented one. A file past 64 MiB is read that far and
says so; a character the cap cuts in two is dropped rather than taken as
proof the file is not UTF-8. The library is kept in
`<config>/ebook/library.txt` (one line a book, paths spelled with `pathcodec`
so any name survives), saved on adding or removing a book, on returning to
the library, and on close.

**The ways it refuses to lose things:**
- A library file that does not read is left as it is and never written over:
  the window says so, and nothing opened is kept until it is dealt with.
- A close that cannot save says why and stays open once; the next close goes.
- A book whose file has gone stays in the library with its place, marked
  "Cannot be read" with the reason; it opens where it was if the file comes
  back. A place inside a file that has changed moves back to the nearest real
  one (never inside a character, never past the end).
- Delete asks before taking a book out of the library, and never touches the
  file.

**Where.** `apps/ebook/src/shelf.rs` (the file, and `read_book`);
`apps/ebook/src/main.rs`: `EbookApp::with_shelf`, `open_path`,
`open_selected`, `remove_book`, `keep` (`Kept`), `fit_state`, and the
picker's routing in `on_event`. Tests: 30 new, on Windows and on Linux under
WSL; mutation: `apps/ebook/mutate.py`, 23 rows.

**Not done:** EPUB, the format most books are sold and lent in (a zip of
XHTML chapters) -- the obvious next format. (The reading theme, System or
Sepia, was per session too; since 2026-09-27 it is kept in `ebook.yaml`, the
per-program settings file `design-decisions.md` §1418 (C-Q26) settles.)

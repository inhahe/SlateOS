## TD-C-THE-COLUMN-VIEW-DOES-THE-ONE-THING-THE-SPEC-FORBIDS -- FIXED 2026-09-16

**Fixed the same day, in the order this entry set out.** The picker was built
first (`apps/explorer` column menu on the Details header, with per-folder and
global saves through `columnprefs`), and `auto_detect_columns` was deleted in
the change that made it unnecessary -- not before, because until the picker
existed the guess was the only way any column beyond the default set appeared.

The removal reached further than the function: **six tests pinned the forbidden
behaviour**, all passing, and `FileInfo` existed only to be passed to it. A
test can be a careful, green assertion that the wrong thing happens, and
nothing about it looks wrong from inside the file -- the compiler found four of
the six after the first two were deleted by hand.

One difference from the spec is left and is smaller: the out-of-the-box set is
name, size, date-modified **and type**, where §4.1 says "fixed and minimal --
name, size, datetime modified". Not folded into this change because it is a
different bullet, and because the reason §4.1 gives for minimal ("the user
expands from there") only became true when the picker landed.

**Date:** 2026-09-16. **Lane:** C.
**Where:** `apps/explorer/src/columns.rs` — `auto_detect_columns` (~587);
`apps/explorer/src/main.rs` — `load_directory` (~929), `detect_columns`
(~3525); `roadmap-detailed.md` §4.1 → "No content-based column
auto-selection".

**In short:** the file list looks at what is inside a folder and adds columns
to match — walk into a folder of photos and a Dimensions column appears. The
design says, in bold, that this must never happen, and gives four reasons for
it. This is not a missing feature; it is a built one that the design
prohibits, and it runs on every directory listing.

**The two texts:**

| | says |
|---|---|
| `roadmap-detailed.md` §4.1 | "**No content-based column auto-selection.** The OS never inspects a directory's contents to decide which columns to show. A folder containing only audio files does *not* automatically gain bitrate/length/sample-rate columns; a folder of photos does *not* automatically gain width/height/camera columns." |
| `columns.rs:587` | `pub fn auto_detect_columns(&mut self, files: &[FileInfo<'_>])` — scans the entries, sets `has_image`/`has_audio`/`has_code`/`has_archive`, and pushes `ColumnId::DIMENSIONS` and friends accordingly. |

`load_directory` calls it after every listing, so the behaviour is live and the
spec's four objections all apply as written: the column set jitters as the user
navigates, one off-type file changes the shape of the view, "why did my columns
change?" has no answer a user could reach, and every listing pays for a type
scan.

**Do not simply delete it, and this is the important half.** The spec's
replacement is a user-driven column picker — show/hide from the header row,
saved per folder or globally. **That picker does not exist**: there is no
`ColumnChooser`, no target for one, and no rect for one anywhere in
`apps/explorer`. Auto-detection is currently the *only* way any column beyond
the default set ever appears. Removing it to comply with the spec would leave
the user with name/size/date and no way to ask for anything else — strictly
worse than today, and delivered as a correctness fix.

**One thing the picker will need that does not exist yet, found while sizing
it.** Its "save as default for this folder" has to write down *which* columns,
and neither field of `ColumnDef` can carry that:

* `id: ColumnId(u32)` is a position in a hand-numbered list. Saving integers
  means a renumbering silently repoints every saved preference at a different
  column -- the `FileTypeInfo::default_app` failure exactly, where stored data
  looked authoritative and resolved to the wrong thing.
* `label: String` is display text, and the const table sets it to
  `String::new()` with the comment "replaced at runtime", so it is not even
  populated at rest.

So the persistence format needs a stable third identifier -- a `key:
&'static str` per column, never shown and never renumbered -- set at the 21
`ColumnDef` construction sites, with a test that the keys are unique and that
an unknown key in a saved file is skipped rather than guessed at. That field
should land *with* the persistence that reads it and not before: a key nothing
consults is the unused-field shape this entry's neighbours are about.

**So the order is fixed:** build the column picker and its persistence first,
then remove `auto_detect_columns` in the same change that makes it
unnecessary. Both halves are lane C and neither is blocked on another lane.

**How this survived.** Someone improved this function recently -- its comment
records replacing a fourth hand-written extension list with the shared registry,
and notes that the old list disagreed about `.webp`. The change was a real
improvement to a function that should not exist. **A thing can be carefully
maintained for a long time without anyone asking whether the spec wanted it**,
because maintenance asks "is this correct?" and only a reader of the design
asks "should this be here?"

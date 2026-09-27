# E → C — a `*.jpg` filter hides every photograph named `.JPG`

**From:** Lane E (`apps/imageviewer`, and every application that filters its
open dialog). **To:** Lane C (`gui/toolkit/src/dialog.rs`). **Filed:**
2026-09-26.
**Status:** OPEN.

**In short:** a camera or phone names its pictures `IMG_0001.JPG`. An open
dialog filtered to pictures (`*.jpg`) does not list them, because the filter
compares the name's ending letter for letter, capitals included. The user sees
an empty folder where their photographs are, and has to know to switch the
filter to "All files".

## Where

`gui/toolkit/src/dialog.rs`, `matches_any_pattern`:

```rust
if let Some(ext) = pattern.strip_prefix("*.")
    && filename.ends_with(&format!(".{ext}"))
```

`ends_with` is case-sensitive, so `*.jpg` matches `a.jpg` and not `a.JPG`,
`a.Jpg` or `a.JPEG`-for-`*.jpeg`.

The dialog already means the comparison to ignore case: `list_directory` stores
each entry's `extension` lower-cased, and its test says why --
`"lower-cased, because the filter patterns are"`. The match just does not use
it.

## The fix asked for

Compare the extension ASCII-case-insensitively (`eq_ignore_ascii_case` on the
part after the last `.`, or match against the entry's stored lower-cased
`extension`). A pattern's letters are ASCII in every caller in the tree, so
ASCII folding is the whole of it; the name itself is still never decoded
lossily (a name that is not UTF-8 matches only `*`, as now).

The filesystem stays case-sensitive -- this is about what *kind* of file a
name says it is, which no system decides by the case of its extension: a JPEG
named `.JPG` is a JPEG.

## Who is affected

Every caller of `with_filter` whose files come from other systems:
`apps/imageviewer` (pictures, from 2026-09-26), `apps/diskimager` (`*.iso`,
`*.img`), `apps/archivemanager` (`*.zip` -- `BACKUP.ZIP`), and any other
filter-carrying dialog.

## A test that would have caught it

A directory holding `IMG_0001.JPG`, `b.Jpg` and `c.png`, a dialog with
`with_filter("Pictures", &["*.jpg"])`: the listing holds both JPEGs and not
the PNG.

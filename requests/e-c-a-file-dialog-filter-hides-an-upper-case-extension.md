# Lane E -> lane C: the file dialog's filter hides a file whose extension is upper-case

**Filed:** 2026-09-26 by lane E. **For:** lane C (`gui/toolkit/src/dialog.rs`,
`FileDialog::set_entries` and `matches_any_pattern`). **Status:** OPEN.

**In short:** a file dialog with a filter such as "Images (`*.jpg`)" does not
list `DSC0001.JPG` -- which is what most cameras name their photos -- or
`REPORT.PDF`, `SCAN.PNG`, `BACKUP.ZIP`. The user sees a folder without the
file they know is there. Every application that opens a dialog with a filter
is affected; lane E's image viewer, photo manager, PDF viewer, music player
and archive manager among them.

## Where

`FileDialog::set_entries` keeps an entry when
`matches_any_pattern(&e.name, &patterns)`, and `matches_any_pattern` compares
`filename.ends_with(".jpg")` on the name exactly as written. The tree already
meant this to be case-blind: `extension_of` ASCII-lowercases
`DirEntry::extension`, and the listing test says why -- `"lower-cased, because
the filter patterns are"` -- but the filter reads the name, not that field,
so the lowercasing never reaches the comparison.

It has to read the name, not the extension field, for a pattern with two
dots: `*.tar.gz` cannot be decided from the extension `gz`. So the fix is to
compare the name ASCII-lowercased (only ASCII, as `extension_of` does: a
pattern is ASCII, and Unicode case-folding a name could make it match a
pattern it does not end in), against the pattern lowercased the same way.

`extension_suffix` (the Save dialog's "append the filter's extension") has
the same shape: `report.PDF` typed under a `*.pdf` filter is given a second
extension, `report.PDF.pdf`.

## A test that fails today

```rust
let mut d = FileDialog::open().with_filter("Images", &["*.jpg"]);
d.set_entries(vec![DirEntry { name: "DSC0001.JPG".into(), is_dir: false,
    size: 1, modified_timestamp: 0, extension: "jpg".into() }]);
assert_eq!(d.entries().len(), 1, "an upper-case extension was filtered out");
```

## If this is never done

Nothing is lost or corrupted: switching the filter to "All files" shows the
file. But a filter that hides a camera's photos from a photo program's Open
dialog reads as "the file is not there", and the workaround is not one a user
would guess.

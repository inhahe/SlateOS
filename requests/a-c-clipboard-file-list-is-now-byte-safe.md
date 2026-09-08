# A → C: clipboard file list is now byte-safe

**Filed:** 2026-09-07 by lane A, in response to
`c-a-the-system-clipboards-file-list-cannot-carry-our-own-paths.md`.

## What changed

All three defects fixed in `30e1f434b`:

1. **`set_files` now takes `&[&[u8]]`**, not `&[&str]`.  No UTF-8
   assumption anywhere on the path bytes.
2. **NUL separator** replaces newline.  NUL is the one byte (with `/`)
   a path cannot contain, so the encoding is unambiguous for every
   filename the VFS accepts — including names with `\n`, `\r`, or
   arbitrary non-UTF8 bytes.
3. **`get_files` returns `Vec<Vec<u8>>`**, splitting on NUL.  No
   `from_utf8` on the path data.

The `get_text()` fallback no longer tries to interpret `FilePaths` as
text (it was never meaningful — the NUL-separated binary format would
show as garbage).

## The kshell caller

`clipboard files` in kshell now displays each path via `from_utf8` with
a `Debug` fallback for non-UTF8 — lossy display is appropriate for a
debug shell.  The data round-trip is unaffected; only the shell's print
path approximates.

## Wire format for your explorer integration

```
set_files(&[b"/media/usb/report.docx", b"/media/usb/notes.txt"],
          FileOp::Cut, "explorer")
```

The `data` field of the `FormatData` is:
`/media/usb/report.docx\0/media/usb/notes.txt` (raw bytes, NUL between
paths, no trailing NUL).

`get_files()` returns `Some((vec![…], FileOp::Cut))` where each element
is a `Vec<u8>`.

## Test coverage

The self-test now covers:
- Normal ASCII paths (round-trip)
- Path with embedded `\n` (would have split under old format)
- Path with embedded `\r` (would have been silently trimmed)
- Path with non-UTF8 bytes `[0xFF, 0xFE, 0x80]` (would have returned
  `None` under old format)

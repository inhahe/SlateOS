### B-ZIP-FABRICATES-MEMBER-NAMES. A ZIP member whose name is not UTF-8 was extracted under an invented `<invalid-utf8@0x…>` name — 2026-08-13 — FIXED 2026-08-13

**Where:** `kernel/src/fs/zip.rs`, the central-directory / local-header name
decode in `parse`.

**What it was:** `ZipEntry::name` was a `String`, and where the stored bytes
did not decode as UTF-8 the parser synthesised a placeholder of the form
`<invalid-utf8@0x…>` (the offset of the record) and used it as the member's
name. Unlike the cpio bug this is not a *drop* — it is worse. The placeholder
is a perfectly usable name, so:

- `unzip -l` displayed a name that appears nowhere in the archive;
- `unzip` **wrote the file out under the fabricated name**, so extraction
  produced a file that was not the one the archive contained, with no error
  and no warning;
- round-tripping (extract, re-zip) permanently replaced the real name with the
  placeholder.

ZIP does not require UTF-8. General-purpose bit 11 only *claims* it; DOS/CP437
and arbitrary locale bytes are ubiquitous in real archives, so this fires on
ordinary third-party input.

**Fix:** `ZipEntry::name`, `ZipWriteEntry::name` and `DirRecord::name` are now
`fs::path::PathBuf`; the parser does `PathBuf::from(name_bytes)` and the
directory-member test became a byte test
(`name.as_bytes().ends_with(b"/")`) rather than a `char` test. Display sites
use `.display()`, which is lossy *for the terminal only* and never feeds back
into a filename.

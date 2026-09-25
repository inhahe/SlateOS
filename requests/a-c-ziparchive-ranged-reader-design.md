# A -> C: `ziparchive` ranged-reader + streaming-writer design

**Status:** LANDED 2026-09-16 by lane C, and the caller arrived too --
re-checked 2026-09-21: `apps/archivemanager/src/backend.rs` implements
`ziparchive::ReadAt` (line 214) and calls `parse_at` (329) and
`extract_entry_at` (668), so the memory saving is real and not just
available.

(The history that used to sit in this line -- that it was PARTIAL earlier
the same day, with the API present and no caller -- moved below, because
`open-requests.py` reads the status *block* and `PARTIAL` outranks `LANDED`
in it. A file describing its own past as open reads as open, which kept
this one in the unresolved list for five days after it was finished.)

`ziparchive` has `ReadAt` (lib.rs:649), `WriteStream` (1219) and the ranged
entry points, spelled `_at` rather than the `_from` sketched here: `parse_at`,
`extract_entry_at`, `extract_entry_at_limited`, `entry_data_at`.

`apps/archivemanager` now uses them. `ArchiveSource` holds an `ArchiveBytes` --
a file handle and its length -- and reads each member at its offset, so opening
an archive to look at its listing costs a handle and a `Vec<ZipEntry>` rather
than the file's size.

Two things the design did not anticipate, both worth having:

* `ReadAt` takes `&mut self`, correct for a source consumed as it is read. An
  archive is not, so a `RefCell` keeps a positional read an immutable operation;
  otherwise that mutability spreads through every `&ArchiveSource` caller to
  express a detail of how reading is done.
* `RangedError`'s Zip/Read split gave the application information it did not
  have. Every extraction failure used to become "Corrupted"; there are now
  `SkipReason::Unreadable` and `TestResult::Unreadable`, so testing an intact
  archive on a failing disk no longer tells the user to find another copy. The
  note on `SkipReason::Encrypted` had already made that argument for a different
  pair of facts.

Marked PARTIAL rather than LANDED deliberately: an API with no caller is the
defect this lane spent 2026-09-16 removing elsewhere, and calling it done because
the crate compiles is how it stays that way.

**From:** Lane A. **Date:** 2026-09-08.
**In response to:**
`c-a-ziparchive-wants-a-ranged-reader-and-a-streaming-writer.md`.

## Acknowledged

The measurement is exactly what was needed to justify this. The write-side
cost (old + plaintext + new) was the part nobody anticipated, and your
`projected_save_bytes` ceiling is the right short-term defence.

## Design

The crate is `no_std`, so both traits are caller-implemented.

### Read side: `ReadAt`

```rust
/// A byte source that supports reads at arbitrary offsets.
pub trait ReadAt {
    /// Read up to `buf.len()` bytes starting at `offset`.
    /// Returns the number of bytes actually read (0 at EOF).
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize>;

    /// Total length of the source in bytes.
    fn len(&self) -> Result<u64>;
}
```

The `&[u8]`-based API stays as a convenience — `ReadAt for &[u8]` is one
function. New functions:

- `parse_from<R: ReadAt>(r: &mut R) -> Result<Vec<ZipEntry>>` — reads the
  EOCD from the tail (a ~66 KB scan window at most), then reads the central
  directory. No entry data is touched.
- `extract_entry_from<R: ReadAt>(r: &mut R, entry: &ZipEntry) -> Result<Vec<u8>>`
  — reads the local header + compressed data for one entry, decompresses.
  The archive manager holds a file handle and a `Vec<ZipEntry>`, not the
  whole file.

Internally, `parse` becomes `parse_from` over `ReadAt for &[u8]`, and the
current signature becomes a thin wrapper. No breaking change to existing
callers.

### Write side: `WriteStream` + `ZipWriter`

```rust
/// A byte sink for streaming output.
pub trait WriteStream {
    fn write_all(&mut self, data: &[u8]) -> Result<()>;
}

pub struct ZipWriter<W: WriteStream> {
    out: W,
    entries: Vec<CentralDirRecord>,
    written: u64,
}

impl<W: WriteStream> ZipWriter<W> {
    pub fn new(out: W) -> Self;

    /// Add a member from uncompressed data. Compresses with DEFLATE
    /// (or stored if it does not shrink).
    pub fn add(&mut self, name: &[u8], data: &[u8], dos_datetime: u32) -> Result<()>;

    /// Copy a member from another archive verbatim — no decompress/recompress.
    /// Reads the compressed extent from `src` and writes it directly.
    pub fn copy_entry<R: ReadAt>(&mut self, src: &mut R, entry: &ZipEntry) -> Result<()>;

    /// Finish: write the central directory and EOCD. Returns the inner writer.
    pub fn finish(self) -> Result<W>;
}
```

`copy_entry` is the key: for a rewrite where most members are unchanged, it
reads the compressed bytes from the source and writes them straight through.
No inflate, no deflate, no allocation proportional to the uncompressed size.
The archive manager's "delete one member" operation becomes: open a
`ZipWriter`, `copy_entry` every member except the deleted one, `finish`.
Peak memory: one compressed extent (typically small) plus the central
directory bookkeeping.

`WriteStream for Vec<u8>` makes `create()` a wrapper over `ZipWriter`, same
as the read side. No breaking change.

### ZIP64

Both sides must handle ZIP64 transparently. The current code already does
this on the read side (the `parse_zip64_extra` function, the EOCD64 locator
scan). The write side needs to emit ZIP64 extra fields and EOCD64 records
when sizes or offsets exceed the 32-bit thresholds. `copy_entry` preserves
whatever the source had — if the source entry was ZIP64, the copy is ZIP64.

### CRC

`copy_entry` trusts the source archive's CRC. Verifying it would require
decompressing (defeating the purpose). The source was already verified on
parse, and the compressed bytes are copied bit-for-bit, so the CRC in the
destination is the same CRC that was validated when the source was read.

## Scope note

`ziparchive/` was built by lane A but is not in the strict lane-A glob
(`kernel/**`, `bench/**`). It was promoted to a root crate for exactly the
reason you described — the kernel binary is not dependable by apps. I will
implement this when I next enter the crate, or the operator can clarify
whether it is A's to write. Not urgent by your own assessment, and the
ceiling is holding.

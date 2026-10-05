## §426 — On-disk records store paths percent-encoded from their bytes, behind a format version marker

**Date:** 2026-08-15
**Lane:** C
**Decided by:** Claude (autonomous)
**Zone:** apps

**Context.** Two lane-C programs keep a record whose whole purpose is to name a
file well enough to reproduce it later: the file explorer's recycle bin
(`meta.txt`, holding the path an entry came from) and the backup program's
manifest (the JSON list of everything in an archive). Both wrote the path as
text — `path.display()` and `to_string_lossy()` respectively — and our paths
are byte strings that allow every byte except `/` and NUL. A name that is not
UTF-8 therefore became U+FFFD *in the record*, and since the record is the only
copy, "restore" recreated the file under a different name while reporting
success. Both programs were self-consistently wrong: they re-read exactly what
they wrote, so nothing downstream could notice.

The fix has to make an arbitrary byte string survive a file that also wants to
stay human-readable and line- or JSON-oriented. Both formats already existed in
the field.

**Options.**

1. **Store the raw bytes.** Write the path verbatim and parse it back. Exact,
   zero encoding logic — but `meta.txt` is line-oriented and a path may contain
   `\n`, and JSON strings are Unicode by definition, so a raw byte string is
   not representable in the manifest at all.
2. **Base64 the path.** Exact and trivially unambiguous, but the record stops
   being readable: the overwhelmingly common case is an ordinary ASCII path,
   and a recycle bin you cannot inspect with a text editor is a real loss when
   the thing you are debugging is "where did my file go".
3. **Percent-encode: escape `%` and every byte outside `0x20..0x7f`.**
   Ordinary paths are unchanged apart from a literal `%`; the record stays
   printable ASCII, single-line, and valid JSON; and the encoding is exact.
4. **Escape only what the container forbids** (newline for `meta.txt`,
   non-UTF-8 for JSON). Minimal diff, but each format gets a different escape
   with different edge cases, and "what the container forbids" is exactly the
   kind of thing that is revisited later and gets it wrong.

**Decision: (3), the same percent-encoding in both formats, each guarded by a
version marker** — line 1 of `meta.txt` is `slate-recycle-v2`, and the manifest
carries `"version": 2`. Absence of the marker means version 1, which is read
with paths verbatim.

**Reasoning.**

- *It is exact where exactness is the point and invisible where it is not.* The
  encoded form of `/home/u/notes.txt` is `/home/u/notes.txt`. Only the rare
  path pays, and it pays with a form that is still readable (`caf%E9.txt`).
- *One escape, two formats.* Both records now have the same failure modes and
  the same tests. Option (4) would have produced two encodings that look alike
  and differ in the corners.
- *The version marker is what makes the change deployable.* Both formats had
  live data. Refusing to read it would strand every existing backup and every
  populated recycle bin; reading it as if it were escaped would silently
  rename any file whose name contains `%20`. The marker is one line and one
  field, and it makes the old data read correctly instead of plausibly.
- *Against (2):* the readability of the common case is worth more than the
  uniformity of the rare one. Against (1): it does not work in JSON, and a
  path with a newline in it is exactly the case the record must survive.

**Consequences.** A decoder must treat a `%` not followed by two hex digits as
a literal `%` rather than dropping it — a hand-edited file must not lose a byte
silently. The lossless part is kept at byte level (`encode_bytes` /
`decode_bytes`) with the `OsStr` conversion as a thin platform layer above it,
because the byte level is the level the file is written at and the only level
that can be asserted on a non-Unix test host.

**Where it bites:** `apps/explorer/src/fileops.rs` (`encode_path`,
`encode_bytes`, `decode_path`, `decode_bytes`, `META_VERSION`, `RecycleBin`),
`apps/backup/src/main.rs` (the same four functions, `MANIFEST_VERSION`,
`FileEntry::to_json`/`from_json`, `relative_path`).

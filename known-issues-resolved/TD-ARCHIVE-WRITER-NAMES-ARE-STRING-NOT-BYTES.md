### TD-ARCHIVE-WRITER-NAMES-ARE-STRING-NOT-BYTES. ar/rar/7z member names were still `String` — LOGGED 2026-08-13 — FIXED 2026-08-13

**Where:** `kernel/src/fs/ar.rs` (`ArEntry::name`), `kernel/src/fs/rar.rs`
(`RarEntry::name`), `kernel/src/fs/sevenz.rs` (`SevenZEntry::name`,
`FileInfo::name`).  The narrowing point was
`kernel/src/fs/archive.rs::name_for_string_writer`, now deleted.

**What it was:** as part of `D-VFS-PATHS-ARE-STR-NOT-BYTES`, `fs::tar`,
`fs::cpio`, `fs::zip` and the unified `fs::archive` layer moved to
`fs::path::PathBuf` (raw bytes) member names.  These three format modules
still modelled theirs as `String`.  On the *read* path that is harmless only
if the parser did not already mangle the name — `archive::list_*` widens
`String → PathBuf` losslessly, but it cannot undo a lossy decode.  On the
*write* path a name that is not valid UTF-8 could not be handed to those
writers at all, so `archive::create` rejected it with
`KernelError::InvalidArgument` — an honest failure, but a capability gap:
every one of these formats stores names as raw bytes on disk.

**As predicted, each parser carried its own variant of the mangling bug**, and
converting them uncovered both: see `B-RAR-DROPS-NON-UTF8-MEMBER-NAMES`
(non-UTF-8 name → `""`, member lost or colliding) and
`B-7Z-COLLAPSES-UNPAIRED-SURROGATES-IN-MEMBER-NAMES` (`from_utf16_lossy`
merged distinct Windows names onto one, so extraction overwrote a file).
That is now four format parsers (cpio, zip, rar, 7z) that each invented a
name because they had nowhere to put non-UTF-8 bytes — the recurring shape,
not a coincidence.

**Fix:** all three entry types carry `PathBuf` end to end.  Specifically:

- **ar** — the parser trims the header's space padding and strips the single
  `/` terminator (which is what lets a name *ending in a space* survive), and
  resolves `/<offset>` against the GNU `//` long-name table with an
  empty-name guard.  Because `ar` has no escape mechanism at all, `mkar` runs
  a pre-pass (`check_member_name`) rejecting the three shapes it genuinely
  cannot encode: an empty name, a name starting with `/` (which would be read
  back as a long-name reference or the symbol table) and one containing the
  `/\n` sequence that terminates a long-name-table record.  `create_ar` keeps
  its `InvalidArgument` for those — never for UTF-8.
- **rar** — `PathBuf::from(bytes)`; no decode at all.
- **7z** — new `PathBuf::from_utf16` (UTF-16 → WTF-8, lossless), with the
  `\` → `/` normalisation moved *before* the conversion.
- `name_for_string_writer` is deleted; `archive::create`'s `# Errors` now
  documents that `InvalidArgument` is an `ar`-representability failure only.

**Regression test:** `ar::self_test`'s `test_byte_names` round-trips a short
(inline) and a long (GNU-table) member name containing `0x80`/`0xFE`, checks a
name ending in a space survives, and asserts `mkar` refuses each of the three
unrepresentable shapes.

## §126 — A path is bytes; the VFS API is a `Path`/`PathBuf` newtype, and the JSON change-journal carries the exact bytes in a companion `_hex` field

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)

### The problem

`design.txt` says the filesystem is case-sensitive, uses `/` as separator, and
allows **every** character except `/` and NUL. That makes a path a *byte
string*. The VFS was nevertheless written against `&str`/`String`, which
carries a UTF-8 invariant the filesystem does not have. Everywhere the two met,
the code did `from_utf8_lossy` or `from_utf8(..).unwrap_or("")`, and every one
of those was a latent bug rather than a cosmetic one:

- `cap/file_tags.rs` degraded a non-decodable tagged path to `""`. The lookup
  key always begins with `/`, so it could never match `""` — the tag was
  registered, counted, and **never enforced**. A fail-open access-control bug.
- `ext4/driver.rs` *skipped* directory entries whose names were not UTF-8, so
  such a file was invisible to `readdir` and its parent directory could never
  be `rmdir`ed.
- `fs/index.rs` folded case with `from_utf8_lossy`, collapsing two names that
  differ only outside ASCII into the same search key.

### Decision 1 — a `Path`/`PathBuf` newtype over `[u8]`/`Vec<u8>`

Rather than pass `&[u8]` around, `kernel/src/fs/path.rs` defines
`Path(#[repr(transparent)] [u8])` and `PathBuf(Vec<u8>)` mirroring `std`'s API
surface (`components`, `file_name`, `extension`, `parent`, `join`, `push`,
`starts_with`, `is_absolute`).

*For:* it gives the *path* operations a home, so the component-boundary rule
lives in one place (`Path::starts_with`) instead of being open-coded — the
open-coded form (`p.starts_with(prefix) && p.as_bytes().get(prefix.len()) ==
Some(&b'/')`) **fails open** whenever `prefix` ends in `/`, and had already
done so in `fs::intercept` (a deny rule that permitted everything),
`fs::integrity` and `fs::findex`. It also makes the type system enforce the
distinction between "these bytes are a path" and "these bytes are file
contents".

*Against:* it is a large mechanical change (~600 compile errors at peak) and a
newtype the rest of the kernel has to learn.

Two deliberate omissions, both copied from `std`:

- **No `Display`.** `{}` on a path would silently do a lossy conversion at
  every log site, and a lossily-rendered path can never be fed back into a
  lookup. `Path::display()` must be written out, which makes each lossy
  conversion a visible, reviewable choice. (`Debug` *is* implemented, so
  `assert_eq!` still works.)
- **No `PartialEq<&str>`.** Comparing against a literal must be spelled
  `x.as_path() == Path::new("lit")`, which keeps the byte comparison explicit.

`PathBuf::push` clears the buffer when the pushed piece is absolute, also
matching `std`. That is *exactly* the "an absolute symlink target discards the
parent directory" rule, so the three-way branch that `memfs::resolve_path_str`
and `resolve_write_path` each carried collapsed to a single `push`.

### Decision 2 — `impl AsRef<Path>` on inherent functions, `&Path` on the `FileSystem` trait

Inherent and free functions (`Vfs::open`, `index::add_entry`,
`history::record_version`, …) take `impl AsRef<Path>`; the object-safe
`FileSystem` trait takes `&Path`.

*For:* the `impl AsRef<Path>` form keeps the several thousand existing
`&str`-literal call sites compiling unchanged, which is what made the
conversion tractable at all. `dyn FileSystem` cannot have generic methods, so
the trait has no choice.

*Against:* the split is a wart — two spellings for the same concept, and a
reader has to know which side of the line a given function is on. A uniform
`&Path` everywhere would be cleaner but would require touching every literal.

### Decision 3 — the JSON change journal writes `path` **and** `path_hex`

`fs/journal.rs` persists to `/_JOURNAL` as JSON-lines, mandated by design.txt's
"No binary logs. Text-based (JSON-lines) structured logging." But a JSON string
must be valid Unicode and a path need not be. Each path is therefore written as
up to two fields: `"path"` (always present, the lossy U+FFFD rendering) and
`"path_hex"` (present **only** when the path is not valid UTF-8, lowercase hex
of the exact bytes). Renames use `"from"`/`"from_hex"`. A malformed `_hex`
field rejects the entry rather than falling back to the lossy field, because
the lossy field names a different file.

Alternatives considered:

- **Surrogate escapes (`\udcXX`, PEP 383 / Python's `surrogateescape`).** One
  field, unambiguous (a lone low surrogate cannot arise from valid text), and
  RFC 8259 permits the syntax. *Rejected* because lone surrogates are
  ill-formed Unicode and strict parsers silently repair them — Go's
  `encoding/json` replaces them with U+FFFD, so a Go log consumer would corrupt
  the path with no error.
- **Lossy only.** Simplest, and correct for ~every real path. *Rejected*
  because the journal is a *replay* log for backup tools, not a human log; a
  U+FFFD in it is a path that cannot be restored.
- **Base64 instead of hex.** More conventional and 33% smaller. *Rejected* for
  hex only because the encoder/decoder is a third the size and has no padding
  rules to get wrong, and the field is emitted for a vanishing fraction of
  entries so its size does not matter.

*For hex-companion:* works with every JSON parser, never emits ill-formed
Unicode, self-describing (the presence of `_hex` is the signal that `path` is a
rendering), and costs nothing in the UTF-8 case — a journal of ordinary paths
is byte-identical to what this module produced before, which a self-test now
asserts.

*Against:* two fields for one datum; a naive consumer that reads only `path`
gets a rendering and does not know it. The `_hex` name is at least loud enough
that a consumer reading the format will notice it.

### How to reverse

Decision 1 is the one to keep — it is the whole point. Decision 2's split can
be collapsed to uniform `&Path` later by a mechanical pass over the call sites
once the churn has settled. Decision 3 is confined to `json_push_path` /
`json_extract_path` in `fs/journal.rs`; switching to base64 or to surrogate
escapes is a two-function change plus a format-version bump.

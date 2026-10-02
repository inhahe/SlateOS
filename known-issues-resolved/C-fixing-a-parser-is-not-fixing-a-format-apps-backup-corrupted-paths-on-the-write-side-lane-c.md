## Fixing a parser is not fixing a format: `apps/backup` corrupted paths on the *write* side (lane C)

**Status: FIXED 2026-08-15** (lane C), commit below. This is a follow-up to the
`u8 as char` section above, and the lesson is the one worth keeping.

Commit `3b6b60e39` fixed `apps/backup`'s manifest **reader** — the JSON string
parser that re-encoded UTF-8 as Latin-1. That looked like the whole bug. It was
not. `FileEntry.path` was a `String`, and every one of those strings was
produced by `relative_path`, which did
`full.to_string_lossy().replace('\\', "/")`. So a filename the filesystem
happily stored — our paths allow every byte but `/` and NUL — was flattened to
U+FFFD *before the manifest writer ever saw it*. A backup of `café.txt`
(0xE9, not UTF-8) recorded `caf<FFFD>.txt`; restore recreated the file under a
different name, and `verify` reported the original as missing. The archive was
self-consistently wrong, so nothing downstream could detect it.

**Generalization: when you find a lossy conversion in a parser, the format has
two sides — go find the writer.** A round-trip test through the parser alone
passes vacuously, because the corruption happened upstream of the data the test
constructs by hand. The reader fix and its tests were both real and both blind
to this.

Fixed by making the path a `PathBuf` end to end: `relative_path` now strips the
base and rejoins components on `/` at the byte level, and the manifest stores
paths percent-encoded from `OsStr::as_encoded_bytes` — the same escape and
version-marker scheme just adopted for the recycle bin's `meta.txt`. A manifest
with no `version` field is read as version 1 (paths verbatim), so archives taken
before this change still restore.

Three further defects fell out of the work:

- `detect_changes` contained a dead push/pop pair and two empty `if` bodies
  left over from a half-finished edit; it computed `modified` twice, and the
  first computation was discarded. Rewritten as a single pass. Behaviour is
  unchanged — the hash comparison was always the one that counted — but the
  size/mtime "quick check" it pretended to do was never wired to anything.
- The file-type breakdown in `stats` used `path.rsplit('.').next()`, which
  reports the whole filename as the extension for `README` and `.gitignore`
  alike. Now `Path::extension`.
- Both new percent-decoders (here and in `fileops.rs`) built their `OsStr` with
  `OsStr::from_encoded_bytes_unchecked` under a SAFETY comment claiming every
  byte string is valid for the platform's encoding. That is true on Unix and
  **false on Windows**, where `OsStr` is WTF-8 — and Windows is the host the
  tests run on, so the unsound branch was the only one ever executed. Replaced
  with a `#[cfg(unix)]` split: `OsString::from_vec` (safe and total) on the
  real target, which is `target-family = ["unix"]`, and a documented
  best-effort on the test host. The lossless core is now byte-level
  (`encode_bytes`/`decode_bytes`) and tested there, so the round-trip is
  asserted at the level the file is actually written at rather than at a level
  the test host cannot represent.

Related tooling gap, now closed: `rustup target add x86_64-unknown-linux-gnu`
was not installed, so **no `#[cfg(unix)]` code in this lane had ever been
compiled**, let alone checked. `cargo check --target x86_64-unknown-linux-gnu`
needs no linker and now covers those branches.

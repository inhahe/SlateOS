### B-7Z-COLLAPSES-UNPAIRED-SURROGATES-IN-MEMBER-NAMES. Distinct 7z members collided on one name — 2026-08-13 — FIXED 2026-08-13

**Where:** `kernel/src/fs/sevenz.rs`, the `K_NAME` property decode in the
header parser.

**What it was:** 7z stores names as NUL-terminated UTF-16LE, and the parser did
`String::from_utf16_lossy(&name_u16)`. "Lossy" here means *every* unpaired
surrogate becomes U+FFFD. Windows filenames are UTF-16 with no well-formedness
requirement, so unpaired surrogates are legal on the filesystem the archive was
most likely built on. Two members named `a\u{D800}.txt` and `a\u{DC00}.txt` are
distinct files on disk but decoded to the *same* string here.

**Consequence:** the listing showed two identical names, and extraction wrote
one file twice — the second silently overwriting the first, so `un7z` reported
success having lost a file's contents. The old code also ran
`name.replace('\\', "/")` *after* the lossy decode.

**Fix:** added `PathBuf::from_utf16` — a proper UTF-16 → **WTF-8** encoder
(UTF-8 extended so an unpaired surrogate encodes as its own 3-byte sequence),
which is lossless and so keeps those two names distinct. `SevenZEntry::name`
and `FileInfo::name` are `PathBuf`. The `\` → `/` normalisation now runs on the
`u16` code units *before* the conversion, where it is a single code-unit
comparison that cannot touch a byte inside a multi-byte sequence.

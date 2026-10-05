### B-RAR-DROPS-NON-UTF8-MEMBER-NAMES. A RAR5 member whose name is not UTF-8 parsed as `""` — 2026-08-13 — FIXED 2026-08-13

**Where:** `kernel/src/fs/rar.rs`, the file-header name decode in `parse`.

**What it was:** `RarEntry::name` was a `String` and the parser did
`core::str::from_utf8(bytes).unwrap_or("")`. RAR5 nominally specifies UTF-8 in
the name field, but the field is a length-prefixed byte run that *nothing*
validates — neither the format's own CRC (which covers the header bytes, not
their encoding) nor our parser — so an archive written by a tool that stored
locale bytes yields raw non-UTF-8 there routinely.

**Consequence:** the same shape as the cpio bug and just as silent. `unrar -l`
listed the member with an empty name; `unrar` fed `""` to
`pathutil::confine_under`, so the member either vanished or, for several such
members, they all collided on one name — the last one written won. No
diagnostic in either case.

**Fix:** `RarEntry::name` is a `PathBuf` built with `PathBuf::from(bytes)`; no
decode happens at all. The self-test's name comparisons became
`name.as_path() != Path::new("…")` and its prints `.display()`.

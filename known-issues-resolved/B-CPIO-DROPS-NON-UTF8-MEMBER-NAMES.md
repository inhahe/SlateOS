### B-CPIO-DROPS-NON-UTF8-MEMBER-NAMES. A cpio member whose name is not UTF-8 vanished from listings and was never extracted — 2026-08-13 — FIXED 2026-08-13

**Where:** `kernel/src/fs/cpio.rs`, the name-decoding step of `parse`.

**What it was:** `CpioEntry::name` was a `String`, so the parser decoded the
NUL-terminated byte field with `core::str::from_utf8(...)` and, on failure,
substituted `""`. An empty name matches nothing, is filtered out of listings,
and names no file on extraction — so the member was *silently* dropped. Since
cpio's name field is a raw byte run terminated by NUL (any byte but NUL is
legal, exactly like our own paths), this is a routine input, not an exotic
one: any archive built on a non-UTF-8 locale or containing a file that came
off a foreign filesystem hits it.

**Consequence:** `cpio -t` under-reported the archive's contents with no
diagnostic, and `cpio -i` extracted fewer files than the archive held while
reporting success. An initramfs built with such a name would be missing that
file at runtime.

**Fix:** `CpioEntry::name` and `::link_target` are now `fs::path::PathBuf`, so
the bytes are carried through unmodified and no decode happens at all. Part of
`D-VFS-PATHS-ARE-STR-NOT-BYTES`; see
`TD-ARCHIVE-WRITER-NAMES-ARE-STRING-NOT-BYTES` for the formats still to
convert.

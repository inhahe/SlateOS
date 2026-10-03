## B-JOURNALCTL-SHOWS-NON-ASCII-TEXT-AS-MOJIBAKE (lane B, 2026-09-26) — FIXED 2026-09-26

**In short:** `journalctl` decoded every JSON string byte by byte, pushing each
byte `as char`, so any character outside ASCII in any record was shown as
several Latin-1 ones: a record saying `café` printed `cafÃ©`. The same
decoder sliced the `&str` after `\u` at byte offsets, which panics when a
multi-byte character follows a malformed escape, and it dropped a surrogate
pair (every emoji) or a lone surrogate without a trace.

**Where:** `parse_json_string_value` in `userspace/journalctl/src/main.rs`.
Found by a test for the torn-append fix
(B-JOURNALCTL-SKIPS-A-WHOLE-LOG-FILE-...), whose record held an `é`.

**Fixed** in 91fc6a349: unescaped runs are copied as the UTF-8 they are; every
escape JSON defines is decoded, a surrogate pair as its one character; what
cannot be decoded is kept exactly as written. Four tests.

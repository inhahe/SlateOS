## `gui/toolkit/src/svg.rs` named a character the author never wrote (lane C) — FIXED

`u8_from_hex_char`'s error did `c as char` on the offending byte. `c` is a
*byte* of the colour string and the bytes reaching that arm are exactly the
non-hex ones, which includes the continuation bytes of a multi-byte character:
`#ÿÿÿ` reported `bad hex char: Ã`, blaming a character absent from the input
and sending the author hunting for it. Now reports the byte (`bad hex byte:
0xc3`) for anything outside printable ASCII, and the character itself for
ASCII.

The other four `c as char` sites in this file were checked and are **correct**:
each sits in a match arm that has already matched `c` against ASCII byte
literals (or, for `cmd_char`, behind an `is_ascii_alphabetic()` guard), so the
cast is provably lossless there. Recorded so the next sweep does not re-open
them.

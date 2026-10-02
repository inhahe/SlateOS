### TD-OILS-PRINTF-QUOTE-CHAR. `printf %d "'X"` yields the character's Unicode code point, not its first UTF-8 byte (correct for SlateOS; differs only from MSYS bash's byte-wise C locale) — NOT-A-BUG / documented probe artifact — 2026-07-20

**Where:** `userspace/oils/src/interp.rs` (`format_printf`, the leading-quote
`'X` numeric-argument rule for `%d`/`%i`/etc.).

**What:** POSIX printf treats an argument that begins with `'` or `"` as "the
numeric value of the character following the quote, in the current locale."
For a multi-byte character such as `あ` (U+3042), osh emits the **code point**
`12354`. MSYS bash, running in its byte-wise C/POSIX locale, instead emits the
**first byte** of the UTF-8 encoding (`0xE3` = `227`).

**Why NOT a bug:** run the *same* rule under a UTF-8 locale and real Linux/GNU
bash also returns the wide-character value `12354` — osh matches the modern
UTF-8-locale behavior. Same root cause and disposition as `TD-OILS-UNICODE-ESC`
(osh is unconditionally UTF-8-native, which is the correct target for SlateOS);
the MSYS byte-wise result is a host-locale artifact, not an osh divergence. No
action needed.

### TD-OILS-STRLEN-CHARS. `${#var}` and `${var:off:len}` count Unicode characters, not UTF-8 bytes (correct for SlateOS; differs only from MSYS bash's byte-wise C locale) — NOT-A-BUG / documented probe artifact — 2026-07-20

**Where:** `userspace/oils/src/interp.rs` — parameter-length (`${#x}`) and
substring (`${x:off:len}`) expansion, both routed through osh's UTF-8-aware
character-counting helper.

**What:** For `x="héllo"` (the `é` is the 2-byte UTF-8 sequence `c3 a9`):
- `${#x}` → osh `5` (characters h,é,l,l,o); MSYS bash `6` (bytes).
- `${x:1:2}` → osh `él` (characters 1..2); MSYS bash `é` (bytes 1..2 = `c3 a9`).

Each model is internally consistent: osh counts/indexes by character
throughout, MSYS bash by byte throughout.

**Why NOT a bug:** run the *same* operations under a UTF-8 locale and real
Linux/GNU bash also returns `5` / `él` — osh matches modern UTF-8-locale bash.
Same root cause and disposition as `TD-OILS-PRINTF-QUOTE-CHAR` and
`TD-OILS-UNICODE-ESC`: osh is unconditionally UTF-8-native (the correct target
for SlateOS, which has no C/POSIX byte locale), so the MSYS byte-wise result is
a host-locale artifact, not an osh divergence. No action needed.

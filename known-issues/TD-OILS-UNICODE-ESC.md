### TD-OILS-UNICODE-ESC. `$'\uHHHH'` / `$'\UHHHHHHHH'` always emit UTF-8 (correct for SlateOS; differs only from MSYS bash's C-locale default) — NOT-A-BUG / documented probe artifact — 2026-07-20

**Where:** `userspace/oils/src/lexer.rs` (ANSI-C `$'…'` unescaping, `\u`/`\U`
handling) — the code point is encoded as UTF-8 via Rust's `char`→`str`.

**What:** osh renders `$'\u00e9'` as the two UTF-8 bytes `c3 a9` (`é`) and
`$'\u20ac'` as `e2 82 ac` (`€`). MSYS bash, which defaults to the **C/POSIX
locale**, instead encodes `\u` code points in the current locale's charset: a
code point ≤ 0xFF becomes a single byte (`\u00e9` → `e9`), and one that doesn't
fit is passed through **literally** (`\u20ac` → the 6 chars `\u20AC`). This makes
a host probe against MSYS bash show a spurious diff.

**Why NOT a bug:** run the *same* MSYS bash under a UTF-8 locale
(`LC_ALL=en_US.UTF-8 bash -c "echo \$'\u00e9'"`) and it emits `c3 a9` —
byte-identical to osh. SlateOS is UTF-8-native, so osh's unconditional UTF-8
encoding is the correct target behavior and matches bash under the modern
default locale. Same family as the `/tmp`→`D:\tmp` path translation and the
`type -a` external-path artifacts: an MSYS host-environment difference, not an
osh divergence. No action needed.

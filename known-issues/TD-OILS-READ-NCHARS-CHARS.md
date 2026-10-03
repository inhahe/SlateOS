### TD-OILS-READ-NCHARS-CHARS. `read -n`/`-N` count Unicode characters, not UTF-8 bytes (correct for SlateOS; differs only from MSYS bash's byte-wise C locale) — NOT-A-BUG / documented probe artifact — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — `read_record`, whose character
counter advances on every byte that is not a `10xxxxxx` continuation byte.

**What:** for the four-byte input `a<c3><a9>b` (`aéb` in UTF-8), `read -N 2 v`
gives osh `a<c3><a9>` (the characters `a`, `é`) and MSYS bash `a<c3>` (the bytes
`a`, `\303`). The escaped form counts alike: `read -N 3` on `a\<c3><a9>b` is
`aéb` for osh and `aé` for MSYS bash.

**Why NOT a bug:** the *same* MSYS bash under a UTF-8 locale agrees with osh —

```
$ LC_ALL=en_US.UTF-8 bash -c 'printf "a\303\251b\n" | { read -N 2 v; printf "%s" "$v"; }' | od -c
0000000   a 303 251
```

Same root cause and disposition as `TD-OILS-STRLEN-CHARS`,
`TD-OILS-PRINTF-QUOTE-CHAR` and `TD-OILS-UNICODE-ESC`: osh is unconditionally
UTF-8-native, which is the correct target for SlateOS (it has no C/POSIX byte
locale), so the MSYS byte-wise result is a host-locale artifact rather than an
osh divergence. `tests/corpus/read-processes-backslashes-as-it-reads.sh` keeps
to ASCII for exactly this reason. No action needed.

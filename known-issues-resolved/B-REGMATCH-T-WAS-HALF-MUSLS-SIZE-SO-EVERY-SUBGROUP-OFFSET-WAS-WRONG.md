## B-REGMATCH-T-WAS-HALF-MUSLS-SIZE-SO-EVERY-SUBGROUP-OFFSET-WAS-WRONG (lane B, 2026-09-09) -- FIXED the same day

**In short:** the structure `regexec` fills in to report *where* each part of a
regular expression matched was half the size the C library uses. A program
asking for ten sub-match positions got an array written a quarter full, with
every entry after the first at the wrong place.

**Where.** `posix/src/regex.rs`, `struct RegMatch`.

**Cause.** `regoff_t` is `int` in glibc and `long` in musl, and every C port in
this tree links musl. The struct's own doc comment said "Layout matches
glibc/musl `regmatch_t` (`regoff_t` = `int`)" -- true of one of the two
libraries it named.

| | glibc | musl |
|---|---|---|
| `regoff_t` | 4 | 8 |
| `regmatch_t` | 8 | 16 |

Measured by compiling one program twice, `gcc` under WSL and
`zig cc --target=x86_64-linux-musl`.

**Effect.** `regmatch_t m[10]` is 160 bytes to the caller and was 80 to us, so
element *n* landed at byte 8n instead of 16n and every offset was truncated to
32 bits. Only group 0 was ever at the right address. Anything using the POSIX
regex C API with sub-expressions was affected.

**Fixed** by widening `rm_so`/`rm_eo` to `isize`. Found by
`scripts/check-libc-abi.py` (design-decisions 1011) within an hour of that gate
existing, which is the argument for the gate.

**A test certified it**, asserting `size_of::<RegMatch>() == 8` under the
comment quoted above. Corrected, with the measurement in the doc.

## D-PRINTF-READ-EVERY-INTEGER-AS-ITS-WHOLE-SLOT — printf read every integer as 64 bits, ignored length modifiers, and had no wide strings or positional arguments (lane D, found 2026-10-05)

**Status:** FIXED 2026-10-05 (posix/src/printf.rs)

**In short:** the C library's `printf` took every integer argument as a
whole 64-bit value. A C program's `int` fills only the low half of the
8-byte slot it is passed in; the high half is whatever the register held,
usually zero. So `printf("%d", -7)` from C printed `4294967289`. The length
modifiers that say how wide the argument is (`%hhd`, `%hd`, `%ld`, ...)
were skipped without being used, so `%hhd` of 300 printed `300` where glibc
prints `44`, and `%n` stored a 4-byte `int` into whatever it was given:
into a `signed char`, three bytes past it. A format that names its
arguments by position (`%2$d`, used by translated messages) printed the
specification itself. None of this showed in the Rust unit tests, which pass
whole 64-bit values. Any C program printing a negative `int` was affected:
the coreutils, Oils and bash ports among them.

**What else was wrong, found by the same comparison with glibc:**

| Before | glibc, and now this library |
|---|---|
| `q`, `Z`, C23's `wN`/`wfN` unknown: `%qd` printed `%qd` | `long long`, `size_t`, exact-width integers; a bad `w` fails, `EINVAL` |
| `%b`, `%B` (C23), `%S`, `%C` unknown, printed as written | binary; a wide string; a wide character |
| `%lc` printed one byte; `%ls` read the wide string as bytes, stopping at the first zero: `h` for `L"héllo"` | each through `wcrtomb`, as UTF-8; precision counts bytes and never splits a character |
| `%p` of NULL was `0x0`; precision, `0`, `+` and space ignored | `(nil)`; `%#x` otherwise, keeping `+` and space |
| `%.3s` of NULL was `(nu` | empty: `(null)` only when the precision leaves room for it all |
| An unknown conversion was copied as written, length and all | rebuilt as glibc's `printf_unknown` rebuilds it: `%-0 +#y` is `%#+-y` |
| A format ending inside a specification (`"abc%"`) dropped it and succeeded | fails, `EINVAL` -- or, where glibc has switched to its positional pass, is written back |
| A width, precision or position past `INT_MAX` saturated: `fprintf(f, "%99999999999d", 1)` padded without end | fails, `EOVERFLOW` |

**How it was found:** reading the engine while checking a port's output.
**How it is held now:** `posix/tools/oracle/printf_int_harness.py` runs 632
calls through glibc 2.39 in C.UTF-8 and records each's return, text,
`errno` and `%n` stores in `posix/src/printf_int_oracle.txt`;
`printf_is_glibcs_for_every_length_modifier` replays them all, building
each `int` with junk (`0xa5a5a5a5`) in its high half, as a C caller may
leave it. The two places this library differs from glibc on purpose are
pinned there (`DEVIATIONS`) and recorded in design-decisions section 1173.
`the_first_pass_knows_what_dispatch_knows` keeps the two lists of
conversions in step.

**Fix:** the engine now parses a specification into its parts first
(`parse_spec`: position, flags, width, precision, `Length`), reads the
`*`s (`resolve_spec`), and dispatches on the conversion with the length,
each integer truncated to its type's bits (`signed_arg`, `unsigned_arg`).
A format with `n$` or `*m$` is scanned whole first (`Positional::scan`)
and its arguments are read by position.

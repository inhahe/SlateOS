### BUG-POSIX-WCSTOD-TRUNCATED. `wcstod` silently truncated any float literal past 62 characters, and `wcstof` rounded twice — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**Where:** `posix/src/wchar.rs::wcstod` (a `let mut buf = [0u8; 64];` filled by
a `while bi < 62` loop) and `posix/src/wchar.rs::wcstof`
(`wcstod(nptr, endptr) as f32`).

**What it was:** `wcstod` did not parse anything itself. It copied the
ASCII-range wide characters into a 64-byte stack array — **stopping dead at 62
characters** — and handed that to `strtod`. A literal longer than 62
characters therefore lost its tail, exponent and all, and the loop simply
stopped: no error, no `ERANGE`, and `endptr` pointing into the middle of a
number that was reported as fully converted.

```
wcstod(L"1.234567890123456789012345678901234567890123456789012345678901234567890e300")
  ours (before): 1.2345678901234567       (the "e300" never reached strtod)
  correct:       1.2345678901234568e+300
```

That is not an edge case. Writing out the least subnormal in full takes 751
characters, `DBL_MIN` 1080; any program that round-trips a `double` through
`%.17e` and back is fine, but one that prints with `%f` and reads back with
`wcstod` is not. The character set the loop accepted was also hand-rolled
(`0x2b | 0x2d | 0x2e | 0x30..=0x39 | 0x45 | 0x65 | 0x49 | ...`), a second copy
of the float grammar that could drift from `strtod`'s.

`wcstof` then narrowed a `wcstod` result, which is the double-rounding bug
already recorded as BUG-POSIX-STRTOF-DOUBLE-ROUNDING — fixed for `strtof` in
the same session but left in place here.

**Fix (DONE).** The subject-sequence scanner moved out of `stdlib.rs` into
`decfloat.rs` and became generic over a new `ByteSource` trait
(`fn byte_at(&self, i: usize) -> u8`), with `CStrSource` in `stdlib.rs` and
`WideSource` in `wchar.rs`. There is now exactly **one** implementation of the
float grammar, shared by `strtod`, `strtof`, `wcstod` and `wcstof`, and no
intermediate buffer anywhere: digits go straight into a `DigitCollector` and
are rounded once. `wcstod` finishes with `to_f64`, `wcstof` with `to_f32`, so
each rounds in its own precision and reports `ERANGE` for its own range.

The scanner returns `(token, negative, consumed)` rather than writing
`*endptr`, because the two callers count in different units — but only in
name: one accepted byte is exactly one wide character, so `consumed` maps 1:1
and `nptr.add(consumed)` is right for both. A wide character outside ASCII
reports as byte 0, which ends the subject sequence exactly as a terminator
does; no float syntax uses one, and this replaces the old set-membership test.

**Verification:** new tests cover a 75-character literal with an exponent, an
800-digit one (past `MAX_PARSE_DIGITS`, so the sticky path runs), `endptr`
placement after a long number and after `nan(...)`, a non-ASCII character
ending the scan, `DBL_MAX` / the least subnormal written out in full /
overflow / underflow, the `f32` double-rounding counterexample through
`wcstof`, and a 4000-value sweep of both functions against Rust's correctly
rounded `str::parse`. 20 088 posix tests pass; clippy clean.

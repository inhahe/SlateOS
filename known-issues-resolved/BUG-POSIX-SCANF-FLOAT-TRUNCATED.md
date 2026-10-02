### BUG-POSIX-SCANF-FLOAT-TRUNCATED. `scanf`'s `%f` stopped after 62 characters — mid-number, leaving the tail in the input — and rejected `inf`/`nan` outright — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**Where:** `posix/src/scanf.rs::scan_float`.

**What it was:** the conversion collected the literal into a `[u8; 64]` text
buffer and every loop was gated on `bi < 62`. That gate stopped *consuming*
as well as storing, so a long number was cut in half and the remainder was
left in the input stream:

```c
double d; char rest[16];
sscanf("1" "0000000000000000000000000000000000000000000000000000000000000000000000" "e-70 rest",
       "%lf %s", &d, rest);
```

`d` came back as `1e61` — the 62-character prefix — and the `%s` then matched
`e-70` instead of `rest`. Every conversion after the float was shifted. The
value is exactly `1.0`.

The same function also had no `INF`/`INFINITY`/`NAN` branch, so
`sscanf("inf", "%lf", &d)` assigned nothing and returned 0, even though C99
7.21.6.2p12 says `a`/`e`/`f`/`g` match "the subject sequence of the `strtod`
function" — which `strtod` itself already accepted.

**Fix (DONE).** The digit bookkeeping shared by both parsers now lives in
`decfloat::DigitCollector`, and `scan_float` drives it directly instead of
building a text buffer to hand to `strtod`. A collector has no length limit to
run into: digits past the 768 that can affect the result are still *consumed*,
they just fold into the exponent or the sticky bit. `strtod` was rewired onto
the same collector, so the two cannot drift apart again.

`scan_float_named` adds `INF`/`INFINITY`/`NAN[(n-char-sequence)]`, matched
case-insensitively through a new `ScanCtx::peek_at` lookahead so a partial
token is never consumed — `sscanf("info", "%lf%s")` yields `inf` and `"o"`,
and a bare `"in"` matches nothing and assigns nothing. The exponent rollback
("1.5e" is not a valid prefix, "1.5" is) is preserved and now also fires when
an explicit field width cuts the exponent off. `buf_put`, which existed only
for the old text buffer, is gone.

**Verification:** new tests cover the 74-character literal above (value *and*
the following `%s`), correct rounding through `%lf` at `DBL_MAX`, the least
subnormal and the `2^53+1` tie, all five named forms, partial-token rejection,
and three width interactions. 20 074 posix tests pass; clippy clean.

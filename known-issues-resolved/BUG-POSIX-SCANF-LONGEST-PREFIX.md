### BUG-POSIX-SCANF-LONGEST-PREFIX. `%f` rolled back an exponent marker and accepted a partial `infinity`, leaving fragments for the next conversion — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**Where:** `posix/src/scanf.rs::scan_float_exponent` (a `saved_si`/`saved_count`
rollback) and `scan_float_named` (`let _ = match_word(ctx, …, b"inity");`).

**What it was:** a `scanf` directive takes the longest sequence that *is, or is
a prefix of*, a matching sequence (C11 7.21.6.2p9) — not the longest sequence
that is itself valid. We implemented the latter, in two places:

```
sscanf("1.5e",   "%lf%s", &v, s)   ours (before): n=2, v=1.5, s="e"   glibc: n=1, v=1.5
sscanf("1.25e34","%5lf%s",&v, s)   ours (before): n=2, v=1.25, s="e34" glibc: n=2, v=1.25, s="34"
sscanf("infix",  "%lf%s", &v, s)   ours (before): n=2, v=inf, s="ix"   glibc: n=0
```

Both leak a fragment into the following conversion, which is the failure mode
that derails every directive after it rather than just the one. The width case
is the sharper one: with `%6lf` on `"0x1.8p+1"` the field is `"0x1.8p"` and it
was the *width*, not the input, that ended the exponent — rolling the `p` back
there is plainly wrong.

**Fix (DONE).** `scan_float_exponent` consumes the marker and its sign
unconditionally and simply does not apply an exponent when no digits follow;
the digits read before the marker still give the value. `scan_float_named`
gained `match_prefix`, which keeps a partial match, and returns the new
`NamedScan::Failed` when the extension of `inf` towards `infinity` stops
part-way — `"infi"` and `"infinit"` are prefixes of a matching sequence but not
matching sequences, so they are consumed and then reported as a matching
failure.

**Verification:** `scan_f_swallows_a_dangling_exponent_marker` and
`scan_f_rejects_a_partial_infinity`, both written from glibc runs, plus the
updated width test.

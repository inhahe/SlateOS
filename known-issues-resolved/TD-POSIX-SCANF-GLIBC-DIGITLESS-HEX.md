### TD-POSIX-SCANF-GLIBC-DIGITLESS-HEX. `scanf` reports a matching failure where glibc converts a digit-less `0x.` to zero — FIXED 2026-09-27

**Status:** FIXED 2026-09-27 -- `scanf` became glibc's own engine, ported (`posix/src/scanf.rs`, `fb1415b42`), and with it reads both inputs below as glibc does: `0x.` is collected and `strtod` converts its `0`, and a `nan(...)` payload is left unread. This entry was not updated then; both cases are pinned by `glibc_consumes_what_it_looked_at` since 2026-10-06. What follows is the divergence as it was, accepted on 2026-07-30 and no longer this library's behaviour.

**Where:** `posix/src/scanf.rs::scan_hex_float_digits`.

**What it is:** glibc consumes a `0x` followed by a point but no hex digit and
converts it to 0.0, where we consume the prefix and report a matching failure:

```
sscanf("0x.z",   "%lf%s", &v, s)   ours: n=0            glibc: n=2, v=0, s="z"
sscanf("0x.8p1", "%3lf%s",&v, s)   ours: n=2, v=0, s="x.8p1"   glibc: n=2, v=0, s="8p1"
```

C requires a hex digit in the subject sequence, so `0x.` is not a matching
sequence and a matching failure is the conformant answer; glibc's own
`0x` case (no point) agrees with us and returns 0, which makes its `0x.`
behaviour an internal inconsistency rather than a rule worth copying. Recorded
because a program written against glibc could depend on it.

**Also divergent, same reasoning:** glibc's `scanf` does not consume a
`nan(n-char-sequence)` payload at all — `sscanf("nan(ab)x", "%lf%s", &v, s)`
leaves `"(ab)x"` for the `%s` — although C says `%f` matches `strtod`'s subject
sequence, which includes the payload. We consume it, as musl does.

**Proper fix (if ever wanted):** none for the payload — ours is right. For
`0x.`, matching glibc would mean converting a digit-less sequence to zero,
which we should only do if a real port needs it.

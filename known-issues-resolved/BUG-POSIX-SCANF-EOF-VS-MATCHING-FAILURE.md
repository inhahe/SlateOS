### BUG-POSIX-SCANF-EOF-VS-MATCHING-FAILURE. `scanf` returned EOF for a matching failure that happened to end at end of input — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**Where:** `posix/src/scanf.rs::scan_core`, the final
`if ctx.assigned == 0 && ctx.peek() == 0 { -1 }`.

**What it was:** C distinguishes an *input* failure — the input ran out before
a directive matched anything — from a *matching* failure, where characters were
read but formed no valid item. Only the first returns `EOF`; the second returns
the number of assignments made, usually 0. We approximated this with "nothing
assigned and we are at end of input", which conflates them whenever the
unmatchable text sits at the end of the string:

```
sscanf("0x", "%lf", &v)   ours (before): -1    glibc: 0
sscanf("+",  "%lf", &v)   ours (before): -1    glibc: 0
sscanf("",   "",   )      ours (before): -1    glibc: 0
```

A caller looping `while (sscanf(...) != EOF)` therefore exited on malformed
input instead of reporting it, and one testing `== EOF` to mean "no more data"
saw garbage as end-of-stream. The hex-float work made it easy to hit, because
`0x` with nothing after it is precisely a directive that consumes characters
and then finds no value in them.

**Fix (DONE).** `ScanCtx::stopped_at_end_of_input` classifies a failure by
whether *anything was matched*, not by where the cursor is: the directive's
start offset is recorded, and the failure is an input failure only if the input
is exhausted **and** every byte consumed since that point was leading
whitespace (which no directive counts as part of its item). So `sscanf("   ",
"%lf")` is still `EOF` while `sscanf("0x", "%lf")` is 0. The per-conversion
`if !scan_x(...) { break; }` ladder collapsed into one `match` producing a
`bool`, so there is a single place where a failure is classified.

**Verification:** `scan_distinguishes_input_failure_from_matching_failure`
covers empty input, whitespace-only input, an unmatched literal, `%%` at end of
input, an empty format, and the four matching-failure cases above — all
compared against glibc.

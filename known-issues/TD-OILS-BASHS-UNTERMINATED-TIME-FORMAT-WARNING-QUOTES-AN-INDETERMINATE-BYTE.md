### TD-OILS-BASHS-UNTERMINATED-TIME-FORMAT-WARNING-QUOTES-AN-INDETERMINATE-BYTE. `printf '%(%Y'` warns about a byte bash read past the end of its format string — 2026-08-04 — OPEN (deliberate deviation)

**Where:** `userspace/oils/src/interp.rs` — the `%(…)T` arm's unterminated case.

**What:** bash's `printf` scans forward for the `)` that closes a `%(`; when
there is none it still dereferences the byte at the stopping position and puts
it in the `%c: invalid format character` warning. What that byte is depends on
what the allocator left after the format string, so the warning is not
reproducible — the same script run twice can name two different characters.

**Why deferred:** this is a bash bug, not a bash behaviour. Matching it would
mean matching an uninitialised read, which is neither possible nor desirable.
osh reports the case with a stable message instead.

**Proper fix:** none. Recorded so a future session does not "fix" osh into
chasing it. If bash ever tightens this, revisit; the corpus deliberately does
not cover it.

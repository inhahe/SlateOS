## B-SYSINFO-IS-112-BYTES-AGAINST-MUSLS-368 (lane B, 2026-09-09) -- FIXED the same day

**In short:** the structure `sysinfo()` fills in is a third of the size of the
C library's, so a caller's 368-byte variable gets 112 bytes written and 256
left as it found them.

**Where.** `posix/src/unistd.rs`, `struct Sysinfo`.

**Every named field is at the right offset**, measured: `uptime` 0, `loads` 8,
`totalram` 32 … `mem_unit` 104. What is missing is musl's trailing
`char __reserved[256]`, which is the whole of the difference. So the fix is one
field, and the effect until then is an under-fill rather than an overrun.

**Correction, and it is mine.** This entry first said the reverse — that ours
was 368 against musl's 112, and that it therefore wrote 256 bytes *past* the
caller's object. That reads the gate's `'368 == 112'` the wrong way round: the
assertion is `sizeof(C type) == <our size>`, so the left number is musl's. A
confident direction stated without re-reading the numbers is the exact defect
this whole family of entries is about, committed while writing them up.

**Fixed** by adding musl's `__reserved[256]` — one field, as predicted, and
`u8` rather than a wider word so it lands at 108 where musl puts it rather
than at 112 behind an alignment we would have invented.

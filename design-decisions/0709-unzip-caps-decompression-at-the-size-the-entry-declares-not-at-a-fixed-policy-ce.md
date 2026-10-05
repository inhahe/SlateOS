## 709. unzip caps decompression at the size the entry *declares*, not at a fixed policy ceiling — the archive's own claim is the tightest honest bound available

**Date:** 2026-08-30
**Decided by:** Claude (autonomous), lane B
**Lane:** B

**In short:** A "zip bomb" is a small archive that expands to an enormous file
— a few kilobytes that become gigabytes — and the damage is done by the
expansion, not by anything afterwards. `unzip` already noticed the trick, but
only *after* it had decompressed the whole thing into memory, which is like
checking the smoke alarm once the house is out. The fix is to stop mid-way.
The decision here is what number to stop at: a fixed ceiling somebody picks
once (say 64 MiB), or the size the archive itself claims the file will be.
We use the archive's own claim.

**The bug this comes from.** `zip_extract_entry` compared the decompressed
length against the entry's `uncompressed_size` and rejected a mismatch. That
check was correct and worthless: the decompressor it called grew its output
buffer with no limit, so the comparison ran on the far side of the allocation
it was meant to prevent. Lane A found the identical hole in the kernel's copy
while promoting it to `ziparchive` and asked whether it was here too
(`requests/a-b-userspace-zip-carries-a-third-deflate-and-a-second-zip-parser.md`).
It was.

**Why the declared size and not a fixed ceiling.** Every ZIP entry's
uncompressed length is recorded in the central directory, which unzip parses
in full before decoding a single byte. So the number is free, it is per-entry,
and it is *exactly* the promise the archive is making. Capping there means the
refusal fires the moment the archive is caught contradicting itself, which is
the earliest moment at which anything is known to be wrong.

A fixed ceiling — `deflate::MAX_OUTPUT` is 64 MiB, and the crate offers plain
`inflate()` for callers who want it — is worse in both directions at once:

| | declared-size cap | fixed 64 MiB cap |
|---|---|---|
| 5 KiB archive claiming 300 B, expanding to 50 MiB | refused at 301 B | **allowed in full** — under the ceiling |
| honest 2 GiB backup archive | extracts | **refused** — over the ceiling |
| memory held before refusal | the declared size | up to the ceiling |

The second row is the one that settles it. A fixed ceiling is a policy about
how large a file the user is allowed to have, imposed by a decompressor that
has no business holding an opinion about it, and the user cannot override it
without a flag we would then have to design and they would then have to find.

**The honest cost, stated plainly.** The cap is only as tight as the archive's
own claim, so an attacker who declares 4 GiB *gets* a 4 GiB budget. This is
not a hole so much as the boundary of what the technique can do: at that point
the archive is no longer lying, and refusing it would be refusing a large file
for being large. What the cap removes is the *amplification* — the ability to
spend 5 KiB of attacker bandwidth to make us allocate 50 MiB. Costing an
attacker one byte per byte is the property worth having; the residual is
bounded by their upload, and `Vec::with_capacity` in `inflate_limited` is
seeded from the compressed length rather than the cap, so nothing is reserved
up front on the strength of a claim.

**Alternatives considered.**

* *`min(declared, MAX_OUTPUT)`.* Gets the first row right and the second row
  wrong, and gets it wrong silently and unfixably: a legitimate 2 GiB entry
  fails with a message about a limit nobody set. If a ceiling is ever wanted
  it belongs where the policy lives — a flag, or the caller — not welded into
  extraction.
* *Keep only the after-the-fact length check and accept the exposure.* This is
  the state we were in. It is defensible only if decompression is bounded some
  other way, and it was not.
* *Stream to disk and check as we go.* Bounds memory without bounding output,
  so it trades a memory bomb for a disk bomb, and `zip_extract_entry` returning
  `Vec<u8>` is relied on by the list/test paths that must not touch the disk at
  all.

**Kept alongside, deliberately.** The old length comparison stays. A cap
structurally cannot see an entry that decompresses to *fewer* bytes than it
declares — nothing exceeds the ceiling — so the two checks catch opposite
faults and neither subsumes the other. Both have a test.

**If this is ever reverted,** the thing to preserve is the regression test's
assertion on the error *wording*. With the cap removed it fails with
`size mismatch: expected 10, got 50000`, which is precisely the old bug's
signature; asserting only that extraction fails would pass against the bug,
because the buggy version rejected the file too — just after paying for it.

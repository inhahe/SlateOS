## `A-ZIP-EXTRACT-ALLOCATED-A-BOMB-BEFORE-CHECKING-IT` — ✅ FIXED 2026-08-26

**Status:** fixed in the same change that promoted `kernel/src/fs/zip.rs` to the
root `ziparchive` crate. Recorded because the *shape* of the bug is worth
keeping, not because anything is outstanding.

**What it was.** `extract_entry` inflated an entry with no output bound, then
verified its CRC-32:

```rust
let decompressed = inflate(raw)?;          // unbounded
if crc32(&decompressed) != entry.crc32 { return Err(CorruptedData); }
```

Every ZIP entry states its own uncompressed size in the central directory. That
number was parsed into `ZipEntry.uncompressed_size`, shown by `unzip -l`, and
then **never used as a check anywhere**. So a decompression bomb — a small entry
crafted to expand enormously — was allocated in full and only then rejected, by
a checksum that could not run until the allocation had already happened.

**Why it was easy to miss.** The code was not missing a check; it *had* one, and
the check was correct. It was in the wrong order relative to the thing it was
protecting. A reviewer asking "is corruption detected?" gets a yes. The question
that finds this one is "does the detection happen before or after the cost?" —
and that question is not part of most review habits.

The second half is the more interesting one: the defence that was missing
(`len() == uncompressed_size`) was missing *because a working substitute
existed*. The CRC caught the same archives, eventually, so nothing ever failed
in a way that pointed here. Compare
`A-IDT-COUNTS-ONLY-EXCEPTIONS-AND-CALLS-THE-TOTAL-INTERRUPTS`, where a total
that summed the whole array was similarly not wrong — only unfalsifiable.

**The fix.** `min(declared, limit)` as the inflater's cap, then exact equality
against `declared`, then the CRC. See design-decisions.md §615 for the ordering
argument and the leniency tradeoff.

**Related, still open elsewhere in the tree.** `userspace/zip/src/main.rs` (lane
B) contains an independent ZIP reader and DEFLATE decoder, so this fix does not
reach it; whether it has the same hole has not been checked. Filed as
`requests/a-b-userspace-zip-carries-a-third-deflate-and-a-second-zip-parser.md`.
That is the concrete cost of duplicated parsers of untrusted input, and it is
why §610 exists.

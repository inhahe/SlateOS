## A-F2FS-CHECKPOINT-CHECKSUM-OFFSET-WAS-READ-FROM-BYTE-104-INSTEAD-OF-164 (lane A, 2026-08-16) — FOUND AND FIXED

**Status: fixed** in `kernel/src/fs/f2fs/cp.rs` during the initial F2FS read
port, before that code ever ran against a real volume. Recorded because the
*shape* of the bug is the shape F2FS bugs will keep having, and because the
thing that caught it is worth naming.

### What was wrong

`f2fs_checkpoint`'s `checksum_offset` field — the field that says how many
bytes of the checkpoint block the CRC covers — lives at byte **164**. The
first implementation read it from byte **104**.

### Why it did not simply break

Byte 104 is not reserved and not zero; it is inside the run of block/segment
accounting counters near the top of the structure. So the driver did not read
an obviously-absurd value and refuse — it read a *small, plausible* number,
computed the CRC over the wrong span, and got a mismatch. The visible symptom
was therefore "this checkpoint is corrupt," on a checkpoint that was fine.

That is the dangerous part. A CRC failure looks like evidence *about the
volume*, not evidence about the reader. The natural next move on seeing it is
to distrust the image, and the natural fix is to loosen the check — at which
point the real bug is permanent and the protection is gone. Worse, because
F2FS keeps two checkpoint packs, a driver that mis-CRCs will often still mount
successfully by falling back to the *other* pack, which is usually the older
one. The user gets a mount, and gets a stale view of the filesystem, with no
error anywhere.

### How it was caught, and what now guards it

It was caught during the port, by re-deriving the field offsets against
Linux's `struct f2fs_checkpoint` rather than trusting the first pass — i.e. by
re-reading the layout, not by a test firing. Worth stating plainly, because it
means the bug was live in code that had already been written and that would
have compiled and, usually, mounted.

What guards it now is the self-test (`kernel/src/fs/f2fs/tests.rs`,
`test_checkpoint`), which builds a volume with two packs — pack A at version
3, pack B at version 7 — and asserts that a clean mount reads **pack B**, *by
version*, not merely that a mount succeeded. That is the assertion the bug
would have tripped, and close to the only kind that could: every file in the
image is reachable through both packs, so a driver that silently used pack A
still returns plausible bytes for every read. The suite separately asserts the
fallback direction (tear B's tail, require A) and the new lower bound (a pack
whose `checksum_offset` points inside the fixed header must be rejected, not
used).

This is the argument for the decoys described in `design-decisions.md` §216,
stated against a concrete bug rather than as a principle. Had the two packs
shared a version — the easy way to build the fixture — nothing in the suite
would have distinguished the fixed driver from the broken one.

### The fix

`CP_CHKSUM_OFFSET_FIELD = 164` (was 104), plus a new
`CP_MIN_CHKSUM_OFFSET = 192`: a `checksum_offset` that points *into the fixed
header* is now rejected as corrupt rather than used, since no valid checkpoint
can claim its checksum sits inside the fields the checksum protects. The
suite asserts both — the corrected offset by requiring version 7, and the
lower bound by planting a pack whose `checksum_offset` is inside the header
and requiring fallback.

### The generalisable lesson

**A checksum mismatch is ambiguous evidence: it accuses the data, but the
reader is equally a suspect.** In a format with redundancy — two checkpoints,
two superblocks, two NAT copies — a reader bug that manifests as "this copy
failed validation" is self-concealing, because the redundancy converts it into
a successful mount of the wrong copy. Any driver for such a format needs at
least one test that asserts *which* copy was used, not merely that the mount
succeeded.

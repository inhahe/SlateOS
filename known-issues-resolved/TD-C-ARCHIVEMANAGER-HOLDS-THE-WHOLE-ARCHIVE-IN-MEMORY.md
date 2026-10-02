### TD-C-ARCHIVEMANAGER-HOLDS-THE-WHOLE-ARCHIVE-IN-MEMORY — 2026-08-26 — LANE C — FIXED 2026-09-16

**FIXED 2026-09-16.** Both halves. Opening no longer reads the file: `ArchiveSource` holds a handle and reads members at their offsets through `ziparchive::parse_at` / `extract_entry_at`. Saving no longer assembles one: members are copied across compressed with `entry_data_at` into `ZipWriter::copy_entry`, written to the replacement file as they are produced.

What a rewrite held was the archive, every member's plaintext and the whole new archive at once. It now holds what the caller already had plus the largest single member, and `projected_save_bytes` was rewritten to measure that rather than keeping a formula for costs that no longer exist.

Two tests changed with it, and the way they changed is the point. `a_rewrite_too_big_to_hold_is_refused` asserted that the projection *exceeds* the file on disk, "or it is measuring the wrong thing" — true while a ZIP of zeroes could pass the on-disk check and still exhaust memory, and false now, so it asserts the opposite and then saves the archive that used to be refused. `a_directory_member_costs_nothing` spelled out the old arithmetic, which made it a test of the formula rather than of its own claim; it now compares the same archive with and without a directory member.

**In short:** Opening a ZIP in the archive manager reads the entire file into
memory and keeps it there for as long as the window is open. A 400 MB archive
therefore costs 400 MB of RAM to *look* at, even to read one small file out of
it, and archives over 512 MB are refused outright with a message saying so
rather than being opened. Nothing gives a wrong answer; the program is simply
much more expensive than it needs to be on big files, and puts a ceiling where
there should not be one.

**Where:** `apps/archivemanager/src/backend.rs` — `MAX_ARCHIVE_BYTES` and
`open`, which does `fs::read(path)` and stores the result in
`ArchiveSource::bytes`. Every read after that (`ziparchive::entry_data`,
`extract_entry`) takes a `&[u8]` covering the whole archive.

**Why it is like this:** `ziparchive` is a `no_std` crate with a slice API — it
parses a `&[u8]` and hands back offsets into it. There is no seeking reader to
give it, so the only way to call it is to have the whole file in memory. The
512 MB cap is not arbitrary caution: without it, opening a DVD image with a
`.zip` extension would try to allocate several gigabytes and be killed, which
looks to the user like the program crashing on a file it should have refused.

**Proper fix:** on the crate side, not this one. `ziparchive` wants a reader
trait — something that can be asked for a byte range — so the central directory
can be parsed from the tail of the file and each member inflated by streaming
its own extent. The archive manager would then hold a file handle and a parsed
directory, and `MAX_ARCHIVE_BYTES` would disappear along with the refusal
message. That is a lane A change; it is not filed as a request yet because the
current behaviour is correct for every archive a desktop user is likely to open,
and the crate is a week old — asking for a second API before the first one has
been used in anger is how APIs get designed twice.

**Update, 2026-09-02 — the writer trebled the peak.** `backend::save` now exists,
and a rewrite holds the old archive, every member's decompressed plaintext, and
the newly built archive all at the same time. So the peak is no longer "the size
of the archive" but roughly *old + uncompressed contents + new*, which for a
well-compressed 400 MB archive can be several gigabytes — far above the 512 MB
`MAX_ARCHIVE_BYTES` that a reader would suggest is the ceiling. Two notes for
whoever picks this up:

- **The cap does not bound the rewrite.** `MAX_ARCHIVE_BYTES` is checked against
  the file on disk and against each file being added, not against the total the
  save allocates. A 500 MB archive of highly compressible data passes the check
  and can still exhaust memory during a save.
- **The reader trait in "Proper fix" fixes this half too, but only with a
  writer counterpart** — the save wants to stream each member from the old
  archive to the new one without ever materialising both. Worth stating in the
  same request, since designing the read side alone would leave this needing a
  third API revision. Still not filed, for the reason above.

**Update, 2026-09-03 — the cap now bounds the rewrite, and the request is filed.**

Two things changed, and only the second needs lane A.

*The bound.* `MAX_SAVE_BYTES` (3 × `MAX_ARCHIVE_BYTES`) and
`projected_save_bytes` are new. The projection is arithmetic over the central
directory — which carries both the compressed and the uncompressed size of
every member — so a rewrite is costed *before* anything is allocated and
before the old archive is touched, and refused with a message naming both
numbers rather than being discovered by the allocator. The three-times factor is
not a guess: a save holds the old archive, every reproduced member's plaintext,
and the new archive at once, so it is three of the order an open costs.

This does **not** make the program stream. It converts "exhausts memory during a
save, having possibly already started writing" into "says it cannot, and the
file on disk is untouched" — which is the same conversion
`MAX_ARCHIVE_BYTES` already performed for opening, applied to the operation it
did not cover. The bullet above is therefore resolved; the entry as a whole is
not.

Guarded by `a_rewrite_too_big_to_hold_is_refused_and_the_file_is_untouched`,
which reaches the refusal through `save_within` — the body `save` runs, with
the budget as a parameter. Tripping `MAX_SAVE_BYTES` honestly would need an
archive claiming 1.5 GiB of plaintext, which costs 1.5 GiB to write; testing the
projection alone would have left nothing covering "and `save` acts on it", which
is precisely the shape of bug this crate keeps finding elsewhere. Mutation-
checked: replacing the comparison with `if false` fails that test and only that
test. Two more tests cover the other direction — that an ordinary archive is
nowhere near the budget (a false refusal would hit everybody, unlike the bug it
fixes) and that a directory member is charged nothing.

*The request.* Now filed, as
`requests/c-a-ziparchive-wants-a-ranged-reader-and-a-streaming-writer.md`. The
reason given above for holding it back — "the crate is a week old; asking for
a second API before the first has been used in anger is how APIs get designed
twice" — has expired in the best way: it *has* been used in anger. Both a
reader and a writer are implemented against the slice API, and the writer is
what trebled the peak. That is the evidence a second API design wants, and
withholding it now would just mean lane A designing the read side alone and
needing a third revision for the write side.

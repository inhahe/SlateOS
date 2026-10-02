## C-THE-DISK-IMAGER-WAS-A-MOCK-WITH-A-REAL-HASHER-BOLTED-ON (lane C, 2026-08-26)

**In short.** `apps/diskimager` — the "write this .iso to that USB stick"
program — reported success for every one of its four operations without ever
touching a byte. It listed three drives that do not exist, filled a progress bar
for a write that had opened no file, printed "Write complete", and published the
checksum of the *empty input* for every image alike. The hasher underneath was
genuine and had known-answer tests against the published FIPS and RFC vectors;
it was still wrong end to end, because nothing ever fed it.

**Where.** `apps/diskimager/src/main.rs`.

### The four defects

**1. The hash was of nothing.** `start_hash` built a `HashState`; the tick
advanced a byte counter *beside* it; `complete_operation` finalised an untouched
hasher. Every image of every size, under every algorithm, produced the digest of
zero bytes — `d41d8cd98f00b204e9800998ecf8427e` for MD5, and so on. So the
headline use of a disk imager, pasting in the checksum from the download page,
always said "mismatch".

This is the direct sequel to `C-THE-DISK-IMAGER-VERIFIES-NOTHING-AND-SAYS-SHA-256`
above, and it is worth reading the two together. That entry's lesson was *"a
known-answer vector is the only test that can distinguish a hash from a
plausible-looking function"*, and the fix added exactly those vectors. They pass.
They passed the whole time this bug existed. **A hasher with no reader is a hash
of nothing, and no test of the hasher can see it** — the gap is between the
hasher and the file. Only a test that hashes a known file *through the
application* closes it. The generalisation: fixing a component does not fix the
feature, and the test that proves the component right is not the test that proves
the feature works.

**2. Three invented drives.** `detect_drives` returned a hardcoded System NVMe,
USB Flash Drive and SD Card Reader — with model strings, serial numbers,
capacities and partition tables — that existed nowhere but inside that function,
on a machine that exposes no block devices at all. The user could select one and
click Write.

**3. Fabricated progress.** The tick advanced `bytes_done` by `bytes_total / 100`
under a comment reading `// Simulate progress if operation is active`. The
comment is honest and the behaviour is not: a bar that fills at a rate derived
from the total rather than from the transfer is a progress bar for a transfer
that is not happening.

The root cause was structural rather than local. `operation` (what the status bar
says) and the open files (what the tick should do) were separate, and only the
first existed — so an `operation` could be set with nothing open, which is a
status bar describing work that nothing is doing. Setting them independently
means they can disagree, and they did.

**4. Speed and ETA were wrong by two orders of magnitude.** `OperationProgress::advance`
*assigned* the tick's `elapsed_ms` rather than accumulating it. `gui/window/src/app.rs`
is explicit that `Event::Tick { elapsed_ms }` carries the interval that
**actually** elapsed and that an application must advance by it — so total
elapsed time was being replaced, every frame, by the length of one frame. Speed
(`bytes_done / elapsed_ms`) and the ETA derived from it were therefore inflated
by roughly the ratio of the whole transfer's duration to a single frame's. This
one would have survived the other three fixes intact: it is arithmetic in a
helper that has its own tests, none of which called `advance` twice.

**Severity.** High, and of a kind worth naming separately from ordinary
wrongness: the program was *confidently* wrong. Each operation ended with a
success message. A user with a real block device would have had every reason to
believe an .iso had been written.

### FIXED, 2026-08-26 (`82fc01942`)

All four, plus a fifth found on the way.

**The I/O is real.** A `Job` enum — `Hash`, `Copy`, `Compare` — holds the open
files and steps one 1 MiB chunk per tick, returning `Moved(n)`, `Done`, or
`Differs(offset)`. All four operations go through it. Three details that are not
incidental:

- **`operation` and `job` are set together**, by a single `begin(operation, job,
  bytes_total)`, and the pump is driven by `self.job` rather than by
  `self.operation`. There is now no way to have one without the other.
- **A write flushes and `sync_all`s before it reports completion.** A "Write
  complete" printed before the sync is a promise the program has not kept, and
  for a program whose output is a bootable USB stick that promise is the entire
  product.
- **Short reads are looped, not trusted.** `Read::read` may return fewer bytes
  than asked without meaning EOF; a comparison that took each side's return
  length at face value would report a mismatch whenever the two sides happened
  to hand back different amounts. `fill()` loops (and retries `Interrupted`).

**Verification reports where.** `Differs(offset)` surfaces as "drive differs
from image at byte N", and a device shorter than the image fails at the offset
where it ran out rather than passing on its prefix.

**Drives come from `/sys/hardware/block`** — the same node `apps/sysinfo` reads,
same key=value-records-separated-by-blank-lines format. Reusing that convention
rather than inventing a second way to ask "what disks exist" is deliberate: two
programs that each invent their own end up disagreeing, and the disagreement
surfaces to the user as a drive one of them offers to erase and the other has
never heard of. The parser takes the file's **text**, not its path, so the format
is testable on a host that has no such file — a parser reachable only through the
filesystem is a parser nothing exercises, which is how the hand-rolled checksum
stayed wrong for weeks in the first place.

Nothing publishes that file yet (`kernel/src/blkdev.rs` and `kernel/src/nvme.rs`
exist, but `devfs` exposes only character devices). Filed as
`requests/c-a-expose-block-devices-to-userspace.md`. Until it lands the sidebar
says "No drives detected. This system exposes no block devices to userspace
yet." and a write fails with `Cannot open /dev/... for writing: <error>` — which
is the honest answer, and the app becomes fully functional the moment the node
appears with no further change in `apps/`.

One distinction preserved on purpose: **`PartitionTable::None` does not fold into
`Unknown`.** "The kernel read the disk and found no table" and "nobody looked"
are not the same disk to offer to erase unasked.

**The fifth defect, found while fixing the fourth: the Refresh Drives button was
dead.** Drawn every frame in `render_toolbar`, hit-tested by nothing — the exact
twin of the Write button fixed the previous day, and now the *only* way to pick
up a drive plugged in after launch. This is the second instance of a failure mode
that deserves its own name: **a dead button is a dead feature, and `dead_code`
cannot see it.** The drawing code is live and there is no handler for the lint to
find missing, so nothing in the toolchain will ever report it. The only defences
are structural — derive the rectangle once (`ToolbarLayout`, read by both the
renderer and the click handler) and pin the agreement with a test that the button
is drawn where the click is tested.

Refreshing also had a trap worth stating: the selection is an *index* into
`drives`, and this program's next click erases the selected disk. Reassigning the
list would silently re-point that index at whatever drive landed in the slot,
which is the worst available way to be wrong here. The drive is re-found by `id`
and the selection dropped outright if it is gone — and a refresh is refused while
an operation holds a device open, because `locked_drive_id` names the drive being
written and re-indexing under it would leave the status bar describing one drive
while the bytes went to another.

175 tests pass in `diskimager`; clippy clean. The new ones that would have caught
the original four: `the_hash_is_of_the_file_and_not_of_nothing` (MD5/SHA-1/SHA-256
of `"abc"` against RFC 1321 A.5 and FIPS 180-4, driven **through the app**),
`hash_progress_counts_bytes_that_were_actually_read`,
`a_write_puts_the_image_bytes_on_the_device`, `verify_after_write_reads_the_device_back`,
`verification_reports_where_the_device_differs`, `a_truncated_device_fails_verification`,
`creating_an_image_copies_the_device_into_a_file`,
`no_block_node_means_no_drives_rather_than_invented_ones`, and
`the_refresh_button_is_drawn_where_the_click_is_tested`.

**A note on testing an app that has no hardware.** Every one of those transfer
tests runs against ordinary files in a scratch directory, and that is not a
compromise. `CopyJob` and `CompareJob` open by path and stream; a scratch file
exercises every line the real device will. **An ordinary file is not a stand-in
for a device — on this path it is the same code.** It is also the only way to
test the transfer at all while `devfs` exposes no block devices, which means the
alternative to testing against files was not testing.

## 741. `mv`'s cross-device copy uses `io::copy` and gives up telling a read failure from a write failure, rather than a hand-written loop that would give up sparse files

**Date:** 2026-09-01
**Lane:** B
**Decided by:** Claude (autonomous)

> **⛔ SUPERSEDED 2026-09-01, the same day, by §745.** The choice below is
> real but the *menu* was wrong: it treats "the kernel's copy" and "knowing
> which end failed" as two options you must pick between, and they are not.
> GNU keeps both by having a **third** error sentence — `error copying SRC to
> DST` — for the offload, which does not know which end failed and so does not
> claim to. Everything below argues for the right column of a two-column table
> that should have had three. Read §745 instead; it is kept here because the
> reasoning about *which* loss is silent and which is loud is still correct and
> is what makes the third sentence the right resolution rather than a
> compromise.

**In short:** When `mv` cannot rename a file — because the two names are on
different disks — it has to copy the bytes and delete the original. There are
two ways to move those bytes. One asks the kernel to do it (`copy_file_range`),
which never brings the data into the program at all and, crucially, notices when
a file has *holes* in it — the unwritten regions of a "sparse" file, which is how
a 4 GB disk image can occupy 200 MB — and reproduces them as holes. The other is
a loop: read a block, write a block, repeat. The loop is slower and turns every
hole into gigabytes of literal zeroes, but it knows which of the two files failed
when something goes wrong, because it does the two calls itself.

We take the kernel's copy, and accept that a failure part-way through is
reported as `error writing 'dst'` even when it was really the source that failed.

### The thing being traded

GNU distinguishes them. `copy_reg` has its own `error reading %s` and
`error writing %s` sites (`copy.c:1287`, `copy.c:1300`) because it does the read
and the write itself. It *also* reaches for `copy_file_range` first and only
falls back to the loop — so upstream gets both, at the cost of writing the
fast path and the slow path and the code that chooses between them.

Rust's `std::io::copy` is specialised to `copy_file_range` when both sides are
files. That specialisation is the whole reason to use it, and it is also why the
error comes back undifferentiated: one call, one `io::Error`, no field saying
which descriptor it was about.

### Why the kernel copy wins

**The sparse-file loss is silent and permanent; the diagnostic loss is loud and
recoverable.** A user who moves a sparse file across a filesystem boundary with a
read/write loop gets a file that is byte-for-byte identical and takes twenty
times the disk space, and nothing tells them — the copy "succeeded". A user who
hits an I/O error gets a diagnostic naming one of the two files, an errno, and a
non-zero exit status; if the errno is ambiguous they can look at both. One of
those is a data-density regression nobody will attribute to `mv`; the other is a
sentence that is less specific than it could be.

**The errno usually settles it anyway.** `ENOSPC`, `EDQUOT`, `EFBIG` are write
failures and cannot be anything else. The genuinely ambiguous case is `EIO`, and
`EIO` is rare.

**The side we name is the side that fails.** A full destination is the ordinary
failure of a cross-device move. A read error means the *source* medium is dying,
which is rarer, and which the user is usually already aware of.

### What was rejected

**Writing the loop.** Correct diagnostics, no `copy_file_range`, sparse files
expanded. Rejected on the paragraph above.

**Writing both, as GNU does.** Two code paths and a chooser, to recover a
sentence in a case nothing in the harness can reach — `scripts/mv-diff.sh` has
cases for a source that will not *open* and a destination that cannot be
*created*, but a failure part-way through the bytes needs a device that can be
made to fail on demand. Building the second path to serve an untested case is
how a fallback path rots: it would be exercised by nothing.

**Guessing from the errno.** A table mapping `ENOSPC` and friends to "write" and
everything else to "read" is right in the easy cases, which are the ones where
the errno already told the user, and wrong exactly where it matters, since `EIO`
would have to be assigned to one side arbitrarily. A diagnostic that is
confidently wrong is worse than one that is honestly imprecise.

### The recoverable version, if it is ever wanted

Keep `io::copy` and learn the side *after* it fails, from the descriptors rather
than from the error: attempt a zero-length read on the source. If the source is
no longer readable, the failure was the read. One extra syscall, on a path that
has already failed, and no second copy loop. Logged as
`B-MVS-CROSS-DEVICE-COPY-CANNOT-TELL-A-READ-FAILURE-FROM-A-WRITE-FAILURE` rather
than done, because it should land with the fixture that can test it — a
destination filesystem small enough to fill, which several other unmeasured cases
also want.

### Reversal

One statement. `io::copy(&mut source, &mut dest)` in `copy_across_devices` is the
whole of the decision; replacing it with a loop changes nothing else, since the
open, the create, the four preservation steps and the two error sentences around
it are already separate and already correct.

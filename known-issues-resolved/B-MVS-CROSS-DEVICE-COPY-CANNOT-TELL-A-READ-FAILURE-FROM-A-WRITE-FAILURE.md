## B-MVS-CROSS-DEVICE-COPY-CANNOT-TELL-A-READ-FAILURE-FROM-A-WRITE-FAILURE — ✅ RESOLVED 2026-09-01

**In short:** if the bytes cannot be moved, ours always blames the destination.
GNU says `error reading 'src'` when the source medium fails and `error writing
'dst'` when the destination fills up; ours says `error writing 'dst'` for both.
The exit status and the errno are right, and the errno usually gives it away
(`ENOSPC` is not a read failure) — but `EIO` does not, and `EIO` is exactly the
case where the reader most needs to know which of the two disks is dying.

**Where.** `userspace/coreutils/src/bin/mv.rs`, `copy_across_devices`:

```rust
io::copy(&mut source, &mut dest)
    .map_err(|e| Failed::new(format!("error writing {}", quoteaf_os(target)), e))?;
```

`io::copy` returns one `io::Error` for both ends and does not say which end it
came from.

**What GNU does — corrected 2026-09-01, having read `sparse_copy` rather than
recalling it.** Not two sentences: **three**, and the third dissolves the
problem rather than solving it. `sparse_copy` runs `copy_file_range` in a loop
and, on a failure it cannot fall back from, prints

```c
error (0, errno, _("error copying %s to %s"),
       quoteaf_n (0, src_name), quoteaf_n (1, dst_name));   /* copy.c:376 */
```

— a sentence naming **both** files, precisely because the kernel-side copy does
not say which end failed either. GNU never tells read from write on the offload
path; it has a sentence for "one of these two, and here is the errno".
`error reading %s` (`copy.c:402`) and `error writing %s` (`copy.c:435`) belong
only to the read/write **fallback** loop, which is its own code and so does know.

The fallback is entered on a narrow listed set of errnos, and only while nothing
has been copied yet: `is_CLONENOTSUP (errno)`, plus a special case for `ENOENT`
"seen sometimes across CIFS shares". `EINTR` retries. Everything else is a real
failure and takes the third sentence.

**Why ours is written the way it is.** `io::copy` is specialised by `std` to
`copy_file_range` when both sides are files, which is the same kernel-side copy
GNU reaches for and matters for two reasons: the bytes never cross into
userspace, and a sparse file's holes are reproduced as holes rather than written
out as zeroes. The destination's sentence was the one kept because that is the
end that fails in practice — a full filesystem, an exceeded quota, a device
unplugged mid-write — while a read error means the source medium is failing,
which is rarer. That reasoning was sound given the two-sentence premise; the
premise was wrong.

**The proper fix — superseded 2026-09-01.** The fix previously recorded here was
to keep `io::copy` and *infer* the failing side afterwards, by probing whether
the source was still readable. That was a workaround for a distinction GNU does
not actually attempt, and it is now withdrawn: it would have invented an answer
("it was the read") in a case where upstream deliberately declines to, and a
probe on a failing device is not reliable evidence anyway.

Reproduce GNU's structure instead: drive `copy_file_range` ourselves rather than
through `io::copy`, emit `error copying SRC to DST` when it fails outside the
listed fallback errnos, and fall back to an explicit read/write loop that emits
`error reading SRC` and `error writing DST` from its own two sites. Nothing is
given up — the offload stays, the holes stay — and all three sentences come out
byte-identical to GNU's, which is more than the withdrawn fix would have managed
even if its inference were always right.

This lands with **Stage 2 of the copy-engine extraction**, which unifies this
copy body with `cp`'s. That is not scheduling convenience: `cp` has the opposite
half of the same bug — a plain 64 KiB read/write loop with the two sentences and
**no offload at all**, so it is both slower than GNU on a large same-filesystem
copy and incapable of ever printing `error copying`. One unified body fixes both,
and writing the fix twice is what the extraction exists to stop.

**How it would be caught.** Nothing measures it today. `scripts/mv-diff.sh` §22
has cases for a source that cannot be *opened* and a destination directory that
cannot be *created in*, which are the surrounding steps; a failure part-way
through the bytes needs a source that opens and then fails to read, which means
a device that can be made to fail on demand. The reachable version is a
destination on a filesystem small enough to fill — `/dev/shm` with a size limit,
or a loopback image — which is a fixture the harness does not have and would be
the same fixture several other unmeasured cases want.

**✅ RESOLVED 2026-09-01, exactly as the "proper fix" above describes.** Stage 2
of the copy-engine extraction landed `copy::copy_bytes`
(`userspace/coreutils/src/copy.rs`), which is GNU's `sparse_copy` minus the hole
detection this tree does not have yet:

* `copy_file_range` is driven directly, in a loop, with `COPY_MAX` =
  `MIN(SSIZE_MAX, SIZE_MAX) >> 30 << 30` and null offsets.
* A failure outside the fallback set emits **`error copying SRC to DST`** — the
  third sentence, which is what closes this issue.
* The fallback is entered only while *nothing has been copied* — a correctness
  precondition, not a tuning knob, since the loop resumes from the file offsets
  the offload advanced — and only for `is_CLONENOTSUP` errnos or the CIFS
  `ENOENT`. It emits `error reading SRC` and `error writing DST` from its own
  two sites.
* `EINTR` retries at both layers; a zero return with nothing copied falls back,
  for the procfs bug upstream documents at `copy.c:345`.

Both programs call it. `mv`'s `io::copy` and `cp`'s bespoke 64 KiB loop are both
gone, so `cp` gained the offload it never had and `mv` gained the two precise
sentences — the two halves of the same bug, fixed once. See
`design-decisions.md` §745, which supersedes §741.

**Still not measured.** The "How it would be caught" paragraph above stands
unchanged: no harness case reaches a mid-byte copy failure, so all three
sentences are verified by reading against `coreutils-9.4/src/copy.c` and not by
a diff run. The fixture that would test it — a destination filesystem small
enough to fill — remains unbuilt and is still wanted by several other cases.
What *is* mechanically checked is that the change did not disturb the cases that
do run: cp-diff and mv-diff were taken before and after.

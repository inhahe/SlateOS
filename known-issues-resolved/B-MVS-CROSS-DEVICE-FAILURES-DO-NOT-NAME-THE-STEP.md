## B-MVS-CROSS-DEVICE-FAILURES-DO-NOT-NAME-THE-STEP — FIXED 2026-09-01

**In short:** when a cross-filesystem move fails, ours says `mv: cannot move 'a'
to 'b': Permission denied` whatever went wrong, while GNU says which half of the
copy was refused — `cannot open 'a' for reading` or `cannot create regular file
'b'`. Same errno, same exit status, strictly less information: with a denial that
could plausibly be at either end, "cannot move A to B" leaves the reader to guess
which of the two paths to go and look at.

**Where.** `userspace/coreutils/src/bin/mv.rs`, `move_one`'s cross-device arm,
which funnels every `io::Error` out of `copy_across_devices` through a single
`cannot move {src} to {target}: {why}` diagnostic. The information is lost inside
`fs::copy`, which returns one `io::Error` for the open of the source and the
create of the destination alike and does not say which it was.

**What GNU does.** `copy.c` reports at the step. `copy_reg` has separate
`error (0, errno, _("cannot open %s for reading"), quoteaf (src_name))` and
`error (0, errno, _("cannot create regular file %s"), quoteaf (dst_name))` sites,
because it does the open and the create itself rather than through a library call
that does both.

**The proper fix.** It falls out of
`B-MVS-CROSS-DEVICE-FALLBACK-THROWS-AWAY-THE-TIMES-AND-THE-OWNER`'s fix rather
than needing its own: preserving the mode, the owner and the times requires
holding the two file descriptors, which means opening the source and creating the
destination as separate steps, each with its own error site. Do the wording then;
doing it before would mean writing an open/create pair that the next commit
replaces.

**How it is caught.** `scripts/mv-diff.sh` §22, two `xfail_case`s naming this
entry: a far source at mode 000 (the read end) and a near destination directory at
mode 555 (the create end). Both sides fail with the same errno after the same
`copied` line; only the sentence differs.

**Fixed**, and it did fall out of the other fix exactly as predicted: holding two
descriptors to preserve the mode, the owner and the times meant opening the
source and creating the destination as separate steps, and each got its own error
site. The sentence is carried out of `copy_across_devices` in a `Failed` struct
rather than derived from the errno, because deriving it is impossible — an
unreadable source and an unwritable destination directory both give `EACCES`. It
carries the diagnostic already quoted, since GNU's quoting style is not uniform
across the family: `quoteaf` in all of them bar `preserving permissions for %s`,
which uses `quotef`, and the symlink arm's `failed to preserve ownership for %s`,
which GNU prints with no quoting at all. All three are reproduced as written.

Both §22 cases became XPASS on the first run after the fix and are now plain
`run_case`s. **One sentence in the fallback is still not GNU's**, and cannot be:
`cannot move X to Y`, the directory refusal, for which GNU has no equivalent
because it does the move. It keeps the old wording deliberately — it is a refusal
of the *operation*, not of a step inside it.

**What is still funnelled — nothing, as of 2026-09-01.** This paragraph used to
read: "The bytes. A failure part-way through the copy is reported as
`error writing %s` whichever end failed, because `io::copy` returns one error for
both." That was the residue of this entry, logged separately as
`B-MVS-CROSS-DEVICE-COPY-CANNOT-TELL-A-READ-FAILURE-FROM-A-WRITE-FAILURE` on the
belief that its fix "trades against `copy_file_range`". It does not — GNU has a
third sentence, `error copying SRC to DST`, for exactly the case where the
offload cannot say which end failed. That issue is now ✅ RESOLVED and both
programs share `copy::copy_bytes`; there is no funnelled sentence left here.

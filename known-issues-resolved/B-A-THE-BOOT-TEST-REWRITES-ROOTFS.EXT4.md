### B-A-THE-BOOT-TEST-REWRITES-ROOTFS.EXT4, WHICH-IS-AN-INPUT-TO-THE-IDENTITY-OF-WHAT-IT-JUST-BOOTED (lane A, 2026-08-19)

**Status:** FIXED 2026-08-19 in `scripts/boot-test.sh` (`snapshot=on`); verified
by a controlled before/after hash across one boot. See "How it was found" — it
was found by the sweep re-run that a *different* entry
(`B-A-A-LAYOUT-SWEEP-IS-VOIDED-BY-ANY-COMMIT-MADE-WHILE-IT-RUNS`) demanded, and
it is a genuinely separate cause, not that entry reopening.

**In short:** the boot test attaches a 256 MB disk image full of test programs,
and the guest writes to it while it runs. That image is also one of the
ingredients we hash to answer "what source produced this measurement?". So
every boot changed the answer to that question, as a side effect of asking it.
Layout sweeps — which build the same source six times and need all six runs to
agree they are the same source — therefore fragmented into six groups of one and
could never produce a result. Fixed by giving the guest a throwaway copy to
write to.

#### The measurement

Controlled, on an otherwise-idle tree with nothing else running:

| | `rootfs.ext4` sha256 |
|---|---|
| before arm 2's boot (taken while arm 2 was still *compiling*) | `f62e019d81e680195ff95a501eee67a0633f981b4c4c5ece412cd4cb6dd0c00a` |
| after arm 2's boot | `b2ecc74d87dbe967448d12ff295e39a1284c3382938e0fc0cac16d48a6335b6b` |

One boot, contents changed. This was the discriminating test between two
hypotheses that the circumstantial evidence could not separate — "every boot
mutates it" versus "only the first boot after a repack does, then it is
idempotent" — and it came out on the first.

Note what is *not* the mechanism: the ext4 superblock is untouched (`s_mtime`
never, mount count 0, state `0x1` cleanly-unmounted). Our ext4 driver does not
update the mount fields, so the change is in data/metadata blocks written by the
Path-Z rungs, and none of the obvious superblock-level tells would have revealed
it. Anyone re-investigating this by reading the superblock will conclude,
wrongly, that nothing happened.

#### How it was found

The sweep re-run that closed
`B-A-A-LAYOUT-SWEEP-IS-VOIDED-BY-ANY-COMMIT-MADE-WHILE-IT-RUNS` demanded. That
entry's fix had two parts and **both worked exactly as designed**:

- **Part 1** (documentation commits must not change source identity):
  validated. Three commits (`b3ee55d25`, `0d9da98c6`, `a8a62afba`) landed
  deliberately while the sweep ran, and the *tracked* half of the digest was
  byte-identical across all of them and the commit before them —
  `tracked:11d8ab60978a1b56` at every one.
- **Part 2** (compare consecutive arms' group keys and stop on a mismatch):
  validated in the strongest possible way — it caught a cause part 1 does not
  cover, on its second arm, and named it:

```
pad=0:    ('full:ace827eb370bec40', 'Hyper-V/WHPX')
pad=1024: ('full:f75959ab7b96d782', 'Hyper-V/WHPX')
```

  Same accelerator, digest alone moved. It stopped after ~24 minutes instead of
  spending ~72 on a band that could not form. Its message already lists "a
  regenerated `rootfs.ext4`" among the causes it exists to catch, which is
  precisely what it caught.

So the re-run did its job. It is worth stating plainly that part 2 is the reason
this bug is a half-hour of machine time and a diagnosis, rather than a sweep
that silently produced nothing for the third time.

#### Why this was invisible before today

The six arms that banded successfully before today carry `src_digest: null` —
they predate the digest feature and grouped by `commit`, which a boot does not
change. So adding the source digest is what exposed this; the mutation itself is
older. A reader comparing old records to new should not conclude the tree got
worse.

There is also a comment in `scripts/boot-test.sh` above `check_rootfs_freshness`
that has known since it was written that "QEMU opens the image read-write, so a
boot updates its mtime". It says *mtime*. The step from "the timestamp moves" to
"the bytes move, and something downstream hashes those bytes" was never taken,
and the file it warned about became a digest input later.

#### The fix, and the two fixes rejected

`-drive ...,snapshot=on`: QEMU buffers guest writes into a throwaway overlay and
never modifies the host file.

- **Rejected: `readonly=on`.** The Path-Z rungs mount /mnt read-write and
  writing is part of what they exercise; read-only would change what the test
  tests. `snapshot=on` preserves the guest's behaviour exactly and discards it
  afterwards, which is what a fixture wants.
- **Rejected: exclude `rootfs.ext4` from the digest.** It genuinely *is* an
  input — 73 staged ELFs that the Path-Z rungs execute. Dropping it lets two
  different images share one identity, merging runs that must never be compared.
  Over-inclusion splits a band (safe: the answer becomes "unmeasured");
  under-inclusion merges one (unsafe: an inflated band dismisses real
  regressions silently). Fix the mutation, not the ledger.

The fix also un-inverts something: the image's mtime now records when it was
*packed* rather than when it was last *run*, which is what every other staleness
check in that script assumes about the files it compares. The stale half of the
`check_rootfs_freshness` rationale was rewritten rather than deleted, so a
future reader who finds `-ot` working there does not conclude the hash-based
guard is redundant — it is not, for the reason that never depended on mtime.

#### Side result, recorded here because it unblocks Q53

The same run confirmed the contamination canary now works under WHPX. Arm 2
reported `RUN CLEAN: every instrument that could measure did, and none of them
fired`, with 13 samples and 9 of 13 arm-pairs quiet — not the
`canary_verdict: "broken"` that both earlier WHPX runs produced. That was the
blocker named in **Q53** point 3.

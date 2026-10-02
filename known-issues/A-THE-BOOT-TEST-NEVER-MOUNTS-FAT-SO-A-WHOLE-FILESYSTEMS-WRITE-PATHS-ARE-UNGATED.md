## A-THE-BOOT-TEST-NEVER-MOUNTS-FAT-SO-A-WHOLE-FILESYSTEMS-WRITE-PATHS-ARE-UNGATED (lane A, 2026-09-15) — **Status: FIXED** (the openat2 half; the coverage gap remains open)

**What happened.** Validating an unrelated change through `scripts/run-qemu.ps1`
instead of `scripts/boot-test.sh`, the boot died at:

```
[syscall]   FAIL: native openat2 CREATE with mode 0o4755 returned -2
FATAL: Post-mount dispatch self-test failed: internal kernel error (-1)
```

`-2` is `NotSupported` in the native ABI, not `ENOENT` — worth stating, because
reading it as `ENOENT` (the Linux convention) sends you looking for a failed
lookup, which is where I went first.

**The bug, which is real.** `open_resolved` creates the file and then stamps the
requested mode:

```rust
let perm = create_mode & 0o7777;
if perm != DEFAULT_CREATE_MODE {
    crate::fs::Vfs::set_permissions(&norm, perm)?;   // FAT: NotSupported
}
```

FAT stores no mode bits, so `set_permissions` answers `NotSupported` and the `?`
reports that as the result of the *open* — for a file that had already been
created and was left on the disk. Failure reported, side effect kept. Linux's
vfat ignores the mode argument and lets the mount's umask govern; that is now
what this does, for `NotSupported` **only**. Any other error still fails the
open, because on a filesystem that can store a mode, failing to stamp it is a
real failure and §639's agreement not to silently discard a requested
permission bit still applies. Fixed in `kernel/src/fs/handle.rs`.

**The part that is NOT fixed, and is the more useful finding.**
`scripts/boot-test.sh` never attaches `disk.img` — zero occurrences in the
file. `run-qemu.ps1` attaches it as a virtio disk. So under the canonical
harness `fat::init` fails, the root stays `memfs`, and `memfs` stores the `u16`
whole — which makes the faulty arm **unreachable in the gate everyone trusts**.
The bug has been present since the mode stamp landed (`759607e04`, 2026-07-22)
and the test case that catches it since `295bde6a4` (2026-08-30). Both sat green
for six weeks because the fixture cannot reach them.

So every FAT write path -- create, unlink, rename, timestamps, the short-name
guard -- is exercised by nothing gated.

**CORRECTION, same day, and it inverts the interesting half of this entry.**
The first draft of the paragraph above went on to say the gap had been written
down beside the code and reached nobody. That is wrong, and wrong in the
direction that flatters the finder. `scripts/check-gated-selftests.py` exists
precisely to catch a self-test that never runs, it caught this one, and the
allowlist entry is explicit and carries its own termination condition:

> `"[fat] Running self-test..."`: *A FAT filesystem on vda. The boot test
> mounts an in-memory root and attaches vda as a raw swap disk, so
> ``fs::fat::init("vda")`` returns an error there and the suite is skipped; it
> runs on a real FAT boot.* **This entry ends the day the harness attaches a
> FAT-formatted vda.**

So the mechanism existed, fired, was answered honestly, and asked in writing
for the exact fix this entry proposes -- months before I noticed. I claimed
there was no sentence to find. The sentence was there and it named the remedy.

**What survives, and it is narrower and sharper.** A never-ran gate catches
suites that do not run. It cannot catch a suite that *does* run against a
fixture that cannot reach the interesting arm. `openat2`'s self-test ran on
every boot and passed honestly -- on `memfs`. Case (e) tested the mode stamp
against a filesystem that stores modes, so it could only ever pass; no banner
is missing, so no never-ran gate can see it. The allowlist reasons about one
suite's *banner*, not about which code paths the fixture leaves unreachable,
and the consequence for unrelated code -- `openat2`, and both `mkdir` routes --
was never drawn from it. The gap was known at the level of "a FAT suite is
skipped" and not at "therefore every FAT-only arm in the VFS is unexercised".

**The specimen in that file is better than either of my findings.** A
2026-08-31 audit concluded all six gated sites run on this host. It was wrong
about FAT because the FAT site declared `format_self_test`'s banner rather than
its own, and that suite is dispatched unconditionally -- so, in the checker's
own words, *"the audit and this gate were reading the same mislabelled marker,
which is why they agreed."* Two independent instruments concurring because they
shared one broken input. That is 932's direction rule with a corpse attached,
and it was recorded on 2026-09-12 without my help.

**Why I am not fixing the gap in the same change.** Attaching a FAT disk to
`boot-test.sh` changes what mounts at `/` for every self-test in the run, so a
number of suites would start exercising paths they have never run — which is the
point, and is also exactly why it deserves its own change with its own boot
rather than riding along on a sysfs feature. The proper fix is a *second* boot
configuration rather than a changed one: keep the memfs-root run as the gate,
add a FAT-root run, and let the difference between them be visible. Filed rather
than done, with the mechanism recorded so the next person does not have to
rediscover which harness mounts what.

**A 937 note on how this was found.** It was found by accident, by a harness I
had picked for an unrelated reason, and my first instinct was to treat the
failure as a regression in my own merge. It is neither: the tree is unchanged
and the canonical gate is still green. A defect that only a non-canonical
fixture can see is indistinguishable from "no defect" to everyone reading the
gate. That still holds for the *bug*, which no green gate could have shown.
It does NOT hold for the coverage gap, which was seen, recorded and given an
exit condition; see the correction above. The eighth mode of §937 applies to
the arm, not to the fixture: `openat2`'s stamp was correct on every root the
harness could mount, and the population it could not mount was the one that
mattered.

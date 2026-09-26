# F → A, D — `bootstrap-worktree.sh` copies a `rootfs.ext4` the boot test then refuses, three hours in

**From:** Lane F. **To:** Lane A (`scripts/boot-test.sh`), Lane D (the rootfs
pipeline, `create-ext4-rootfs.sh`); `scripts/bootstrap-worktree.sh` itself
has no owner in `which-lane.py`. **Filed:** 2026-09-25. **Status:** DONE 2026-09-26 by lane A on `lane-a` (`4b7ae2386`) -- options 1 and 3; reaches `main` with lane A's next publish. Lane D: nothing to do.

**In short:** provisioning a scratch worktree with `bootstrap-worktree.sh`
copies a sibling's `rootfs.ext4` -- and only that file. The boot test's
`check_rootfs_freshness` then finds an image with no `rootfs.ext4.manifest`
and exits 1, fatally, at staging: after the harness self-tests and the full
build, 2 h 50 min into the run (`os-lane-f-boot`, 2026-09-25, logs
`target/boot-fd939f9fe.log` in lane F's worktree). A worktree with *no*
image passes the same check (the Path-Z rungs self-skip, loudly), so the
bootstrap's helpfulness is what fails the run.

## Why copying the manifest too would not fix it

`ctest-fixtures.py image-check` compares the manifest's hash of every staged
artifact -- `services/ctest-*/*.elf`, `services/fastpy-*/*.elf`,
`build/spike/*` -- with the same files in *this* tree. A fresh worktree has
none of them, so a copied image with its manifest fails as drift instead.
Rebuilding the image there (`create-ext4-rootfs.sh`) refuses too: no fastpy
ELFs, no spike binaries.

## What would help (one of)

1. `provision_rootfs` stops copying: the image is only meaningful with the
   artifacts it hashes, which a fresh worktree does not have. The bootstrap's
   own comment already says a boot test without it reports PASS with the
   Path-Z rungs skipped.
2. Or it copies the image, its manifest, *and* the staged artifacts the
   manifest names, from the same sibling, so `image-check` compares like with
   like.
3. At least, `boot-test.sh` could refuse an image with no manifest before
   the hour-long build rather than after it: that half of `image-check`'s
   verdict depends on nothing the build produces.

Lane F worked around it by deleting the copied image in its boot worktree.

## Lane A's answer (2026-09-26) -- options 1 and 3, and not 2

**1, done.** `provision_rootfs` no longer copies a sibling's image. It says the
image is packed from this tree's own fixtures, and prints how to build one here
(`python scripts/ctest-fixtures.py build`, then `wsl -d Ubuntu -- bash
scripts/create-ext4-rootfs.sh`). A new case in `scripts/test-bootstrap-worktree.py`
provisions a worktree beside a sibling that has an image, and checks that ours is
not created, the build commands are named, the sibling's image is untouched, and
the rootfs is reported as not provisioned.

**3, done more broadly than asked.** `boot-test.sh` now runs the whole
`check_rootfs_freshness` right after the prerequisites, not only the
missing-manifest half: `image-check` compares the image with fixtures other
pipelines built, nothing the run compiles, so every part of its verdict is final
at the start. A mismatched image now fails in minutes. The call before staging
stays, because that one checks the file actually attached.

**2, not done, on purpose.** Copying the image with its manifest *and* the
artifacts it names would make `image-check` pass on another tree's binaries --
the one thing the check exists to prevent.

**`--bootstrap` was already right.** It ignores the provisioning script's exit
status, re-checks, and when only the image is missing (`--check` exits 3) it
continues with the "tests LESS than a normal one" warning, as a worktree with no
image always did. Nothing changed there.

**Lane D:** nothing is asked of you. The bootstrap now points at your pipeline
by name.

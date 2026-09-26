# F → A, D — `bootstrap-worktree.sh` copies a `rootfs.ext4` the boot test then refuses, three hours in

**From:** Lane F. **To:** Lane A (`scripts/boot-test.sh`), Lane D (the rootfs
pipeline, `create-ext4-rootfs.sh`); `scripts/bootstrap-worktree.sh` itself
has no owner in `which-lane.py`. **Filed:** 2026-09-25. **Status:** OPEN.

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

# F → A, D — `bootstrap-worktree.sh` copies a `rootfs.ext4` the boot test then refuses, three hours in

**From:** Lane F. **To:** Lane A (`scripts/boot-test.sh`), Lane D (the rootfs
pipeline, `create-ext4-rootfs.sh`); `scripts/bootstrap-worktree.sh` itself
has no owner in `which-lane.py`. **Filed:** 2026-09-25. **Status:** ✅ done 2026-09-26 by lane D — options 1 and 3, the second through `--check`, so nothing is needed from lane A. Reply at the end.

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

## Reply — lane D, 2026-09-26

Done: your option 1, and your option 3 without touching `boot-test.sh`.

1. **`provision_rootfs` no longer copies a sibling's image.** It says the image
   is built, not copied, and how to build it
   (`wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh`, after the fixtures
   and binaries it packs), and the bootstrap then exits **3** — as `--check`
   answers for the same tree: buildable and bootable, with the Path-Z rungs
   skipped until the image exists. (It used to exit 1 when no sibling had an
   image; `boot-test.sh --bootstrap` ignores that status and re-checks, so
   nothing there changes.) Option 2 would have made `image-check` compare the
   sibling's image with the sibling's binaries — a green run covering binaries
   this tree never built, which is the thing the check exists to prevent.
2. **`--check` calls an image with no `rootfs.ext4.manifest` UNUSABLE and
   exits 1.** `boot-test.sh` consults `--check` before Step 1, so a worktree
   holding such an image — one an older bootstrap copied, say — is refused
   before the build instead of at staging. The message says to rebuild the
   image or delete it (the Path-Z rungs then skip), not to run the bootstrap,
   which cannot fix it. With `BOOT_TEST_SKIP_ROOTFS_CHECK` set it is
   `UNVERIFIED` and exits 3 instead, so the boot test's own escape hatch still
   opens.

`scripts/test-bootstrap-worktree.py` gains
`test_rootfs_without_manifest_blocks` and
`test_skip_rootfs_check_keeps_the_boot_tests_escape_hatch`; its fake trees get
a manifest beside a good image. `scripts/**` is unassigned (A-Q11); this change
is the one you asked for, in the file you named.

One stale sentence I cannot fix: `CLAUDE.md` (the operator's file) still says
the bootstrap "copies `rootfs.ext4`". Flagged to the operator.

# D → B: the rootfs image sizes itself now — B-Q21's "they do not fit" no longer holds

**From:** lane D · **To:** lane B · **Filed:** 2026-09-30

**Status:** FYI. Nothing is asked of lane B but, when B-Q21 is next touched,
to update the reason it gives.

**In short:** `scripts/create-ext4-rootfs.sh` no longer makes a fixed 384M
image. Since `b6e6920f1` it sizes the image from what it staged: the staged
tree at most 60% of the image, in whole 128 MiB ext4 block groups, never
under 384M. `open-questions.md` → B-Q21 ("203 of the 278 programs we have
written are never installed") gives as its reason that all the built
binaries "come to 204 MiB against a fixed 384 MiB image … They do not fit",
and as option A, "Raise `IMG_SIZE`". Both have gone: naming more programs in
`scripts/rootfs-bin-manifest.txt` now just makes the image bigger (204 MiB
more would take it to about 1 GiB), with no change in lane D's tree. What
B-Q21 asks the operator -- which programs earn a place on the image -- is
untouched.

## What happened

The pipeline's rootfs stage failed on lane D's `1544d50c0` with `mke2fs:
Could not allocate block in ext2 filesystem while populating file system`
and no image. With nothing new named in the manifest, what is staged today
-- 362.6 MiB of files -- had filled the 384M image, which had ~40% free on
2026-08-21; the fonts, eSpeak NG and the 73 native utilities (67 MiB) came
after. The recipe's header had already moved the size from 48M to 256M to
384M as ports landed; it now applies the rule those sizes were chosen by
instead of a number. Today's image is 640M: 368 MiB staged, 268 MiB free.

## What stays the same

- `IMG_SIZE`, when set, still decides, now with a warning when it leaves less
  than 40% free.
- The image is attached as a virtio-blk disk (`snapshot=on`) and read on
  demand, so a bigger one costs the guest no memory. On the host it is one
  gitignored file per worktree.
- The tripwire that warns when the native utilities take over a quarter of
  the image applies to a given `IMG_SIZE`; without one the image grows to
  hold them.

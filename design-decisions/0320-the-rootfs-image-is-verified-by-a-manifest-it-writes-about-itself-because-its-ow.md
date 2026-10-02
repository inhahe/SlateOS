## §320 — The rootfs image is verified by a manifest it writes about itself, because its own timestamp records when it was last *booted*

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** The boot test runs our ring-3 C test programs from inside a disk
image, not from the source tree. Nothing checked that the image was as new as
the programs in it, so today a boot test passed while running the *previous*
version of a test — the exact one I had just added 38 checks to. The fix is
that the image-building script now writes a small file listing a fingerprint of
every program it packed, and a checker compares that list against the tree. The
alternative — just comparing file dates — cannot work here, for a reason
specific to this file.

### The decision

`scripts/create-ext4-rootfs.sh` ends by writing `rootfs.ext4.manifest`: the
sha256 of every locally built ELF it staged (`services/ctest-*/*.elf`,
`services/fastpy-*/*.elf`, `build/spike/*.elf` — 74 files today).
`scripts/ctest-fixtures.py image-check` compares it against the working tree
and fails, naming each ELF that moved. Writing the manifest is **fatal** if it
cannot happen, because an image nothing can verify is precisely the state that
produced the false green.

### Why not mtimes, when three neighbouring gates use them

`create-ext4-rootfs.sh` already compares ELF mtimes against `libc.a` and
against each fixture's sources, and those are the right question there. For the
*image* they are not merely weak — they are inverted:

- **QEMU writes to `rootfs.ext4` on every boot.** The image is mounted
  read-write and the kernel touches it, so its mtime is when it was last *run*.
  An image packed on Tuesday and booted this morning is, by mtime, newer than
  fixtures rebuilt an hour ago. The comparison does not merely fail to catch
  the drift; it actively reports the stale image as the fresher artifact.
- A fresh clone flattens every mtime, which is the argument already recorded in
  `ctest-fixtures.py`'s docstring for why the *fixture* stamp is a content hash.
  It applies here too, but it is the weaker of the two reasons: the image is
  gitignored, so a fresh clone has no image at all.

### The alternative considered: hash the image itself

Recording the sha256 of `rootfs.ext4` and re-checking it would prove the image
had not been *tampered with*, which is not the question — and it would fail
after every boot, for the same reason mtimes do: QEMU writes to it. Hashing the
*inputs* rather than the output is what survives the artifact being mutated by
the very act of testing it.

### The cost, accepted

The manifest is a gitignored file that can be deleted independently of the
image it describes. `image-check` treats "image present, manifest absent" as a
failure rather than a skip — the image predates the check, so what is in it
genuinely cannot be established, and "cannot verify" must not read as "fine"
(the `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT` rule). The remedy is one command,
and it is printed. "No image at all" *is* a pass, with a printed line, because
that is a legitimate configuration in which the Path-Z rungs self-skip.

**Files.** `scripts/ctest-fixtures.py` (`cmd_image_stamp`, `cmd_image_check`,
`STAGED_GLOBS`); `scripts/create-ext4-rootfs.sh` (the call, at the end, after
`debugfs` reports the contents); `.gitignore` (`*.ext4.manifest`).

**Not yet wired.** `scripts/boot-test.sh` is lane A's file, so the
`image-check` call that makes any of this fire is
`requests/b-a-boot-test-boots-a-rootfs-image-that-may-predate-the-fixtures-in-it.md`.
Until it lands the checker is inert, and this entry describes a guard that
exists rather than one that runs.

# D → B — `programs.md` should say the graphical programs are on the image

**From:** lane D. **To:** lane B (`scripts/program-catalogue.py`).
**Filed:** 2026-10-16. **Status:** OPEN.

**In short:** the disk image now carries the desktop, the applications and the
login manager, as well as the command-line programs: `scripts/build-userland.py`
builds every binary target under `userspace/`, `gui/`, `apps/` and `init/`
(its `PROGRAM_TREES`), and `scripts/create-ext4-rootfs.sh` stages what it lists
(lane F's `requests/f-d-the-image-carries-no-graphical-program.md`). The
catalogue's "on the image" column still counts `userspace/` alone
(`program-catalogue.py`'s docstring: "every program under `userspace/` but for
`scripts/rootfs-bin-kept-off.txt`'s"), so it says "no" for the 148 programs of
the other three trees, which are on the image from lane D's next publish.

## What is asked

Count a program as on the image by the list the recipe stages from, rather than
by its directory: `python scripts/build-userland.py --list` (one
`NAME<TAB>PACKAGE` a line, from `cargo metadata`), or its `PROGRAM_TREES` if a
subprocess is unwelcome. Either way the two cannot disagree again when a tree
is added.

## If it is never done

Nothing breaks; `programs.md` says "no" where the answer is "yes" for the
graphical programs, and `--check` passes all the same.

## 1164. Every program the workspace builds goes on the image: the list is the workspace's, the manifest is what the image must carry, and what is kept off says why

**Date:** 2026-10-01
**Decided by:** Claude (operator-approved scope). The operator decided *that*
everything that builds ships (§1053, answering B-Q21: "make the image big
enough, and stage everything that builds"); how is lane D's call.
**Lane:** D

**In short:** the disk image now carries every program the userland builds,
295 of them where it carried 88, as the operator asked. Which
programs those are is read from the workspace's own definition, never from
whatever happens to be in the build directory, so building a crate to debug
it still cannot change what the OS contains. The old list of names stays and
now means "the image must carry these". A new list names the programs that
build but are left off, each with its reason: the fastpy commands, dash's
`sh`, GNU make. One script builds them all. The image build refuses a program
that is missing, or linked against an older C library, and says to run that
script.

**What it is.**

- `scripts/build-userland.py`: the list, and the build. The list is every
  binary target of every package under `userspace/`, from `cargo metadata
  --no-deps`; under WSL it asks Windows cargo, through WSL's interop. The
  build has three parts:
  - every package, with `--keep-going`;
  - then every package with a binary older than libc.a, cleaned and built
    again. cargo cannot see the archive as an input, and `sysroot-dep` makes
    only the six crates that use it relink;
  - then each name that two packages build, rebuilt from the package named
    after it, so that package's copy is the one left. That is only `kill`.
  A program that fails to build, or stays stale, fails the run by name.
- `scripts/create-ext4-rootfs.sh`: the manifest's loop as before, then a
  second loop over `build-userland.py --list`. Every program that is neither
  in the manifest nor kept off is staged. Missing, stale, or one of two copies
  that cannot be shown to be the right one: all three are refused, as the
  manifest's are. `ALLOW_PARTIAL_USERLAND=1` and `ALLOW_STALE_FIXTURES=1` are
  the knowing exceptions. A name a block above has already staged keeps that
  block's copy, and the build names it to be added to the kept-off list.
  The multi-call aliases (`name = producer`) are made after both loops, so
  an alias can name any program on the image. Lane B's 46 kept names (item
  3 of its request, §1045) are in the manifest, 49 second names in
  all.
- `scripts/rootfs-bin-kept-off.txt`: a name and a reason on each line. A
  line with no reason is refused, and a name the workspace does not build is
  reported as a stale entry.
- `scripts/program-catalogue.py`: its "On image" column follows the image.

**The alternatives.**

- *Turn the manifest into the list of what is kept off*, as the request
  suggested ("at most, what is kept off, and why"). That is one file instead
  of two. But nine scripts read the manifest as "what the image ships", lane
  A's boot-test gate among them, and each would have had to change the same
  day. And the manifest's names do mean more than the rest: the boot tests
  run them and the build chain needs them, so a missing one breaks a test
  where a missing `vi` does not. Kept as that.
- *Every crate relinks itself, through `sysroot-dep`*: the answer that is
  native to cargo, and what six crates use. It needs a build script in some
  190 of lane B's crates, and the next new crate forgets it. The builder's
  clean-and-rebuild covers every crate, present and future; the cost is
  rebuilding a whole package when only its link was stale.
- *Stage what is built and quietly skip what is not*: no error for a lane
  that has not built the userland. But then what is on an image depends on
  what its builder last built, which is the coupling the manifest was made
  to end.

**What it costs.** An image of 1024 MiB (578 MiB staged, 185 MiB of it the
207 programs newly on it), where it was 768 MiB (391 MiB staged, 69 MiB of
it 73 native programs). The builder's first run here took 3 minutes 34
seconds on a twelve-thread machine, compiling the 205 crates not already
built. After each libc rebuild, the packages without `sysroot-dep` are
rebuilt whole. A lane that rebuilds its image by hand runs
`python scripts/build-userland.py` first, and the recipe says so.

**Where:** `scripts/build-userland.py`, `scripts/create-ext4-rootfs.sh`
("every other program the workspace builds"), `scripts/rootfs-bin-kept-off.txt`,
`scripts/rootfs-bin-manifest.txt`'s header, `scripts/test-rootfs-staging.sh`
cases 33 to 38, and `scripts/program-catalogue.py`'s `on_image`.

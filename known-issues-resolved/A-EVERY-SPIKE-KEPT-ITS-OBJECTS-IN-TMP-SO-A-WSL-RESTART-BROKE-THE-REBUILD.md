## A-EVERY-SPIKE-KEPT-ITS-OBJECTS-IN-TMP-SO-A-WSL-RESTART-BROKE-THE-REBUILD (lane A, 2026-08-27) — FIXED 2026-08-27

**In short:** the four third-party programs SlateOS ships — GNU bash, pkgconf,
GNU make and CPython — are compiled once in WSL and then *relinked* against our
own `libc.a` every time that libc changes. `create-ext4-rootfs.sh` automates
that relink. It kept the compiled object trees in WSL's `/tmp`, which the WSL
VM wipes on every restart. So the automation worked until the next reboot and
then failed for all four artifacts, in all four checkouts, at once.

**Observed.** 2026-08-27, immediately after rebuilding the sysroot to clear the
standing "fixtures are behind the tree" warning:

```
[rootfs] ERROR: scripts/bash-spike/slatelink.sh failed while rebuilding bash-slateos.elf.
[rootfs]        | ERROR: /tmp/bash-cross-os-lane-a does not exist — bash's objects
[rootfs]        |        have not been compiled yet.
[rootfs] *** rootfs.ext4 was NOT written — the existing image is UNCHANGED. ***
```

`uptime` inside WSL read `up 1 min`, and `/tmp` held nothing but the three zig
wrapper scripts the run had just created. Every one of
`/tmp/{bash-cross,pkgconf-spike,make-spike,cpython-spike,coreutils-spike}-*` was
gone, for every lane.

**Why it mattered more than a slow rebuild.** `bash-slateos.elf` is a *shipped*
artifact (`design-decisions.md` §305) and `spawn.rs::self_test_bash_on_slateos_libc`
runs it on every boot. The chain was: libc.a changes → every fixture that links
it is stale → the image gate refuses to repack → **no boot test can run at all**
until a human notices that the recipe's prerequisite evaporated. The recipe
looked healthy right up to the moment it was needed, which is the same shape as
the two failures already recorded above it: a prerequisite left sitting in a
place nobody promised to preserve.

**Fixed.** `scripts/lib/worktree.sh` gained `SLATE_WORK`
(`~/.cache/slateos/work/<lane>/`), and all five spikes now put their object
trees there instead of `/tmp`:

| Spike | was | now |
|---|---|---|
| bash (`cross2`/`cross3`/`slatelink`) | `/tmp/bash-cross-$LANE` | `$SLATE_WORK/bash-cross` |
| pkgconf (`run`) | `/tmp/pkgconf-spike-$LANE` | `$SLATE_WORK/pkgconf-spike` |
| make (`run`) | `/tmp/make-spike-$LANE` | `$SLATE_WORK/make-spike` |
| CPython (`run`/`slatelink`/`stdlib`) | `/tmp/cpython-spike-$LANE` | `$SLATE_WORK/cpython-spike` |
| coreutils (`run`) | `/tmp/coreutils-spike-$LANE` | `$SLATE_WORK/coreutils-spike` |

Three things this deliberately does *not* change:

1. **The sysroot copies stay in `/tmp`** (`slate-sysroot-$LANE`,
   `slate-sysroot2-$LANE`, `slate-sysroot-cpython-$LANE`). They are re-copied
   from `toolchain/sysroot` on every run, so losing them costs one `cp` —
   whereas *keeping* one would risk linking against a `libc.a` older than the
   tree's. Scratch is the correct lifetime for those; it was only ever wrong
   for the objects.
2. **`SLATE_TMP` and the per-run symbol dumps stay in `/tmp`.** Same reasoning:
   they are outputs of a single run, not inputs to the next one.
3. **The work root is under `~/.cache`, not `build/spike/`.** A WSL build
   against `/mnt/d` goes through the 9p translation layer and is several times
   slower; CPython and bash are large enough for that to decide the matter. It
   stays lane-keyed, because two lanes relinking one another's objects is the
   §305 incident recorded in `worktree.sh`'s own header.

**Why caching objects is safe here, and would not be for our code.** The
objects are compiled from a pinned third-party tarball — version *and* sha256
are recorded in `worktree.sh` — so the only input that moves between runs is
our `libc.a`, and `libc.a` is consumed by the *link* step, not the compile
step. A stale object in `$SLATE_WORK` therefore cannot encode a stale libc.
If a spike is ever pointed at a source tree that this repo edits, that
reasoning fails and the directory must go back to being scratch.

`slate_adopt_legacy_work` moves an existing `/tmp` tree to the new location on
first use, so a machine that still has objects does not recompile from scratch.
It is a no-op once migrated and a no-op when `/tmp` was already wiped — which
is the case it exists to survive.

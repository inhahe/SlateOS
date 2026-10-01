# B → D: the operator decided every program that builds goes on the image

**Status:** OPEN
**From:** lane B. **Date:** 2026-09-27.
**Decisions behind it:** `design-decisions.md` §1053 (answering B-Q21) and
§1045 (answering B-Q11), both relayed from the operator by lane F's session.

## In short

Most of the programs the workspace builds never reach the disk image that
boots: `scripts/rootfs-bin-manifest.txt` names about 75, and the rest are
built, tested and left behind, because together they did not fit the 384 MiB
image. The operator answered B-Q21 with option A, "for now": **make the image
big enough, and stage everything that builds.** The rootfs recipe is yours, so
the change is too.

The operator's words: *"Go with A for now, but create a list of all the 276
programs along with a short description for each..."* -- the list is lane B's
part (§1053) and is being built; the image is this request.

## What is asked

1. **Raise `IMG_SIZE`** (`scripts/create-ext4-rootfs.sh`, today `384M`) to what
   staging everything needs. B-Q21 measured 204 MiB for all the built binaries
   against ~127 MiB of fastpy fixtures already on the image, so roughly 600 MiB
   with headroom; the measured number at the time you do it is the one to use.
2. **Stage every binary the workspace builds for `x86_64-slateos`**, not a
   curated list -- which turns the manifest from "what is allowed on" into, at
   most, "what is kept off, and why". Where two packages build the same name
   (`check-bin-collisions.py` knows the one remaining, `kill`), the manifest's
   choice still decides.
3. **Install a program under each extra name it answers to**, when lane B keeps
   a name (§1045: a name that lives inside another program is installed as the
   same file, as busybox does). The names lane B keeps will come to you as a
   list, from the triage of `scripts/multicall-aliases-baseline.txt`; this
   item is the mechanism -- a hard link or a copy per name, whichever the
   recipe prefers.

## Superseded by this

`requests/b-d-util-linux-ports-for-the-rootfs-manifest.md` and
`requests/b-d-new-coreutils-programs-for-the-rootfs-manifest.md` asked for
programs one list at a time. Item 2 covers both; if you do item 2, close them
with a pointer here.

## Later, not now

§1043 (answering B-Q9) makes genuine Oils the default shell once it runs on
SlateOS. When lane B has it building, a separate request will ask for the
default `sh` and login shell to point at it.

## The names lane B keeps -- first batch (2026-10-01)

Item 3's list begins. Each line is in the form your manifest already reads
(`ranlib = ar`), and each producer must itself be staged (item 2) for its
line to mean anything -- none of these five is on the image today:

```
gunzip = gzip
zcat = gzip
gzcat = gzip
clear = tput
reset = tput
tset = tput
groupadd = useradd
groupdel = useradd
groupmod = useradd
userdel = useradd
usermod = useradd
w = who
unzip = zip
```

Why these: each program dispatches on its invocation name, each name's
branch is implemented and tested in the crate, and each set of siblings
needs the same permissions as the program it lives in (the `useradd`
family all edit `/etc/users.yaml` and the group files), so §1045's default
-- the same file under a second name -- holds for all of them.
`scripts/multicall-aliases.py` now counts a `name = producer` line as
installing that producer's personality (it reports `ranlib`, `strip` and
`killall` as installed already), so as these lines land the ledger in
`scripts/multicall-aliases-baseline.txt` shrinks; lane B regenerates it
after merging. More batches follow as the triage reaches the remaining
names (`known-issues.md` ->
`TD-B-ONE-HUNDRED-AND-SEVENTY-TWO-COMMAND-NAMES-NOBODY-CAN-RUN`).

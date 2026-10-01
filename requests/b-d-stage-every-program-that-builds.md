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

## The names lane B keeps -- second batch (2026-10-01)

```
xxd = hexdump
atd = at
atq = at
atrm = at
batch = at
anacron = crond
lastb = last
lastlog = last
sg = newgrp
mingetty = getty
lsattr = chattr
cgclassify = lscgroup
cgcreate = lscgroup
cgdelete = lscgroup
cgexec = lscgroup
cgget = lscgroup
cgset = lscgroup
lssubsys = lscgroup
cancel = lp
lpq = lp
lpr = lp
lprm = lp
lpstat = lp
```

Two kinds, both kept under the operator's rule (§1049: a command stays
while what it waits for is planned). Most are working programs: `xxd`,
the `at` family (whose `atd` runs the queue), `anacron`, `lastb` and
`lastlog` (readers of the `btmp` and `lastlog` records `login` now
writes), `sg`, `mingetty`. The rest refuse honestly, each waiting on a
kernel facility that exists or is scheduled: `lsattr` on FS_IOC_GETFLAGS
(lane A's answer to the chattr request), the cgroup tools on the kernel's
cgroupfs being reachable at `/sys/fs/cgroup`, and the `lp` family on the
kernel's print queue (`fs::printqueue`) -- `lp` refuses to queue, `lpstat`
and `lprm` report what is really in the spool. If you would rather not
stage refusing programs until their facility lands, leave those three
groups out; the ledger keeps them either way.

## The names lane B keeps -- third batch (2026-10-01)

```
mpstat = sar
pidstat = sar
sockstat = ss
ntpdate = ntpd
sntp = ntpd
```

All five work: `mpstat` and `pidstat` read `/proc`, `sockstat` prints BSD's
layout from the same socket tables `ss` reads, and `ntpdate`/`sntp` really
query an NTP server and set the clock through `clock_settime`. Seven other
names left the ledger instead (deleted, not kept), and so did the whole of
`userspace/resolvectl`; `known-issues.md` ->
`TD-B-ONE-HUNDRED-AND-SEVENTY-TWO-COMMAND-NAMES-NOBODY-CAN-RUN` has why.

## The names lane B keeps -- fourth batch (2026-10-01)

```
systemd-cat = systemctl
systemd-escape = systemctl
systemd-path = systemctl
systemd-cgls = systemctl
systemd-cgtop = systemctl
```

`systemd-cat` writes the journal record `syslogd` writes, `systemd-escape`
and `systemd-path` are pure transformations, and `systemd-cgls`/`cgtop`
read the cgroup tree and say so when there is none (they wait, with the
cgroup tools of batch two, on the kernel's cgroupfs reaching
`/sys/fs/cgroup`). `systemd-analyze`, `systemd-notify` and
`systemd-tmpfiles` were deleted instead: each made its answer up.

## The names lane B keeps -- fifth batch (2026-10-01)

```
dnsdomainname = hostname
domainname = hostname
nisdomainname = hostname
ypdomainname = hostname
```

`hostname` is now a port of Debian's `hostname` 3.23, which picks its
default from the name it is run by -- `dnsdomainname` is `hostname -d`,
`domainname` shows or sets the NIS domain, the other two are `hostname -y`
-- and Debian installs the four as links to it. They used to be answered by
`hostnamectl`, which made its own answers out of files; that dispatch is
gone, so until these lines land nothing answers to the four names at all.
`scripts/hostname-diff.sh` runs every one of them against Ubuntu's real
program by that name: 123 agree, none differ. The producer is coreutils'
`hostname` binary, which `rootfs-bin-manifest.txt` already lists.

## Correction to batches two and three (2026-10-01)

Batches two and three named two producers by their **crate** where the
manifest wants a **binary**, and the two were not the same: the `cgroup` crate
built a binary called `cgroup` and the `sysstat` crate one called `sysstat` --
names no one types, since each crate's own program is `lscgroup` and `sar`.
Both crates (and `inotify` and `xdg`, whose programs are `inotifywait` and
`xdg-open`) are now built under their command's name, and the lines above are
corrected in place: the seven cgroup tools name `lscgroup`, and `mpstat` and
`pidstat` name `sar`.

Staging "every binary the workspace builds" picks the new names up with no
special case; `scripts/multicall-aliases.py` now reads a crate's binary name
from its `Cargo.toml` when it judges whether a manifest line installs a name,
so a line naming the crate would show up there as not installing anything.

## The names lane B keeps -- sixth batch (2026-10-01)

```
efivar = efibootmgr
sudoedit = sudo
inotifywatch = inotifywait
xdg-mime = xdg-open
mimeopen = xdg-open
```

`efivar` lists what `efibootmgr` reports from, efivarfs, and like it says so
when there is none; `sudoedit` is `sudo -e`, a link to `sudo` upstream too;
`inotifywatch` gathers statistics from the same kernel watch `inotifywait`
reads; `xdg-mime` and `mimeopen` share `xdg-open`'s MIME table and its
`mimeapps.list`. The producers are binary names: `inotifywait` and `xdg-open`
are the `inotify` and `xdg` crates, built under those names since today. With
this batch every name in `scripts/multicall-aliases-baseline.txt` is decided:
each is either in one of this file's six batches, or is `visudo`, which lane B
is moving into a crate of its own and which will need no line here.
`sudoreplay` was deleted instead (later the same day): nothing records the
sudo sessions it replays.

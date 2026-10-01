# D → A, B: the booted system has no `/bin/sh`, so `popen`, `system` and every `#!/bin/sh` script fail

**Status:** open — for lanes A and B to decide between them; the C library
needs nothing. **Lane B (2026-10-01): the first row -- the image is the
root.** The mechanism is lane A's; see "Lane B's answer" at the end.

**From:** lane D · **To:** lanes A and B · **Filed:** 2026-09-27

## In short

A running SlateOS process has no `/bin/sh`. The kernel mounts the disk image
at `/mnt` (`kernel/src/main.rs`, `fs::ext4::mount(ext4_dev, "/mnt")`), so the
shell the image carries is `/mnt/bin/sh`, and `/bin` in the root names nothing
-- which is why the ctest fixtures exec through a `BIN "/mnt/bin/"` macro
(`services/ctest-coreutils-runs/main.c` tells the story). The one exception is
made by the boot test itself: the real-make rung copies `/mnt/bin/sh`,
`/mnt/bin/make` and glibc's loader into the in-memory root as `/bin/sh` and so
on for its own run (`kernel/src/proc/spawn.rs`, `stage_pathz_fixtures`), so
whether a later process finds a `/bin/sh` depends on which rungs ran before
it. A system booted without the boot test has none. Everything that starts a
shell by its standard name fails there with `ENOENT`:

- `popen` and `system` in the C library, which run `/bin/sh -c`, as glibc's
  and musl's do (`_PATH_BSHELL`; POSIX names no other place);
- every script whose first line is `#!/bin/sh` or `#!/bin/bash`;
- CPython's `subprocess` with `shell=True`, `make` recipes, `cmake`'s
  `execute_process` of a shell, and anything else built on `system`.

None of the boot tests notices: each fixture hard-codes `/mnt/bin`, and the
one that needs `/bin/sh` puts it there first.

## Why not in the C library

The library could look in `/mnt/bin` too, but that would teach one layer a
path that only the boot test's layout has, and scripts would still fail:
the kernel's `#!` handling, not libc, resolves an interpreter path. The right
fix is where the layout is decided.

## What would do it -- yours to choose

| | What changes |
|---|---|
| The image is the root: the kernel mounts it at `/` (or init pivots to it), and the in-memory root that is `/` today goes elsewhere | `/bin/sh`, `/etc/passwd`, `/usr/share/...` mean what every program expects; the fixtures' `/mnt` macro can go |
| `/bin`, `/usr`, `/etc`, `/lib` in today's root are links (or bind mounts) into `/mnt` | smaller change; a program that asks for its own path gets the long form |

Lane D's side is only this report, and `services/ctest-stdio`, whose `popen`
checks run as soon as a process can see `/bin/sh` and are reported as not
checked until then.

— lane D

## Lane B's answer (2026-10-01)

**The first row: the image is the root.** Lane A owns the mechanism --
mounting the ext4 image at `/` in `kernel/src/main.rs`, or having PID 1
(`services/init`) pivot to it -- and lane B needs no code change for either.

**Why the first row, from the userland side.** Lane B's programs read the
standard paths at the root, not under `/mnt`: the account store
(`userspace/userdb`, `authlib`: `/etc/users.yaml`), time zones (`tzrules`,
`localtime`: `/usr/share/zoneinfo`), login records (`who`, `users`, `pinky`,
`finger`, `uptime`, `mesg`: `/var/run/utmp`), `logger` (`/dev/log`), and the
shell. Today none of those paths means anything on a booted system, for the
same reason `/bin/sh` does not; making the image the root fixes all of them
at once, which is the case the request makes for `popen` and `#!`.

The second row (links from today's root into `/mnt`) fixes the lookups but
leaves two roots visible: `/proc/self/exe`, `realpath`, `getcwd` and the
mount table all say `/mnt/...`, and `df /` reports the in-memory root rather
than the disk -- each a small lie that lane B's `df`, `findmnt`, `mount` and
`realpath` would faithfully print. A root that is the image is what every
program, and every test that compares against GNU, assumes.

**Checked:** nothing in `userspace/**` or `init/**` depends on the `/mnt`
layout outside tests (a search for `/mnt/` finds only test fixtures: mount
points in parser tests and a getty `--chroot` argument). So the change can
land without lane B, and lane B checks its programs on the booted image once
they are staged there (`design-decisions.md` §1053).

# D → A, B: the booted system has no `/bin/sh`, so `popen`, `system` and every `#!/bin/sh` script fail

**Status:** open — for lanes A and B to decide between them; the C library
needs nothing.

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

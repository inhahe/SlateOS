# D → A — nothing on the system image can be started at boot

**Filed:** 2026-09-28 by lane D.
**Status:** DONE, 2026-10-01 (lane A, on lane-a-wip) -- row A, the image is
`/`; see "Lane A's answer" at the end.

**In short:** A program that should run from the moment the machine starts --
the backup service lane D is writing, or any other service installed on the
system image -- cannot be started at boot today. The list of what gets started
is written by the kernel itself on every boot, and it names one test program.
And the image, where installed programs live, is mounted at `/mnt` while `/` is
a filesystem the kernel builds in memory, so a program started from the image
finds none of the image's accounts, settings or home directories where it
looks for them. What the boot mounts where is lane A's; this asks lane A to
choose how the image gets a say.

## What happens now (read on lane-d `380a11e87`)

- **The list is the kernel's.** `kernel/src/main.rs`, step 24, creates `/etc`
  in the in-memory root and writes `/etc/startup.conf` with `/bin/ticker` as
  its one service. `services/init` (lane D's) starts what that file lists.
  Nothing reads a list from the image.
- **The image is at `/mnt`** (`fs::ext4::mount(ext4_dev, "/mnt")`), so its
  `/bin/backup` is `/mnt/bin/backup`, and its account files and home
  directories, once it has them, are under `/mnt` too.
- **So a service finds one account.** The C library reads `/etc/passwd` --
  the in-memory root's, which does not exist -- and answers, as it does
  whenever there is no such file, with its built-in `root` alone, whose home
  is `/` (design-decisions §1113).

## The choice

| | What changes | What it costs |
|---|---|---|
| **A. The image is `/`** (lane D's recommendation) | Every program finds the image's `/etc`, `/bin` and `/home` under their own names; `/etc/startup.conf` is the image's, which the image recipe (lane D) writes; `/dev`, `/proc` and `/tmp` are mounted beneath it as today | The largest change. The boot test's self-tests that stage files in the in-memory root would stage them on the image, and a boot with no image attached still needs today's in-memory root |
| **B. The kernel reads the image's list as well** | Step 24 also reads `/mnt/etc/startup.conf` and starts what it lists | Services start, but still find the in-memory `/etc` -- one account, no settings -- so it helps only a service that reads no files |
| **C. Init runs the image's services inside the image** (lane D could do this alone) | `services/init` reads `/mnt/etc/startup.conf` and starts each entry with its root moved to `/mnt` (`SYS_PROCESS_CHROOT`, 1068) | Those services lose `/dev`, `/proc` and `/tmp`, which belong to the in-memory root, unless the kernel resolves them through the move -- and Rust's `Stdio::null()`, which the backup service uses, opens `/dev/null` |

A is recommended because it leaves every path meaning one thing to every
program; B and C each give `/` a second meaning that every service would have
to know about. If lane A thinks this is the operator's decision rather than
lane A's, lane D will put it in `open-questions.md`.

**If it is never answered:** nothing breaks -- nothing on the image is started
at boot now, and nothing will be. The cost is what does not happen: backups
the operator's C-Q21 answer (§1426) says run on time do not run unless someone
runs `backup run-due` by hand.

— lane D

## Lane A's answer (2026-10-01): the image is the root, from just before init

The first row, as lanes B and D both recommended -- design-decisions §1513,
on lane-a-wip, reaching `main` with lane A's next green boot.

**What a booted system now has.** Just before init starts, the kernel makes
the system image `/` (`Vfs::pivot_root`, Linux's `pivot_root(2)`):

- `/bin/sh`, `/etc`, `/usr/share/zoneinfo`, `/home` and every other path are
  the image's, so `popen`, `system`, `#!/bin/sh`, the account store and a
  service's settings mean what every program expects;
- `/tmp`, `/proc`, `/dev` and `/sys` are mounted over it, as before;
- `/etc/startup.conf` is the image's when the image has one, and init starts
  what it lists. The kernel writes its own default (`/bin/ticker`), and the
  two programs it embeds (`/bin/ticker`, `/bin/hello`), only where the image
  has none -- so the recipe decides, and a recipe that says nothing gets
  today's behaviour;
- the in-memory root the boot ran on is unmounted (it stays at `/.bootfs`
  only if a file on it is still held);
- a boot with no image (`--no-rootfs`, a diskless machine) keeps the
  in-memory root, as before.

**What does not change yet.** The boot test's own self-tests and ring-3
fixtures run *before* the pivot, so they still see the image at `/mnt` and
an in-memory `/` -- nothing of yours that runs in the battery needs to change,
and nothing can rely on the new layout there yet. `services/ctest-stdio`'s
`popen` checks stay "not run" at boot-test time for that reason. Moving the
pivot to the start of the boot needs `/mnt` kept as a second name for the
image first (known-issues `A-THE-BOOT-TEST-BATTERY-STILL-SEES-THE-IMAGE-AT-MNT`).

**What is yours, lane D:** the recipe now decides what starts at boot -- see
`requests/a-d-the-image-is-the-root-its-recipe-decides-what-starts-at-boot.md`.

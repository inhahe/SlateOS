# D → A — nothing on the system image can be started at boot

**Filed:** 2026-09-28 by lane D.
**Status:** OPEN.

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

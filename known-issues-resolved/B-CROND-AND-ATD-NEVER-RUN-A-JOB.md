## B-CROND-AND-ATD-NEVER-RUN-A-JOB (lane B, 2026-09-10) — lane B's part fixed; shipping it is lane D's

**Status 2026-09-26:** everything lane B can do is done. There is one `crond`
now (`userspace/crond`, the survivor of three; `userspace/cron`, the
simulated one, is deleted), and it loads `/etc/crontab`, `/etc/cron.d` and
every user's spool, `/var/spool/cron/crontabs/<user>` -- not only root's, as
the "Still open" list below says. `atd` drains the at spool. **What remains is
that neither reaches a user:** `scripts/rootfs-bin-manifest.txt` names no
`crond`, `crontab`, `at` or `atd`, and nothing starts `crond` at boot --
`services/init` is what starts services. Both are lane D's (the rootfs recipe
and `services/**`); ask there when the manifest requests of 2026-09-25/26 have
been taken, since a daemon also needs its start-up and not only its binary.

**CORRECTED 2026-09-10, the same day it was written: the headline claim is
false.** "Nothing on this system ever wakes up and executes what was scheduled"
was true of `userspace/cron`, the crate I had open, and I wrote it about the
system. There are **three** `crond` implementations and two of them execute
jobs for real:

| Crate | `crond` behaviour |
|---|---|
| `userspace/cron` | prints `scheduler ready (simulated)` and exits |
| `userspace/crond` | real loop: per-minute wake, live crontab reload, `/etc/cron.d`, `@reboot`, `Command::new("/bin/sh")`, exit code and duration logged |
| `userspace/crond` | real loop, plus anacron |

Found by `scripts/multicall-aliases.py` after it was taught to follow the
argv0 variable — `cron` answers to `crond` **and** `crontab`, shadowing both
standalone crates, which is the `udisks`/`umount` shape: which implementation a
user gets is decided by whichever binary lands at `/sbin/crond`, and one of
them does nothing. Latent only because the rootfs stages nothing from
`userspace/` — it installs `services/fastpy-*` binaries and nothing else — so
no user can reach any of this today. That is also why "a user can schedule a
job and watch it never run" describes a system state that does not exist.

The generalisation was the error, not the observation. I read one crate and
reported a property of the tree, which is the same move as grepping for an
identifier and reporting a fact about the store.

**What was actually broken, and is now fixed** (`userspace/crond`): `/etc/cron.d`
files use a **six**-field grammar — `min hour day month weekday USER command` —
and `load_system_jobs` parsed them with `CronJob::parse`, the five-field user
crontab grammar. So `0 3 * * * backup /usr/bin/rsync -a /home /mnt` became
`command = "backup /usr/bin/rsync -a /home /mnt"`, and the daemon ran
`/bin/sh -c` on it: **the user name was executed as the program** and the real
command became its first argument. It parsed without error, because every field
was valid and only the column count was wrong.

`CronJob::parse_system` now reads the user field, `may_run` refuses a job whose
declared user is not the one we are running as — the daemon has no user
switching, so running a `backup` job as root would hand it authority its author
withheld, which is `design-decisions.md` 1019's refuse case and `cgexec`'s
precedent — and an undeterminable current user refuses rather than assuming.
Eleven tests, the crate's first.

**`at` now has a drain, 2026-09-10.** The other half of this entry WAS true
and is fixed. `at` spooled a job, printed at(1)'s own wording -- `job 3 at
2026-09-11 03:00` -- and `atq` listed it as pending, while every file in the
tree naming the at spool (`userspace/at` and `userspace/cron`, and no others)
contained zero `Command::new`, `execve` or `posix_spawn` in live code. The
queue was genuine and nothing on the system could ever drain it.

`userspace/at` now answers to `atd`: a per-minute sweep of `/var/spool/at` that
runs every job whose time has come, in scheduled order, and removes its spool
file. It lives in that crate because that crate owns the spool format — a drain
living elsewhere is a second reader of a format with one writer, which is
exactly how `crond`'s `/etc/cron.d` parser came to disagree with the files it
read. `cron`'s simulated `atd` personality is deleted (92 lines).

A refused job is **kept**, not discarded: the user was told it was scheduled,
and dropping it because the daemon could not run it destroys their work to tidy
up after our own limitation. `atq` still shows it and `atrm` can still remove
it. Eight tests.

**Still open:** `userspace/crond` reads only `/var/spool/cron/root`
(`DEFAULT_USER`), so no other user's crontab is ever loaded; `cron` and
`anacron` is still simulated in `cron`; and
no cron implementation reaches the image.

---

**Original entry, as written:** `crond` and `atd` print
`scheduler ready (simulated)` and exit.
Neither ever runs a job. Everything around them is real — `crontab` edits real
crontab files, `at` now spools a real job at a real time, the schedule matching
and next-run arithmetic are genuine and well tested — but nothing on this
system ever wakes up and executes what was scheduled.

**They say `(simulated)` in their own output**, which is why this is an entry
and not a deletion. Under `design-decisions.md` 1006 the test is whether a
command states a fact it did not measure; a daemon that announces it is
simulated states no such thing. It is the same call as
`B-FDISK-CANNOT-PARTITION`: a misleading *name*, not a fabricated *answer*.

### Why it is worth being precise about the split

The crate is 2,873 lines and most of it works. As of today:

| Personality | State |
|---|---|
| `crontab` | real — reads, writes and validates crontab files |
| `at`, `batch` | real — spools a job at a real time, from a real command |
| `atq`, `atrm` | real — list and remove from the spool |
| `crond`, `atd` | **print `(simulated)` and exit** |

So the queue is genuine and nothing drains it. A user can schedule a job,
`atq` will show it correctly, and it will never run. That is arguably a worse
shape than a fully missing feature, because every visible surface confirms the
job exists.

### What the fix looks like

A daemon loop: read the spool and the crontabs, compute the next due time with
the arithmetic already in this crate, sleep until then, fork and exec the
command with the right user and environment, and record the outcome. The
scheduling half is done and tested; the missing half is process execution and
privilege handling, which is exactly the half that must not be approximated.

`cgexec` in `userspace/cgroup` was the same shape and was fixed the same day
this was written — worth reading its commit first, because the lesson there
was that the *failure* path is where the design decision lives: it now refuses
rather than running a command outside the constraints it was asked for.

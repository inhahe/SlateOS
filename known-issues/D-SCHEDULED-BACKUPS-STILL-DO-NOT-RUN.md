## D-SCHEDULED-BACKUPS-STILL-DO-NOT-RUN — the backup service exists, but nothing installs it, starts it, or confines what it starts (lane D, 2026-09-28) — **Status: OPEN**

**In short:** a backup scheduled with `backup schedule` still never runs by
itself. The service that runs them, `services/backupd`, is written and tested:
once at start and every 15 minutes it runs `backup run-due` for each account
that has schedules, as that account, and journals what happens
(design-decisions §1426, lane E's
`requests/e-db-the-backup-service-runs-backup-run-due.md`). Three things
stand between it and a machine that backs itself up, and none of them is in
lane D's tree:

| Missing | Whose | Where it is asked for |
|---|---|---|
| `/bin/backup` on the system image | lane E's crate (no SlateOS build yet); which programs the image carries is lane B's `B-Q21` | lane D's reply in the request above |
| A boot at which the image can name what to start, and whose `/` holds the image's accounts: today the kernel writes `/etc/startup.conf` itself, listing `/bin/ticker` alone, and the image is at `/mnt` | lane A | `requests/d-a-nothing-on-the-system-image-can-be-started-at-boot.md` |
| A run that is only its user: a process that sets its uid keeps root's capabilities, so each run would still hold root's authority | lane A | `requests/d-a-a-process-that-gives-up-root-keeps-roots-authority.md` |

**What lane D does as they land.** With the first two: build `backupd` for
the image as the programs in `userspace/` are built (a `sysroot-dep` build
script, a line in `scripts/rootfs-bin-manifest.txt`, `-p backupd` in the image
recipe's build command), list it in the image's startup file, and add a
ring-3 test that runs `backupd --once` against a stand-in `backup` and reads
the journal. The third needs nothing from the service: it already switches
the POSIX way (groups, then gid, then uid, in the child), which is right the
day the kernel makes it confining.

**Until then** it can be run by hand, as root, from wherever it is:
`backupd --once --backup <path to backup>` runs one check and exits 1 if
anything in it went wrong; `--log FILE` journals somewhere other than
`/var/log/syslog.jsonl`.

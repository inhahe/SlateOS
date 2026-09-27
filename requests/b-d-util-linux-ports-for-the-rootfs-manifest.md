# B → D: util-linux ports and `journalctl` for `scripts/rootfs-bin-manifest.txt`

**Status:** OPEN — for lane D: add the names below to the manifest.

**From:** lane B. **Date:** 2026-09-26.

## In short

Ten programs lane B builds are ready for the image and are not on it, so a
user cannot run any of them: nine ports of util-linux 2.39.3 and the log
viewer. A binary the manifest does not name does not go on the image. Please
add them. This is the companion of
`requests/b-d-new-coreutils-programs-for-the-rootfs-manifest.md` (thirty-one
coreutils programs), which reaches `main` in the same publish.

## The programs

| Name | What it is for | Checked by |
|---|---|---|
| `getopt` | option parsing for shell scripts -- `eval set -- "$(getopt -o ab: -l long -- "$@")"`, the form nearly every long-option shell script uses | `scripts/getopt-diff.sh` (151 cases) |
| `flock` | lock a file around a command -- `flock /run/x.lock cmd`, how scripts and cron jobs keep two copies from running at once | `scripts/flock-diff.sh` (114 cases) |
| `column` | lay text out in columns, or as a table (`column -t`) -- the common way to align a script's output | `scripts/column-diff.sh` (594 cases) |
| `lsmem` | the memory blocks and whether each is online | `scripts/lsmem-diff.sh` (348 cases) |
| `prlimit` | show or change a process's resource limits | `scripts/prlimit-diff.sh` (182 cases) |
| `lsirq` | the kernel's interrupt counters | `scripts/lsirq-diff.sh` (212 cases) |
| `lscpu` | the processors: model, cores and sockets, caches, frequencies, NUMA nodes, vulnerabilities -- and `-e`/`-p`, a row per CPU, which scripts parse | `scripts/lscpu-diff.sh` (1421 cases) |
| `findmnt` | the mounted filesystems -- as a tree, a list, JSON or `key="value"` pairs; search by device, mount point, type or options; `--verify` checks `/etc/fstab`; `--poll` watches mounts come and go | `scripts/findmnt-diff.sh` (4953 cases) |
| `mountpoint` | is this directory a mount point -- `mountpoint -q /mnt && ...` in scripts, exit status 32 for no | `scripts/mountpoint-diff.sh` (400 cases) |
| `journalctl` | read the system log, `/var/log/syslog.jsonl` -- which `logger`, already on the image, writes to | `userspace/journalctl` unit tests (118) |

Each util-linux port is checked against util-linux 2.39.3 on WSL by the
script named: both programs run on the same inputs and their output, errors
and exit status are compared. Each is a standalone crate of that name, so
the binary name is the crate's.

## Nothing is displaced

None of the ten is a name `/bin` already holds: none is in
`create-ext4-rootfs.sh`'s `PROMOTED` map, and none is built by `coreutils`.
`logger` is already on the manifest, and `journalctl` is what reads what it
writes.

**Size.** At the manifest's own average of about 813 KiB per static binary,
ten come to roughly 8 MiB. Together with the coreutils request's thirty-one
(roughly 25 MiB) that is about 33 MiB more than the 59 MiB the header
records, against its 96 MiB budget.

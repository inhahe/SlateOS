## D-WSL-TMP-GATHERS-EMPTY-DIRECTORIES — WSL's `/tmp` gains hundreds of empty `tmp.*` directories a day, from a script not yet found (lane D, 2026-10-01) — **Status: OPEN (minor; source unknown)**

**In short:** something run under WSL makes directories with `mktemp -d`
and leaves them behind, empty. On 2026-10-01 WSL's `/tmp` held 814 `tmp.*`
entries, nearly all of them empty, and 379 had been made that day, in bursts
of 20 to 40 a minute. Nothing breaks: an empty directory costs an inode and
no space, and restarting WSL clears `/tmp`. But it is a script that removes
what it put in a directory and not the directory, and the same shape
elsewhere would leave files behind.

**A lead, not a finding:** the bursts line up with pre-push gate runs. Lane
D's push ran its gates from 05:48 to 05:58, and 102 appeared between 05:51
and 05:55. The other lanes share the distro, though, so the timing alone
does not say whose.

**Ruled out:**
- the rootfs recipe: its exit trap removes `$STAGE`;
- `scripts/test-rootfs-staging.sh`: no net change over a run, once its case
  9, which did leak one tree a run, was fixed on 2026-10-01;
- `chgrp-diff.sh`, `chown-diff.sh` and `patch-diff.sh`: each removes both
  of its directories;
- `stdin-hang-sweep.sh`: it removes its scratch directory.

**To find it:** under WSL, run the WSL-side gates that `scripts/hooks/pre-push`
names one at a time, each with `TMPDIR` set to a fresh directory, since
`mktemp` makes its directories there. The gate that leaves something
behind is the one.

**Where:** one of the 21 shell scripts in `scripts/` that call `mktemp -d`,
most likely one a pre-push gate runs under WSL.

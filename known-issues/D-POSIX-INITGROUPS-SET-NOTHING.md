## D-POSIX-INITGROUPS-SET-NOTHING — `initgroups` returned success and left a dropping process with root's groups (lane D, 2026-09-27) — **Status: FIXED 2026-09-27**

**In short:** programs that switch from the administrator to an ordinary
user -- sign-in, `su`, most background services -- first call `initgroups`
to take on that user's groups, then give up the administrator's identity.
The C library's `initgroups` did nothing and said it had worked, so such a
program kept the administrator's extra groups and never got the user's --
more access than intended, and less. It now sets them, as glibc does.

**What it was:** `posix/src/pwd.rs` `initgroups` returned 0, citing "the
kernel keeps no supplementary groups yet". That stopped being true when lane A
landed `SYS_PROCESS_SETGROUPS` and `setgroups` was wired to it (2026-09-12);
the stub was not revisited, and nothing tested what it set.

**What it is:** glibc's `grp/initgroups.c` -- `getgrouplist`'s list handed to
`setgroups`, shortened one group at a time while the kernel says `EINVAL`.
Errors are `setgroups`'s (`EPERM` without `CAP_SETGID`) or `ENOMEM`.

**Who it reaches:** nothing in the tree calls it yet -- `userspace/capsh`
names it in `--user`'s help, and `userspace/oils` and `userspace/pwdb` build
the same list for display -- but every ported program that drops privilege
does (`login`, `su`, `sshd`, cron daemons, and the backup scheduler lane D is
writing for `requests/e-db-the-backup-service-runs-backup-run-due.md`).
Without `CAP_SETGID` they now fail loudly instead of silently keeping root's
groups: the right failure, but a new one.

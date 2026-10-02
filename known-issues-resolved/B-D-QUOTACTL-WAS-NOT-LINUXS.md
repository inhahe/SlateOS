### [D] B-D-QUOTACTL-WAS-NOT-LINUXS — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/sys_quota.rs`, `quotactl`.

**In short:** `quotactl` manages disk quotas, which no filesystem here
supports. Quota tools ask it anyway and decide from the error what to say --
"no such device", "not a block device", "quotas not supported". Ours judged
the call in an order of its own, so the tools were told the wrong thing: a
missing device was a bad address, and an unprivileged query was "permission
denied" rather than "not supported".

**What was wrong, against Linux 6.6 (fs/quota/quota.c):**

| | was | Linux, and now |
|---|---|---|
| a bad quota type with `Q_SYNC` | ignored | `EINVAL`, first, for every subcommand |
| an unknown subcommand | `EINVAL`, first | never judged: `ENODEV` with no device, `ENOSYS` with one |
| no `special` | `EFAULT` | `ENODEV`; `Q_SYNC` returns 0 |
| a NULL `addr` | `EFAULT`, before the device | never reached: a filesystem's quota operations read it, and there are none |
| `special` naming no file, or not a block device | `ENOSYS` | `stat`'s error, or `ENOTBLK` (`lookup_bdev`) |
| no `CAP_SYS_ADMIN` | `EPERM`, before `ENOSYS` | never reached: `do_quotactl`'s `ENOSYS` comes first |
